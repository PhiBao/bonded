"use client";

import { useEffect, useMemo, useState } from "react";
import {
  BalanceView,
  JobView,
  KeyScopeView,
  RunResult,
  RunView,
  explorerTx,
  formatNear,
  getAgentKey,
  getBalances,
  getJobs,
  paidOf,
  pollRun,
  seedDemo,
  shortHash,
  startJobRun,
  startRun,
} from "@/lib/api";

type Outcome = "success" | "failure" | "cheat" | "tamper";
type Phase = "idle" | "seeding" | "running";

const FALLBACK_LOG = `$ npm test

# Subtest: add returns the sum
not ok 1 - add returns the sum
  expected: 5
  actual: -1

# pass 2
# fail 1`;

export default function DemoBond() {
  const [view, setView] = useState<JobView | null>(null);
  const [result, setResult] = useState<RunResult | null>(null);
  const [liveSteps, setLiveSteps] = useState<RunView["steps"]>([]);
  const [balances, setBalances] = useState<BalanceView[]>([]);
  const [keyScope, setKeyScope] = useState<KeyScopeView | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getJobs()
      .then(async (jobs) => {
        if (cancelled) return;
        const active = jobs.find(
          (item) =>
            item.job.repo_url.startsWith("preset:") && item.job.state === "Open",
        );
        if (active) {
          setView(active);
          return;
        }
        if (jobs.length > 0) {
          setView(jobs[0]);
          return;
        }
        const seeded = await seedDemo();
        if (!cancelled) setView(seeded);
      })
      .catch((err: Error) => {
        if (!cancelled) setError(err.message);
      });
    getBalances()
      .then((b) => {
        if (!cancelled) setBalances(b);
      })
      .catch(() => {});
    getAgentKey("forge")
      .then((k) => {
        if (!cancelled) setKeyScope(k);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const job = view?.job;
  const evaluation = result?.evaluation ?? job?.evaluation;
  const repoLabel = job ? prettifyRepo(job.repo_url) : "preset:add";
  const paid = paidOf(evaluation);
  const active = job && (job.state === "Open" || job.state === "Hired");
  const isSettled = job?.state === "Settled" || job?.state === "Expired";
  const agents = view?.agents ?? [];
  const forge = agents.find((agent) => agent.id === "forge");
  const sable = agents.find((agent) => agent.id === "sable");

  const stampTone = evaluation ? (paid ? "green" : "red") : job ? "neutral" : "neutral";
  const stampText = evaluation
    ? paid
      ? "Paid"
      : "Slashed"
    : job
      ? job.state === "Open"
        ? "Bond locked"
        : job.state
      : "Awaiting job";

  const events = useMemo(() => {
    if (!view) return [];
    return [...view.events].sort((a, b) => {
      const rank = eventRank(a.kind) - eventRank(b.kind);
      return rank !== 0 ? rank : a.id - b.id;
    });
  }, [view]);

  async function handleRun(outcome: Outcome) {
    setPhase("running");
    setError(null);
    setResult(null);
    setLiveSteps([]);
    try {
      const started = await startRun(outcome);
      const response = await pollRun(started.run_id, (run) => setLiveSteps(run.steps));
      setResult(response);
      setView(response.job);
      getBalances().then(setBalances).catch(() => {});
      setPhase("idle");
    } catch (err) {
      setError(err instanceof Error ? err.message : "The bonded run failed");
      setPhase("idle");
      getBalances().then(setBalances).catch(() => {});
    }
  }

  async function handleExistingRun(outcome: Outcome) {
    if (!job) return;
    setPhase("running");
    setError(null);
    setResult(null);
    setLiveSteps([]);
    try {
      const started = await startJobRun(job.id, outcome);
      const response = await pollRun(started.run_id, (run) => setLiveSteps(run.steps));
      setResult(response);
      setView(response.job);
      getBalances().then(setBalances).catch(() => {});
      setPhase("idle");
    } catch (err) {
      setError(err instanceof Error ? err.message : "The bonded run failed");
      setPhase("idle");
    }
  }

  const loading = phase === "running" || phase === "seeding";
  const logs = result?.evaluation.logs ?? (evaluation ? signedSummary(job, evaluation) : FALLBACK_LOG);
  const buyerBalance = balances.find((b) => b.role === "buyer");
  const contractBalance = balances.find((b) => b.role === "contract");

  return (
    <div className="demo-shell">
      <div className="demo-bar">
        <span className="live-dot">Live bond · NEAR testnet · {job?.id ?? "loading"}</span>
        <span>
          {job ? `${formatNear(job.budget)} budget · ${job.bond_bps / 100}% bond · ${repoLabel}` : "reading contract…"}
        </span>
      </div>

      <div className="balances-strip">
        <span>
          Buyer <b>{buyerBalance ? formatNear(buyerBalance.balance_yocto ?? undefined) : "…"}</b>
        </span>
        <span>
          Contract <b>{contractBalance ? formatNear(contractBalance.balance_yocto ?? undefined) : "…"}</b>
        </span>
        <span>
          Forge pool <b>{forge ? formatNear(forge.stats.bond_pool) : "…"}</b>
        </span>
        <span>
          Sable pool <b>{sable ? formatNear(sable.stats.bond_pool) : "…"}</b>
        </span>
      </div>

      <div className="demo-grid">
        <div className="demo-main">
          <div className="demo-title-row">
            <div>
              <div className="kicker">Bond certificate</div>
              <h3 className="demo-title">{job?.title ?? "Preparing a bonded job"}</h3>
            </div>
            <span className={`stamp ${stampTone === "red" ? "red" : stampTone === "green" ? "" : "neutral"} ${result ? "land" : ""}`}>
              {stampText}
            </span>
          </div>

          <div className="demo-terms">
            <div className="term">
              <span>Buyer</span>
              <b>{job?.buyer ?? "buyer-v2.testnet"}</b>
            </div>
            <div className="term">
              <span>Bond at risk</span>
              <b>{job ? formatNear(((BigInt(job.budget) * BigInt(job.bond_bps)) / 10000n).toString()) : "0.10 NEAR"}</b>
            </div>
            <div className="term">
              <span>Acceptance test</span>
              <b className="mono">{job?.test_command ?? "npm test"}</b>
            </div>
            <div className="term">
              <span>Repository</span>
              <b>{repoLabel}</b>
            </div>
            <div className="term">
              <span>Selected agent</span>
              <b>{job?.selected_agent ?? "not hired yet"}</b>
            </div>
            <div className="term">
              <span>Pool · locked</span>
              <b>
                {forge ? `${formatNear(forge.stats.bond_pool)} · ${formatNear(forge.stats.bond_locked)} locked` : "…"}
              </b>
            </div>
          </div>

          <div className="bids">
            <div className="bid bid-head">
              <span>Bonded agent</span>
              <span>Price</span>
              <span>Bond</span>
              <span>Status</span>
            </div>
            {job?.bids.map((bid) => {
              const agent = agents.find((item) => item.account_id === bid.agent);
              const selected = job.selected_agent === bid.agent;
              return (
                <div className="bid" key={bid.agent}>
                  <span className="name">{agent?.name ?? shortHash(bid.agent, 10, 4)}</span>
                  <span className="mono">{formatNear(bid.price)}</span>
                  <span className="mono">{formatNear(bid.bond)}</span>
                  <span
                    className={`pill ${
                      paid && selected ? "green" : evaluation && selected ? "red" : bid.released ? "" : "green"
                    }`}
                  >
                    {paid && selected
                      ? "paid"
                      : evaluation && selected
                        ? "slashed"
                        : bid.released
                          ? "released"
                          : "bonded"}
                  </span>
                </div>
              );
            })}
            {!job?.bids.length && <div className="empty">Waiting for bonded agents to bid.</div>}
          </div>

          <div className="actions-row">
            {active && job?.state === "Hired" ? (
              <>
                <button className="button" onClick={() => handleExistingRun("success")} disabled={loading}>
                  {phase === "running" ? "Running…" : "Run bonded job"}
                </button>
                <button className="button button-red" onClick={() => handleExistingRun("failure")} disabled={loading}>
                  Run failure path
                </button>
              </>
            ) : (
              <>
                <button className="button" onClick={() => handleRun("success")} disabled={loading}>
                  {phase === "running" ? "Running the bond…" : "Run live bond demo"}
                </button>
                <button className="button button-ghost" onClick={() => handleRun("failure")} disabled={loading}>
                  Failure path · 50% slash
                </button>
                <button className="button button-ghost" onClick={() => handleRun("cheat")} disabled={loading}>
                  Cheat path · hidden tests
                </button>
                <button className="button button-red" onClick={() => handleRun("tamper")} disabled={loading}>
                  Tamper path · 100% slash
                </button>
              </>
            )}
            <a className="button button-ghost" href="#mechanism">
              How the bond works
            </a>
          </div>

          {loading && liveSteps.length > 0 && (
            <ol className="run-steps">
              {liveSteps.map((s, i) => (
                <li key={`${s.name}-${i}`} className={s.status === "done" ? "done" : "active"}>
                  <b>{s.name}</b>
                  {s.detail && <span>{s.detail}</span>}
                  {s.tx_hash && (
                    <a href={explorerTx(s.tx_hash)} target="_blank" rel="noreferrer">
                      {shortHash(s.tx_hash)} ↗
                    </a>
                  )}
                </li>
              ))}
            </ol>
          )}
          {loading && (
            <div className="micro" style={{ marginTop: -4 }}>
              {loading
                ? "Creating the job, locking bonds, running the agent, evaluating in Docker, reviewing, and settling on-chain…"
                : ""}
            </div>
          )}

          {error && <div className="error-box">{error}</div>}

          <div className="terminal">
            <div className="terminal-head">
              <span>Acceptance test</span>
              <span>{result ? "evaluator output · docker" : "repo · preset or custom git"}</span>
            </div>
            <pre>{logs}</pre>
          </div>

          {evaluation && (
            <div className="result">
              <div className="result-head">
                <h4 className="result-title">
                  {paid ? "The bond paid the agent." : "The bond paid the buyer."}
                </h4>
                <span className={`stamp ${paid ? "" : "red"}`}>{paid ? "Paid" : "Slashed"}</span>
              </div>
              <div className="result-row">
                <span>TypeSafe review</span>
                <b>
                  {evaluation.semantic_verdict} · {evaluation.semantic_score / 100}% ·{" "}
                  {result?.evaluation.typesafe_model ?? "on-chain verdict"}
                </b>
              </div>
              <div className="result-row">
                <span>Test result</span>
                <b>
                  {evaluation.passed ? "pass" : "fail"} · {evaluation.logs_hash.slice(0, 22)}…
                </b>
              </div>
              {result && (
                <div className="result-row">
                  <span>Review source</span>
                  <b>
                    {result.evaluation.review_source} · real-fix{" "}
                    {Math.round(result.evaluation.real_fix_probability * 100)}% · tamper{" "}
                    {Math.round(result.evaluation.tamper_probability * 100)}%
                  </b>
                </div>
              )}
              <div className="result-row">
                <span>Agent runtime</span>
                <b>
                  {result
                    ? `${result.agent_run.runtime} · ${result.agent_run.duration_ms}ms · ${result.agent_run.patch_hash.slice(0, 18)}…`
                    : "recorded by the evaluator"}
                </b>
              </div>
              <div className="result-row">
                <span>Public page</span>
                <b>
                  <a className="receipt-link" href={`/receipts/${job?.id}`} target="_blank" rel="noreferrer">
                    Open job receipt {job?.id} ↗
                  </a>
                </b>
              </div>
              <div className="result-row">
                <span>Receipts</span>
                <b>
                  {result
                    ? result.receipts.map((receipt) => (
                        <a
                          className="receipt-link"
                          key={receipt.tx_hash}
                          href={receipt.explorer_url}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {receipt.kind}: {shortHash(receipt.tx_hash)}
                        </a>
                      ))
                    : events
                        .filter((event) => event.tx_hash)
                        .slice(-3)
                        .map((event) => (
                          <a
                            className="receipt-link"
                            key={`${event.kind}-${event.id}`}
                            href={explorerTx(event.tx_hash)}
                            target="_blank"
                            rel="noreferrer"
                          >
                            {event.kind}: {shortHash(event.tx_hash)}
                          </a>
                        ))}
                </b>
              </div>
            </div>
          )}
        </div>

        <aside className="demo-side">
          <p className="side-title">Bond ledger</p>
          <ul className="timeline">
            {(events.length ? events : placeholderEvents(job)).map((event, index) => (
              <li
                className={`event ${event.tx_hash ? "done" : ""} ${event.kind === "settled" && !paid && evaluation ? "fail" : ""}`}
                key={`${event.kind}-${index}`}
              >
                <b>{eventLabel(event.kind)}</b>
                {event.detail && <span>{event.detail}</span>}
                {event.tx_hash && (
                  <span>
                    <a href={explorerTx(event.tx_hash)} target="_blank" rel="noreferrer">
                      {shortHash(event.tx_hash)}
                    </a>
                  </span>
                )}
                {!event.tx_hash && <span>pending</span>}
              </li>
            ))}
          </ul>

          <div className="side-card" style={{ marginTop: 20 }}>
            <p className="side-title">Agent reputation</p>
            {[forge, sable].filter(Boolean).map((agent) => (
              <div className="result-row" key={agent!.id}>
                <span>{agent!.name}</span>
                <b>
                  {agent!.stats.completed} paid · {agent!.stats.failed} slashed ·{" "}
                  {formatNear(agent!.stats.bond_pool)} pool
                </b>
              </div>
            ))}
            <p style={{ marginTop: 12 }}>Bond history is public. A slash is a receipt, not a review.</p>
          </div>

          <div className="side-card">
            <p className="side-title">Key scope</p>
            {keyScope ? (
              <>
                <p>
                  Forge signs bids with a function-call key scoped to{" "}
                  <b className="mono">{keyScope.permission.FunctionCall?.receiver_id ?? "—"}</b>.
                  It cannot transfer, withdraw, or call anything else.
                </p>
                <div className="result-row">
                  <span>Methods</span>
                  <b>{(keyScope.permission.FunctionCall?.method_names ?? []).join(", ") || "all"}</b>
                </div>
                <div className="result-row">
                  <span>Allowance</span>
                  <b>{formatNear(keyScope.permission.FunctionCall?.allowance)}</b>
                </div>
              </>
            ) : (
              <p>Loading key scope…</p>
            )}
          </div>

          <div className="side-card">
            <p className="side-title">The mechanism</p>
            <p>
              The buyer pays into a NEAR contract. The agent locks a bond from its pool. The
              acceptance test is the oracle. If it fails, part of the bond moves to the buyer.
            </p>
            <p>
              Cheat with hardcoded vectors and the hidden acceptance check fails. Edit a test and
              the tamper check slashes 100%. TypeSafe reviews passing patches before they can pay.
            </p>
            <a
              className="receipt-link"
              href="https://testnet.nearblocks.io/address/bonded-v2.testnet"
              target="_blank"
              rel="noreferrer"
            >
              Contract: bonded-v2.testnet ↗
            </a>
          </div>

          {isSettled && !result && (
            <div className="empty">This job is settled. Run another bonded job to see the loop again.</div>
          )}
        </aside>
      </div>
    </div>
  );
}

function prettifyRepo(repoUrl: string): string {
  if (repoUrl.startsWith("preset:")) return repoUrl;
  return repoUrl.replace("https://", "").replace(".git", "");
}

function signedSummary(
  job: JobView["job"] | undefined,
  evaluation: { semantic_verdict: string; semantic_score: number; passed: boolean; logs_hash: string },
): string {
  return `$ ${job?.test_command ?? "npm test"}\n\nverdict: ${evaluation.semantic_verdict}\npassed: ${evaluation.passed}\nscore: ${evaluation.semantic_score / 100}%\nlogs: ${evaluation.logs_hash}`;
}

function eventRank(kind: string): number {
  return ["created", "pool_topup", "bid", "hired", "submitted", "evaluated", "settled"].indexOf(kind) + 1 || 99;
}

function eventLabel(kind: string): string {
  switch (kind) {
    case "created":
      return "Job created";
    case "pool_topup":
      return "Bond pool top-up";
    case "bid":
      return "Bonded bid";
    case "hired":
      return "Agent hired";
    case "submitted":
      return "Patch submitted";
    case "evaluated":
      return "Evaluator verdict";
    case "settled":
      return "Settlement";
    default:
      return kind;
  }
}

function placeholderEvents(job: JobView["job"] | undefined): Array<{
  kind: string;
  detail: string | null;
  tx_hash: string | null;
}> {
  return [
    { kind: "created", detail: job?.title ?? "Awaiting job", tx_hash: null },
    { kind: "bid", detail: "Agent bond locks from its pool", tx_hash: null },
    { kind: "hired", detail: "Buyer hires the bonded agent", tx_hash: null },
    { kind: "submitted", detail: "Agent submits a patch", tx_hash: null },
    { kind: "evaluated", detail: "Test suite + hidden check + TypeSafe", tx_hash: null },
    { kind: "settled", detail: "Payment or slash", tx_hash: null },
  ];
}
