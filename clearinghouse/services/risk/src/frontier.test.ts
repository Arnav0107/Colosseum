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
  assert.equal(p.breachesDailyFloor, d.breachesRiskFloorProposal);
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

test("the floor never frees more margin or adds a breach", () => {
  for (const p of points) {
    assert.ok(p.freedPctDailyFloor <= p.freedPctDaily + 1e-12);
    assert.ok(p.breachesDailyFloor <= p.breachesDaily);
    assert.ok(p.breaches20mFloor <= p.breaches20m);
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
