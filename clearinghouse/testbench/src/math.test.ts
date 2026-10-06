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

  it("negative variance throws error", () => {
    const legs: MathLeg[] = [
      { asset: 0, signedRequiredMargin: 10_000_000_000n },
      { asset: 1, signedRequiredMargin: -10_000_000_000n },
    ];
    const invalidRho = () => 1_500_000n;
    expect(() => combinedRisk(legs, invalidRho)).toThrow("NegativeVariance");
  });
});
