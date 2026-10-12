#!/usr/bin/env python3
"""A9 mutation checks: each mutation is a one-line source change that a test
must catch ("killed" = at least one of its test commands fails).

Run from clearinghouse/ with the Rust/Solana toolchain on PATH:

    python -I scripts/ci/mutate.py            # all mutations
    python -I scripts/ci/mutate.py M8 M11     # a subset

For each mutation: check the target file is clean in git, apply the change,
rebuild what the tests load (SBF when the test runs the compiled program),
run the test commands, then restore the file with `git checkout --`. At the
end the localnet programs are rebuilt from the clean tree. Results go to
stdout and target/mutation-results.json. Exit code = number of SURVIVING
mutations (0 = every mutation killed).
"""
import json
import subprocess
import sys

SBF_LOCAL = "cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle,dev-venue-signer"
SBF_PROD = "cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features pyth,venue-cpi --sbf-out-dir target/deploy-prod"
SVM = "cargo test --manifest-path crates/ch_svm_tests/Cargo.toml"

MUTATIONS = [
    {
        "id": "M5", "what": "A1: views ignore the allowlist (every view = all legs)",
        "file": "crates/ch_math/src/view.rs",
        "find": "let mask = (accepts(v) | own) & present;", "replace": "let mask = present;",
        "build": [SBF_LOCAL],
        "tests": ["cargo test -p ch_math --test view_props", SVM + " --test allowlist"],
    },
    {
        "id": "M6", "what": "A1: usable_credit skips the view_mask check",
        "file": "programs/ch_core/src/state.rs",
        "find": "ch_math::view::view_still_accepted(l.view_mask, venue_index, venue_accepts_now)",
        "replace": "true",
        "build": [],
        "tests": [SVM + " --test allowlist"],
    },
    {
        "id": "M7", "what": "A1: final credit = view credit only (no min with joint)",
        "file": "crates/ch_math/src/view.rs",
        "find": "leg_credit[i] = joint.leg_credit[i].min(view_c);", "replace": "leg_credit[i] = view_c;",
        "build": [SBF_LOCAL],
        "tests": ["cargo test -p ch_math --test view_props", SVM + " --test allowlist"],
    },
    {
        "id": "M8", "what": "A2: floor (sum IM - R_p)+ removed",
        "file": "crates/ch_math/src/portfolio.rs",
        "find": "        .min(sum_im.saturating_sub(portfolio_risk));", "replace": "        ;",
        "build": [],
        "tests": ["cargo test -p ch_math --test portfolio_props", "cargo test -p ch_math --test vectors"],
    },
    {
        "id": "M9", "what": "A3: on-chain model ignores hedge_break_bps",
        "file": "programs/ch_core/src/instructions/portfolio.rs",
        "find": "let r = if p.hedge_break_bps > 0 {", "replace": "let r = if false {",
        "build": [SBF_LOCAL],
        "tests": [SVM + " --test portfolio same_asset_basis_trade"],
    },
    {
        "id": "M10", "what": "A3: lowering hedge_break_bps classified as credit-reducing",
        "file": "programs/ch_core/src/governance.rs",
        "find": "&& new.hedge_break_bps >= old.hedge_break_bps", "replace": "&& true",
        "build": [],
        "tests": ["cargo test -p ch_core --features mock-oracle,dev-venue-signer --lib governance"],
    },
    {
        "id": "M11", "what": "A4: PSD check always accepts",
        "file": "crates/ch_math/src/psd.rs",
        "find": "    if n <= 1 {\n        return true;\n    }", "replace": "    if n <= 100 {\n        return true;\n    }",
        "build": [],
        "tests": ["cargo test -p ch_math --test non_psd", "cargo test -p ch_math --lib psd"],
    },
    {
        "id": "M12", "what": "A5a: per-leg notional cap not checked",
        "file": "programs/ch_core/src/instructions/snapshot.rs",
        "find": "a.notional <= p.max_leg_notional,", "replace": "true,",
        "build": [SBF_LOCAL],
        "tests": [SVM + " --test report_bounds"],
    },
    {
        "id": "M13", "what": "A5b: per-(user, venue) cap counts only the new leg",
        "file": "programs/ch_core/src/instructions/snapshot.rs",
        "find": "portfolio.venue_notional(key.venue_index) <= p.max_venue_notional_per_user as u128,",
        "replace": "a.notional as u128 <= p.max_venue_notional_per_user as u128,",
        "build": [SBF_LOCAL],
        "tests": [SVM + " --test report_bounds"],
    },
    {
        "id": "M14", "what": "A5c: growth limit never checked",
        "file": "programs/ch_core/src/instructions/snapshot.rs",
        "find": "if p.max_notional_growth_bps > 0 {", "replace": "if false {",
        "build": [SBF_LOCAL],
        "tests": [SVM + " --test report_bounds"],
    },
    {
        "id": "M15", "what": "A7: set_roles skips the distinct-roles check",
        "file": "programs/ch_core/src/instructions/admin.rs",
        "find": "        check_roles(self.config.admin, risk_authority, venue_manager, guardian)?;\n",
        "replace": "\n",
        "build": [SBF_PROD],
        "tests": [SVM + " --test production_roles"],
    },
    {
        "id": "M17", "what": "refresh does not release the previous share (counter drift)",
        "file": "programs/ch_core/src/instructions/portfolio.rs",
        "find": "    // Drop the previous computation's share before computing a new one, so a\n    // repeated refresh never double-counts.\n    portfolio.release(config)?;",
        "replace": "    // MUTATION: no release",
        "build": [SBF_LOCAL],
        "tests": [SVM + " --test portfolio", "CH_STATEFUL_SEEDS=1,2,3 " + SVM + " --test stateful"],
    },
]


def sh(cmd):
    r = subprocess.run(cmd, shell=True, capture_output=True, text=True, encoding="utf-8", errors="replace")
    return r.returncode, (r.stdout + r.stderr)[-4000:]


def main(selected):
    results = []
    for m in MUTATIONS:
        if selected and m["id"] not in selected:
            continue
        code, out = sh(f"git diff --quiet -- {m['file']}")
        if code != 0:
            sys.exit(f"{m['file']} has uncommitted changes; commit or stash first")
        # Normalize CRLF (a Windows checkout restores files with CRLF).
        src = open(m["file"], encoding="utf-8").read().replace("
", "
")
        if m["find"] not in src:
            sys.exit(f"{m['id']}: pattern not found in {m['file']}")
        open(m["file"], "w", encoding="utf-8", newline="\n").write(src.replace(m["find"], m["replace"], 1))
        failed_tests = []
        try:
            for b in m["build"]:
                code, out = sh(b)
                if code != 0:
                    failed_tests.append(f"build failed: {b}")
            if not failed_tests:
                for t in m["tests"]:
                    code, out = sh(t)
                    if code != 0:
                        failed_tests.append(t)
        finally:
            sh(f"git checkout -- {m['file']}")
        killed = len(failed_tests) > 0
        results.append({"id": m["id"], "what": m["what"], "killed": killed, "failing": failed_tests})
        print(f"{m['id']}: {'KILLED' if killed else 'SURVIVED'}  {m['what']}  <- {failed_tests}", flush=True)
    print("restoring builds from the clean tree ...", flush=True)
    sh(SBF_LOCAL)
    sh(SBF_PROD)
    json.dump(results, open("target/mutation-results.json", "w", encoding="utf-8"), indent=2)
    survivors = [r["id"] for r in results if not r["killed"]]
    print(f"{len(results) - len(survivors)} of {len(results)} killed; survivors: {survivors}")
    sys.exit(len(survivors))


if __name__ == "__main__":
    main(set(sys.argv[1:]))
