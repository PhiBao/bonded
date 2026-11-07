#!/usr/bin/env bash
# Minimal CLI for external agents to participate in Bonded jobs.
#
# Usage:
#   scripts/agent-cli.sh deposit <amount-near> [account]
#   scripts/agent-cli.sh bid <job-id> <price-near> <bond-near> [account]
#   scripts/agent-cli.sh submit <job-id> <patch-file> [account]
#   scripts/agent-cli.sh withdraw <amount-near> [account]
#   scripts/agent-cli.sh pool [account]
#
# Uses the caller's local near credentials (~/.near-credentials/testnet).
# For zero-deposit scoped-key bids, point AGENT_ACCOUNT at an account holding
# a function-call key (see scripts/scoped-key.sh) — the CLI signs with that
# key automatically when AGENT_PRIVATE_KEY is set.
set -euo pipefail

CONTRACT_ID="${CONTRACT_ID:-bonded-v2.testnet}"
ACCOUNT="${AGENT_ACCOUNT:-}"
PRIVATE_KEY="${AGENT_PRIVATE_KEY:-}"

call() {
  local method="$1" args="$2" deposit="$3"
  local extra=()
  if [ -n "$PRIVATE_KEY" ]; then
    extra+=(--privateKey "$PRIVATE_KEY")
  fi
  near call "$CONTRACT_ID" "$method" "$args" \
    --accountId "$ACCOUNT" \
    --deposit "$deposit" \
    --gas 100000000000000 \
    "${extra[@]}"
}

yocto() {
  python3 -c "import sys; print(round(float(sys.argv[1]) * 1_000_000_000_000_000_000_000_000))" "$1"
}

cmd="${1:?usage: agent-cli.sh deposit|bid|submit|withdraw|pool ...}"
case "$cmd" in
  deposit)
    amount="${2:?amount required}"
    call deposit_bond '{}' "$amount"
    ;;
  bid)
    job="${2:?job id required}"
    price="${3:?price required}"
    bond="${4:?bond required}"
    call bid "{\"job_id\":\"$job\",\"price\":\"$(yocto "$price")\",\"bond\":\"$(yocto "$bond")\"}" 0
    ;;
  submit)
    job="${2:?job id required}"
    patch_file="${3:?patch file required}"
    hash="$(python3 -c "import hashlib,sys; b=open(sys.argv[1],'rb').read(); print('sha256:'+hashlib.sha256(b).hexdigest())" "$patch_file")"
    echo "patch hash: $hash"
    call submit_patch "{\"job_id\":\"$job\",\"patch_hash\":\"$hash\"}" 0
    ;;
  withdraw)
    amount="${2:?amount required}"
    call withdraw_bond "{\"amount\":\"$(yocto "$amount")\"}" 0
    ;;
  pool)
    near view "$CONTRACT_ID" get_pool "{\"account_id\":\"$ACCOUNT\"}"
    ;;
  *)
    echo "unknown command: $cmd" >&2
    exit 1
    ;;
esac
