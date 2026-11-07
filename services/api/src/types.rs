use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bid {
    pub agent: String,
    pub price: String,
    pub bond: String,
    /// True once the locked bond left the pool (settled, slashed, or refunded).
    pub released: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub passed: bool,
    pub logs_hash: String,
    pub semantic_verdict: String,
    pub semantic_score: u16,
    pub evaluated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub buyer: String,
    pub title: String,
    pub repo_url: String,
    pub commit: String,
    pub test_command: String,
    pub budget: String,
    pub bond_bps: u16,
    pub state: String,
    pub bids: Vec<Bid>,
    pub selected_agent: Option<String>,
    pub selected_price: String,
    pub selected_bond: String,
    pub patch_hash: Option<String>,
    pub evaluation: Option<Evaluation>,
    pub created_at: u64,
    pub deadline_at: u64,
    pub settled: bool,
}

impl Job {
    pub fn budget_yocto(&self) -> u128 {
        self.budget.parse().unwrap_or(0)
    }
    pub fn bond_required_yocto(&self) -> u128 {
        self.budget_yocto()
            .saturating_mul(self.bond_bps as u128)
            .saturating_add(9_999)
            / 10_000
    }
    pub fn selected_bond_yocto(&self) -> u128 {
        self.selected_bond.parse().unwrap_or(0)
    }
    pub fn selected_price_yocto(&self) -> u128 {
        self.selected_price.parse().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStats {
    pub completed: u64,
    pub failed: u64,
    pub bond_slashed: String,
    pub bond_returned: String,
    pub active_jobs: u64,
    pub bond_pool: String,
    pub bond_locked: String,
}

impl Default for AgentStats {
    fn default() -> Self {
        Self {
            completed: 0,
            failed: 0,
            bond_slashed: "0".to_string(),
            bond_returned: "0".to_string(),
            active_jobs: 0,
            bond_pool: "0".to_string(),
            bond_locked: "0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPool {
    pub balance: String,
    pub locked: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: String,
    pub name: String,
    pub account_id: String,
    pub description: String,
    pub runtime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: i64,
    pub job_id: String,
    pub kind: String,
    pub tx_hash: Option<String>,
    pub detail: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TxReceipt {
    pub kind: String,
    pub tx_hash: String,
    pub explorer_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub job: Job,
    pub events: Vec<EventRecord>,
    pub agents: Vec<AgentView>,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub account_id: String,
    pub description: String,
    pub runtime: String,
    pub stats: AgentStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsView {
    pub jobs: usize,
    pub open_jobs: usize,
    pub settled_jobs: usize,
    pub total_budget_yocto: String,
    pub total_bonds_locked_yocto: String,
    pub total_slashed_yocto: String,
    pub total_paid_yocto: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateJobReq {
    pub title: String,
    pub repo_url: String,
    pub commit: String,
    pub test_command: String,
    pub budget_near: f64,
    pub bond_bps: u16,
    pub deadline_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct BidReq {
    pub agent_id: String,
    pub price_near: f64,
    pub bond_near: f64,
}

#[derive(Debug, Deserialize)]
pub struct HireReq {
    pub agent_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitReq {
    pub agent_id: String,
    pub patch_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct EvaluateReq {
    pub passed: bool,
    pub logs_hash: String,
    pub semantic_verdict: String,
    pub semantic_score: u16,
}

#[derive(Debug, Deserialize)]
pub struct RegisterAgentReq {
    pub id: String,
    pub name: String,
    pub account_id: String,
    pub description: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
}

fn default_runtime() -> String {
    "external".to_string()
}

#[derive(Debug, Deserialize)]
pub struct RunDemoReq {
    #[serde(default = "default_outcome")]
    pub outcome: String,
}

fn default_outcome() -> String {
    "success".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct RunDemoResp {
    pub job: JobView,
    pub agent_run: AgentRunView,
    pub evaluation: EvaluationView,
    pub receipts: Vec<TxReceipt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRunView {
    pub agent_id: String,
    pub runtime: String,
    pub patch_hash: String,
    pub patch_preview: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationView {
    pub passed: bool,
    pub semantic_verdict: String,
    pub semantic_score: u16,
    pub typesafe_model: String,
    pub test_command: String,
    pub logs: String,
    pub logs_hash: String,
    pub tamper_check: bool,
    pub duration_ms: u128,
    pub real_fix_probability: f64,
    pub tamper_probability: f64,
    pub review_source: String,
}

/// One visible step in a demo run, polled by the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct RunStep {
    pub name: String,
    pub status: String,
    pub detail: Option<String>,
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub run_id: String,
    pub status: String,
    pub steps: Vec<RunStep>,
    pub error: Option<String>,
    pub result: Option<RunDemoResp>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactsView {
    pub patch: Option<String>,
    pub patch_hash: Option<String>,
    pub logs: Option<String>,
    pub logs_hash: Option<String>,
    pub verdict: Option<String>,
    pub score: Option<u16>,
    pub rerun: Option<RerunView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RerunView {
    pub passed: bool,
    pub logs_hash: String,
    pub log_hashes_match: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyScopeView {
    pub account: String,
    pub public_key: String,
    pub permission: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct BalanceView {
    pub account: String,
    pub role: String,
    pub balance_yocto: Option<String>,
    pub pool_yocto: Option<String>,
    pub locked_yocto: Option<String>,
}
