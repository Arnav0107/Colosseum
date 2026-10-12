import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "fs";
import * as path from "path";
import { bookLegs, creditFor, DEFAULT_BOOK, DEFAULT_OPTIONS, effectiveFrom, hypothetical, realizedLoss, runBacktest, stressedModel } from "./backtest";
import { assertNoLookAhead, calibrate, CurrentParams } from "./calibrate";
import { loadSeries } from "./cli";
import { Bar, BAR_MS, DAY_MS, REPO_ROOT } from "./data";

const current = (): CurrentParams => JSON.parse(readFileSync(path.join(REPO_ROOT, "services/risk/params/localnet.json"), "utf8"));
const DATA = path.join(REPO_ROOT, "crates/ch_math/tests/data");

test("engine sanity: the initial params give the pinned worked-example credit (7,111,111,110)", () => {
  // Same number as testbench fixtures, crates/ch_math/tests/vectors and the LiteSVM suite.
  assert.equal(creditFor(effectiveFrom(current()), DEFAULT_BOOK, current().credit, false), 7_111_111_110n);
  const legs = bookLegs(DEFAULT_BOOK);
  assert.equal(legs[0].initialMargin, 10_000_000_000n);
  assert.equal(legs[0].maintenanceMargin, 5_000_000_000n);
});

test("look-ahead guard: a bar closing after the cutoff is rejected", () => {
  const bar = (t: number): Bar => ({ t, o: 1, h: 1, l: 1, c: 1 });
  const cutoff = 10 * BAR_MS;
  assert.doesNotThrow(() => assertNoLookAhead([{ name: "X", assetId: 0, bars: [bar(0), bar(9 * BAR_MS)] }], cutoff));
  assert.throws(() => assertNoLookAhead([{ name: "X", assetId: 0, bars: [bar(0), bar(9 * BAR_MS + 1)] }], cutoff), /look-ahead/);
  const { sol, eth } = loadSeries(DATA);
  const early = sol[2_000].t;
  assert.throws(
    () =>
      calibrate(
        [
          { name: "SOL", assetId: 0, bars: sol },
          { name: "ETH", assetId: 1, bars: eth },
        ],
        current(),
        undefined,
        early
      ),
    /look-ahead/
  );
});

test("no look-ahead: corrupting all data after a date leaves every earlier day's parameters and credit unchanged", () => {
  const { sol, eth } = loadSeries(DATA);
  const split = Date.UTC(2022, 10, 15); // 2022-11-15 00:00
  const corrupt = (bars: Bar[]) => bars.map((b) => (b.t >= split ? { ...b, o: b.o * 3, h: b.h * 3, l: b.l * 0.1, c: b.c * 0.2 } : b));
  const a = runBacktest(sol, eth, current(), DEFAULT_OPTIONS);
  const b = runBacktest(corrupt(sol), corrupt(eth), current(), DEFAULT_OPTIONS);
  // Days whose parameters were fixed from data before the split. The loss
  // window of day d is [d, d+1 day), so compare parameters and credit only.
  const before = (r: { date: string }) => Date.parse(r.date) - DAY_MS < split;
  const pa = a.filter(before);
  const pb = b.filter(before);
  assert.ok(pa.length >= 14);
  assert.deepEqual(
    pb.map((r) => [r.date, r.params, r.credit, r.dataCutoff]),
    pa.map((r) => [r.date, r.params, r.credit, r.dataCutoff])
  );
  // And it is not vacuous: later days do change.
  const after = (r: { date: string }) => r.date >= "2022-11-17" && r.date < "2022-12";
  assert.notDeepEqual(
    b.filter(after).map((r) => r.params),
    a.filter(after).map((r) => r.params)
  );
});

test("every day's data cutoff is at least one timelock before the day it is used", () => {
  const { sol, eth } = loadSeries(DATA);
  const rows = runBacktest(sol, eth, current(), DEFAULT_OPTIONS);
  assert.equal(rows.length, 60);
  for (const r of rows) assert.ok(Date.parse(r.dataCutoff) <= Date.parse(r.date) - DAY_MS, r.date);
});

test("backtest is deterministic", () => {
  const { sol, eth } = loadSeries(DATA);
  assert.deepEqual(runBacktest(sol, eth, current(), DEFAULT_OPTIONS), runBacktest(sol, eth, current(), DEFAULT_OPTIONS));
});

test("realized loss: long leg uses lows, short leg uses highs, from the window's opening prices", () => {
  const sol: Bar[] = [
    { t: 0, o: 100, h: 101, l: 90, c: 95 },
    { t: BAR_MS, o: 95, h: 96, l: 80, c: 85 },
  ];
  const eth: Bar[] = [
    { t: 0, o: 200, h: 210, l: 199, c: 205 },
    { t: BAR_MS, o: 205, h: 220, l: 200, c: 201 },
  ];
  const r = realizedLoss(sol, eth, 0, 2 * BAR_MS, { notionalUsd: 1_000, imBps: 2_000, mmBps: 1_000 });
  // bar 2: SOL low 80 (−20%) → $200; ETH high 220 (+10%) → $100.
  assert.ok(Math.abs(r.pathLoss - 300) < 1e-9);
  // closes: bar 2 SOL 85 (−15%) → $150, ETH 201 (+0.5%) → $5.
  assert.ok(Math.abs(r.closeLoss - 155) < 1e-9);
});

test("HYPOTHETICAL decorrelation is labelled and computed from the stated moves", () => {
  const h = hypothetical("HYPOTHETICAL x", -0.3, 0.15, effectiveFrom(current()), "initial", current());
  assert.match(h.label, /^HYPOTHETICAL/);
  assert.equal(h.loss, 22_500);
  assert.equal(h.sumIm, 20_000);
  assert.equal(h.breachBaseline, true);
});

test("floor (A2): scaling every shock and gap now removes credit once R_p reaches Σ IM", () => {
  // Before A2 (KNOWN_LIMITATIONS #19) credit stayed $7,111.11 at x1, x2 and x4,
  // because Σ IM · (1 − R_p / S) · (1 − haircut) depends only on shock ratios.
  // With credit ≤ (Σ IM − R_p)⁺ it still is $7,111.11 while the floor is slack
  // (x1: Σ IM − R_p = $15,000; x2: $10,000) and drops to 0 at x4 (R_p = Σ IM).
  const base = effectiveFrom(current());
  const scaled = (k: number) => ({
    ...base,
    sol: { ...base.sol, shock_bps: base.sol.shock_bps * k, gap_bps: base.sol.gap_bps * k },
    eth: { ...base.eth, shock_bps: base.eth.shock_bps * k, gap_bps: base.eth.gap_bps * k },
  });
  const legs = bookLegs(DEFAULT_BOOK);
  assert.equal(creditFor(scaled(1), DEFAULT_BOOK, current().credit, false), 7_111_111_110n, "shocks x1");
  assert.equal(creditFor(scaled(2), DEFAULT_BOOK, current().credit, false), 7_111_111_110n, "shocks x2");
  assert.equal(stressedModel(scaled(4)).portfolioRisk(legs), 20_000_000_000n, "x4: R_p equals Σ IM");
  assert.equal(creditFor(scaled(4), DEFAULT_BOOK, current().credit, false), 0n, "x4: floor gives 0");
});
