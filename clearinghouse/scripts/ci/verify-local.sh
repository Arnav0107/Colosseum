#!/usr/bin/env bash
# Local reproduction of the Rust / Solana CI jobs, in CI order, with one
# summary line per step. Run from clearinghouse/ (on the Windows dev host,
# `source "$LOCALAPPDATA/xwin-bin/sbf-env.sh"` first). Does not use the
# network, keys or a validator. Exit code: number of failed steps.
#
#   scripts/ci/verify-local.sh            # everything
#   scripts/ci/verify-local.sh quick      # fmt, clippy, cargo test only
set -uo pipefail
cd "$(dirname "$0")/../.."
LOG="${VERIFY_LOG:-target/verify-local.log}"
mkdir -p "$(dirname "$LOG")"
: > "$LOG"
FAIL=0
step() {
  local name="$1"; shift
  echo "=== $name: $*" >> "$LOG"
  if "$@" >> "$LOG" 2>&1; then
    echo "PASS  $name"
  else
    echo "FAIL  $name"
    FAIL=$((FAIL + 1))
  fi
}
counts() { # sum "test result" lines of the last step's output
  awk '/^=== /{buf=""} /test result:/{p+=$4; f+=$6} END{printf "      tests: %d passed, %d failed\n", p, f}' \
    <(sed -n "/^=== $1/,/^=== /p" "$LOG")
}
must_not_compile() {
  ! cargo check -p ch_core --features "$1" >/dev/null 2>&1
}

step fmt cargo fmt --all -- --check
step fmt-svm bash -c "cd crates/ch_svm_tests && cargo fmt --all -- --check"
step fmt-engine cargo fmt --manifest-path programs/mock_perps_engine/Cargo.toml -- --check
step clippy-localnet cargo clippy --workspace --all-targets --features ch_core/mock-oracle,ch_core/dev-venue-signer -- -D warnings
step clippy-prod cargo clippy -p ch_core --all-targets --features pyth,venue-cpi -- -D warnings
step clippy-cpi cargo clippy -p ch_core --all-targets --features mock-oracle,venue-cpi -- -D warnings
step clippy-pyth cargo clippy -p ch_core --all-targets --features pyth,dev-venue-signer -- -D warnings
step clippy-svm bash -c "cd crates/ch_svm_tests && cargo clippy --all-targets -- -D warnings"
step clippy-engine cargo clippy --manifest-path programs/mock_perps_engine/Cargo.toml --all-targets --features instance-a -- -D warnings
step test-engine cargo test --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-a
counts test-engine
step cargo-test cargo test --workspace --features ch_core/mock-oracle,ch_core/dev-venue-signer
counts cargo-test
step cargo-test-pyth cargo test -p ch_core --features pyth,venue-cpi
counts cargo-test-pyth
for f in "" mock-oracle dev-venue-signer pyth venue-cpi mock-oracle,pyth,venue-cpi mock-oracle,venue-cpi,dev-venue-signer; do
  step "must-not-compile[$f]" must_not_compile "$f"
done
[ "${1:-}" = quick ] && exit "$FAIL"

step sbf-localnet cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer
step sbf-cpi cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,venue-cpi --sbf-out-dir target/deploy-cpi
step sbf-pyth cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features pyth,dev-venue-signer --sbf-out-dir target/deploy-pyth
step sbf-prod cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features pyth,venue-cpi --sbf-out-dir target/deploy-prod
for extra in scripts/ci/sbf-extra.sh; do [ -x "$extra" ] && step sbf-extra "$extra"; done
step sbf-mock-a cargo build-sbf --manifest-path programs/mock_perps_a/Cargo.toml
step sbf-mock-b cargo build-sbf --manifest-path programs/mock_perps_b/Cargo.toml
step sbf-engine-a cargo build-sbf --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-a --sbf-out-dir target/deploy-engine-a
step sbf-engine-b cargo build-sbf --manifest-path programs/mock_perps_engine/Cargo.toml --features instance-b --sbf-out-dir target/deploy-engine-b
step litesvm cargo test --manifest-path crates/ch_svm_tests/Cargo.toml
counts litesvm
step idl-build env -u RUSTUP_TOOLCHAIN anchor idl build -p ch_core -o target/idl/ch_core.json -t target/types/ch_core.ts -- --features mock-oracle,dev-venue-signer
step idl-check node scripts/ci/check-idl.cjs
step program-ids node scripts/ci/check-program-ids.cjs
exit "$FAIL"
