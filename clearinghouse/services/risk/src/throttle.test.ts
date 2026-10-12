import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "fs";
import * as path from "path";
import { DEFAULT_OPTIONS, runBacktest, summarize } from "./backtest";
import { CurrentParams } from "./calibrate";
import { loadSeries } from "./cli";
import { BAR_MS, REPO_ROOT } from "./data";
import { emaSeries, linPpm, moveBps, PPM, scalePpm } from "./throttle";

const current = (): CurrentParams => JSON.parse(readFileSync(path.join(REPO_ROOT, "services/risk/params/localnet.json"), "utf8"));

test("throttle: linear scale between start and full, min over legs, 0 start disables", () => {
  const p = { startBps: 50, fullBps: 150 };
  assert.equal(linPpm(0, p), PPM);
  assert.equal(linPpm(50, p), PPM);
  assert.equal(linPpm(100, p), 500_000);
  assert.equal(linPpm(149, p), 10_000);
  assert.equal(linPpm(150, p), 0);
  assert.equal(linPpm(5_000, p), 0);
  assert.equal(linPpm(5_000, { startBps: 0, fullBps: 0 }), PPM, "off");
  assert.equal(scalePpm([10, 100, 40], p), 500_000, "minimum over legs");
  assert.equal(scalePpm([], p), PPM);
  assert.equal(moveBps(100, 103), 300);
  assert.equal(moveBps(100, 97), 300);
  assert.equal(moveBps(100, 100.001), 1, "rounded up");
});

test("throttle: EMA follows closes with a 1-hour time constant", () => {
  const bars = [100, 100, 130].map((c, i) => ({ t: i * BAR_MS, o: c, h: c, l: c, c }));
  const e = emaSeries(bars);
  assert.equal(e[0], 100);
  assert.equal(e[1], 100);
  assert.ok(e[2] > 100 && e[2] < 110, `one bar moves the EMA ~28% of the way: ${e[2]}`);
});

test("B3 evidence (five months): throttle 50/150 bps removes every breach at both horizons; off leaves one at each", () => {
  const { sol, eth } = loadSeries(path.join(REPO_ROOT, "crates/ch_math/tests/data"));
  const run = (t: { startBps: number; fullBps: number }) => {
    const d = summarize("d", runBacktest(sol, eth, current(), { ...DEFAULT_OPTIONS, throttle: t }));
    const b = summarize("b", runBacktest(sol, eth, current(), { ...DEFAULT_OPTIONS, horizonMs: BAR_MS, stepMs: BAR_MS, throttle: t }));
    return [d.breachesThrottled, b.breachesThrottled];
  };
  assert.deepEqual(run({ startBps: 0, fullBps: 0 }), [1, 1]);
  assert.deepEqual(run({ startBps: 50, fullBps: 150 }), [0, 0]);
});
