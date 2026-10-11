/**
 * TypeScript port of ch_math::portfolio and ch_math::risk.
 *
 * Bit-for-bit parity with Rust is enforced by vectors.test.ts, which runs the
 * shared vectors in crates/ch_math/tests/vectors/portfolio.json. Mirror every
 * rounding step, error precedence and overflow bound of the Rust code; do not
 * "simplify" anything here without changing Rust and regenerating vectors.
 */
import { combinedRisk, isqrtCeil } from "./math";

export const MAX_LEGS = 8;
export const MAX_ASSETS = 32;
const PPM = 1_000_000n;
const U64_MAX = (1n << 64n) - 1n;
const I64_MAX = (1n << 63n) - 1n;
const I128_MAX = (1n << 127n) - 1n;
const I128_MIN = -(1n << 127n);

export type PortfolioErrorName =
  | "TooManyLegs"
  | "DuplicateLeg"
  | "InvalidAsset"
  | "UnsupportedAsset"
  | "ZeroNotional"
  | "InvalidMargin"
  | "NegativeVariance"
  | "Overflow";

export class PortfolioError extends Error {
  constructor(public readonly code: PortfolioErrorName) {
    super(code);
    this.name = "PortfolioError";
  }
}

export interface PortfolioLeg {
  venueIndex: number;
  asset: number;
  isLong: boolean;
  notional: bigint;
  initialMargin: bigint;
  maintenanceMargin: bigint;
}

export interface CreditParams {
  haircutBps: number;
  maxCreditPerUser: bigint;
  maxCreditBpsOfIm: number;
  capAtImMinusMm: boolean;
}

export interface PortfolioCredit {
  sumStandaloneRisk: bigint;
  portfolioRisk: bigint;
  hedgeBenefitPpm: number;
  sumInitialMargin: bigint;
  totalBeforeLegCaps: bigint;
  legCredit: bigint[];
}

export interface RiskModel {
  standaloneRisk(leg: PortfolioLeg): bigint;
  portfolioRisk(legs: PortfolioLeg[]): bigint;
}

export const totalCredit = (c: PortfolioCredit): bigint => c.legCredit.reduce((a, b) => a + b, 0n);

const fail = (code: PortfolioErrorName): never => {
  throw new PortfolioError(code);
};
const checkedU64 = (v: bigint): bigint => (v < 0n || v > U64_MAX ? fail("Overflow") : v);
const checkedI128 = (v: bigint): bigint => (v < I128_MIN || v > I128_MAX ? fail("Overflow") : v);
const divCeil = (a: bigint, b: bigint): bigint => (a + b - 1n) / b; // a >= 0, b > 0
/** Rust `x as i16` (two's-complement truncation). */
const asI16 = (x: number): number => ((((x + 32768) % 65536) + 65536) % 65536) - 32768;

export function validateLegs(legs: PortfolioLeg[]): void {
  if (legs.length > MAX_LEGS) fail("TooManyLegs");
  legs.forEach((a, i) => {
    if (a.asset >= MAX_ASSETS) fail("InvalidAsset");
    if (a.notional === 0n) fail("ZeroNotional");
    if (
      a.maintenanceMargin === 0n ||
      a.maintenanceMargin > a.initialMargin ||
      a.initialMargin > a.notional
    ) {
      fail("InvalidMargin");
    }
    for (const b of legs.slice(i + 1)) {
      if (a.venueIndex === b.venueIndex && a.asset === b.asset) fail("DuplicateLeg");
    }
  });
}

export function hedgeBenefitPpm(sumStandalone: bigint, portfolioRisk: bigint): number {
  if (sumStandalone === 0n || portfolioRisk >= sumStandalone) return 0;
  return Number(((sumStandalone - portfolioRisk) * PPM) / sumStandalone);
}

export function hedgedCredit(sumIm: bigint, sumStandalone: bigint, portfolioRisk: bigint, haircutBps: number): bigint {
  if (sumStandalone === 0n || portfolioRisk >= sumStandalone || haircutBps >= 10_000) return 0n;
  const raw = (sumIm * (sumStandalone - portfolioRisk)) / sumStandalone;
  return (raw * BigInt(10_000 - haircutBps)) / 10_000n;
}

export function computePortfolioCredit(legs: PortfolioLeg[], model: RiskModel, params: CreditParams): PortfolioCredit {
  validateLegs(legs);
  let sumStandalone = 0n;
  let sumIm = 0n;
  for (const leg of legs) {
    sumStandalone = checkedU64(sumStandalone + model.standaloneRisk(leg));
    sumIm = checkedU64(sumIm + leg.initialMargin);
  }
  const portfolioRisk = model.portfolioRisk(legs);
  let total = hedgedCredit(sumIm, sumStandalone, portfolioRisk, params.haircutBps);
  if (total > params.maxCreditPerUser) total = params.maxCreditPerUser;

  const legCredit = legs.map((leg) => {
    if (total === 0n || sumIm === 0n) return 0n;
    const share = (total * leg.initialMargin) / sumIm;
    let cap = (leg.initialMargin * BigInt(params.maxCreditBpsOfIm)) / 10_000n;
    if (params.capAtImMinusMm) {
      const room = leg.initialMargin - leg.maintenanceMargin;
      if (room < cap) cap = room;
    }
    return share < cap ? share : cap;
  });

  return {
    sumStandaloneRisk: sumStandalone,
    portfolioRisk,
    hedgeBenefitPpm: hedgeBenefitPpm(sumStandalone, portfolioRisk),
    sumInitialMargin: sumIm,
    totalBeforeLegCaps: total,
    legCredit,
  };
}

/** v0 semantics: initial margin used as risk, netted with the correlation matrix. */
export class MarginAsRiskModel implements RiskModel {
  constructor(private readonly rho: (a: number, b: number) => bigint) {}

  standaloneRisk(leg: PortfolioLeg): bigint {
    return leg.initialMargin;
  }

  portfolioRisk(legs: PortfolioLeg[]): bigint {
    if (legs.length > MAX_LEGS) fail("TooManyLegs");
    const signed = legs.map((l) => {
      if (l.initialMargin > I64_MAX) fail("Overflow");
      return { asset: l.asset, signedRequiredMargin: l.isLong ? l.initialMargin : -l.initialMargin };
    });
    try {
      return combinedRisk(signed, this.rho);
    } catch (e) {
      if ((e as Error).message === "NegativeVariance") fail("NegativeVariance");
      throw e;
    }
  }
}

export interface AssetRisk {
  shockBps: number;
  gapBps: number;
  basisBps: number;
}

export const MAX_SCENARIOS = 8;

export class StressedRiskModel implements RiskModel {
  constructor(
    /** Indexed by asset id; missing or shockBps 0 = unsupported. */
    private readonly assets: AssetRisk[],
    private readonly rho: (a: number, b: number) => bigint,
    private readonly corrStressPpm: number,
    private readonly basisDecorrPpm: number,
    /** Per-asset signed moves in bps (length MAX_ASSETS each). */
    private readonly scenarios: number[][]
  ) {}

  private asset(a: number): AssetRisk {
    if (a >= MAX_ASSETS) fail("InvalidAsset");
    const r = this.assets[a] ?? { shockBps: 0, gapBps: 0, basisBps: 0 };
    if (r.shockBps === 0) fail("UnsupportedAsset");
    return r;
  }

  standaloneRisk(leg: PortfolioLeg): bigint {
    const r = this.asset(leg.asset);
    return checkedU64(divCeil(leg.notional * BigInt(r.shockBps), 10_000n));
  }

  private stressedRho(a: PortfolioLeg, b: PortfolioLeg, sameSign: boolean): bigint {
    if (a.asset === b.asset) {
      const d = BigInt(this.basisDecorrPpm) < PPM ? BigInt(this.basisDecorrPpm) : PPM;
      return sameSign ? PPM : PPM - d;
    }
    let base = this.rho(a.asset, b.asset);
    if (base > PPM) base = PPM;
    if (base < -PPM) base = -PPM;
    let d = BigInt(this.corrStressPpm);
    if (d > 2n * PPM) d = 2n * PPM;
    let r = sameSign ? base + d : base - d;
    if (r > PPM) r = PPM;
    if (r < -PPM) r = -PPM;
    return r;
  }

  parametricRisk(legs: PortfolioLeg[]): bigint {
    if (legs.length > MAX_LEGS) fail("TooManyLegs");
    const x = legs.map((l) => {
      const r = this.standaloneRisk(l);
      return l.isLong ? r : -r;
    });
    let cov = 0n;
    for (let i = 0; i < legs.length; i++) {
      for (let j = 0; j < legs.length; j++) {
        const rho = i === j ? PPM : this.stressedRho(legs[i], legs[j], x[i] >= 0n === x[j] >= 0n);
        const term = checkedI128(checkedI128(x[i] * x[j]) * rho);
        cov = checkedI128(cov + term);
      }
    }
    if (cov < 0n) fail("NegativeVariance");
    const variance = divCeil(cov, PPM);
    return checkedU64(isqrtCeil(variance));
  }

  private loss(legs: PortfolioLeg[], moveOf: (i: number) => number): bigint {
    let pnl = 0n;
    legs.forEach((leg, i) => {
      const side = leg.isLong ? 1n : -1n;
      pnl = checkedI128(pnl + side * leg.notional * BigInt(moveOf(i)));
    });
    if (pnl >= 0n) return 0n;
    return checkedU64(divCeil(-pnl, 10_000n));
  }

  scenarioRisk(legs: PortfolioLeg[]): bigint {
    let worst = 0n;
    const take = (l: bigint) => {
      if (l > worst) worst = l;
    };
    for (const dir of [-1, 1]) {
      const shocks = legs.map((l) => asI16(dir * asI16(this.asset(l.asset).shockBps)));
      take(this.loss(legs, (i) => shocks[i]));
    }
    for (const a of legs.map((l) => l.asset)) {
      const g = asI16(this.asset(a).gapBps);
      for (const s of [g, asI16(-g)]) take(this.loss(legs, (i) => (legs[i].asset === a ? s : 0)));
    }
    legs.forEach((leg, k) => {
      if (!legs.some((o, j) => j !== k && o.asset === leg.asset)) return;
      const b = asI16(this.asset(leg.asset).basisBps);
      for (const s of [b, asI16(-b)]) take(this.loss(legs, (i) => (i === k ? s : 0)));
    });
    for (const sc of this.scenarios) take(this.loss(legs, (i) => sc[legs[i].asset] ?? 0));
    return worst;
  }

  portfolioRisk(legs: PortfolioLeg[]): bigint {
    const p = this.parametricRisk(legs);
    const s = this.scenarioRisk(legs);
    return p > s ? p : s;
  }
}

/**
 * Port of `ch_math::portfolio::cap_groups`: scale every credit in a group down
 * pro rata (floored) when the group's sum exceeds its headroom. A group index
 * outside `headroom` gets 0 (fail safe), as in Rust.
 */
export function capGroups(credits: bigint[], groupOf: (i: number) => number, headroom: bigint[]): bigint[] {
  const nGroups = Math.min(headroom.length, 64);
  const sums = new Array<bigint>(64).fill(0n);
  credits.forEach((c, i) => {
    const g = groupOf(i);
    if (g < nGroups) sums[g] += c;
  });
  return credits.map((c, i) => {
    const g = groupOf(i);
    if (g >= nGroups) return 0n;
    const s = sums[g];
    const h = headroom[g];
    return s > h ? (c * h) / s : c;
  });
}
