use anyhow::{Context, Result};
use near_kit::rpc::FinalExecutionOutcome;
use near_kit::{AccountId, Gas, Near, NearToken};
use serde_json::json;

use crate::config::Config;
use crate::types::{AgentStats, BondPool, Job};

#[derive(Clone)]
pub struct NearClient {
    pub contract_id: AccountId,
    pub read: Near,
    pub buyer: Near,
    /// Agent signers. In production these hold *scoped function-call keys*
    /// (bid / submit_patch only — no deposits, no transfers, no withdraw).
    pub agent_a: Near,
    pub agent_b: Near,
    /// Full-access signers used ONLY to top up bond pools (deposit_bond).
    pub agent_a_full: Near,
    pub agent_b_full: Near,
    pub evaluator: Near,
    rpc_url: String,
}

impl NearClient {
    pub fn new(config: &Config) -> Result<Self> {
        let contract_id: AccountId = config
            .contract_id
            .parse()
            .context("invalid CONTRACT_ID account")?;
        let read = Near::testnet().build();
        let buyer = Near::testnet()
            .credentials(&config.buyer_key, &config.buyer_account)?
            .build();
        let agent_a = Near::testnet()
            .credentials(&config.agent_a_key, &config.agent_a_account)?
            .build();
        let agent_b = Near::testnet()
            .credentials(&config.agent_b_key, &config.agent_b_account)?
            .build();
        let agent_a_full = Near::testnet()
            .credentials(&config.agent_a_full_key, &config.agent_a_account)?
            .build();
        let agent_b_full = Near::testnet()
            .credentials(&config.agent_b_full_key, &config.agent_b_account)?
            .build();
        let evaluator = Near::testnet()
            .credentials(&config.evaluator_key, &config.evaluator_account)?
            .build();
        Ok(Self {
            contract_id,
            read,
            buyer,
            agent_a,
            agent_b,
            agent_a_full,
            agent_b_full,
            evaluator,
            rpc_url: "https://rpc.testnet.near.org".to_string(),
        })
    }

    pub async fn jobs(&self) -> Result<Vec<Job>> {
        let jobs: Vec<Job> = self
            .read
            .view(&self.contract_id, "get_jobs")
            .await
            .context("get_jobs view failed")?;
        Ok(jobs)
    }

    pub async fn job(&self, job_id: &str) -> Result<Option<Job>> {
        let job: Option<Job> = self
            .read
            .view(&self.contract_id, "get_job")
            .args(json!({ "job_id": job_id }))
            .await
            .context("get_job view failed")?;
        Ok(job)
    }

    pub async fn agent(&self, account_id: &str) -> Result<AgentStats> {
        let stats: AgentStats = self
            .read
            .view(&self.contract_id, "get_agent")
            .args(json!({ "account_id": account_id }))
            .await
            .context("get_agent view failed")?;
        Ok(stats)
    }

    pub async fn pool(&self, account_id: &str) -> Result<BondPool> {
        let pool: BondPool = self
            .read
            .view(&self.contract_id, "get_pool")
            .args(json!({ "account_id": account_id }))
            .await
            .context("get_pool view failed")?;
        Ok(pool)
    }

    pub async fn config(&self) -> Result<(String, String)> {
        let config: (String, String) = self
            .read
            .view(&self.contract_id, "get_config")
            .await
            .context("get_config view failed")?;
        Ok(config)
    }

    pub async fn create_job(
        &self,
        title: &str,
        repo_url: &str,
        commit: &str,
        test_command: &str,
        budget_yocto: u128,
        bond_bps: u16,
        deadline_secs: u64,
    ) -> Result<(String, Job)> {
        let outcome = self
            .call_outcome(
                &self.buyer,
                "create_job",
                json!({
                    "title": title,
                    "repo_url": repo_url,
                    "commit": commit,
                    "test_command": test_command,
                    "bond_bps": bond_bps,
                    "deadline_secs": deadline_secs
                }),
                budget_yocto,
            )
            .await?;
        let hash = outcome.transaction_hash().to_string();
        let job: Job = outcome
            .json()
            .context("create_job returned an invalid job")?;
        Ok((hash, job))
    }

    /// Top up an agent's bond pool. Signed with the agent's full-access key
    /// (the scoped key cannot attach deposits — by design).
    pub async fn deposit_bond(&self, agent_account: &str, amount_yocto: u128) -> Result<String> {
        self.call(
            &self.full_for(agent_account)?,
            "deposit_bond",
            json!({}),
            amount_yocto,
        )
        .await
    }

    /// Bid with ZERO attached deposit, locking from the pool. This is the call
    /// a scoped function-call key is allowed to sign.
    pub async fn bid(
        &self,
        agent_account: &str,
        job_id: &str,
        price_yocto: u128,
        bond_yocto: u128,
    ) -> Result<String> {
        self.call(
            &self.near_for(agent_account)?,
            "bid",
            json!({
                "job_id": job_id,
                "price": price_yocto.to_string(),
                "bond": bond_yocto.to_string()
            }),
            0,
        )
        .await
    }

    pub async fn hire(&self, job_id: &str, agent_account: &str) -> Result<String> {
        self.call(
            &self.buyer,
            "hire",
            json!({ "job_id": job_id, "agent": agent_account }),
            0,
        )
        .await
    }

    pub async fn submit_patch(
        &self,
        agent_account: &str,
        job_id: &str,
        patch_hash: &str,
    ) -> Result<String> {
        self.call(
            &self.near_for(agent_account)?,
            "submit_patch",
            json!({ "job_id": job_id, "patch_hash": patch_hash }),
            0,
        )
        .await
    }

    pub async fn evaluate(
        &self,
        job_id: &str,
        passed: bool,
        logs_hash: &str,
        semantic_verdict: &str,
        semantic_score: u16,
    ) -> Result<String> {
        self.call(
            &self.evaluator,
            "evaluate",
            json!({
                "job_id": job_id,
                "passed": passed,
                "logs_hash": logs_hash,
                "semantic_verdict": semantic_verdict,
                "semantic_score": semantic_score
            }),
            0,
        )
        .await
    }

    pub async fn settle(&self, job_id: &str) -> Result<String> {
        self.call(&self.buyer, "settle", json!({ "job_id": job_id }), 0)
            .await
    }

    /// On-chain balance of an account (yoctoNEAR) via raw RPC.
    pub async fn balance(&self, account_id: &str) -> Result<u128> {
        let value = self
            .rpc_query(json!({
                "request_type": "view_account",
                "account_id": account_id
            }))
            .await?;
        Ok(value
            .get("amount")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .parse()
            .unwrap_or(0))
    }

    /// Access key scope proof via raw RPC `view_access_key`.
    pub async fn access_key(
        &self,
        account_id: &str,
        public_key: &str,
    ) -> Result<serde_json::Value> {
        self.rpc_query(json!({
            "request_type": "view_access_key",
            "account_id": account_id,
            "public_key": public_key
        }))
        .await
    }

    async fn rpc_query(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        let client = reqwest::Client::new();
        let mut full_params = params
            .as_object()
            .cloned()
            .context("RPC params must be an object")?;
        full_params.insert("finality".to_string(), json!("final"));
        let value: serde_json::Value = client
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "bonded",
                "method": "query",
                "params": full_params
            }))
            .send()
            .await
            .context("RPC request failed")?
            .json()
            .await
            .context("RPC response was not JSON")?;
        value.get("result").cloned().with_context(|| {
            format!(
                "RPC error: {}",
                value
                    .get("error")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            )
        })
    }

    fn account_of(near: &Near) -> String {
        near.account_id().map(|a| a.to_string()).unwrap_or_default()
    }

    fn near_for(&self, account: &str) -> Result<Near> {
        if account == Self::account_of(&self.agent_a) {
            Ok(self.agent_a.clone())
        } else if account == Self::account_of(&self.agent_b) {
            Ok(self.agent_b.clone())
        } else {
            anyhow::bail!("unknown signer account {account}")
        }
    }

    fn full_for(&self, account: &str) -> Result<Near> {
        if account == Self::account_of(&self.agent_a_full) {
            Ok(self.agent_a_full.clone())
        } else if account == Self::account_of(&self.agent_b_full) {
            Ok(self.agent_b_full.clone())
        } else {
            anyhow::bail!("unknown signer account {account}")
        }
    }

    async fn call(
        &self,
        near: &Near,
        method: &str,
        args: serde_json::Value,
        deposit_yocto: u128,
    ) -> Result<String> {
        let outcome = self.call_outcome(near, method, args, deposit_yocto).await?;
        Ok(outcome.transaction_hash().to_string())
    }

    async fn call_outcome(
        &self,
        near: &Near,
        method: &str,
        args: serde_json::Value,
        deposit_yocto: u128,
    ) -> Result<FinalExecutionOutcome> {
        let call = near
            .call(self.contract_id.clone(), method)
            .args(args)
            .gas(Gas::from_tgas(120))
            .deposit(NearToken::from_yoctonear(deposit_yocto));
        let outcome = match tokio::time::timeout(std::time::Duration::from_secs(25), call).await {
            Ok(result) => result.with_context(|| format!("{method} call failed"))?,
            Err(_) => anyhow::bail!("{method} timed out after 25s"),
        };
        if !outcome.is_success() {
            let message = outcome
                .failure_message()
                .unwrap_or_else(|| "unknown on-chain failure".to_string());
            anyhow::bail!("{method} failed on-chain: {message}");
        }
        Ok(outcome)
    }
}

pub fn near_to_yocto(near: f64) -> u128 {
    // Round to microNEAR so common demo amounts (0.1, 0.5) are exact.
    let micro = (near * 1_000_000.0).round() as u128;
    micro.saturating_mul(1_000_000_000_000_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_amounts_round_to_micro_near() {
        assert_eq!(near_to_yocto(0.5), 500_000_000_000_000_000_000_000);
        assert_eq!(near_to_yocto(0.1), 100_000_000_000_000_000_000_000);
        assert_eq!(near_to_yocto(1.0), 1_000_000_000_000_000_000_000_000);
    }
}
