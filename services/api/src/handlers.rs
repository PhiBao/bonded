use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::db;
use crate::evaluator::{self, preset_of};
use crate::near_client::near_to_yocto;
use crate::types::*;
use crate::typesafe;
use crate::AppState;

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------
// Async demo runs. POST returns 202 with a run_id; GET polls progress steps.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RunState {
    pub run_id: String,
    pub status: String,
    pub steps: Vec<RunStep>,
    pub error: Option<String>,
    pub result: Option<RunDemoResp>,
}

pub type RunRegistry = Arc<RwLock<HashMap<String, RunState>>>;

async fn set_step(registry: &RunRegistry, run_id: &str, step: RunStep) {
    let mut runs = registry.write().await;
    if let Some(run) = runs.get_mut(run_id) {
        run.steps.push(step);
    }
}

async fn finish_run(
    registry: &RunRegistry,
    run_id: &str,
    status: &str,
    error: Option<String>,
    result: Option<RunDemoResp>,
) {
    let mut runs = registry.write().await;
    if let Some(run) = runs.get_mut(run_id) {
        run.status = status.to_string();
        run.error = error;
        run.result = result;
    }
}

fn step(name: &str, status: &str, detail: Option<String>, tx_hash: Option<String>) -> RunStep {
    RunStep {
        name: name.to_string(),
        status: status.to_string(),
        detail,
        tx_hash,
    }
}

pub async fn health(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let config = state.near.config().await.map_err(ApiError::internal)?;
    Ok(Json(json!({
        "ok": true,
        "network": state.config.network,
        "contract_id": state.config.contract_id,
        "owner": config.0,
        "evaluator": config.1,
        "agent_runtime": state.config.agent_runtime
    })))
}

pub async fn config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "network": state.config.network,
        "contract_id": state.config.contract_id,
        "explorer_base": state.config.explorer_base,
        "buyer": state.config.buyer_account,
        "agents": {
            "forge": state.config.agent_a_account,
            "sable": state.config.agent_b_account
        },
        "agent_runtime": state.config.agent_runtime,
    }))
}

pub async fn list_jobs(State(state): State<AppState>) -> ApiResult<Json<Vec<JobView>>> {
    let mut jobs = state.near.jobs().await.map_err(ApiError::internal)?;
    jobs.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    let mut views = Vec::with_capacity(jobs.len());
    for job in jobs {
        views.push(job_view(&state, job).await?);
    }
    Ok(Json(views))
}

pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<JobView>> {
    let job = load_job(&state, &job_id).await?;
    Ok(Json(job_view(&state, job).await?))
}

pub async fn list_agents(State(state): State<AppState>) -> ApiResult<Json<Vec<AgentView>>> {
    let agents = db::list_agents(&state.db)
        .await
        .map_err(ApiError::internal)?;
    let mut views = Vec::new();
    for agent in agents {
        views.push(agent_view(&state, agent).await?);
    }
    Ok(Json(views))
}

pub async fn register_agent(
    State(state): State<AppState>,
    Json(req): Json<RegisterAgentReq>,
) -> ApiResult<Json<AgentView>> {
    if req.id.trim().is_empty() || req.account_id.trim().is_empty() {
        return Err(ApiError::bad("id and account_id are required"));
    }
    let created = db::register_agent(
        &state.db,
        req.id.trim(),
        req.name.trim(),
        req.account_id.trim(),
        &req.description,
        &req.runtime,
    )
    .await
    .map_err(ApiError::internal)?;
    if !created {
        return Err(ApiError::bad("agent id already registered"));
    }
    let agent = db::get_agent(&state.db, req.id.trim())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("registered agent not found"))?;
    Ok(Json(agent_view(&state, agent).await?))
}

pub async fn agent_key(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> ApiResult<Json<KeyScopeView>> {
    let (account, public_key) = match agent_id.as_str() {
        "forge" => (
            state.config.agent_a_account.clone(),
            state
                .config
                .agent_a_public_key
                .clone()
                .ok_or_else(|| ApiError::bad("scoped public key not configured"))?,
        ),
        "sable" => (
            state.config.agent_b_account.clone(),
            state
                .config
                .agent_b_public_key
                .clone()
                .ok_or_else(|| ApiError::bad("scoped public key not configured"))?,
        ),
        _ => return Err(ApiError::not_found("unknown agent")),
    };
    let permission = state
        .near
        .access_key(&account, &public_key)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(KeyScopeView {
        account,
        public_key,
        permission: permission.get("permission").cloned().unwrap_or(permission),
    }))
}

pub async fn stats(State(state): State<AppState>) -> ApiResult<Json<StatsView>> {
    let jobs = state.near.jobs().await.map_err(ApiError::internal)?;
    let agents = db::list_agents(&state.db)
        .await
        .map_err(ApiError::internal)?;
    let mut total_budget: u128 = 0;
    let mut total_locked: u128 = 0;
    let mut total_slashed: u128 = 0;
    let mut total_paid: u128 = 0;
    let mut open = 0;
    let mut settled = 0;
    for job in &jobs {
        total_budget += job.budget_yocto();
        if job.state == "Open" || job.state == "Hired" || job.state == "Submitted" {
            open += 1;
        }
        if job.state == "Settled" {
            settled += 1;
            if let Some(eval) = &job.evaluation {
                if eval.passed && eval.semantic_verdict == "clean" {
                    total_paid += job.selected_price_yocto();
                }
            }
        }
    }
    for agent in &agents {
        let stats = state
            .near
            .agent(&agent.account_id)
            .await
            .unwrap_or_default();
        total_locked += stats.bond_locked.parse().unwrap_or(0);
        total_slashed += stats.bond_slashed.parse().unwrap_or(0);
    }
    Ok(Json(StatsView {
        jobs: jobs.len(),
        open_jobs: open,
        settled_jobs: settled,
        total_budget_yocto: total_budget.to_string(),
        total_bonds_locked_yocto: total_locked.to_string(),
        total_slashed_yocto: total_slashed.to_string(),
        total_paid_yocto: total_paid.to_string(),
    }))
}

pub async fn balances(State(state): State<AppState>) -> ApiResult<Json<Vec<BalanceView>>> {
    let mut views = Vec::new();
    let contract_pool = state
        .near
        .pool(&state.config.agent_a_account)
        .await
        .map(|_| ());

    let rows = [
        (
            state.config.contract_id.clone(),
            "contract".to_string(),
            None::<String>,
        ),
        (
            state.config.buyer_account.clone(),
            "buyer".to_string(),
            None,
        ),
        (
            state.config.agent_a_account.clone(),
            "agent:a".to_string(),
            Some("forge".to_string()),
        ),
        (
            state.config.agent_b_account.clone(),
            "agent:b".to_string(),
            Some("sable".to_string()),
        ),
        (
            state.config.evaluator_account.clone(),
            "evaluator".to_string(),
            None,
        ),
    ];
    let _ = contract_pool;

    for (account, role, agent_id) in rows {
        let balance = state.near.balance(&account).await.map(|b| b.to_string());
        let (pool, locked) = if let Some(_id) = &agent_id {
            match state.near.pool(&account).await {
                Ok(p) => (Some(p.balance), Some(p.locked)),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };
        views.push(BalanceView {
            account,
            role,
            balance_yocto: balance.ok(),
            pool_yocto: pool,
            locked_yocto: locked,
        });
    }
    Ok(Json(views))
}

pub async fn create_job(
    State(state): State<AppState>,
    Json(req): Json<CreateJobReq>,
) -> ApiResult<Json<JobView>> {
    if req.budget_near <= 0.0 {
        return Err(ApiError::bad("budget must be greater than zero"));
    }
    if req.bond_bps == 0 || req.bond_bps > 5_000 {
        return Err(ApiError::bad("bond_bps must be between 1 and 5000"));
    }
    if req.deadline_secs < 60 {
        return Err(ApiError::bad("deadline must be at least 60 seconds"));
    }
    let budget = near_to_yocto(req.budget_near);
    let (tx, job) = state
        .near
        .create_job(
            &req.title,
            &req.repo_url,
            &req.commit,
            &req.test_command,
            budget,
            req.bond_bps,
            req.deadline_secs,
        )
        .await
        .map_err(ApiError::internal)?;
    db::record_event(&state.db, &job.id, "created", Some(&tx), Some(&req.title))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(job_view(&state, job).await?))
}

pub async fn bid(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(req): Json<BidReq>,
) -> ApiResult<Json<JobView>> {
    let agent = db::get_agent(&state.db, &req.agent_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    if req.price_near <= 0.0 || req.bond_near <= 0.0 {
        return Err(ApiError::bad("price and bond must be greater than zero"));
    }
    let tx = state
        .near
        .bid(
            &agent.account_id,
            &job_id,
            near_to_yocto(req.price_near),
            near_to_yocto(req.bond_near),
        )
        .await
        .map_err(ApiError::internal)?;
    db::record_event(
        &state.db,
        &job_id,
        "bid",
        Some(&tx),
        Some(&format!("{} bid {:.2} NEAR", agent.name, req.price_near)),
    )
    .await
    .map_err(ApiError::internal)?;
    let job = load_job(&state, &job_id).await?;
    Ok(Json(job_view(&state, job).await?))
}

pub async fn hire(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(req): Json<HireReq>,
) -> ApiResult<Json<JobView>> {
    let agent = db::get_agent(&state.db, &req.agent_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    let tx = state
        .near
        .hire(&job_id, &agent.account_id)
        .await
        .map_err(ApiError::internal)?;
    db::record_event(
        &state.db,
        &job_id,
        "hired",
        Some(&tx),
        Some(&format!("{} hired", agent.name)),
    )
    .await
    .map_err(ApiError::internal)?;
    let job = load_job(&state, &job_id).await?;
    Ok(Json(job_view(&state, job).await?))
}

pub async fn submit(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(req): Json<SubmitReq>,
) -> ApiResult<Json<JobView>> {
    let agent = db::get_agent(&state.db, &req.agent_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    let tx = state
        .near
        .submit_patch(&agent.account_id, &job_id, &req.patch_hash)
        .await
        .map_err(ApiError::internal)?;
    db::record_event(
        &state.db,
        &job_id,
        "submitted",
        Some(&tx),
        Some(&req.patch_hash),
    )
    .await
    .map_err(ApiError::internal)?;
    let job = load_job(&state, &job_id).await?;
    Ok(Json(job_view(&state, job).await?))
}

pub async fn evaluate(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(req): Json<EvaluateReq>,
) -> ApiResult<Json<JobView>> {
    let tx = state
        .near
        .evaluate(
            &job_id,
            req.passed,
            &req.logs_hash,
            &req.semantic_verdict,
            req.semantic_score,
        )
        .await
        .map_err(ApiError::internal)?;
    db::record_event(
        &state.db,
        &job_id,
        "evaluated",
        Some(&tx),
        Some(&format!(
            "passed={} verdict={}",
            req.passed, req.semantic_verdict
        )),
    )
    .await
    .map_err(ApiError::internal)?;
    let job = load_job(&state, &job_id).await?;
    Ok(Json(job_view(&state, job).await?))
}

pub async fn settle(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<JobView>> {
    let tx = state
        .near
        .settle(&job_id)
        .await
        .map_err(ApiError::internal)?;
    db::record_event(&state.db, &job_id, "settled", Some(&tx), None)
        .await
        .map_err(ApiError::internal)?;
    let job = wait_for_state(&state, &job_id, "Settled").await?;
    Ok(Json(job_view(&state, job).await?))
}

/// Patch + logs + independent re-run for one job, if evaluated.
pub async fn artifacts(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<ArtifactsView>> {
    let job = load_job(&state, &job_id).await?;
    let hash = job.patch_hash.clone();
    let Some(hash) = hash else {
        return Ok(Json(ArtifactsView {
            patch: None,
            patch_hash: None,
            logs: None,
            logs_hash: None,
            verdict: job.evaluation.as_ref().map(|e| e.semantic_verdict.clone()),
            score: job.evaluation.as_ref().map(|e| e.semantic_score),
            rerun: None,
        }));
    };
    let record = db::get_patch_record(&state.db, &hash)
        .await
        .map_err(ApiError::internal)?;
    let Some((content, logs, logs_hash, rerun_hash)) = record else {
        return Ok(Json(ArtifactsView {
            patch: None,
            patch_hash: Some(hash),
            logs: None,
            logs_hash: None,
            verdict: job.evaluation.as_ref().map(|e| e.semantic_verdict.clone()),
            score: job.evaluation.as_ref().map(|e| e.semantic_score),
            rerun: None,
        }));
    };
    let rerun = rerun_hash.map(|h| RerunView {
        passed: job.evaluation.as_ref().map(|e| e.passed).unwrap_or(false),
        logs_hash: h.clone(),
        log_hashes_match: logs_hash.as_deref() == Some(h.as_str()),
    });
    Ok(Json(ArtifactsView {
        patch: Some(content),
        patch_hash: Some(hash),
        logs,
        logs_hash,
        verdict: job.evaluation.as_ref().map(|e| e.semantic_verdict.clone()),
        score: job.evaluation.as_ref().map(|e| e.semantic_score),
        rerun,
    }))
}

/// Re-run the stored patch against the job's repo and compare log hashes.
/// Read-only: never touches the chain.
pub async fn rerun(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<RerunView>> {
    let job = load_job(&state, &job_id).await?;
    let hash = job
        .patch_hash
        .clone()
        .ok_or_else(|| ApiError::bad("no patch submitted yet"))?;
    let patch = db::get_patch(&state.db, &hash)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad("patch content not stored"))?;
    let eval = evaluator::evaluate_patch(
        &state.config.demo_fixtures,
        &state.config.hidden_dir,
        &job.repo_url,
        &job.commit,
        &patch,
        &job.test_command,
    )
    .await
    .map_err(ApiError::internal)?;
    db::save_rerun(&state.db, &hash, &eval.logs, &eval.logs_hash)
        .await
        .map_err(ApiError::internal)?;
    let record = db::get_patch_record(&state.db, &hash)
        .await
        .map_err(ApiError::internal)?;
    let match_ok = record
        .as_ref()
        .and_then(|(_, _, logs_hash, _)| logs_hash.as_deref())
        .map(|h| h == eval.logs_hash)
        .unwrap_or(false);
    Ok(Json(RerunView {
        passed: eval.passed,
        logs_hash: eval.logs_hash,
        log_hashes_match: match_ok,
    }))
}

pub async fn seed_demo(State(state): State<AppState>) -> ApiResult<Json<JobView>> {
    let _guard = state.demo_lock.clone().lock_owned().await;
    Ok(Json(seed_demo_inner(&state).await?))
}

async fn seed_demo_inner(state: &AppState) -> ApiResult<JobView> {
    let jobs = state.near.jobs().await.map_err(ApiError::internal)?;
    if let Some(job) = jobs
        .into_iter()
        .rev()
        .find(|j| j.state == "Open" && j.repo_url.starts_with("preset:"))
    {
        let job = ensure_demo_bids(state, job).await?;
        return Ok(job_view(state, job).await?);
    }

    let preset = "add";
    let budget = near_to_yocto(0.5);
    let bond = near_to_yocto(0.1);
    tracing::info!("seed: creating job");
    let (tx, job) = state
        .near
        .create_job(
            "Fix the failing test in bonded-fixture",
            &format!("preset:{preset}"),
            "demo",
            "npm test",
            budget,
            2_000,
            3_600,
        )
        .await
        .map_err(ApiError::internal)?;
    tracing::info!(job = %job.id, "seed: job created");
    db::record_event(
        &state.db,
        &job.id,
        "created",
        Some(&tx),
        Some("Seeded demo job"),
    )
    .await
    .map_err(ApiError::internal)?;

    let job = load_job_retry(state, &job.id).await?;
    let job = ensure_demo_bids(state, job).await?;
    Ok(job_view(state, job).await?)
}

pub async fn run_demo(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(req): Json<RunDemoReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let runs = state.runs.clone();
    runs.write().await.insert(
        run_id.clone(),
        RunState {
            run_id: run_id.clone(),
            status: "queued".to_string(),
            steps: Vec::new(),
            error: None,
            result: None,
        },
    );
    let guard = state.demo_lock.clone().lock_owned().await;
    let job_id = job_id.clone();
    let outcome = req.outcome.clone();
    let response_id = run_id.clone();
    spawn_run(state, run_id, job_id, Some(outcome), None, guard);
    Ok(Json(json!({ "run_id": response_id, "status": "queued" })))
}

pub async fn run_live_demo(
    State(state): State<AppState>,
    Json(req): Json<RunDemoReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let runs = state.runs.clone();
    runs.write().await.insert(
        run_id.clone(),
        RunState {
            run_id: run_id.clone(),
            status: "queued".to_string(),
            steps: Vec::new(),
            error: None,
            result: None,
        },
    );
    let guard = state.demo_lock.clone().lock_owned().await;
    let outcome = req.outcome.clone();
    let response_id = run_id.clone();
    spawn_run(state, run_id, String::new(), Some(outcome), Some(()), guard);
    Ok(Json(json!({ "run_id": response_id, "status": "queued" })))
}

fn spawn_run(
    state: AppState,
    run_id: String,
    job_id: String,
    outcome: Option<String>,
    seed_first: Option<()>,
    guard: tokio::sync::OwnedMutexGuard<()>,
) {
    let registry = state.runs.clone();
    tokio::spawn(async move {
        let _guard = guard;
        let _ = seed_first;
        // The actual async work runs in run_job; errors are captured, not lost.
        let outcome_str = outcome.unwrap_or_else(|| "success".to_string());
        let job_id = job_id.clone();
        let result = if seed_first.is_some() {
            run_live_inner(&state, &registry, &run_id, &outcome_str).await
        } else {
            run_job(&state, &registry, &run_id, &job_id, &outcome_str).await
        };
        match result {
            Ok(resp) => {
                finish_run(&registry, &run_id, "complete", None, Some(resp)).await;
            }
            Err(e) => {
                finish_run(&registry, &run_id, "failed", Some(e.message), None).await;
            }
        }
    });
}

pub async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> ApiResult<Json<RunView>> {
    let runs = state.runs.read().await;
    let run = runs
        .get(&run_id)
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    Ok(Json(RunView {
        run_id: run.run_id.clone(),
        status: run.status.clone(),
        steps: run.steps.clone(),
        error: run.error.clone(),
        result: run.result.clone(),
    }))
}

async fn run_live_inner(
    state: &AppState,
    registry: &RunRegistry,
    run_id: &str,
    outcome: &str,
) -> ApiResult<RunDemoResp> {
    set_step(
        registry,
        run_id,
        step(
            "seed",
            "running",
            Some("creating job and locking bonds".into()),
            None,
        ),
    )
    .await;
    let seeded = seed_demo_inner(state).await?;
    set_step(
        registry,
        run_id,
        step(
            "seed",
            "done",
            Some(format!(
                "{} · {} bids",
                seeded.job.id,
                seeded.job.bids.len()
            )),
            None,
        ),
    )
    .await;
    run_job(state, registry, run_id, &seeded.job.id, outcome).await
}

async fn run_job(
    state: &AppState,
    registry: &RunRegistry,
    run_id: &str,
    job_id: &str,
    outcome: &str,
) -> ApiResult<RunDemoResp> {
    let started = Instant::now();
    let job = load_job_retry(state, job_id).await?;
    if job.state != "Open" && job.state != "Hired" {
        return Err(ApiError::bad(format!(
            "job {} is {}, seed a new demo job",
            job.id, job.state
        )));
    }
    let outcome = match outcome {
        "failure" => "failure",
        "cheat" => "cheat",
        "tamper" => "tamper",
        _ => "success",
    };
    let preset = preset_of(&job.repo_url).ok_or_else(|| {
        ApiError::bad("reference outcomes require a preset repo; this job uses a custom repo")
    })?;
    let agent_id = if outcome == "success" {
        "forge"
    } else {
        "sable"
    };
    let agent = db::get_agent(&state.db, agent_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    if job.state == "Hired" && job.selected_agent.as_deref() != Some(agent.account_id.as_str()) {
        return Err(ApiError::bad(
            "the hired agent does not match this demo path",
        ));
    }

    if job.state == "Open" {
        let tx = state
            .near
            .hire(&job.id, &agent.account_id)
            .await
            .map_err(ApiError::internal)?;
        db::record_event(
            &state.db,
            &job.id,
            "hired",
            Some(&tx),
            Some(&format!("{} hired for demo", agent.name)),
        )
        .await
        .map_err(ApiError::internal)?;
        set_step(
            registry,
            run_id,
            step(
                "hire",
                "done",
                Some(format!("{} hired", agent.name)),
                Some(tx),
            ),
        )
        .await;
    }

    // The agent: live runtime for the success path, deterministic otherwise.
    let use_live = state.config.agent_runtime == "opencode" && outcome == "success";
    let run = if use_live {
        let issue = evaluator::issue_text(&state.config.demo_fixtures, &preset, &job.title);
        match evaluator::opencode_agent(
            &state.config.demo_fixtures,
            &preset,
            &issue,
            &job.test_command,
            &state.config.opencode_model,
        )
        .await
        {
            Ok(run) => run,
            Err(error) => {
                tracing::warn!(%error, "live agent failed; falling back to the reference agent");
                let mut fallback =
                    evaluator::reference_agent(&state.config.reference_patches, &preset, outcome)
                        .map_err(ApiError::internal)?;
                fallback.view.runtime = "reference (live agent fallback)".to_string();
                fallback
            }
        }
    } else {
        evaluator::reference_agent(&state.config.reference_patches, &preset, outcome)
            .map_err(ApiError::internal)?
    };
    set_step(
        registry,
        run_id,
        step(
            "agent",
            "done",
            Some(format!(
                "{} · {}ms · {}…",
                run.view.runtime,
                run.view.duration_ms,
                &run.view.patch_hash[..18.min(run.view.patch_hash.len())]
            )),
            None,
        ),
    )
    .await;

    db::save_patch(&state.db, &run.view.patch_hash, &run.patch)
        .await
        .map_err(ApiError::internal)?;

    let eval = evaluator::evaluate_patch(
        &state.config.demo_fixtures,
        &state.config.hidden_dir,
        &job.repo_url,
        &job.commit,
        &run.patch,
        &job.test_command,
    )
    .await
    .map_err(ApiError::internal)?;
    db::save_patch_logs(&state.db, &run.view.patch_hash, &eval.logs, &eval.logs_hash)
        .await
        .map_err(ApiError::internal)?;
    set_step(
        registry,
        run_id,
        step(
            "test",
            "done",
            Some(format!(
                "passed={} · tampered={} · {}ms",
                eval.passed, eval.tamper_check, eval.duration_ms
            )),
            None,
        ),
    )
    .await;

    let review = typesafe::review(
        &state.http,
        state.config.typesafe_key.as_deref(),
        &job.title,
        &run.patch,
        &eval.logs,
        eval.passed,
        eval.tamper_check,
    )
    .await;
    set_step(
        registry,
        run_id,
        step(
            "review",
            "done",
            Some(format!(
                "{} · {} · {}",
                review.verdict, review.score, review.source
            )),
            None,
        ),
    )
    .await;

    let tx = state
        .near
        .submit_patch(&agent.account_id, &job.id, &run.view.patch_hash)
        .await
        .map_err(ApiError::internal)?;
    let mut receipts = vec![TxReceipt {
        kind: "submitted".to_string(),
        tx_hash: tx.clone(),
        explorer_url: state.config.explorer_tx(&tx),
    }];
    db::record_event(
        &state.db,
        &job.id,
        "submitted",
        Some(&tx),
        Some(&run.view.patch_hash),
    )
    .await
    .map_err(ApiError::internal)?;
    set_step(
        registry,
        run_id,
        step("submit", "done", None, Some(tx.clone())),
    )
    .await;

    let tx = state
        .near
        .evaluate(
            &job.id,
            eval.passed,
            &eval.logs_hash,
            &review.verdict,
            review.score,
        )
        .await
        .map_err(ApiError::internal)?;
    receipts.push(TxReceipt {
        kind: "evaluated".to_string(),
        tx_hash: tx.clone(),
        explorer_url: state.config.explorer_tx(&tx),
    });
    db::record_event(
        &state.db,
        &job.id,
        "evaluated",
        Some(&tx),
        Some(&format!(
            "passed={} verdict={} score={} model={}",
            eval.passed, review.verdict, review.score, review.model
        )),
    )
    .await
    .map_err(ApiError::internal)?;
    set_step(
        registry,
        run_id,
        step("evaluate", "done", None, Some(tx.clone())),
    )
    .await;

    match state.near.settle(&job.id).await {
        Ok(tx) => {
            receipts.push(TxReceipt {
                kind: "settled".to_string(),
                tx_hash: tx.clone(),
                explorer_url: state.config.explorer_tx(&tx),
            });
            db::record_event(&state.db, &job.id, "settled", Some(&tx), None)
                .await
                .map_err(ApiError::internal)?;
            set_step(registry, run_id, step("settle", "done", None, Some(tx))).await;
        }
        Err(error) => {
            // A duplicate settle after a first success is fine.
            let current = load_job(state, &job.id).await?;
            if current.state != "Settled" {
                return Err(ApiError::internal(error));
            }
            set_step(
                registry,
                run_id,
                step("settle", "done", Some("already settled".into()), None),
            )
            .await;
        }
    }

    let job = wait_for_state(state, &job.id, "Settled").await?;
    let evaluation = evaluator::evaluation_view(&eval, &review, &job.test_command);
    tracing::info!(job = %job.id, ms = started.elapsed().as_millis(), "demo run complete");
    Ok(RunDemoResp {
        job: job_view(state, job).await?,
        agent_run: run.view,
        evaluation,
        receipts,
    })
}

async fn wait_for_bids(state: &AppState, job_id: &str, expected: usize) -> ApiResult<Job> {
    let mut job = load_job(state, job_id).await?;
    for _ in 0..12 {
        if job.bids.len() >= expected {
            return Ok(job);
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        job = load_job(state, job_id).await?;
    }
    Ok(job)
}

async fn wait_for_state(state: &AppState, job_id: &str, expected: &str) -> ApiResult<Job> {
    let mut last = None;
    for _ in 0..12 {
        let job = load_job(state, job_id).await?;
        if job.state == expected {
            return Ok(job);
        }
        last = Some(job.state.clone());
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    Err(ApiError::internal(format!(
        "job did not reach {expected}; last state: {last:?}",
    )))
}

async fn load_job(state: &AppState, job_id: &str) -> ApiResult<Job> {
    state
        .near
        .job(job_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("job not found"))
}

/// load_job with view-RPC lag tolerance (freshly created jobs may not be
/// visible to the read client for a block or two).
async fn load_job_retry(state: &AppState, job_id: &str) -> ApiResult<Job> {
    for attempt in 0..10 {
        match load_job(state, job_id).await {
            Ok(job) => return Ok(job),
            Err(_) if attempt < 9 => {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
            Err(e) => return Err(e),
        }
    }
    load_job(state, job_id).await
}

async fn ensure_demo_bids(state: &AppState, job: Job) -> ApiResult<Job> {
    let job = load_job_retry(state, &job.id).await?;
    if job.bids.len() >= 2 {
        return Ok(job);
    }
    let forge = db::get_agent(&state.db, "forge")
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("forge agent missing"))?;
    let sable = db::get_agent(&state.db, "sable")
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("sable agent missing"))?;
    let budget = job.budget_yocto();
    let bond = job.bond_required_yocto();

    // Bonded capital is pooled: top each demo agent's pool up to at least the
    // required bond before bidding (deposits use the full-access key).
    for agent in [&forge, &sable] {
        let pool = state
            .near
            .pool(&agent.account_id)
            .await
            .map_err(ApiError::internal)?;
        let free =
            pool.balance.parse::<u128>().unwrap_or(0) - pool.locked.parse::<u128>().unwrap_or(0);
        if free < bond {
            let top_up = bond - free + near_to_yocto(0.1);
            tracing::info!(agent = %agent.account_id, "seed: topping up bond pool");
            let tx = state
                .near
                .deposit_bond(&agent.account_id, top_up)
                .await
                .map_err(|e| {
                    ApiError::internal(format!(
                        "bond pool for {} is short; run scripts/refill.sh — {e}",
                        agent.account_id
                    ))
                })?;
            db::record_event(
                &state.db,
                &job.id,
                "pool_topup",
                Some(&tx),
                Some(&format!("{} bond pool top-up", agent.name)),
            )
            .await
            .map_err(ApiError::internal)?;
        }
    }

    for (agent, price, label) in [
        (&forge, budget, "Forge bid 0.50 NEAR · 0.10 NEAR bond"),
        (
            &sable,
            near_to_yocto(0.45),
            "Sable bid 0.45 NEAR · 0.10 NEAR bond",
        ),
    ] {
        let refreshed = load_job(state, &job.id).await?;
        if refreshed
            .bids
            .iter()
            .any(|bid| bid.agent == agent.account_id)
        {
            continue;
        }
        match state
            .near
            .bid(&agent.account_id, &job.id, price, bond)
            .await
        {
            Ok(tx) => {
                db::record_event(&state.db, &job.id, "bid", Some(&tx), Some(label))
                    .await
                    .map_err(ApiError::internal)?;
            }
            Err(error) => {
                let refreshed = load_job(state, &job.id).await?;
                if !refreshed
                    .bids
                    .iter()
                    .any(|bid| bid.agent == agent.account_id)
                {
                    return Err(ApiError::internal(error));
                }
            }
        }
    }
    wait_for_bids(state, &job.id, 2).await
}

async fn agent_view(state: &AppState, agent: crate::types::AgentRecord) -> ApiResult<AgentView> {
    let stats = state
        .near
        .agent(&agent.account_id)
        .await
        .unwrap_or_default();
    Ok(AgentView {
        id: agent.id,
        name: agent.name,
        account_id: agent.account_id,
        description: agent.description,
        runtime: agent.runtime,
        stats,
    })
}

async fn job_view(state: &AppState, job: Job) -> ApiResult<JobView> {
    let events = db::events_for_job(&state.db, &job.id)
        .await
        .map_err(ApiError::internal)?;
    let patch = if let Some(hash) = &job.patch_hash {
        db::get_patch(&state.db, hash)
            .await
            .map_err(ApiError::internal)?
    } else {
        None
    };
    let agents = db::list_agents(&state.db)
        .await
        .map_err(ApiError::internal)?;
    let mut agent_views = Vec::new();
    for agent in agents {
        agent_views.push(agent_view(state, agent).await?);
    }
    Ok(JobView {
        job,
        events,
        agents: agent_views,
        patch,
    })
}
