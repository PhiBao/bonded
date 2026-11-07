use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub contract_id: String,
    pub network: String,
    pub buyer_account: String,
    pub buyer_key: String,
    /// Scoped function-call key for agent A (bids + submits). Cannot withdraw.
    pub agent_a_key: String,
    pub agent_a_account: String,
    /// Full-access key for agent A, used ONLY to fund/top up the bond pool.
    pub agent_a_full_key: String,
    pub agent_a_public_key: Option<String>,
    pub agent_b_key: String,
    pub agent_b_account: String,
    pub agent_b_full_key: String,
    pub agent_b_public_key: Option<String>,
    pub evaluator_account: String,
    pub evaluator_key: String,
    pub typesafe_key: Option<String>,
    pub database_url: String,
    pub port: u16,
    /// demo/fixtures/{add,sub,mul}
    pub demo_fixtures: PathBuf,
    /// demo/reference-patches/{preset}-{outcome}.patch
    pub reference_patches: PathBuf,
    /// demo/hidden/{preset}.mjs
    pub hidden_dir: PathBuf,
    pub agent_runtime: String,
    pub opencode_model: String,
    pub explorer_base: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let network = std::env::var("NEAR_NETWORK").unwrap_or_else(|_| "testnet".to_string());
        let credentials_dir = std::env::var("NEAR_CREDENTIALS_DIR").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{home}/.near-credentials/{network}")
        });
        let contract_id =
            std::env::var("CONTRACT_ID").unwrap_or_else(|_| "bonded-v2.testnet".to_string());
        let buyer_account =
            std::env::var("BUYER_ACCOUNT").unwrap_or_else(|_| format!("buyer.{contract_id}"));
        let agent_a_account =
            std::env::var("AGENT_A_ACCOUNT").unwrap_or_else(|_| format!("ag-a.{contract_id}"));
        let agent_b_account =
            std::env::var("AGENT_B_ACCOUNT").unwrap_or_else(|_| format!("agent-v2-6659.testnet"));
        let evaluator_account = std::env::var("EVALUATOR_ACCOUNT")
            .unwrap_or_else(|_| format!("evaluator.{contract_id}"));

        let buyer_key = load_key("BUYER_PRIVATE_KEY", &credentials_dir, &buyer_account)?;
        // Scoped keys come from env (the API never needs the agent's full key
        // for bids); fall back to the credentials file for local development.
        let agent_a_key = load_key("AGENT_A_PRIVATE_KEY", &credentials_dir, &agent_a_account)?;
        let agent_b_key = load_key("AGENT_B_PRIVATE_KEY", &credentials_dir, &agent_b_account)?;
        // Full keys are only ever used to top up bond pools (deposit_bond).
        let agent_a_full_key = load_key_opt("AGENT_A_FULL_KEY", &credentials_dir, &agent_a_account)
            .unwrap_or_else(|| agent_a_key.clone());
        let agent_b_full_key = load_key_opt("AGENT_B_FULL_KEY", &credentials_dir, &agent_b_account)
            .unwrap_or_else(|| agent_b_key.clone());
        let evaluator_key = load_key(
            "EVALUATOR_PRIVATE_KEY",
            &credentials_dir,
            &evaluator_account,
        )?;

        let demo_root = std::env::var("DEMO_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo"));

        let agent_runtime = std::env::var("AGENT_RUNTIME").unwrap_or_else(|_| {
            if command_exists("opencode") {
                "opencode".to_string()
            } else {
                "reference".to_string()
            }
        });

        Ok(Self {
            contract_id: contract_id.clone(),
            network,
            buyer_account,
            buyer_key,
            agent_a_key,
            agent_a_account,
            agent_a_full_key,
            agent_a_public_key: std::env::var("AGENT_A_PUBLIC_KEY").ok(),
            agent_b_key,
            agent_b_account,
            agent_b_full_key,
            agent_b_public_key: std::env::var("AGENT_B_PUBLIC_KEY").ok(),
            evaluator_account,
            evaluator_key,
            typesafe_key: std::env::var("TYPESAFE_API_KEY").ok(),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/bonded.db?mode=rwc".to_string()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8787),
            demo_fixtures: demo_root.join("fixtures"),
            reference_patches: demo_root.join("reference-patches"),
            hidden_dir: demo_root.join("hidden"),
            agent_runtime,
            opencode_model: std::env::var("OPENCODE_MODEL")
                .unwrap_or_else(|_| "opencode-go/deepseek-v4.1-flash".to_string()),
            explorer_base: std::env::var("EXPLORER_BASE")
                .unwrap_or_else(|_| "https://testnet.nearblocks.io".to_string()),
        })
    }

    pub fn explorer_tx(&self, hash: &str) -> String {
        format!("{}/txns/{}", self.explorer_base, hash)
    }
}

fn command_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn load_key(env_name: &str, credentials_dir: &str, account: &str) -> Result<String> {
    load_key_opt(env_name, credentials_dir, account)
        .with_context(|| format!("no key for {account}; set {env_name} or create credentials"))
}

fn load_key_opt(env_name: &str, credentials_dir: &str, account: &str) -> Option<String> {
    if let Ok(key) = std::env::var(env_name) {
        if !key.trim().is_empty() {
            return Some(key);
        }
    }
    let path = PathBuf::from(credentials_dir).join(format!("{account}.json"));
    let raw = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("private_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
