import { describe, it, expect } from "vitest";
import {
  combinedRisk,
  creditTotal,
  splitProRata,
  fundCap,
  liquidationDropBps,
  priceMoveBps,
  isqrtFloor,
  isqrtCeil,
  pairCreditExpectation,
  MathLeg,
} from "./math";

describe("TypeScript Math Port — Golden Vectors", () => {
  it("hedged pair: rho 800_000, SOL long 10B, ETH short 10B", () => {
    const legs: MathLeg[] = [
      { asset: 0, signedRequiredMargin: 10_000_000_000n },
      { asset: 1, signedRequiredMargin: -10_000_000_000n },
    ];

    const rho800k = () => 800_000n;
    const risk = combinedRisk(legs, rho800k);
    expect(risk).toBe(6_324_555_321n);

    const credit = creditTotal(20_000_000_000n, risk, 0);
    expect(credit).toBe(13_675_444_679n);

    const split = splitProRata(credit, [10_000_000_000n, 10_000_000_000n]);
    expect(split).toEqual([6_837_722_339n, 6_837_722_340n]);

    const rho0 = () => 0n;
    expect(combinedRisk(legs, rho0)).toBe(14_142_135_624n);

    const rho1m = () => 1_000_000n;
    expect(combinedRisk(legs, rho1m)).toBe(0n);

    const rhoNeg1m = () => -1_000_000n;
    expect(combinedRisk(legs, rhoNeg1m)).toBe(20_000_000_000n);
  });

  it("same direction legs: [+10B, +10B], rho 800_000", () => {
    const legs: MathLeg[] = [
      { asset: 0, signedRequiredMargin: 10_000_000_000n },
      { asset: 1, signedRequiredMargin: 10_000_000_000n },
    ];

    const rho800k = () => 800_000n;
    const risk = combinedRisk(legs, rho800k);
    expect(risk).toBe(18_973_665_962n);

    const credit = creditTotal(20_000_000_000n, risk, 0);
    expect(credit).toBe(1_026_334_038n);
  });

  it("auxiliary risk formulas", () => {
    expect(fundCap(100_000_000_000n, 200)).toBe(5_000_000_000_000n);

    expect(liquidationDropBps(10_000_000_000n, 50_000_000_000n, 8_000)).toBe(1_600n);
    expect(liquidationDropBps(7_000_000_000n, 50_000_000_000n, 8_000)).toBe(1_120n);
    expect(liquidationDropBps(3_162_277_661n, 50_000_000_000n, 8_000)).toBe(505n);

    expect(priceMoveBps(100_000_000n, 110_000_000n)).toBe(1_000n);
    expect(priceMoveBps(0n, 1n)).toBe(18446744073709551615n);
  });

  it("isqrt edge cases", () => {
    expect(isqrtFloor(0n)).toBe(0n);
    expect(isqrtCeil(0n)).toBe(0n);
    expect(isqrtFloor(1n)).toBe(1n);
    expect(isqrtCeil(1n)).toBe(1n);
    expect(isqrtFloor(2n)).toBe(1n);
    expect(isqrtCeil(2n)).toBe(2n);
    expect(isqrtFloor(4n)).toBe(2n);
    expect(isqrtCeil(4n)).toBe(2n);

    const maxU128 = (1n << 128n) - 1n;
    expect(isqrtFloor(maxU128)).toBe((1n << 64n) - 1n);
    expect(isqrtCeil(maxU128)).toBe(1n << 64n);
  });

  // F12 regression: the testbench initializes the chain with haircut 2000 bps,
  // max_credit_bps_of_required 7500 and max_credit_per_user 100B. On-chain
  // compute_credit then yields 5_470_177_871 for venue A (value observed in
  // the review); the old panel compared it with haircut-0 output 6_837_722_339.
  it("pairCreditExpectation with the testbench's on-chain config reproduces the on-chain credit", () => {
    const legA: MathLeg = { asset: 0, signedRequiredMargin: 10_000_000_000n };
    const legB: MathLeg = { asset: 1, signedRequiredMargin: -10_000_000_000n };
    const cfg = { haircutBps: 2_000, maxCreditPerUser: 100_000_000_000n, maxCreditBpsOfRequired: 7_500 };
    const r = pairCreditExpectation(legA, legB, () => 800_000n, cfg);
    expect(r.combined).toBe(6_324_555_321n);
    expect(r.totalCredit).toBe(10_940_355_743n);
    expect(r.creditA).toBe(5_470_177_871n);
    expect(r.creditB).toBe(5_470_177_872n);

    // The old (buggy) expectation, for the record: haircut 0, no caps.
    const old = splitProRata(creditTotal(20_000_000_000n, r.combined, 0), [10_000_000_000n, 10_000_000_000n]);
    expect(old[0]).toBe(6_837_722_339n);
    expect(old[0]).not.toBe(r.creditA);
  });

  it("pairCreditExpectation applies the per-user and per-leg caps", () => {
    const legA: MathLeg = { asset: 0, signedRequiredMargin: 10_000_000_000n };
    const legB: MathLeg = { asset: 1, signedRequiredMargin: -10_000_000_000n };
    // Perfect hedge: total 20B at haircut 0; per-leg cap 75% binds at 7.5B.
    const capped = pairCreditExpectation(legA, legB, () => 1_000_000n, {
      haircutBps: 0,
      maxCreditPerUser: 100_000_000_000n,
      maxCreditBpsOfRequired: 7_500,
    });
    expect(capped.totalCredit).toBe(20_000_000_000n);
    expect([capped.creditA, capped.creditB]).toEqual([7_500_000_000n, 7_500_000_000n]);
    // Per-user cap binds before the split.
    const userCapped = pairCreditExpectation(legA, legB, () => 1_000_000n, {
      haircutBps: 0,
      maxCreditPerUser: 1_000_000_000n,
      maxCreditBpsOfRequired: 10_000,
    });
    expect(userCapped.totalCredit).toBe(1_000_000_000n);
    expect(userCapped.creditA + userCapped.creditB).toBe(1_000_000_000n);
  });

  it("negative variance throws error", () => {
    const legs: MathLeg[] = [
      { asset: 0, signedRequiredMargin: 10_000_000_000n },
      { asset: 1, signedRequiredMargin: -10_000_000_000n },
    ];
    const invalidRho = () => 1_500_000n;
    expect(() => combinedRisk(legs, invalidRho)).toThrow("NegativeVariance");
  });
});
