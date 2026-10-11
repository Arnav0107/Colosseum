import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "fs";
import * as path from "path";
import * as B from "./bounds";
import { calibrate, CurrentParams, renderReport } from "./calibrate";
import { loadSeries } from "./cli";
import { REPO_ROOT } from "./data";
import { checkAndRepair, isPsd, minEigenvalue, nearestCorrelation } from "./psd";
import { quantile } from "./stats";

const current = (): CurrentParams => JSON.parse(readFileSync(path.join(REPO_ROOT, "services/risk/params/localnet.json"), "utf8"));
const DATA = path.join(REPO_ROOT, "crates/ch_math/tests/data");

function series() {
  const { sol, eth } = loadSeries(DATA);
  return [
    { name: "SOL", assetId: 0, bars: sol },
    { name: "ETH", assetId: 1, bars: eth },
  ];
}

test("bounds mirror the Rust constants (fails on drift)", () => {
  const rs = (f: string) => readFileSync(path.join(REPO_ROOT, f), "utf8");
  const src = rs("programs/ch_core/src/state.rs") + rs("programs/ch_core/src/governance.rs") + rs("crates/ch_math/src/risk.rs");
  const constant = (name: string): number => {
    const m = new RegExp(`pub const ${name}: [ui]\\d+ = (-?[\\d_]+);`).exec(src);
    assert.ok(m, `constant ${name} not found in Rust sources`);
    return Number(m![1].replace(/_/g, ""));
  };
  assert.equal(B.MIN_SHOCK_BPS, constant("MIN_SHOCK_BPS"));
  assert.equal(B.MAX_MOVE_BPS, constant("MAX_MOVE_BPS"));
  assert.equal(B.MIN_BASIS_BPS, constant("MIN_BASIS_BPS"));
  assert.equal(B.MIN_CORR_STRESS_PPM, constant("MIN_CORR_STRESS_PPM"));
  assert.equal(B.MIN_BASIS_DECORR_PPM, constant("MIN_BASIS_DECORR_PPM"));
  assert.equal(B.PPM_ONE, constant("PPM_ONE"));
  assert.equal(B.RHO_ONE, constant("RHO_ONE"));
  assert.equal(B.MAX_CORR_STEP_PPM, constant("MAX_CORR_STEP_PPM"));
  assert.equal(B.HEDGE_BREAK_MIN_BPS, constant("HEDGE_BREAK_MIN_BPS"));
  assert.equal(B.HEDGE_BREAK_MAX_BPS, constant("HEDGE_BREAK_MAX_BPS"));
});

test("calibration is deterministic and matches the committed output", () => {
  const a = calibrate(series(), current());
  const b = calibrate(series(), current());
  assert.deepEqual(a, b, "same input, same output");
  const golden = JSON.parse(readFileSync(path.join(REPO_ROOT, "services/risk/params/calibration.json"), "utf8"));
  // Re-serialise so float formatting is compared the way it is stored.
  const fresh = JSON.parse(JSON.stringify(a));
  for (const k of ["method", "sample", "estimates", "psd", "clamps", "payloads", "offchain", "diff"]) {
    assert.deepEqual(fresh[k], golden[k], `committed params/calibration.json differs in "${k}"; regenerate with npm run calibrate`);
  }
  assert.equal(renderReport(a), renderReport(b));
});

test("payloads have the exact instruction shapes and respect every bound", () => {
  const c = calibrate(series(), current());
  for (const e of c.payloads.update_correlations.entries) {
    assert.deepEqual(Object.keys(e).sort(), ["a", "b", "rho"]);
    assert.ok(Number.isInteger(e.rho) && Math.abs(e.rho) <= B.RHO_ONE);
    assert.ok(e.a < e.b);
  }
  for (const a of c.payloads.set_asset_risk) {
    assert.deepEqual(Object.keys(a.params).sort(), ["basis_bps", "gap_bps", "shock_bps"]);
    const { shock_bps: s, gap_bps: g, basis_bps: bb } = a.params;
    assert.ok(s >= B.MIN_SHOCK_BPS && s <= B.MAX_MOVE_BPS && g >= s && g <= B.MAX_MOVE_BPS && bb >= B.MIN_BASIS_BPS);
    assert.ok([s, g, bb].every(Number.isInteger));
  }
  const st = c.payloads.set_stress_params;
  assert.ok(st.corr_stress_ppm >= B.MIN_CORR_STRESS_PPM && st.basis_decorr_ppm >= B.MIN_BASIS_DECORR_PPM);
  assert.ok(c.offchain.hedge_break_bps >= B.HEDGE_BREAK_MIN_BPS && c.offchain.hedge_break_bps <= B.HEDGE_BREAK_MAX_BPS);
  // On the committed data the raw SOL shock exceeds the step limit: the clamp is reported, not silent.
  const sol = c.clamps.find((k) => k.field === "SOL.shock_bps");
  assert.ok(sol && sol.raw > 2_000 && sol.value === 2_000, JSON.stringify(c.clamps));
  assert.ok(renderReport(c).includes("SOL.shock_bps"), "every clamp appears in the human-readable report");
});

test("clamping: floors, ceilings, step limits and gap ≥ shock, each one recorded", () => {
  const out: B.Clamp[] = [];
  const cur = { shock_bps: 1_000, gap_bps: 1_000, basis_bps: 100 };
  assert.deepEqual(B.clampAssetRisk("X", { shock_bps: 50, gap_bps: 20, basis_bps: 1 }, null, out), { shock_bps: 100, gap_bps: 100, basis_bps: 10 });
  assert.equal(out.length, 3);
  out.length = 0;
  assert.equal(B.clampAssetRisk("X", { shock_bps: 9_000, gap_bps: 9_000, basis_bps: 100 }, cur, out).shock_bps, 2_000);
  assert.equal(B.clampAssetRisk("X", { shock_bps: 100, gap_bps: 100, basis_bps: 100 }, cur, out).shock_bps, 500);
  assert.equal(B.clampAssetRisk("X", { shock_bps: 999, gap_bps: 999, basis_bps: 100 }, { ...cur, shock_bps: 1_999 }, out).shock_bps, 1_000, "ceil(old/2)");
  assert.equal(B.clampAssetRisk("X", { shock_bps: 20_000, gap_bps: 20_000, basis_bps: 20_000 }, null, out).gap_bps, 10_000);
  assert.ok(out.every((k) => k.raw !== k.value && k.rule.length > 0));
  out.length = 0;
  assert.equal(B.clampCorr("r", 1_400_000, null, out), 1_000_000);
  assert.equal(B.clampCorr("r", 200_000, 800_000, out), 550_000);
  assert.equal(B.clampCorr("r", -900_000, -1_000_000, out), -900_000, "within step: unchanged");
  assert.equal(out.length, 2);
  out.length = 0;
  assert.deepEqual(B.clampStress({ corr_stress_ppm: 1, basis_decorr_ppm: 2_000_000 }, out), { corr_stress_ppm: 50_000, basis_decorr_ppm: 1_000_000 });
  assert.equal(B.clampHedgeBreak(100, out), 2_500);
  assert.equal(B.clampHedgeBreak(12_000, out), 10_000);
  assert.equal(out.length, 4);
  // No clamp when already inside: nothing recorded.
  const none: B.Clamp[] = [];
  B.clampAssetRisk("X", cur, cur, none);
  B.clampCorr("r", 800_000, 800_000, none);
  assert.deepEqual(none, []);
});

test("PSD: a valid matrix is left untouched and reported as such", () => {
  const m = [
    [1, 0.8, 0.3],
    [0.8, 1, 0.5],
    [0.3, 0.5, 1],
  ];
  const { matrix, report } = checkAndRepair(m);
  assert.equal(report.repaired, false);
  assert.equal(report.psd, true);
  assert.strictEqual(matrix, m, "not copied, not modified");
});

test("PSD: a non-PSD matrix is repaired to Higham's published nearest correlation matrix, and the repair is reported", () => {
  // Higham (2002), "Computing the nearest correlation matrix", §4 example.
  const m = [
    [1, 1, 0],
    [1, 1, 1],
    [0, 1, 1],
  ];
  assert.equal(isPsd(m), false);
  const { matrix, report } = checkAndRepair(m);
  assert.equal(report.psd, false);
  assert.equal(report.repaired, true);
  assert.ok(report.minEigenvalueBefore < -0.4);
  assert.ok(report.minEigenvalueAfter! >= -1e-9);
  assert.ok(report.frobeniusDistance! > 0);
  assert.match(report.method!, /Higham/);
  const expected = [
    [1, 0.7607, 0.1573],
    [0.7607, 1, 0.7607],
    [0.1573, 0.7607, 1],
  ];
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) assert.ok(Math.abs(matrix[i][j] - expected[i][j]) < 1e-3, `[${i}][${j}] = ${matrix[i][j]}`);
  }
  assert.deepEqual(m[0], [1, 1, 0], "input not mutated");
});

test("PSD repair keeps symmetry, unit diagonal and [-1, 1] on a random non-PSD matrix", () => {
  let s = 12345;
  const rnd = () => ((s = (Math.imul(s, 1103515245) + 12345) >>> 0) / 2 ** 32) * 2 - 1;
  const n = 6;
  const m = Array.from({ length: n }, () => new Array(n).fill(0));
  for (let i = 0; i < n; i++) {
    m[i][i] = 1;
    for (let j = i + 1; j < n; j++) m[i][j] = m[j][i] = rnd();
  }
  assert.ok(minEigenvalue(m) < 0, "the fixture is not PSD");
  const r = nearestCorrelation(m);
  assert.ok(minEigenvalue(r) >= -1e-9);
  for (let i = 0; i < n; i++) {
    assert.equal(r[i][i], 1);
    for (let j = 0; j < n; j++) assert.ok(r[i][j] === r[j][i] && Math.abs(r[i][j]) <= 1);
  }
});

test("quantile is type-7 linear interpolation", () => {
  assert.equal(quantile([1, 2, 3, 4], 0.5), 2.5);
  assert.equal(quantile([4, 1, 3, 2], 0), 1);
  assert.equal(quantile([4, 1, 3, 2], 1), 4);
  assert.ok(Math.abs(quantile([0, 10], 0.99) - 9.9) < 1e-12);
});
