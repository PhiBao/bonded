use near_sdk::json_types::U128;
use near_sdk::store::IterableMap;
use near_sdk::{env, near, AccountId, NearToken, PanicOnDefault, Promise};

/// Maximum bond percentage of the job budget (5_000 = 50%).
const MAX_BOND_BPS: u16 = 5_000;
/// Maximum job deadline: 30 days.
const MAX_DEADLINE_SECS: u64 = 30 * 24 * 60 * 60;
const EVENT_PREFIX: &str = "EVENT_JSON:";

/// Slash rate for a failed acceptance test (test_failed / unrelated / review).
const FAIL_SLASH_BPS: u16 = 5_000;
/// Slash rate for test manipulation or an expired (no-show) job. Full bond.
const CHEAT_SLASH_BPS: u16 = 10_000;

#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
#[serde(crate = "near_sdk::serde")]
pub enum JobState {
    Open,
    Hired,
    Submitted,
    Evaluated,
    Settled,
    Refunded,
    Cancelled,
    Expired,
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    Clone,
    Debug,
)]
#[serde(crate = "near_sdk::serde")]
pub struct Bid {
    pub agent: AccountId,
    pub price: U128,
    pub bond: U128,
    /// True once the locked bond left `locked` (settled, slashed, or refunded).
    pub released: bool,
    pub created_at: u64,
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    Clone,
    Debug,
)]
#[serde(crate = "near_sdk::serde")]
pub struct Evaluation {
    pub passed: bool,
    pub logs_hash: String,
    pub semantic_verdict: String,
    pub semantic_score: u16,
    pub evaluated_at: u64,
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    Clone,
    Debug,
)]
#[serde(crate = "near_sdk::serde")]
pub struct Job {
    pub id: String,
    pub buyer: AccountId,
    pub title: String,
    pub repo_url: String,
    pub commit: String,
    pub test_command: String,
    pub budget: U128,
    pub bond_bps: u16,
    pub state: JobState,
    pub bids: Vec<Bid>,
    pub selected_agent: Option<AccountId>,
    pub selected_price: U128,
    pub selected_bond: U128,
    pub patch_hash: Option<String>,
    pub evaluation: Option<Evaluation>,
    pub created_at: u64,
    pub deadline_at: u64,
    pub settled: bool,
}

/// Bond capital pooled across jobs for one agent.
///
/// `balance` is everything the agent deposited; `locked` is the part currently
/// at risk on open jobs. Free capital = balance - locked and can be withdrawn
/// or locked on new bids. This is what makes scoped, zero-deposit bids possible:
/// the bond moves by accounting, not by an attached deposit.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    Clone,
    Debug,
    Default,
)]
#[serde(crate = "near_sdk::serde")]
pub struct BondPool {
    pub balance: U128,
    pub locked: U128,
}

impl BondPool {
    fn free(&self) -> u128 {
        self.balance.0.saturating_sub(self.locked.0)
    }
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    Clone,
    Debug,
    Default,
)]
#[serde(crate = "near_sdk::serde")]
pub struct AgentStats {
    pub completed: u64,
    pub failed: u64,
    pub bond_slashed: U128,
    pub bond_returned: U128,
    pub active_jobs: u64,
    pub bond_pool: U128,
    pub bond_locked: U128,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    owner: AccountId,
    evaluator: AccountId,
    jobs: IterableMap<String, Job>,
    agents: IterableMap<AccountId, AgentStats>,
    pools: IterableMap<AccountId, BondPool>,
    job_seq: u64,
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner: AccountId, evaluator: AccountId) -> Self {
        Self {
            owner,
            evaluator,
            jobs: IterableMap::new(b"j"),
            agents: IterableMap::new(b"a"),
            pools: IterableMap::new(b"p"),
            job_seq: 0,
        }
    }

    /// Deposit bond capital into the caller's pool. Payable; the full
    /// deposited amount joins the pool balance and can back bids or be
    /// withdrawn later once unlocked.
    #[payable]
    pub fn deposit_bond(&mut self) {
        let agent = env::predecessor_account_id();
        let amount = env::attached_deposit().as_yoctonear();
        assert!(amount > 0, "deposit required");
        let mut pool = self.pool(&agent);
        pool.balance = U128(pool.balance.0 + amount);
        self.pools.insert(agent.clone(), pool);
        self.sync_agent_pool(&agent);
        emit(
            "bond_deposited",
            &serde_json::json!({ "agent": agent, "amount": U128(amount) }),
        );
    }

    /// Withdraw unlocked bond capital. Never touches funds locked on live jobs.
    pub fn withdraw_bond(&mut self, amount: U128) {
        let agent = env::predecessor_account_id();
        let mut pool = self.pool(&agent);
        assert!(amount.0 > 0, "amount must be positive");
        assert!(pool.free() >= amount.0, "insufficient free bond capital");
        pool.balance = U128(pool.balance.0 - amount.0);
        self.pools.insert(agent.clone(), pool);
        self.sync_agent_pool(&agent);
        let _ = Promise::new(agent.clone()).transfer(NearToken::from_yoctonear(amount.0));
        emit(
            "bond_withdrawn",
            &serde_json::json!({ "agent": agent, "amount": amount }),
        );
    }

    #[payable]
    pub fn create_job(
        &mut self,
        title: String,
        repo_url: String,
        commit: String,
        test_command: String,
        bond_bps: u16,
        deadline_secs: u64,
    ) -> Job {
        let budget = env::attached_deposit().as_yoctonear();
        assert!(budget > 0, "budget must be attached");
        assert!(bond_bps <= MAX_BOND_BPS, "bond bps too high");
        assert!(
            deadline_secs > 0 && deadline_secs <= MAX_DEADLINE_SECS,
            "invalid deadline"
        );
        assert!(!title.is_empty(), "title required");
        assert!(!test_command.is_empty(), "test command required");

        self.job_seq += 1;
        let id = format!("JOB-{:04}", self.job_seq);
        let now = env::block_timestamp() / 1_000_000_000;
        let job = Job {
            id: id.clone(),
            buyer: env::predecessor_account_id(),
            title,
            repo_url,
            commit,
            test_command,
            budget: U128(budget),
            bond_bps,
            state: JobState::Open,
            bids: Vec::new(),
            selected_agent: None,
            selected_price: U128(0),
            selected_bond: U128(0),
            patch_hash: None,
            evaluation: None,
            created_at: now,
            deadline_at: now + deadline_secs,
            settled: false,
        };
        self.jobs.insert(id.clone(), job.clone());
        emit("job_created", &job);
        job
    }

    /// Bid on an open job, locking `bond` from the caller's pool.
    /// Intentionally non-payable so a scoped function-call key can sign it.
    pub fn bid(&mut self, job_id: String, price: U128, bond: U128) {
        let agent = env::predecessor_account_id();
        let now = env::block_timestamp() / 1_000_000_000;
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        assert!(job.state == JobState::Open, "job is not open");
        assert!(now < job.deadline_at, "job deadline passed");
        assert!(price.0 > 0 && price.0 <= job.budget.0, "invalid price");
        assert!(bond.0 > 0, "bond must be positive");
        assert!(
            job.bids.iter().all(|b| b.agent != agent),
            "agent already bid"
        );
        let required = calc_required_bond(job.budget.0, job.bond_bps);
        assert!(bond.0 >= required, "bond below required amount");
        assert_eq!(
            env::attached_deposit().as_yoctonear(),
            0,
            "bid attaches no deposit; lock bond from your pool"
        );

        let mut pool = self.pool(&agent);
        assert!(pool.free() >= bond.0, "insufficient free bond capital");
        pool.locked = U128(pool.locked.0 + bond.0);
        self.pools.insert(agent.clone(), pool);
        self.sync_agent_pool(&agent);

        job.bids.push(Bid {
            agent: agent.clone(),
            price,
            bond,
            released: false,
            created_at: now,
        });
        self.jobs.insert(job_id.clone(), job);
        emit(
            "bid_placed",
            &serde_json::json!({
                "job_id": job_id, "agent": agent, "price": price, "bond": bond
            }),
        );
    }

    pub fn hire(&mut self, job_id: String, agent: AccountId) {
        let buyer = env::predecessor_account_id();
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        assert!(job.buyer == buyer, "only buyer can hire");
        assert!(job.state == JobState::Open, "job is not open");
        let bid = job
            .bids
            .iter()
            .find(|b| b.agent == agent && !b.released)
            .cloned()
            .expect("bid not found");

        job.selected_agent = Some(agent.clone());
        job.selected_price = bid.price;
        job.selected_bond = bid.bond;
        job.state = JobState::Hired;
        self.jobs.insert(job_id.clone(), job);

        let mut stats = self.agents.get(&agent).cloned().unwrap_or_default();
        stats.active_jobs += 1;
        self.agents.insert(agent.clone(), stats);
        emit(
            "hired",
            &serde_json::json!({
                "job_id": job_id, "agent": agent, "price": bid.price, "bond": bid.bond
            }),
        );
    }

    pub fn submit_patch(&mut self, job_id: String, patch_hash: String) {
        let agent = env::predecessor_account_id();
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        assert!(job.state == JobState::Hired, "job is not hired");
        assert!(
            job.selected_agent.as_ref() == Some(&agent),
            "only selected agent can submit"
        );
        assert!(!patch_hash.is_empty(), "patch hash required");
        job.patch_hash = Some(patch_hash);
        job.state = JobState::Submitted;
        self.jobs.insert(job_id.clone(), job);
        emit(
            "patch_submitted",
            &serde_json::json!({ "job_id": job_id, "agent": agent }),
        );
    }

    pub fn evaluate(
        &mut self,
        job_id: String,
        passed: bool,
        logs_hash: String,
        semantic_verdict: String,
        semantic_score: u16,
    ) {
        assert_eq!(
            env::predecessor_account_id(),
            self.evaluator,
            "only evaluator can evaluate"
        );
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        assert!(job.state == JobState::Submitted, "job is not submitted");
        assert!(semantic_score <= 10_000, "semantic score must be bps");
        let evaluation = Evaluation {
            passed,
            logs_hash,
            semantic_verdict,
            semantic_score,
            evaluated_at: env::block_timestamp() / 1_000_000_000,
        };
        job.evaluation = Some(evaluation.clone());
        job.state = JobState::Evaluated;
        self.jobs.insert(job_id.clone(), job);
        emit(
            "evaluated",
            &serde_json::json!({
                "job_id": job_id, "passed": evaluation.passed,
                "semantic_verdict": evaluation.semantic_verdict,
                "semantic_score": evaluation.semantic_score
            }),
        );
    }

    pub fn settle(&mut self, job_id: String) {
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        assert!(job.state == JobState::Evaluated, "job is not evaluated");
        assert!(!job.settled, "job already settled");
        let evaluation = job.evaluation.clone().expect("evaluation missing");
        let agent = job.selected_agent.clone().expect("selected agent missing");
        let buyer = job.buyer.clone();
        let budget = job.budget.0;
        let price = job.selected_price.0;
        let bond = job.selected_bond.0;
        let success = evaluation.passed && evaluation.semantic_verdict == "clean";
        // Tiered slash: cheating (test manipulation) loses the whole bond;
        // an honest failed attempt loses half. Either way the buyer is made
        // whole in budget plus the slashed portion.
        let slash_bps = if evaluation.semantic_verdict == "test_manipulation" {
            CHEAT_SLASH_BPS
        } else {
            FAIL_SLASH_BPS
        };
        let slash = apply_bps(bond, slash_bps);

        if let Some(selected) = job.bids.iter_mut().find(|b| b.agent == agent) {
            selected.released = true;
        }
        job.settled = true;
        job.state = JobState::Settled;
        self.jobs.insert(job_id.clone(), job);

        // Release the lock; the slashed part leaves the pool to the buyer.
        self.release_lock(&agent, bond);
        let mut stats = self.agents.get(&agent).cloned().unwrap_or_default();
        if success {
            let _ = Promise::new(agent.clone()).transfer(NearToken::from_yoctonear(price));
            if budget > price {
                let _ = Promise::new(buyer).transfer(NearToken::from_yoctonear(budget - price));
            }
            stats.completed += 1;
            stats.bond_returned = U128(stats.bond_returned.0 + bond);
        } else {
            let _ = Promise::new(buyer).transfer(NearToken::from_yoctonear(budget + slash));
            let mut pool = self.pool(&agent);
            pool.balance = U128(pool.balance.0.saturating_sub(slash));
            self.pools.insert(agent.clone(), pool);
            stats.failed += 1;
            stats.bond_slashed = U128(stats.bond_slashed.0 + slash);
        }
        stats.active_jobs = stats.active_jobs.saturating_sub(1);
        self.agents.insert(agent.clone(), stats);
        self.sync_agent_pool(&agent);

        self.refund_losing_bids(&job_id);
        emit(
            "settled",
            &serde_json::json!({
                "job_id": job_id, "success": success, "agent": agent,
                "price": U128(price), "bond": U128(bond), "slash": U128(slash)
            }),
        );
    }

    /// Release a bid's locked bond back to the pool (no transfer needed:
    /// pooled capital never left the contract until settlement).
    pub fn release_bid(&mut self, job_id: String) {
        let agent = env::predecessor_account_id();
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        let idx = job
            .bids
            .iter()
            .position(|b| b.agent == agent && !b.released)
            .expect("releaseable bid not found");
        let selected = job.selected_agent.as_ref() == Some(&agent);
        let can_release = job.state == JobState::Open
            || job.state == JobState::Settled
            || job.state == JobState::Refunded
            || job.state == JobState::Cancelled
            || job.state == JobState::Expired;
        assert!(can_release || !selected, "selected agent bond is locked");
        let amount = job.bids[idx].bond.0;
        job.bids[idx].released = true;
        self.jobs.insert(job_id.clone(), job);
        self.release_lock(&agent, amount);
        emit(
            "bid_released",
            &serde_json::json!({ "job_id": job_id, "agent": agent, "amount": U128(amount) }),
        );
    }

    pub fn expire(&mut self, job_id: String) {
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        let now = env::block_timestamp() / 1_000_000_000;
        assert!(now >= job.deadline_at, "deadline not reached");
        assert!(
            job.state == JobState::Open
                || job.state == JobState::Hired
                || job.state == JobState::Submitted,
            "job cannot expire in this state"
        );
        let was_open = job.state == JobState::Open;
        let agent = job.selected_agent.clone();
        let buyer = job.buyer.clone();
        let budget = job.budget.0;
        let bond = job.selected_bond.0;

        if was_open {
            job.state = JobState::Refunded;
            let _ = Promise::new(buyer).transfer(NearToken::from_yoctonear(budget));
        } else {
            job.state = JobState::Expired;
            job.settled = true;
            // No-show: the whole bond is slashed to the buyer.
            self.release_lock(agent.as_ref().unwrap(), bond);
            let _ = Promise::new(buyer).transfer(NearToken::from_yoctonear(budget + bond));
            if let Some(agent_id) = agent {
                let mut pool = self.pool(&agent_id);
                pool.balance = U128(pool.balance.0.saturating_sub(bond));
                self.pools.insert(agent_id.clone(), pool);
                let mut stats = self.agents.get(&agent_id).cloned().unwrap_or_default();
                stats.failed += 1;
                stats.bond_slashed = U128(stats.bond_slashed.0 + bond);
                stats.active_jobs = stats.active_jobs.saturating_sub(1);
                self.agents.insert(agent_id.clone(), stats);
                self.sync_agent_pool(&agent_id);
            }
        }
        self.jobs.insert(job_id.clone(), job);
        self.refund_losing_bids(&job_id);
        emit(
            "expired",
            &serde_json::json!({ "job_id": job_id, "had_agent": !was_open }),
        );
    }

    pub fn cancel(&mut self, job_id: String) {
        let buyer = env::predecessor_account_id();
        let mut job = self.jobs.get(&job_id).cloned().expect("job not found");
        assert!(job.buyer == buyer, "only buyer can cancel");
        assert!(
            job.state == JobState::Open,
            "only open jobs can be cancelled"
        );
        let budget = job.budget.0;
        job.state = JobState::Cancelled;
        self.jobs.insert(job_id.clone(), job);
        let _ = Promise::new(buyer).transfer(NearToken::from_yoctonear(budget));
        self.refund_losing_bids(&job_id);
        emit("cancelled", &serde_json::json!({ "job_id": job_id }));
    }

    pub fn set_evaluator(&mut self, evaluator: AccountId) {
        assert_eq!(env::predecessor_account_id(), self.owner, "only owner");
        self.evaluator = evaluator;
    }

    pub fn get_config(&self) -> (AccountId, AccountId) {
        (self.owner.clone(), self.evaluator.clone())
    }

    pub fn get_job(&self, job_id: String) -> Option<Job> {
        self.jobs.get(&job_id).cloned()
    }

    pub fn get_jobs(&self) -> Vec<Job> {
        self.jobs.values().cloned().collect()
    }

    pub fn get_agent(&self, account_id: AccountId) -> AgentStats {
        let mut stats = self.agents.get(&account_id).cloned().unwrap_or_default();
        let pool = self.pool(&account_id);
        stats.bond_pool = pool.balance;
        stats.bond_locked = pool.locked;
        stats
    }

    pub fn get_pool(&self, account_id: AccountId) -> BondPool {
        self.pool(&account_id)
    }

    pub fn required_bond(&self, budget: U128, bond_bps: u16) -> U128 {
        U128(calc_required_bond(budget.0, bond_bps))
    }
}

impl Contract {
    fn pool(&self, agent: &AccountId) -> BondPool {
        self.pools.get(agent).cloned().unwrap_or_default()
    }

    fn release_lock(&mut self, agent: &AccountId, bond: u128) {
        let mut pool = self.pool(agent);
        pool.locked = U128(pool.locked.0.saturating_sub(bond));
        self.pools.insert(agent.clone(), pool);
        self.sync_agent_pool(agent);
    }

    /// Release bids that did not win. With pooled capital this is pure
    /// accounting again — no transfers required.
    fn refund_losing_bids(&mut self, job_id: &str) {
        let mut job = self.jobs.get(job_id).cloned().expect("job not found");
        let selected = job.selected_agent.clone();
        let mut released: Vec<(AccountId, u128)> = Vec::new();
        for bid in job.bids.iter_mut() {
            if bid.released || selected.as_ref() == Some(&bid.agent) {
                continue;
            }
            bid.released = true;
            released.push((bid.agent.clone(), bid.bond.0));
        }
        self.jobs.insert(job_id.to_string(), job);
        for (agent, bond) in released {
            self.release_lock(&agent, bond);
        }
    }

    fn sync_agent_pool(&mut self, agent: &AccountId) {
        let pool = self.pool(agent);
        let mut stats = self.agents.get(agent).cloned().unwrap_or_default();
        stats.bond_pool = pool.balance;
        stats.bond_locked = pool.locked;
        self.agents.insert(agent.clone(), stats);
    }
}

fn calc_required_bond(budget: u128, bond_bps: u16) -> u128 {
    budget
        .saturating_mul(bond_bps as u128)
        .saturating_add(9_999)
        / 10_000
}

fn apply_bps(amount: u128, bps: u16) -> u128 {
    amount.saturating_mul(bps as u128) / 10_000
}

fn emit(kind: &str, data: &impl serde::Serialize) {
    let payload = serde_json::json!({ "standard": "bonded", "version": "1.0.0", "event": kind, "data": data });
    env::log_str(&format!("{EVENT_PREFIX}{payload}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    fn acct(name: &str) -> AccountId {
        name.parse().unwrap()
    }

    fn context(predecessor: &str, deposit_yocto: u128, timestamp: u64) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(acct("bonded.testnet"))
            .predecessor_account_id(acct(predecessor))
            .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
            .block_timestamp(timestamp);
        builder
    }

    fn setup() -> Contract {
        testing_env!(context("owner.testnet", 0, 1_000_000_000_000_000_000).build());
        Contract::new(acct("owner.testnet"), acct("evaluator.testnet"))
    }

    fn deposit(agent: &str, near: f64) {
        testing_env!(context(
            agent,
            (near * 1_000_000_000_000_000_000_000_000.0) as u128,
            1_000_000_000_000_000_000
        )
        .build());
    }

    fn create_default_job(contract: &mut Contract) -> Job {
        testing_env!(context(
            "buyer.testnet",
            NearToken::from_near(10).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.create_job(
            "Fix add()".to_string(),
            "preset:add".to_string(),
            "abc123".to_string(),
            "npm test".to_string(),
            1_000,
            3_600,
        )
    }

    /// Deposit bond capital, bid, hire, submit, evaluate. Returns the job.
    fn run_to_evaluated(contract: &mut Contract, passed: bool, verdict: &str, score: u16) -> Job {
        let job = create_default_job(contract);
        testing_env!(context(
            "agent-a.testnet",
            NearToken::from_near(2).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.deposit_bond();
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id.clone(),
            U128(NearToken::from_near(10).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.hire(job.id.clone(), acct("agent-a.testnet"));
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.submit_patch(job.id.clone(), "patch-hash".to_string());
        testing_env!(context("evaluator.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.evaluate(
            job.id.clone(),
            passed,
            "logs-hash".to_string(),
            verdict.to_string(),
            score,
        );
        contract.get_job(job.id).unwrap()
    }

    #[test]
    fn create_job_stores_budget_and_open_state() {
        let mut contract = setup();
        let job = create_default_job(&mut contract);
        assert_eq!(job.budget.0, NearToken::from_near(10).as_yoctonear());
        assert_eq!(job.bond_bps, 1_000);
        assert_eq!(job.state, JobState::Open);
        assert_eq!(contract.get_jobs().len(), 1);
    }

    #[test]
    fn pool_deposit_and_lock_and_withdraw() {
        let mut contract = setup();
        testing_env!(context(
            "agent-a.testnet",
            NearToken::from_near(2).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.deposit_bond();
        let pool = contract.get_pool(acct("agent-a.testnet"));
        assert_eq!(pool.balance.0, NearToken::from_near(2).as_yoctonear());
        assert_eq!(pool.locked.0, 0);

        let job = create_default_job(&mut contract);
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id.clone(),
            U128(NearToken::from_near(9).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
        let pool = contract.get_pool(acct("agent-a.testnet"));
        assert_eq!(pool.locked.0, NearToken::from_near(1).as_yoctonear());

        // Free = 2 - 1 locked; withdrawing 0.5 works, 1.5 does not.
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.withdraw_bond(U128(NearToken::from_millinear(500).as_yoctonear()));
        let pool = contract.get_pool(acct("agent-a.testnet"));
        assert_eq!(
            pool.balance.0,
            NearToken::from_millinear(1500).as_yoctonear()
        );
    }

    #[test]
    #[should_panic(expected = "insufficient free bond capital")]
    fn bid_without_pool_panics() {
        let mut contract = setup();
        let job = create_default_job(&mut contract);
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id,
            U128(NearToken::from_near(10).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
    }

    #[test]
    fn successful_job_returns_bond_and_updates_reputation() {
        let mut contract = setup();
        let job = run_to_evaluated(&mut contract, true, "clean", 9_500);
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.settle(job.id.clone());

        let stored = contract.get_job(job.id).unwrap();
        assert_eq!(stored.state, JobState::Settled);
        assert!(stored.settled);
        let stats = contract.get_agent(acct("agent-a.testnet"));
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.failed, 0);
        assert_eq!(
            stats.bond_returned.0,
            NearToken::from_near(1).as_yoctonear()
        );
        // Pool intact: balance 2, locked 0.
        assert_eq!(stats.bond_pool.0, NearToken::from_near(2).as_yoctonear());
        assert_eq!(stats.bond_locked.0, 0);
    }

    #[test]
    fn failed_test_slashes_half_the_bond() {
        let mut contract = setup();
        let job = run_to_evaluated(&mut contract, false, "test_failed", 0);
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.settle(job.id.clone());

        let stats = contract.get_agent(acct("agent-a.testnet"));
        assert_eq!(stats.completed, 0);
        assert_eq!(stats.failed, 1);
        assert_eq!(
            stats.bond_slashed.0,
            NearToken::from_millinear(500).as_yoctonear()
        );
        // Pool: 2 - 0.5 slashed = 1.5; nothing locked.
        assert_eq!(
            stats.bond_pool.0,
            NearToken::from_millinear(1500).as_yoctonear()
        );
        assert_eq!(stats.bond_locked.0, 0);
    }

    #[test]
    fn cheating_slashes_the_whole_bond() {
        let mut contract = setup();
        // Tests "passed" but the verdict is test manipulation: still a slash.
        let job = run_to_evaluated(&mut contract, true, "test_manipulation", 0);
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.settle(job.id.clone());

        let stats = contract.get_agent(acct("agent-a.testnet"));
        assert_eq!(stats.completed, 0);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.bond_slashed.0, NearToken::from_near(1).as_yoctonear());
    }

    #[test]
    fn expire_after_deadline_slashes_selected_agent() {
        let mut contract = setup();
        let job = create_default_job(&mut contract);
        testing_env!(context(
            "agent-a.testnet",
            NearToken::from_near(2).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.deposit_bond();
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id.clone(),
            U128(NearToken::from_near(10).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.hire(job.id.clone(), acct("agent-a.testnet"));
        testing_env!(context(
            "buyer.testnet",
            0,
            1_000_000_000_000_000_000 + 4_000_000_000_000
        )
        .build());
        contract.expire(job.id.clone());

        let stored = contract.get_job(job.id).unwrap();
        assert_eq!(stored.state, JobState::Expired);
        let stats = contract.get_agent(acct("agent-a.testnet"));
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.bond_slashed.0, NearToken::from_near(1).as_yoctonear());
    }

    #[test]
    #[should_panic(expected = "only evaluator can evaluate")]
    fn only_evaluator_can_evaluate() {
        let mut contract = setup();
        let job = create_default_job(&mut contract);
        testing_env!(context(
            "agent-a.testnet",
            NearToken::from_near(2).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.deposit_bond();
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id.clone(),
            U128(NearToken::from_near(10).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.hire(job.id.clone(), acct("agent-a.testnet"));
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.submit_patch(job.id.clone(), "patch-hash".to_string());
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.evaluate(
            job.id,
            true,
            "logs-hash".to_string(),
            "clean".to_string(),
            9_000,
        );
    }

    #[test]
    fn losing_bid_lock_is_released_on_settle() {
        let mut contract = setup();
        let job = create_default_job(&mut contract);
        // agent-a bids and wins
        testing_env!(context(
            "agent-a.testnet",
            NearToken::from_near(2).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.deposit_bond();
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id.clone(),
            U128(NearToken::from_near(10).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
        // agent-b bids and loses
        testing_env!(context(
            "agent-b.testnet",
            NearToken::from_near(2).as_yoctonear(),
            1_000_000_000_000_000_000
        )
        .build());
        contract.deposit_bond();
        testing_env!(context("agent-b.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.bid(
            job.id.clone(),
            U128(NearToken::from_near(9).as_yoctonear()),
            U128(NearToken::from_near(1).as_yoctonear()),
        );
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.hire(job.id.clone(), acct("agent-a.testnet"));
        testing_env!(context("agent-a.testnet", 0, 1_000_000_000_000_000).build());
        contract.submit_patch(job.id.clone(), "patch-hash".to_string());
        testing_env!(context("evaluator.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.evaluate(
            job.id.clone(),
            true,
            "logs-hash".to_string(),
            "clean".to_string(),
            9_000,
        );
        testing_env!(context("buyer.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.settle(job.id.clone());

        let loser = contract.get_agent(acct("agent-b.testnet"));
        assert_eq!(loser.bond_locked.0, 0);
        assert_eq!(loser.bond_pool.0, NearToken::from_near(2).as_yoctonear());
        // Loser can now withdraw everything.
        testing_env!(context("agent-b.testnet", 0, 1_000_000_000_000_000_000).build());
        contract.withdraw_bond(U128(NearToken::from_near(2).as_yoctonear()));
    }
}
