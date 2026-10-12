import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "fs";
import * as path from "path";
import { DEFAULT_BOOK, DEFAULT_OPTIONS, runBacktest, summarize } from "./backtest";
import { CurrentParams } from "./calibrate";
import { loadSeries } from "./cli";
import { BAR_MS, REPO_ROOT } from "./data";
import { frontier, paretoFront } from "./frontier";

const current = (): CurrentParams => JSON.parse(readFileSync(path.join(REPO_ROOT, "services/risk/params/localnet.json"), "utf8"));
const { sol, eth } = loadSeries(path.join(REPO_ROOT, "crates/ch_math/tests/data"));
const daily = runBacktest(sol, eth, current(), DEFAULT_OPTIONS);
const bar = runBacktest(sol, eth, current(), { ...DEFAULT_OPTIONS, horizonMs: BAR_MS, stepMs: BAR_MS });
const points = frontier(daily, bar, current(), DEFAULT_BOOK);

test("frontier reproduces the backtest at the parameters in force (haircut 2,000, no hedge break)", () => {
  const p = points.find((x) => x.haircutBps === current().credit.haircut_bps && x.hedgeBreakBps === 0)!;
  const d = summarize("all", daily);
  const b = summarize("all", bar);
  assert.equal(p.breachesDaily, d.breachesWithCh);
  assert.equal(p.breaches20m, b.breachesWithCh);
  assert.equal(p.breachesDailyNoFloor, d.breachesNoFloor);
  const meanFreed = (100 * daily.reduce((s, r) => s + r.credit / r.sumIm, 0)) / daily.length;
  assert.ok(Math.abs(p.freedPctDaily - meanFreed) < 1e-9);
});

test("a higher haircut or hedge break never frees more margin or adds a breach", () => {
  for (const p of points) {
    for (const q of points) {
      if (q.haircutBps >= p.haircutBps && q.hedgeBreakBps >= p.hedgeBreakBps) {
        assert.ok(q.freedPctDaily <= p.freedPctDaily + 1e-12, `${JSON.stringify(q)} vs ${JSON.stringify(p)}`);
        assert.ok(q.breachesDaily <= p.breachesDaily);
        assert.ok(q.breaches20m <= p.breaches20m);
      }
    }
  }
});

test("the floor (A2) never frees more margin or adds a breach compared with the pre-A2 reference", () => {
  for (const p of points) {
    assert.ok(p.freedPctDaily <= p.freedPctDailyNoFloor + 1e-12);
    assert.ok(p.breachesDaily <= p.breachesDailyNoFloor);
    assert.ok(p.breaches20m <= p.breaches20mNoFloor);
  }
});

test("frontier is deterministic and the Pareto set is non-empty and undominated", () => {
  assert.deepEqual(frontier(daily, bar, current(), DEFAULT_BOOK), points);
  const front = paretoFront(points);
  assert.ok(front.length > 0);
  for (const p of front) {
    assert.ok(!points.some((q) => q.freedPctDaily > p.freedPctDaily && q.breachesDaily <= p.breachesDaily && q.breaches20m <= p.breaches20m));
  }
});

test("pre-A2 reference reproduces the committed B1 numbers (27.38% freed at haircut 2,000, two-month sample)", () => {
  // The B1 figure was measured on 2022-11 + 2026-09 only (before B2 added months).
  const two = loadSeries(path.join(REPO_ROOT, "crates/ch_math/tests/data"), ["2022-11", "2026-09"]);
  const d2 = runBacktest(two.sol, two.eth, current(), DEFAULT_OPTIONS);
  const b2 = runBacktest(two.sol, two.eth, current(), { ...DEFAULT_OPTIONS, horizonMs: BAR_MS, stepMs: BAR_MS });
  const p = frontier(d2, b2, current(), DEFAULT_BOOK).find((x) => x.haircutBps === 2_000 && x.hedgeBreakBps === 0)!;
  assert.equal(p.freedPctDailyNoFloor.toFixed(2), "27.38");
  assert.equal(p.breachesDailyNoFloor, 1);
});
