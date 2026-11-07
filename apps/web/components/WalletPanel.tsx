"use client";

/**
 * NEAR wallet connection (wallet-selector v10). Connects the buyer's own
 * wallet so they can fund a job from their own account — no demo keys involved.
 */
import { useEffect, useRef, useState } from "react";

type Selector = {
  wallet: () => Promise<Wallet>;
  isSignedIn: () => boolean;
  store: { getState: () => { accounts: Array<{ accountId: string }> } };
};
type Wallet = {
  getAccounts: () => Promise<Array<{ accountId: string }>>;
  signAndSendTransaction: (input: {
    receiverId: string;
    actions: unknown[];
  }) => Promise<{ transaction?: { hash: string } } | { hash?: string }>;
};

const CONTRACT_ID = "bonded-v2.testnet";

let sharedSelector: Selector | null = null;

export default function WalletPanel({
  onConnected,
}: {
  onConnected?: (accountId: string) => void;
}) {
  const selectorRef = useRef<Selector | null>(null);
  const [account, setAccount] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { setupWalletSelector } = await import("@near-wallet-selector/core");
        const { setupMyNearWallet } = await import("@near-wallet-selector/my-near-wallet");
        const { setupMeteorWallet } = await import("@near-wallet-selector/meteor-wallet");
        const selector = (await setupWalletSelector({
          network: "testnet",
          modules: [setupMyNearWallet(), setupMeteorWallet()],
        })) as unknown as Selector;
        if (cancelled) return;
        selectorRef.current = selector;
        sharedSelector = selector;
        if (selector.isSignedIn()) {
          const accounts = selector.store.getState().accounts;
          const accountId = accounts[0]?.accountId ?? null;
          setAccount(accountId);
          if (accountId) onConnected?.(accountId);
        }
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : "Wallet setup failed");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [onConnected]);

  async function connect() {
    setError(null);
    try {
      const { setupModal } = await import("@near-wallet-selector/modal-ui");
      await import("@near-wallet-selector/modal-ui/styles.css");
      if (!selectorRef.current) return;
      const modal = setupModal(selectorRef.current as never, {
        contractId: CONTRACT_ID,
      });
      modal.show();
      // Poll briefly for the sign-in to land.
      for (let i = 0; i < 40; i++) {
        await new Promise((r) => setTimeout(r, 500));
        if (selectorRef.current?.isSignedIn()) {
          const accounts = selectorRef.current.store.getState().accounts;
          const accountId = accounts[0]?.accountId ?? null;
          setAccount(accountId);
          if (accountId) onConnected?.(accountId);
          return;
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Wallet connection failed");
    }
  }

  async function signOut() {
    try {
      const wallet = await selectorRef.current?.wallet();
      await (wallet as unknown as { signOut: () => Promise<void> })?.signOut?.();
    } catch {
      // Ignore; state refresh below covers it.
    }
    setAccount(null);
  }

  if (account) {
    return (
      <div className="wallet-row">
        <span className="pill green">connected</span>
        <b className="mono">{account}</b>
        <button className="button button-ghost" onClick={signOut}>
          Disconnect
        </button>
      </div>
    );
  }

  return (
    <div className="wallet-row">
      <button className="button" onClick={connect}>
        Connect NEAR wallet
      </button>
      <span className="micro">Testnet · MyNEARWallet / Meteor</span>
      {error && <span className="micro error-text">{error}</span>}
    </div>
  );
}

/** Call create_job from the connected wallet with an attached budget. */
export async function createJobWithWallet(
  input: {
    title: string;
    repo_url: string;
    commit: string;
    test_command: string;
    budget_near: number;
    bond_bps: number;
    deadline_secs: number;
  },
): Promise<string> {
  const { actionCreators } = await import("@near-wallet-selector/core");
  const { parseNearAmount } = await import("@near-js/utils");
  const selector = sharedSelector;
  if (!selector) throw new Error("Open the wallet panel and connect first");
  const wallet = await selector.wallet();
  const accounts = await wallet.getAccounts();
  if (!accounts.length) throw new Error("No wallet account connected");
  const { functionCall } = actionCreators as unknown as {
    functionCall: (
      method: string,
      args: Record<string, unknown>,
      gas: bigint,
      deposit: bigint,
    ) => unknown;
  };
  const result = await wallet.signAndSendTransaction({
    receiverId: CONTRACT_ID,
    actions: [
      functionCall(
        "create_job",
        {
          title: input.title,
          repo_url: input.repo_url,
          commit: input.commit,
          test_command: input.test_command,
          bond_bps: input.bond_bps,
          deadline_secs: input.deadline_secs,
        },
        BigInt("100000000000000"),
        BigInt(parseNearAmount(String(input.budget_near)) ?? "0"),
      ),
    ],
  });
  const hash =
    (result as { transaction?: { hash: string } })?.transaction?.hash ??
    (result as { hash?: string })?.hash;
  if (!hash) throw new Error("The wallet did not return a transaction hash");
  return hash;
}
