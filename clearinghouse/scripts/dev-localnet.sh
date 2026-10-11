#!/usr/bin/env bash
# Localnet for the testbench: build ch_core with the MOCK oracle, start a
# solana-test-validator with the program preloaded at its declare_id! address,
# and sync the IDL into the testbench.
#
# The program is loaded with --bpf-program instead of `anchor deploy`: the
# program keypair for 2XcTzg5k... is not in the repository, so a deploy would
# land at a different address than declare_id! and every call would fail with
# DeclaredProgramIdMismatch.
#
# This script does NOT fund testbench accounts. The testbench airdrops its own
# dev keys from the UI ("Airdrop" step).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

PROGRAM_ID="$(grep -oP 'declare_id!\("\K[^"]+' programs/ch_core/src/lib.rs)"

echo "=== Building ch_core (SBF, --features mock-oracle: LOCALNET ONLY) ==="
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle

echo "=== Generating IDL and syncing it to the testbench ==="
anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts -- --features mock-oracle
mkdir -p testbench/src/idl
cp target/idl/ch_core.json testbench/src/idl/ch_core.json

echo "=== Starting solana-test-validator with ch_core at ${PROGRAM_ID} ==="
if pgrep -x "solana-test-val" > /dev/null; then
    echo "solana-test-validator is already running; stop it first so the fresh build is loaded." >&2
    exit 1
fi
solana-test-validator --reset --quiet \
    --bpf-program "${PROGRAM_ID}" target/deploy/ch_core.so &
VALIDATOR_PID=$!
echo "Waiting for validator (PID ${VALIDATOR_PID})..."
for _ in $(seq 1 60); do
    if solana cluster-version --url http://127.0.0.1:8899 >/dev/null 2>&1; then break; fi
    sleep 1
done

echo "=== Localnet ready ==="
echo "RPC URL:    http://127.0.0.1:8899"
echo "Program ID: ${PROGRAM_ID}"
echo "Testbench:  cd testbench && npm run dev   (fund dev keys from the UI)"
