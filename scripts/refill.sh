#!/usr/bin/env bash
# Top up demo balances and agent bond pools on the deployed contract.
#
# Usage:
#   FITTER_ACCOUNT=funder.testnet scripts/refill.sh
#
# FUNDS (all optional overrides):
#   CONTRACT_ID       default bonded-v2.testnet
#   BUYER_ACCOUNT     default buyer-v2.testnet
#   AGENT_A_ACCOUNT   default ag-a.bonded-v2.testnet
#   AGENT_B_ACCOUNT   default agent-v2-6659.testnet
#   FUNDER_ACCOUNT    default fnder-v2.testnet
#   POOL_TOPUP        NEAR per agent pool (default 2)
#   BUYER_TOPUP       NEAR to buyer (default 5)
#
# The funder and accounts need local credentials (~/.near-credentials/testnet).
set -euo pipefail

CONTRACT_ID="${CONTRACT_ID:-bonded-v2.testnet}"
BUYER_ACCOUNT="${BUYER_ACCOUNT:-buyer-v2.testnet}"
AGENT_A_ACCOUNT="${AGENT_A_ACCOUNT:-ag-a.$CONTRACT_ID}"
AGENT_B_ACCOUNT="${AGENT_B_ACCOUNT:-agent-v2-6659.testnet}"
FUNDER_ACCOUNT="${FUNDER_ACCOUNT:-fnder-v2.testnet}"
POOL_TOPUP="${POOL_TOPUP:-2}"
BUYER_TOPUP="${BUYER_TOPUP:-5}"

balance() {
  near state "$1" 2>/dev/null | grep formattedAmount | sed "s/.*'\(.*\)'.*/\1/" || echo 0
}

echo "funder: $FUNDER_ACCOUNT ($(balance "$FUNDER_ACCOUNT") NEAR)"

if [ "$(balance "$BUYER_ACCOUNT" | cut -d. -f1)" -lt 2 ]; then
  echo "buyer is low; sending $BUYER_TOPUP NEAR"
  near send "$FUNDER_ACCOUNT" "$BUYER_ACCOUNT" "$BUYER_TOPUP"
fi

for agent in "$AGENT_A_ACCOUNT" "$AGENT_B_ACCOUNT"; do
  free=$(near view "$CONTRACT_ID" get_pool "{\"account_id\":\"$agent\"}" 2>/dev/null \
    | python3 -c "import sys,re; s=sys.stdin.read(); m=re.search(r\"balance: '(\d+)'\", s); b=int(m.group(1)) if m else 0; m=re.search(r\"locked: '(\d+)'\", s); l=int(m.group(1)) if m else 0; print(int((b-l)/1e24*100)/100)")
  echo "$agent pool free: $free NEAR"
  if [ "$(echo "$free < 0.5" | bc -l)" -eq 1 ]; then
    echo "  topping up pool by $POOL_TOPUP NEAR"
    near call "$CONTRACT_ID" deposit_bond '{}' --accountId "$agent" --deposit "$POOL_TOPUP" --gas 30000000000000
  fi
done

echo "ok"
