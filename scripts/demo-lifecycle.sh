#!/usr/bin/env bash
# End-to-end Umbra stealth-payment lifecycle demo.
#
# Starts a fresh solana-test-validator with the pinboard program loaded
# at G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P, runs the
# `lifecycle` example, then tears down.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROGRAM_ID="G2zSN8WVP9TujyNCtXRW3nvNqymUW7QiuxB273UF9z6P"
PROGRAM_SO="target/deploy/pinboard.so"

# 1. Ensure pinboard is built.
if [[ ! -f "$PROGRAM_SO" ]]; then
  echo "Pinboard .so not found; building..."
  ./scripts/build.sh
fi

# 2. Stop any stray validator from a previous run.
pkill -f solana-test-validator 2>/dev/null || true
rm -rf test-ledger 2>/dev/null || true

# 3. Start the validator with pinboard preloaded.
echo "Starting solana-test-validator..."
solana-test-validator \
  --bpf-program "$PROGRAM_ID" "$PROGRAM_SO" \
  --reset \
  --quiet \
  > /tmp/umbra-lifecycle-validator.log 2>&1 &
VALIDATOR_PID=$!
trap 'kill "$VALIDATOR_PID" 2>/dev/null || true; rm -rf test-ledger 2>/dev/null || true' EXIT

# 4. Wait for it to be ready.
echo "Waiting for validator (max 60s)..."
for i in {1..120}; do
  if solana --url http://127.0.0.1:8899 cluster-version > /dev/null 2>&1; then
    echo "Validator ready (after ~$((i / 2))s)"
    break
  fi
  if [[ $i -eq 120 ]]; then
    echo "Validator did not become ready in 60s. Last log lines:"
    tail -20 /tmp/umbra-lifecycle-validator.log
    exit 1
  fi
  sleep 0.5
done

# 5. Run the lifecycle.
echo
cargo run --release \
  --manifest-path crates/umbra-sdk/Cargo.toml \
  --example lifecycle

# 6. Cleanup happens via the EXIT trap.
echo
echo "Demo complete — tearing down validator."
