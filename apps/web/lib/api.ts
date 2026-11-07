export type Bid = {
  agent: string;
  price: string;
  bond: string;
  released: boolean;
  created_at: number;
};

export type Evaluation = {
  passed: boolean;
  logs_hash: string;
  semantic_verdict: string;
  semantic_score: number;
  evaluated_at: number;
};

export type Job = {
  id: string;
  buyer: string;
  title: string;
  repo_url: string;
  commit: string;
  test_command: string;
  budget: string;
  bond_bps: number;
  state: string;
  bids: Bid[];
  selected_agent: string | null;
  selected_price: string;
  selected_bond: string;
  patch_hash: string | null;
  evaluation: Evaluation | null;
  created_at: number;
  deadline_at: number;
  settled: boolean;
};

export type EventRecord = {
  id: number;
  job_id: string;
  kind: string;
  tx_hash: string | null;
  detail: string | null;
  created_at: string;
};

export type AgentStats = {
  completed: number;
  failed: number;
  bond_slashed: string;
  bond_returned: string;
  active_jobs: number;
  bond_pool: string;
  bond_locked: string;
};

export type AgentView = {
  id: string;
  name: string;
  account_id: string;
  description: string;
  runtime: string;
  stats: AgentStats;
};

export type JobView = {
  job: Job;
  events: EventRecord[];
  agents: AgentView[];
  patch: string | null;
};

export type AgentRun = {
  agent_id: string;
  runtime: string;
  patch_hash: string;
  patch_preview: string;
  duration_ms: number;
};

export type EvaluationView = {
  passed: boolean;
  semantic_verdict: string;
  semantic_score: number;
  typesafe_model: string;
  test_command: string;
  logs: string;
  logs_hash: string;
  tamper_check: boolean;
  duration_ms: number;
  real_fix_probability: number;
  tamper_probability: number;
  review_source: string;
};

export type TxReceipt = {
  kind: string;
  tx_hash: string;
  explorer_url: string;
};

export type RunResult = {
  job: JobView;
  agent_run: AgentRun;
  evaluation: EvaluationView;
  receipts: TxReceipt[];
};

export type RunStep = {
  name: string;
  status: string;
  detail: string | null;
  tx_hash: string | null;
};

export type RunView = {
  run_id: string;
  status: string;
  steps: RunStep[];
  error: string | null;
  result: RunResult | null;
};

export type ArtifactsView = {
  patch: string | null;
  patch_hash: string | null;
  logs: string | null;
  logs_hash: string | null;
  verdict: string | null;
  score: number | null;
  rerun: RerunView | null;
};

export type RerunView = {
  passed: boolean;
  logs_hash: string;
  log_hashes_match: boolean;
};

export type KeyScopeView = {
  account: string;
  public_key: string;
  permission: {
    FunctionCall?: {
      allowance?: string;
      receiver_id?: string;
      method_names?: string[];
    };
  };
};

export type BalanceView = {
  account: string;
  role: string;
  balance_yocto: string | null;
  pool_yocto: string | null;
  locked_yocto: string | null;
};

export type Stats = {
  jobs: number;
  open_jobs: number;
  settled_jobs: number;
  total_budget_yocto: string;
  total_bonds_locked_yocto: string;
  total_slashed_yocto: string;
  total_paid_yocto: string;
};

const BASE = process.env.NEXT_PUBLIC_API_URL ?? "";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
    cache: "no-store",
  });
  const text = await response.text();
  let body: unknown = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  if (!response.ok) {
    const message =
      body && typeof body === "object" && "error" in body
        ? String((body as { error: unknown }).error)
        : `Request failed (${response.status})`;
    throw new Error(message);
  }
  return body as T;
}

export function getJobs() {
  return request<JobView[]>("/api/jobs");
}

export function getJob(id: string) {
  return request<JobView>(`/api/jobs/${id}`);
}

export function getStats() {
  return request<Stats>("/api/stats");
}

export function getAgents() {
  return request<AgentView[]>("/api/agents");
}

export function getBalances() {
  return request<BalanceView[]>("/api/balances");
}

export function getAgentKey(agentId: string) {
  return request<KeyScopeView>(`/api/agents/${agentId}/key`);
}

export function getArtifacts(id: string) {
  return request<ArtifactsView>(`/api/jobs/${id}/artifacts`);
}

export function rerunJob(id: string) {
  return request<RerunView>(`/api/jobs/${id}/rerun`, { method: "POST" });
}

export function registerAgent(input: {
  id: string;
  name: string;
  account_id: string;
  description: string;
}) {
  return request<AgentView>("/api/agents", {
    method: "POST",
    body: JSON.stringify({ ...input, runtime: "external" }),
  });
}

export function seedDemo() {
  return request<JobView>("/api/demo/seed", { method: "POST", body: "{}" });
}

export function startRun(outcome: "success" | "failure" | "cheat" | "tamper") {
  return request<{ run_id: string; status: string }>("/api/demo/run", {
    method: "POST",
    body: JSON.stringify({ outcome }),
  });
}

export function startJobRun(id: string, outcome: "success" | "failure" | "cheat" | "tamper") {
  return request<{ run_id: string; status: string }>(`/api/jobs/${id}/run`, {
    method: "POST",
    body: JSON.stringify({ outcome }),
  });
}

export function getRun(runId: string) {
  return request<RunView>(`/api/runs/${runId}`);
}

/** Poll a run until it completes or fails. Throws on failure. */
export async function pollRun(
  runId: string,
  onStep?: (run: RunView) => void,
  timeoutMs = 300_000,
): Promise<RunResult> {
  const started = Date.now();
  for (;;) {
    const run = await getRun(runId);
    onStep?.(run);
    if (run.status === "complete") {
      if (!run.result) throw new Error("Run completed without a result");
      return run.result;
    }
    if (run.status === "failed") {
      throw new Error(run.error ?? "Run failed");
    }
    if (Date.now() - started > timeoutMs) {
      throw new Error("Run timed out — the demo may be stuck; check the API logs");
    }
    await new Promise((resolve) => setTimeout(resolve, 1500));
  }
}

export function formatNear(yocto: string | undefined, digits = 2): string {
  if (!yocto) return "0 NEAR";
  const value = Number(yocto) / 1_000_000_000_000_000_000_000_000;
  if (!Number.isFinite(value)) return "0 NEAR";
  if (value > 0 && value < 0.01) return `${value.toFixed(4)} NEAR`;
  return `${value.toFixed(digits)} NEAR`;
}

export function shortHash(hash: string | null | undefined, head = 8, tail = 6): string {
  if (!hash) return "—";
  if (hash.length <= head + tail + 1) return hash;
  return `${hash.slice(0, head)}…${hash.slice(-tail)}`;
}

export function explorerTx(hash: string | null | undefined): string {
  if (!hash) return "#";
  return `https://testnet.nearblocks.io/txns/${hash}`;
}

export function stateTone(state: string): "green" | "red" | "neutral" {
  if (state === "Settled") return "green";
  if (state === "Expired") return "red";
  return "neutral";
}

export function paidOf(
  evaluation?: { passed: boolean; semantic_verdict: string } | null,
): boolean {
  return Boolean(evaluation?.passed && evaluation.semantic_verdict === "clean");
}
