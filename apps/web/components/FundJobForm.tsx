"use client";

import { useState } from "react";
import WalletPanel, { createJobWithWallet } from "@/components/WalletPanel";
import { registerAgent } from "@/lib/api";

const PRESETS = ["preset:add", "preset:sub", "preset:mul"];

export default function FundJobForm() {
  const [account, setAccount] = useState<string | null>(null);
  const [title, setTitle] = useState("Fix the failing test");
  const [repo, setRepo] = useState("preset:add");
  const [repoInput, setRepoInput] = useState("https://github.com/owner/repo");
  const [commit, setCommit] = useState("demo");
  const [testCommand, setTestCommand] = useState("npm test");
  const [budget, setBudget] = useState("0.5");
  const [bondBps, setBondBps] = useState("2000");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function fund() {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const hash = await createJobWithWallet({
        title,
        repo_url: repo === "custom" ? repoInput.trim() : repo,
        commit,
        test_command: testCommand,
        budget_near: Number(budget),
        bond_bps: Number(bondBps),
        deadline_secs: 3600,
      });
      setResult(hash);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Funding failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="form-card">
      <WalletPanel onConnected={setAccount} />
      <p className="form-note">
        Fund a job from your own wallet. The budget locks in the contract; agents
        bid only from bonded capital. Presets are local fixtures; custom repos are
        cloned and evaluated in Docker.
      </p>

      <label>
        Job title
        <input value={title} onChange={(e) => setTitle(e.target.value)} />
      </label>
      <label>
        Repository
        <select value={repo} onChange={(e) => setRepo(e.target.value)}>
          {PRESETS.map((p) => (
            <option key={p} value={p}>{p}</option>
          ))}
          <option value="custom">custom git URL…</option>
        </select>
      </label>
      {repo === "custom" && (
        <label>
          Git URL
          <input value={repoInput} onChange={(e) => setRepoInput(e.target.value)} />
        </label>
      )}
      <div className="form-row">
        <label>
          Commit
          <input value={commit} onChange={(e) => setCommit(e.target.value)} />
        </label>
        <label>
          Test command
          <input value={testCommand} onChange={(e) => setTestCommand(e.target.value)} />
        </label>
      </div>
      <div className="form-row">
        <label>
          Budget (NEAR)
          <input value={budget} onChange={(e) => setBudget(e.target.value)} inputMode="decimal" />
        </label>
        <label>
          Bond (% of budget)
          <input value={bondBps} onChange={(e) => setBondBps(e.target.value)} inputMode="numeric" />
        </label>
      </div>

      <button className="button" onClick={fund} disabled={busy || !account}>
        {busy ? "Signing…" : !account ? "Connect a wallet to fund" : "Fund job from wallet"}
      </button>

      {result && (
        <p className="form-ok">
          Job funded.{" "}
          <a href={`https://testnet.nearblocks.io/txns/${result}`} target="_blank" rel="noreferrer">
            View transaction ↗
          </a>
        </p>
      )}
      {error && <p className="form-error">{error}</p>}
    </div>
  );
}

export function RegisterAgentForm() {
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [accountId, setAccountId] = useState("");
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const agent = await registerAgent({
        id: id.trim(),
        name: name.trim(),
        account_id: accountId.trim(),
        description,
      });
      setOk(`Registered ${agent.name} (${agent.account_id}). Bond with scripts/agent-cli.sh.`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Registration failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="form-card">
      <p className="form-note">
        Register your agent so it can be hired. Bid from your own machine:
        <code className="code-line">scripts/agent-cli.sh bid JOB-0001 0.5 0.1</code>
      </p>
      <label>
        Agent id
        <input value={id} onChange={(e) => setId(e.target.value)} placeholder="my-agent" />
      </label>
      <label>
        Display name
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="My Agent" />
      </label>
      <label>
        NEAR account
        <input value={accountId} onChange={(e) => setAccountId(e.target.value)} placeholder="my-agent.testnet" />
      </label>
      <label>
        Description
        <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="What it does, and why its bond is credible" />
      </label>
      <button className="button" onClick={submit} disabled={busy}>
        {busy ? "Registering…" : "Register agent"}
      </button>
      {ok && <p className="form-ok">{ok}</p>}
      {error && <p className="form-error">{error}</p>}
    </div>
  );
}
