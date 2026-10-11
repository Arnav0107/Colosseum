#!/usr/bin/env bash
# One-command localnet demo: build, start a validator with ch_core loaded,
# run the end-to-end smoke (setup, credit, independent verification,
# invalidation) and print the before/after margin table.
#
#   npm run demo            (from clearinghouse/)
#
# Needs: Rust + cargo-build-sbf (Agave), solana-test-validator, Node + npm.
# Localnet only: builds with --features mock-oracle,dev-venue-signer.
# "Deploy" here means the program is loaded at its declare_id! address when
# the validator starts (--bpf-program). The program keypair is not in the
# repository, so `anchor deploy` would land at a different address
# (see scripts/dev-localnet.sh).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"
RPC_URL="http://127.0.0.1:8899"
LEDGER="${DEMO_LEDGER:-$(mktemp -d)}"

for tool in cargo cargo-build-sbf solana-test-validator solana node npm; do
  command -v "$tool" >/dev/null 2>&1 || { echo "demo: missing tool: $tool" >&2; exit 1; }
done
if solana cluster-version --url "$RPC_URL" >/dev/null 2>&1; then
  echo "demo: something is already serving $RPC_URL; stop it so the fresh build is the one loaded." >&2
  exit 1
fi

PROGRAM_ID="$(sed -n 's/^declare_id!("\([^"]*\)");/\1/p' programs/ch_core/src/lib.rs)"

echo "=== 1/4 build ch_core (SBF, mock-oracle + dev-venue-signer: LOCALNET ONLY) ==="
cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer

echo "=== 2/4 start solana-test-validator with ch_core at ${PROGRAM_ID} ==="
solana-test-validator --reset --quiet --ledger "$LEDGER" --bpf-program "$PROGRAM_ID" target/deploy/ch_core.so &
VALIDATOR_PID=$!
trap 'kill "$VALIDATOR_PID" 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do
  solana cluster-version --url "$RPC_URL" >/dev/null 2>&1 && break
  sleep 1
done
solana cluster-version --url "$RPC_URL" >/dev/null || { echo "demo: validator did not start" >&2; exit 1; }

echo "=== 3/4 testbench dependencies ==="
[ -d testbench/node_modules ] || npm --prefix testbench ci

echo "=== 4/4 smoke: setup, credit, verification, invalidation, before/after table ==="
RPC_URL="$RPC_URL" npm --prefix testbench run smoke
