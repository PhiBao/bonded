# Bonded — 90-second demo script

The demo is designed to be recorded in one take, with a reliable fallback.

## Pre-flight

```bash
# terminal 1
cargo run --manifest-path services/api/Cargo.toml

# terminal 2
pnpm --dir apps/web dev
```

Open the live demo instead (recommended — same code, same contract):
**https://bonded-gamma.vercel.app**. Confirm the header says **LIVE BOND ·
NEAR TESTNET** and the page shows a job.

Have these tabs open:

1. The Bonded landing page.
2. `https://testnet.nearblocks.io/address/bonded-v2.testnet`.
3. The live demo: https://bonded-gamma.vercel.app
3. The public job receipt page (the page link appears in the demo result).

## Script (90 seconds)

### 0:00 — Hook

**On screen:** the hero headline.

**Voiceover:** "AI agents fail 70 to 95 percent of the time in production. When
one fails your job, you still pay. Bonded makes the agent pay instead."

### 0:12 — The mechanism

**On screen:** scroll to the live bond card. Point at the budget, 20% bond, and
`npm test`.

**Voiceover:** "The buyer funds the job in a NEAR contract. The agent locks 20%
of its own money as a bond. The acceptance test decides who gets paid."

### 0:25 — Success path (paid)

**Action:** click **Run live bond demo**.

**On screen:** the job is created, two agents bid with scoped keys, Forge is
hired, a live agent writes the patch, the test runs in Docker, TypeSafe
reviews it, the contract pays Forge and returns the bond.

**Voiceover:** "Forge fixed the failing test. The test passes, TypeSafe says it
is a clean fix, and the contract releases payment and returns the bond."

**Point at:** the green **PAID** stamp and the three receipts. Open one receipt
in NearBlocks.

### 0:50 — Failure path (50% slash)

**Action:** click **Failure path · 50% slash**.

**On screen:** Sable submits a patch that fails the test. Half the bond is
slashed to the buyer.

**Voiceover:** "Sable shipped a patch that fails the test. The buyer is refunded,
and half of Sable's bond is slashed. No dispute ticket. No support queue. The
test is the oracle — and an honest failure costs half, not everything."

**Point at:** the red **SLASHED** stamp, the pool dropping, and the receipt.

### 1:05 — Cheat path (hidden tests)

**Action:** click **Cheat path · hidden tests**.

**On screen:** Sable hardcodes the two visible test vectors. Visible tests
pass — then the hidden acceptance check fails.

**Voiceover:** "Sable's patch passes every test you can see. The hidden
acceptance check — values the agent never saw — fails it. The bond still pays
the buyer."

### 1:20 — Tamper path (100% slash)

**Action:** click **Tamper path · 100% slash**.

**On screen:** Sable edits a test file to skip a test. The tamper check flags
it before any model judgment, and the FULL bond is slashed.

**Voiceover:** "Edit the test to make it pass? The whole bond is gone. Honest
failure costs half. Gaming the acceptance criteria costs everything."

**Point at:** verdict `test_manipulation`, the pool dropping the full 0.10,
and the public receipt.

### 1:35 — Trust layer

**On screen:** scroll to **Agent reputation**, **Key scope**, and **The
mechanism**.

**Voiceover:** "Bond history is public and portable. And the agent's key is
scoped — it can bid and submit, but it cannot touch the bond. A slash is a
receipt, not a review. Agents that pass build a capital-backed reputation."

### 1:45 — Close

**On screen:** the contract link and the repo.

**Voiceover:** "This is Bonded: AI agents with skin in the game. Built on NEAR
testnet for NEAR HACKATHON 2026. The contract, Docker evaluator, TypeSafe
review, and settlement are real. Try it at bonded-gamma.vercel.app."

## X post draft

Do not post automatically. The human publishes this and adds the video.

> AI agents fail 70–95% of the time in production. When one fails your job, you
> still pay.
>
> We built Bonded: agents stake their own money on the work they sell. If the
> acceptance test fails, the buyer is paid from the agent's bond automatically.
>
> Built at NEAR HACKATHON 2026. Watch an agent lose its bond 👇
>
> @NEARProtocol #NEARHackathon #NEARBuilders

Attach the 90-second video or the two screenshots (`docs/screenshot-demo.png` and
`docs/screenshot-slash.png`). Optional tags: `@NearLegion`, `@NearBuilders`.

## Reliability notes

- The default demo agents are deterministic (`reference` runtime). The bond,
  test runner, TypeSafe review, and settlement are real. This is stated in the
  README; do not claim the demo agent is a live model.
- The live OpenCode runtime is experimental and not the default. Do not enable
  it for the recorded demo unless it has been rehearsed in an isolated
  workspace.
- If the API is unavailable during recording, use the recorded fallback and show
  the public contract/receipt links. Do not edit or invent transactions.
- Keep the demo job IDs visible so the receipts can be checked afterwards.
