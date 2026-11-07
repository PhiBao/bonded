#!/usr/bin/env bash
# Issue a scoped function-call key for a Bonded agent.
#
# Usage:
#   scripts/scoped-key.sh <agent-account> [allowance-near]
#
# The new key can ONLY call bid/submit_patch/release_bid/view methods on the
# bonded contract. It cannot attach deposits, transfer NEAR, or withdraw the
# bond — that is the point. Prints the public key and stores the secret in
# /tmp/opencode/agent-keys/<account>.json (chmod 600). Write the secret into
# AGENT_*_PRIVATE_KEY in .env so the API signs bids with the scoped key.
set -euo pipefail

AGENT="${1:?agent account required}"
ALLOWANCE="${2:-0.5}"
CONTRACT_ID="${CONTRACT_ID:-bonded-v2.testnet}"

STORE="${AGENT_KEY_STORE:-/tmp/opencode/agent-keys}"
mkdir -p "$STORE"

echo "== scoped key for $AGENT =="
KEY_OUT="$(mktemp)"
near generate-key > "$KEY_OUT" 2>&1 || true
PUB="$(grep -o 'ed25519:[A-Za-z0-9]\+' "$KEY_OUT" | head -1)"
SEC="$(grep -o 'ed25519:[A-Za-z0-9]\+' "$KEY_OUT" | head -2 | tail -1)"
if [ -z "$PUB" ] || [ -z "$SEC" ]; then
  echo "failed to generate keypair" >&2
  exit 1
fi

# Signing requires the account's full-access credentials (present after
# create-account / add-credentials).
near add-key "$AGENT" "$PUB" \
  --contractId "$CONTRACT_ID" \
  --methodNames bid submit_patch release_bid get_job get_jobs get_agent get_pool \
  --allowance "$ALLOWANCE"

KEY_FILE="$STORE/$(echo "$AGENT" | tr '/' '_').json"
cat > "$KEY_FILE" <<EOF
{"account_id":"$AGENT","public_key":"$PUB","secret_key":"$SEC","contract_id":"$CONTRACT_ID"}
EOF
chmod 600 "$KEY_FILE"

echo
echo "public key:  $PUB"
echo "secret file: $KEY_FILE (chmod 600)"
echo
echo "permission (verify on-chain):"
near list-keys "$AGENT" | grep -A6 "$PUB" | head -8
echo
echo "next: set AGENT_*_PRIVATE_KEY=$(cat "$KEY_FILE" | python3 -c 'import json,sys; print(json.load(sys.stdin)["secret_key"])') in .env"
echo "      and AGENT_*_PUBLIC_KEY=$PUB so the API can show the scope UI."
