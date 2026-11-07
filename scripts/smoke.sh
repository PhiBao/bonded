#!/usr/bin/env bash
# End-to-end smoke test: seed a job, run the success path, verify settlement.
# Requires the API running (services/api on port 8787) and TYPESAFE_API_KEY
# set for a semantic review (without it the review fails closed by design).
set -euo pipefail

API="${API_URL:-http://127.0.0.1:8787}"

echo "health:"
curl -fsS "$API/api/health"
echo

echo "starting a live demo run"
RUN_FILE="$(mktemp)"
RESULT_FILE="$(mktemp)"
trap 'rm -f "$RUN_FILE" "$RESULT_FILE"' EXIT
curl -fsS -X POST "$API/api/demo/run" \
  -H 'Content-Type: application/json' \
  -d '{"outcome":"success"}' > "$RUN_FILE"
RUN_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["run_id"])' < "$RUN_FILE")"
echo "run: $RUN_ID (polling)"

for _ in $(seq 1 60); do
  curl -fsS "$API/api/runs/$RUN_ID" > "$RESULT_FILE"
  STATUS="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' < "$RESULT_FILE")"
  if [ "$STATUS" = "complete" ] || [ "$STATUS" = "failed" ]; then
    break
  fi
  sleep 3
done

python3 - "$RESULT_FILE" <<'PY'
import json
import sys

run = json.load(open(sys.argv[1]))
status, error = run["status"], run.get("error")
print("run status:", status)
for step in run["steps"]:
    print(f"  - {step['name']}: {step.get('detail') or ''}")
if status != "complete":
    raise AssertionError(error or "run did not complete")

result = run["result"]
job = result["job"]["job"]
ev = result["evaluation"]
print("job:", job["id"], job["state"])
print("agent runtime:", result["agent_run"]["runtime"])
print("test passed:", ev["passed"])
print("semantic verdict:", ev["semantic_verdict"], "·", ev["review_source"])
print("receipts:")
for receipt in result["receipts"]:
    print(" ", receipt["kind"], receipt["explorer_url"])
assert job["state"] == "Settled", job["state"]
assert ev["passed"] is True, ev
assert ev["semantic_verdict"] == "clean", ev
PY

echo "smoke test passed"
