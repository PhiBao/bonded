mod config;
mod db;
mod evaluator;
mod handlers;
mod near_client;
mod types;
mod typesafe;

use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::handlers::{RunRegistry, RunState};
use crate::near_client::NearClient;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub near: NearClient,
    pub db: SqlitePool,
    pub http: reqwest::Client,
    pub demo_lock: Arc<Mutex<()>>,
    pub runs: RunRegistry,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bonded_api=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    let near = NearClient::new(&config)?;
    let db = db::connect(&config.database_url).await?;
    db::seed_agents(&db, &config.agent_a_account, &config.agent_b_account).await?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;

    let state = AppState {
        config: config.clone(),
        near,
        db,
        http,
        demo_lock: Arc::new(Mutex::new(())),
        runs: Arc::new(RwLock::new(HashMap::<String, RunState>::new())),
    };

    let app = Router::new()
        .route("/api/health", get(handlers::health))
        .route("/api/config", get(handlers::config))
        .route("/api/stats", get(handlers::stats))
        .route("/api/balances", get(handlers::balances))
        .route(
            "/api/agents",
            get(handlers::list_agents).post(handlers::register_agent),
        )
        .route("/api/agents/{id}/key", get(handlers::agent_key))
        .route(
            "/api/jobs",
            get(handlers::list_jobs).post(handlers::create_job),
        )
        .route("/api/jobs/{id}", get(handlers::get_job))
        .route("/api/jobs/{id}/bid", post(handlers::bid))
        .route("/api/jobs/{id}/hire", post(handlers::hire))
        .route("/api/jobs/{id}/submit", post(handlers::submit))
        .route("/api/jobs/{id}/evaluate", post(handlers::evaluate))
        .route("/api/jobs/{id}/settle", post(handlers::settle))
        .route("/api/jobs/{id}/run", post(handlers::run_demo))
        .route("/api/jobs/{id}/artifacts", get(handlers::artifacts))
        .route("/api/jobs/{id}/rerun", post(handlers::rerun))
        .route("/api/demo/seed", post(handlers::seed_demo))
        .route("/api/demo/run", post(handlers::run_live_demo))
        .route("/api/runs/{id}", get(handlers::get_run))
        .route("/api/receipts/{id}", get(handlers::get_job))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await?;
    tracing::info!(
        port = config.port,
        contract = %config.contract_id,
        runtime = %config.agent_runtime,
        "Bonded API listening"
    );
    axum::serve(listener, app).await?;
    Ok(())
}
