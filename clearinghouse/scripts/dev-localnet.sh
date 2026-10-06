#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${ROOT_DIR}"

echo "=== Building Anchor Programs ==="
anchor build

echo "=== Syncing IDL to Test Bench ==="
mkdir -p testbench/src/idl
cp target/idl/ch_core.json testbench/src/idl/ch_core.json

echo "=== Starting Solana Localnet Validator ==="
# Check if validator is already running
if pgrep -x "solana-test-val" > /dev/null; then
    echo "solana-test-validator already running."
else
    solana-test-validator --reset --quiet &
    VALIDATOR_PID=$!
    echo "Waiting for validator to start (PID: ${VALIDATOR_PID})..."
    sleep 4
fi

echo "=== Deploying Programs to Localnet ==="
anchor deploy

echo "=== Localnet Ready ==="
echo "RPC URL: http://127.0.0.1:8899"
echo "Program ID: $(solana address -k target/deploy/ch_core-keypair.json 2>/dev/null || echo '2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg')"
echo "To run testbench: cd testbench && npm run dev"
