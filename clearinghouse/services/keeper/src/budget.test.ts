import { test } from "node:test";
import assert from "node:assert/strict";
import { computeUnitLimitFor, DEFAULT_CU_LIMIT, extraViewRuns, MAX_CU_LIMIT } from "./budget";

const accepts = (m: Record<number, bigint>): bigint[] => Array.from({ length: 64 }, (_, i) => m[i] ?? 0n);

test("A1 budget: no extra view run when everybody or nobody is allowlisted", () => {
  // Mutual allowlist: every view is the full set (joint result reused).
  assert.equal(extraViewRuns([0, 1], accepts({ 0: 0b10n, 1: 0b01n })), 0);
  // Nobody: each view is one venue's lone leg, 0 without computing.
  assert.equal(extraViewRuns([0, 1], accepts({})), 0);
  assert.equal(computeUnitLimitFor([0, 1], accepts({})), null);
  // Same-venue legs: the view is the full set.
  assert.equal(extraViewRuns([0, 0, 0], accepts({})), 0);
});

test("A1 budget: one run per distinct partial view with two or more legs", () => {
  // Venue 0 accepts 1 but not 2; 1 and 2 accept everybody: one partial view {0,1}.
  assert.equal(extraViewRuns([0, 1, 2], accepts({ 0: 0b010n, 1: 0b101n, 2: 0b011n })), 1);
  // Two venues with the same partial view share one run.
  assert.equal(extraViewRuns([0, 1, 2, 3], accepts({ 0: 0b0010n, 1: 0b0001n, 2: 0b1000n, 3: 0b0100n })), 2);
  const units = computeUnitLimitFor([0, 1, 2], accepts({ 0: 0b010n, 1: 0b101n, 2: 0b011n }));
  assert.equal(units, DEFAULT_CU_LIMIT + 80_000);
});

test("A1 budget: the measured worst case (8 venues, 8 distinct 7-leg views) is requested and capped", () => {
  const legs = [0, 1, 2, 3, 4, 5, 6, 7];
  const m: Record<number, bigint> = {};
  for (const v of legs) m[v] = 0xffn & ~(1n << BigInt((v + 1) % 8));
  assert.equal(extraViewRuns(legs, accepts(m)), 8);
  const units = computeUnitLimitFor(legs, accepts(m))!;
  // perf.rs measured 739,513 CU for this book.
  assert.ok(units >= 739_513, `requested ${units}`);
  assert.ok(units <= MAX_CU_LIMIT);
});
