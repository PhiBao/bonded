use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

use crate::types::{AgentRunView, EvaluationView};
use crate::typesafe::SemanticReview;

#[derive(Debug, Clone)]
pub struct AgentRun {
    pub patch: String,
    pub view: AgentRunView,
}

#[derive(Debug, Clone)]
pub struct EvalRun {
    pub passed: bool,
    pub logs: String,
    pub logs_hash: String,
    pub tamper_check: bool,
    pub duration_ms: u128,
}

pub fn hash_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

/// Strip nondeterministic content (timings, temp paths, durations) so the
/// hash stored on-chain can be reproduced by an independent re-run.
fn normalize_logs(logs: &str) -> String {
    let mut out = String::with_capacity(logs.len());
    for line in logs.lines() {
        let mut l = line.to_string();
        if l.contains("duration_ms") {
            l = l
                .split("duration_ms")
                .next()
                .unwrap_or("")
                .trim_end()
                .to_string();
        }
        // Temp dir paths differ per run.
        if let Some(idx) = l.find("/tmp/") {
            l.truncate(idx);
        }
        out.push_str(l.trim_end());
        out.push('\n');
    }
    out
}

/// `"preset:add"` → `Some("add")`; anything else is treated as a git URL.
pub fn preset_of(repo_url: &str) -> Option<String> {
    repo_url.strip_prefix("preset:").map(|s| s.to_string())
}

pub fn issue_text(fixtures_dir: &Path, preset: &str, fallback: &str) -> String {
    std::fs::read_to_string(fixtures_dir.join(preset).join("issue.txt"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| fallback.to_string())
}

// ---------------------------------------------------------------------------
// Reference agents (deterministic demo paths). Reads pre-written patches from
// demo/reference-patches — a directory agents never see.
// ---------------------------------------------------------------------------

/// outcome: "success" | "failure" | "cheat" | "tamper"
pub fn reference_agent(ref_patches: &Path, preset: &str, outcome: &str) -> Result<AgentRun> {
    let started = Instant::now();
    let kind = match outcome {
        "failure" => "bad",
        "cheat" => "cheat",
        "tamper" => "tamper",
        _ => "fix",
    };
    let patch_name = format!("{preset}-{kind}.patch");
    let patch = std::fs::read_to_string(ref_patches.join(&patch_name))
        .with_context(|| format!("missing {patch_name}"))?;
    let patch_hash = hash_hex(&patch);
    let preview = patch.lines().take(24).collect::<Vec<_>>().join("\n");
    let agent_id = if outcome == "success" {
        "forge"
    } else {
        "sable"
    }
    .to_string();
    Ok(AgentRun {
        patch,
        view: AgentRunView {
            agent_id,
            runtime: "reference".to_string(),
            patch_hash,
            patch_preview: preview,
            duration_ms: started.elapsed().as_millis(),
        },
    })
}

// ---------------------------------------------------------------------------
// Live agent (OpenCode headless). The work dir contains ONLY the fixture — no
// patches, no hidden tests, no solution hints anywhere on the path.
// ---------------------------------------------------------------------------

pub async fn opencode_agent(
    fixtures_dir: &Path,
    preset: &str,
    issue: &str,
    test_command: &str,
    model: &str,
) -> Result<AgentRun> {
    let started = Instant::now();
    let work = temp_dir("bonded-agent")?;
    copy_dir(&fixtures_dir.join(preset), &work)?;
    init_git(&work)?;

    let prompt = format!(
        "You are a coding agent working in this repository. The failing command is `{test_command}`. \
         The issue is: {issue}. Make the smallest production-code change that fixes the failing test. \
         Do not modify, delete, skip, or weaken tests or test configuration. Do not edit package.json. \
         When finished, stop. Do not explain."
    );
    let output = tokio::time::timeout(
        Duration::from_secs(150),
        Command::new("opencode")
            .arg("--standalone")
            .arg("run")
            .arg("--model")
            .arg(model)
            .arg("--auto")
            .arg(&prompt)
            .current_dir(&work)
            .env("PWD", &work)
            .env("OLDPWD", &work)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("opencode agent timed out")??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&work);
        anyhow::bail!("opencode agent failed: {stderr}");
    }
    tracing::info!(
        stdout = %String::from_utf8_lossy(&output.stdout).chars().take(600).collect::<String>(),
        "opencode agent finished"
    );

    let diff = Command::new("git")
        .args(["diff", "--", "."])
        .current_dir(&work)
        .output()
        .await
        .context("git diff failed")?;
    let patch = String::from_utf8_lossy(&diff.stdout).to_string();
    let _ = std::fs::remove_dir_all(&work);
    if patch.trim().is_empty() {
        anyhow::bail!("opencode agent produced no diff");
    }
    let patch_hash = hash_hex(&patch);
    let preview = patch.lines().take(24).collect::<Vec<_>>().join("\n");
    Ok(AgentRun {
        patch,
        view: AgentRunView {
            agent_id: "forge".to_string(),
            runtime: "opencode".to_string(),
            patch_hash,
            patch_preview: preview,
            duration_ms: started.elapsed().as_millis(),
        },
    })
}

// ---------------------------------------------------------------------------
// Evaluation: prepare the repo (preset copy or git clone), apply the patch,
// run visible tests + hidden acceptance check in Docker, hash the logs.
// ---------------------------------------------------------------------------

pub async fn evaluate_patch(
    fixtures_dir: &Path,
    hidden_dir: &Path,
    repo_url: &str,
    commit: &str,
    patch: &str,
    test_command: &str,
) -> Result<EvalRun> {
    let started = Instant::now();
    let work = prepare_work(fixtures_dir, repo_url, commit).await?;

    let patch_path = work.join(".bonded.patch");
    std::fs::write(&patch_path, patch)?;
    let apply = Command::new("git")
        .args(["apply", "--whitespace=nowarn", ".bonded.patch"])
        .current_dir(&work)
        .output()
        .await
        .context("git apply failed to start")?;
    if !apply.status.success() {
        let stderr = String::from_utf8_lossy(&apply.stderr);
        let _ = std::fs::remove_dir_all(&work);
        anyhow::bail!("patch did not apply: {stderr}");
    }

    let tamper_check = touches_tests(patch);
    let (visible_logs, visible_ok) = run_tests(&work, test_command, true).await?;

    let mut logs = format!("## acceptance test\n\n{visible_logs}");
    let mut passed = visible_ok && !tamper_check;

    // Hidden acceptance check: values the agent never saw. Runs only when the
    // visible suite passed, so hardcoded patches are caught deterministically.
    if visible_ok {
        if let Some(preset) = preset_of(repo_url) {
            let hidden_src = hidden_dir.join(format!("{preset}.mjs"));
            if hidden_src.exists() {
                std::fs::copy(&hidden_src, work.join(".hidden-check.mjs"))?;
                let (hidden_logs, hidden_ok) = run_command(
                    &work,
                    "node .hidden-check.mjs",
                    false,
                    Duration::from_secs(20),
                )
                .await?;
                logs.push_str(&format!("\n\n## hidden acceptance check\n\n{hidden_logs}"));
                passed = passed && hidden_ok;
            }
        }
    }

    let logs_hash = hash_hex(&normalize_logs(&logs));
    let duration_ms = started.elapsed().as_millis();
    let _ = std::fs::remove_dir_all(&work);
    Ok(EvalRun {
        passed,
        logs,
        logs_hash,
        tamper_check,
        duration_ms,
    })
}

/// Prepare a working copy of the job's repository.
///
/// Presets copy a local fixture. Anything else is treated as a git URL and
/// cloned at `commit` ("HEAD"/"demo" check out the default branch).
pub async fn prepare_work(fixtures_dir: &Path, repo_url: &str, commit: &str) -> Result<PathBuf> {
    let work = temp_dir("bonded-eval")?;
    if let Some(preset) = preset_of(repo_url) {
        copy_dir(&fixtures_dir.join(&preset), &work)?;
        init_git(&work)?;
        return Ok(work);
    }

    // Custom repo: clone on the host, then install deps for the sandbox run.
    Command::new("git")
        .args(["clone", "--quiet", repo_url])
        .arg(&work)
        .output()
        .await
        .context("git clone failed to start")?
        .status
        .success()
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("git clone failed for {repo_url}"))?;
    if !commit.is_empty() && commit != "demo" && commit != "HEAD" {
        let status = Command::new("git")
            .args(["checkout", "--quiet", commit])
            .current_dir(&work)
            .output()
            .await
            .context("git checkout failed to start")?;
        if !status.status.success() {
            anyhow::bail!("commit {commit} not found in {repo_url}");
        }
    }
    Ok(work)
}

async fn install_deps(work: &Path) -> Result<()> {
    if !work.join("package.json").exists() || work.join("node_modules").exists() {
        return Ok(());
    }
    let install = if work.join("package-lock.json").exists() {
        "npm ci --no-audit --no-fund"
    } else {
        "npm install --no-audit --no-fund"
    };
    let (_, ok) = run_command(work, install, true, Duration::from_secs(180)).await?;
    if !ok {
        anyhow::bail!("dependency install failed for cloned repo");
    }
    Ok(())
}

async fn run_tests(
    work: &Path,
    test_command: &str,
    allow_network_install: bool,
) -> Result<(String, bool)> {
    if allow_network_install {
        install_deps(work).await.ok();
    }
    run_command(work, test_command, false, Duration::from_secs(60)).await
}

fn docker_image() -> String {
    std::env::var("DOCKER_IMAGE").unwrap_or_else(|_| "node:22-alpine".to_string())
}

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `cmd` in the work dir, preferably inside Docker with no network,
/// memory and CPU limits. Falls back to a plain host shell when Docker
/// is unavailable (documented limitation).
async fn run_command(
    work: &Path,
    cmd: &str,
    allow_network: bool,
    timeout: Duration,
) -> Result<(String, bool)> {
    if docker_available() {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--memory".to_string(),
            "512m".to_string(),
            "--cpus".to_string(),
            "1".to_string(),
        ];
        if !allow_network {
            args.push("--network".to_string());
            args.push("none".to_string());
        }
        args.push("-v".to_string());
        args.push(format!("{}:/work", work.display()));
        args.push("-w".to_string());
        args.push("/work".to_string());
        args.push(docker_image());
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(cmd.to_string());

        let output = tokio::time::timeout(
            timeout,
            Command::new("docker")
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("command timed out"))??;
        let combined = format!(
            "$ {cmd}\n\n{}\n{}\n\nexit: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            output.status.code().unwrap_or(-1)
        );
        return Ok((combined, output.status.success()));
    }

    tracing::warn!("docker unavailable; running test command on host");
    let output = tokio::time::timeout(
        timeout,
        Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(work)
            .env("CI", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("command timed out"))??;
    let combined = format!(
        "$ {cmd}\n\n{}\n{}\n\nexit: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        output.status.code().unwrap_or(-1)
    );
    Ok((combined, output.status.success()))
}

pub fn evaluation_view(
    eval: &EvalRun,
    review: &SemanticReview,
    test_command: &str,
) -> EvaluationView {
    EvaluationView {
        passed: eval.passed,
        semantic_verdict: review.verdict.clone(),
        semantic_score: review.score,
        typesafe_model: review.model.clone(),
        test_command: test_command.to_string(),
        logs: eval.logs.clone(),
        logs_hash: eval.logs_hash.clone(),
        tamper_check: eval.tamper_check,
        duration_ms: eval.duration_ms,
        real_fix_probability: review.real_fix_probability,
        tamper_probability: review.tamper_probability,
        review_source: review.source.clone(),
    }
}

fn touches_tests(patch: &str) -> bool {
    const PROTECTED: &[&str] = &[
        "test/",
        "tests/",
        "spec/",
        "__tests__/",
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "jest.config",
        "vitest.config",
        "Cargo.toml",
    ];
    patch
        .lines()
        .filter_map(|line| {
            line.strip_prefix("+++ b/")
                .or_else(|| line.strip_prefix("--- a/"))
        })
        .any(|path| {
            let path = path.trim();
            PROTECTED.iter().any(|protected| {
                path == *protected
                    || path.starts_with(protected)
                    || path.contains(&format!("/{protected}"))
            })
        })
}

fn temp_dir(prefix: &str) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn copy_dir(source: &Path, destination: &Path) -> Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn init_git(dir: &Path) -> Result<()> {
    let run = |args: &[&str]| -> Result<()> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    };
    run(&["init", "-q"])?;
    run(&["config", "user.email", "demo@bonded.local"])?;
    run(&["config", "user.name", "Bonded Demo"])?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "baseline"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable() {
        assert_eq!(
            hash_hex("abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_file_tampering_is_detected() {
        assert!(touches_tests(
            "diff --git a/test/math.test.js b/test/math.test.js\n+++ b/test/math.test.js\n"
        ));
        assert!(touches_tests("+++ b/package.json\n"));
        assert!(touches_tests("--- a/test/math.test.js\n+++ /dev/null\n"));
        assert!(!touches_tests("+++ b/src/math.js\n"));
    }

    #[test]
    fn preset_parsing() {
        assert_eq!(preset_of("preset:add"), Some("add".to_string()));
        assert_eq!(preset_of("https://github.com/x/y"), None);
    }

    #[test]
    fn normalized_log_hashes_match_across_runs() {
        let a = "ok 1 - add returns the sum\n  duration_ms: 9.332077\n  location: '/tmp/bonded-eval-aaa/x.js:5:1'\n";
        let b = "ok 1 - add returns the sum\n  duration_ms: 3.1\n  location: '/tmp/bonded-eval-bbb/x.js:5:1'\n";
        assert_eq!(hash_hex(&normalize_logs(a)), hash_hex(&normalize_logs(b)));
    }
}
