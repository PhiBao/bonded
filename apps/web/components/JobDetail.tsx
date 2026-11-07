"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import {
  ArtifactsView,
  JobView,
  explorerTx,
  formatNear,
  getArtifacts,
  getJob,
  rerunJob,
  shortHash,
  stateTone,
} from "@/lib/api";

export default function JobDetail({ id, receipt = false }: { id: string; receipt?: boolean }) {
  const [view, setView] = useState<JobView | null>(null);
  const [artifacts, setArtifacts] = useState<ArtifactsView | null>(null);
  const [rerunBusy, setRerunBusy] = useState(false);
  const [rerunMsg, setRerunMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getJob(id)
      .then(setView)
      .catch((err: Error) => setError(err.message));
    getArtifacts(id)
      .then(setArtifacts)
      .catch(() => {});
  }, [id]);

  const job = view?.job;
  const evaluation = job?.evaluation;
  const paid = evaluation?.passed && evaluation.semantic_verdict === "clean";

  return (
    <main>
      <header className="topbar">
        <div className="wrap nav">
          <Link className="brand" href="/">
            <span className="mark">B</span>
            Bonded
          </Link>
          <nav className="navlinks">
            <Link href="/">Live bond</Link>
            <a href="#proof">Proof</a>
            <a
              className="nav-cta"
              href="https://testnet.nearblocks.io/address/bonded-v2.testnet"
              target="_blank"
              rel="noreferrer"
            >
              Contract ↗
            </a>
          </nav>
        </div>
      </header>

      <div className="wrap">
        <div className="page-head">
          <div>
            <div className="kicker">{receipt ? "Public receipt" : "Bond certificate"}</div>
            <h1 className="page-title">{job?.title ?? "Reading the bond…"}</h1>
            <p className="page-sub">
              {job
                ? `${job.id} · ${job.buyer} · ${formatNear(job.budget)} budget · ${job.bond_bps / 100}% bond`
                : "Loading from the NEAR testnet contract."}
            </p>
          </div>
          {job && (
            <span className={`stamp ${stateTone(job.state) === "red" ? "red" : stateTone(job.state) === "green" ? "" : "neutral"}`}>
              {job.state}
            </span>
          )}
        </div>

        {error && <div className="error-box" style={{ marginTop: 24 }}>{error}</div>}

        {view && job && (
          <div className="job-layout">
            <div className="job-doc">
              <div className="job-doc-head">
                <div>
                  <div className="cert-kicker" style={{ border: 0, padding: 0 }}>
                    <span>Performance bond</span>
                    <span>{job.id}</span>
                  </div>
                  <h2 className="cert-title" style={{ marginBottom: 0 }}>
                    {job.title}
                  </h2>
                </div>
                <span className={`pill ${paid ? "green" : evaluation ? "red" : ""}`}>
                  {paid ? "paid" : evaluation ? "slashed" : job.state.toLowerCase()}
                </span>
              </div>
              <div className="job-doc-body">
                <div className="cert-grid" style={{ marginBottom: 22 }}>
                  <div className="cert-field">
                    <span className="cert-label">Repository</span>
                    <span className="cert-value mono">{shortHash(job.repo_url, 34, 8)}</span>
                  </div>
                  <div className="cert-field">
                    <span className="cert-label">Commit</span>
                    <span className="cert-value mono">{job.commit}</span>
                  </div>
                  <div className="cert-field">
                    <span className="cert-label">Acceptance test</span>
                    <span className="cert-value mono">{job.test_command}</span>
                  </div>
                  <div className="cert-field">
                    <span className="cert-label">Deadline</span>
                    <span className="cert-value mono">
                      {new Date(job.deadline_at * 1000).toISOString().replace(".000Z", "Z")}
                    </span>
                  </div>
                  <div className="cert-field">
                    <span className="cert-label">Selected agent</span>
                    <span className="cert-value mono">{job.selected_agent ?? "not hired"}</span>
                  </div>
                  <div className="cert-field">
                    <span className="cert-label">Bond at risk</span>
                    <span className="cert-value mono">{formatNear(job.selected_bond || job.budget)}</span>
                  </div>
                </div>

                <p className="side-title">Bids</p>
                <div className="bids" style={{ marginTop: 0 }}>
                  {job.bids.map((bid) => {
                    const agent = view.agents.find((item) => item.account_id === bid.agent);
                    return (
                      <div className="bid" key={bid.agent}>
                        <span className="name">{agent?.name ?? bid.agent}</span>
                        <span className="mono">{formatNear(bid.price)}</span>
                        <span className="mono">{formatNear(bid.bond)} bond</span>
                        <span
                          className={`pill ${
                            paid && job.selected_agent === bid.agent
                              ? "green"
                              : evaluation && job.selected_agent === bid.agent
                                ? "red"
                                : bid.released
                                  ? ""
                                  : "green"
                          }`}
                        >
                          {paid && job.selected_agent === bid.agent
                            ? "paid"
                            : evaluation && job.selected_agent === bid.agent
                              ? "slashed"
                              : bid.released
                                ? "released"
                                : "bonded"}
                        </span>
                      </div>
                    );
                  })}
                  {!job.bids.length && <div className="empty">No bids yet.</div>}
                </div>

                <div className="terminal" style={{ marginTop: 22 }}>
                  <div className="terminal-head">
                    <span>Evaluator output</span>
                    <span>{evaluation ? evaluation.logs_hash : "not evaluated"}</span>
                  </div>
                  <pre>
                    {evaluation
                      ? `verdict: ${evaluation.semantic_verdict}\nscore: ${evaluation.semantic_score / 100}%\npassed: ${evaluation.passed}\nlogs: ${evaluation.logs_hash}`
                      : "The evaluator has not run yet. The job is bonded and waiting for work."}
                  </pre>
                </div>

                {artifacts?.patch && (
                  <div className="terminal" style={{ marginTop: 14 }}>
                    <div className="terminal-head">
                      <span>Submitted patch</span>
                      <span>{artifacts.patch_hash}</span>
                    </div>
                    <pre>{artifacts.patch.slice(0, 4000)}</pre>
                  </div>
                )}

                {artifacts?.logs && (
                  <div className="terminal" style={{ marginTop: 14 }}>
                    <div className="terminal-head">
                      <span>Evaluator logs · stored</span>
                      <span>{artifacts.logs_hash}</span>
                    </div>
                    <pre>{artifacts.logs.slice(0, 4000)}</pre>
                  </div>
                )}

                {job.patch_hash && (
                  <div style={{ marginTop: 14 }}>
                    <button
                      className="button"
                      disabled={rerunBusy}
                      onClick={async () => {
                        setRerunBusy(true);
                        setRerunMsg(null);
                        try {
                          const r = await rerunJob(job.id);
                          setRerunMsg(
                            r.log_hashes_match
                              ? `Re-run reproduced the verdict: ${r.logs_hash.slice(0, 22)}… (passed=${r.passed})`
                              : `Re-run drifted: ${r.logs_hash.slice(0, 22)}…`,
                          );
                          const refreshed = await getArtifacts(job.id);
                          setArtifacts(refreshed);
                        } catch (err) {
                          setRerunMsg(err instanceof Error ? err.message : "Re-run failed");
                        } finally {
                          setRerunBusy(false);
                        }
                      }}
                    >
                      {rerunBusy ? "Re-running evaluation…" : "Re-run this evaluation"}
                    </button>
                    {artifacts?.rerun?.log_hashes_match && (
                      <span className="pill green" style={{ marginLeft: 10 }}>
                        hash reproduced ✓
                      </span>
                    )}
                    {rerunMsg && <p className="micro" style={{ marginTop: 8 }}>{rerunMsg}</p>}
                  </div>
                )}

                {view.events.length > 0 && (
                  <div style={{ marginTop: 22 }} id="proof">
                    <p className="side-title">On-chain events</p>
                    <ul className="timeline">
                      {[...view.events]
                        .sort((a, b) => {
                          const rank = eventRank(a.kind) - eventRank(b.kind);
                          return rank !== 0 ? rank : a.id - b.id;
                        })
                        .map((event) => (
                        <li className="event done" key={event.id}>
                          <b>{event.kind}</b>
                          {event.detail && <span>{event.detail}</span>}
                          {event.tx_hash && (
                            <span>
                              <a href={explorerTx(event.tx_hash)} target="_blank" rel="noreferrer">
                                {shortHash(event.tx_hash)}
                              </a>
                            </span>
                          )}
                        </li>
                        ))}
                    </ul>
                  </div>
                )}
              </div>
            </div>

            <aside className="job-side">
              <div className="side-card">
                <p className="side-title">Settlement</p>
                <div className="result-row">
                  <span>State</span>
                  <b>{job.state}</b>
                </div>
                <div className="result-row">
                  <span>Test</span>
                  <b>{evaluation ? (evaluation.passed ? "pass" : "fail") : "pending"}</b>
                </div>
                <div className="result-row">
                  <span>TypeSafe</span>
                  <b>{evaluation ? `${evaluation.semantic_verdict} · ${evaluation.semantic_score / 100}%` : "pending"}</b>
                </div>
                <div className="result-row">
                  <span>Outcome</span>
                  <b>{paid ? "agent paid + bond returned" : evaluation ? "buyer refunded + bond slashed" : "pending"}</b>
                </div>
                <a className="receipt-link" href={`/receipts/${job.id}`}>
                  Open public receipt →
                </a>
              </div>

              <div className="side-card">
                <p className="side-title">Agents</p>
                {view.agents.map((agent) => (
                  <div className="result-row" key={agent.id}>
                    <span>{agent.name}</span>
                    <b>
                      {agent.stats.completed} paid · {agent.stats.failed} slashed
                    </b>
                  </div>
                ))}
              </div>

              <div className="side-card">
                <p className="side-title">Verify it yourself</p>
                <p>
                  Every event above is a NEAR testnet transaction. The contract
                  is public, the test fixture is in the repo, and the slash is a
                  receipt.
                </p>
                <a
                  className="receipt-link"
                  href="https://testnet.nearblocks.io/address/bonded-v2.testnet"
                  target="_blank"
                  rel="noreferrer"
                >
                  bonded-v2.testnet ↗
                </a>
              </div>
            </aside>
          </div>
        )}
      </div>

      <footer className="wrap footer">
        <p>Bonded · AI agents with skin in the game · NEAR HACKATHON 2026</p>
        <div className="footer-links">
          <Link href="/">Live bond</Link>
          <a href="https://nearhackathon.com/" target="_blank" rel="noreferrer">
            Hackathon
          </a>
        </div>
      </footer>
    </main>
  );
}

function eventRank(kind: string): number {
  return ["created", "bid", "hired", "submitted", "evaluated", "settled"].indexOf(kind) + 1 || 99;
}
