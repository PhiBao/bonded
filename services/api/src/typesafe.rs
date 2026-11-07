use anyhow::{Context, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct SemanticReview {
    pub verdict: String,
    pub score: u16,
    pub model: String,
    pub real_fix_probability: f64,
    pub tamper_probability: f64,
    pub source: String,
}

pub async fn review(
    client: &reqwest::Client,
    api_key: Option<&str>,
    issue: &str,
    patch: &str,
    test_output: &str,
    tests_passed: bool,
    tamper_check: bool,
) -> SemanticReview {
    if tamper_check {
        return SemanticReview {
            verdict: "test_manipulation".to_string(),
            score: 0,
            model: "deterministic".to_string(),
            real_fix_probability: 0.0,
            tamper_probability: 1.0,
            source: "deterministic".to_string(),
        };
    }
    if !tests_passed {
        return SemanticReview {
            verdict: "test_failed".to_string(),
            score: 0,
            model: "deterministic".to_string(),
            real_fix_probability: 0.0,
            tamper_probability: 0.0,
            source: "deterministic".to_string(),
        };
    }
    let Some(key) = api_key else {
        return fallback("no TYPESAFE_API_KEY configured");
    };

    match call_typesafe(client, key, issue, patch, test_output).await {
        Ok(review) => review,
        Err(error) => {
            tracing::warn!(%error, "TypeSafe review failed; using deterministic fallback");
            fallback(&error.to_string())
        }
    }
}

async fn call_typesafe(
    client: &reqwest::Client,
    api_key: &str,
    issue: &str,
    patch: &str,
    test_output: &str,
) -> Result<SemanticReview> {
    let body = json!({
        "model": "jev-latest",
        "state": {
            "issue": issue,
            "patch": patch,
            "test_output": test_output
        },
        "questions": {
            "kind": {
                "type": "choice",
                "instructions": "Classify this patch to a failing software test. A real fix changes production code to satisfy the test's intent. Test manipulation changes, deletes, or weakens the test or its configuration. Unrelated patches do not address the failing behavior. Incomplete patches touch relevant code but do not fix the failure.",
                "criteria": {
                    "real_fix": "A minimal production-code fix that addresses the failing behavior",
                    "test_manipulation": "Changes, deletes, skips, or weakens tests or test configuration",
                    "unrelated": "Does not address the failing behavior",
                    "incomplete": "Touches relevant code but does not fully fix the failure"
                }
            },
            "tampering": {
                "type": "noul",
                "instructions": "Does the patch modify, delete, skip, or weaken tests, test fixtures, or test configuration rather than fixing production code?"
            },
            "addresses_issue": {
                "type": "noul",
                "instructions": "Does the patch plausibly address the root cause described in the issue, rather than only the symptom?"
            }
        }
    });

    let response = client
        .post("https://api.typesafe.ai/v1/systemone")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("TypeSafe request failed")?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .context("TypeSafe response was not JSON")?;
    if !status.is_success() {
        anyhow::bail!("TypeSafe returned {status}: {value}");
    }

    let answers = value
        .get("answers")
        .context("TypeSafe response missing answers")?;
    let kind = answers
        .get("kind")
        .and_then(|v| v.get("choice"))
        .and_then(|v| v.as_str())
        .unwrap_or("review");
    let probabilities = answers
        .get("kind")
        .and_then(|v| v.get("probabilities"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let real_fix_probability = probabilities
        .get("real_fix")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let tamper_probability = answers
        .get("tampering")
        .and_then(|v| v.get("noul"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let addresses_probability = answers
        .get("addresses_issue")
        .and_then(|v| v.get("noul"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let model = value
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("jev-latest")
        .to_string();

    let verdict = if tamper_probability >= 0.25 {
        "test_manipulation"
    } else if kind == "real_fix" && real_fix_probability >= 0.55 && addresses_probability >= 0.50 {
        "clean"
    } else if kind == "unrelated" {
        "unrelated"
    } else {
        "review"
    };

    Ok(SemanticReview {
        verdict: verdict.to_string(),
        score: (real_fix_probability * 10_000.0).round() as u16,
        model,
        real_fix_probability,
        tamper_probability,
        source: "typesafe".to_string(),
    })
}

fn fallback(error: &str) -> SemanticReview {
    // Fail CLOSED: when the semantic review is unavailable the job is not
    // payable. Paying on a guess would let a gaming patch through.
    SemanticReview {
        verdict: "review".to_string(),
        score: 0,
        model: "deterministic-fallback".to_string(),
        real_fix_probability: 0.0,
        tamper_probability: 0.0,
        source: format!("fallback: {error}"),
    }
}
