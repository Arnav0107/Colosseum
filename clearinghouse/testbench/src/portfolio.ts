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
  /** A4: correlation sub-matrix not positive definite; R_p = max(S, scenarios). */
  psdFallback: boolean;
}

export interface RiskModel {
  standaloneRisk(leg: PortfolioLeg): bigint;
  portfolioRisk(legs: PortfolioLeg[]): bigint;
  /** Port of `RiskModel::portfolio_risk_report` (default: no fallback). */
  portfolioRiskReport?(legs: PortfolioLeg[]): [bigint, boolean];
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
  const [portfolioRisk, psdFallback] = model.portfolioRiskReport ? model.portfolioRiskReport(legs) : [model.portfolioRisk(legs), false];
  let total = hedgedCredit(sumIm, sumStandalone, portfolioRisk, params.haircutBps);
  if (total > params.maxCreditPerUser) total = params.maxCreditPerUser;
  // Floor (A2): credit ≤ (ΣIM − R_p)⁺, as in Rust.
  const floor = sumIm > portfolioRisk ? sumIm - portfolioRisk : 0n;
  if (total > floor) total = floor;

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
    psdFallback,
  };
}

/** Port of `ch_math::psd` (A4): Crout LDLᵀ, entries ρ_ppm·2^30, l at 2^50, one reciprocal per pivot. */
export const PSD_PPM_SHIFT = 30n;
export const PSD_SCALE = 1_000_000n << PSD_PPM_SHIFT;
export const PSD_MIN_PIVOT = 1n << PSD_PPM_SHIFT;
export const PSD_L_SHIFT = 50n;
export const PSD_MAX_DIM = 8;

const inI128 = (v: bigint): boolean => v >= I128_MIN && v <= I128_MAX;
/** Rust `x >> k` on i128 (arithmetic, floors); BigInt >> also floors. */
const shrCeil = (x: bigint, k: bigint): bigint => -(-x >> k);

/** Port of `corr_is_positive_definite`. Overflow anywhere = false, as in Rust. */
export function corrIsPositiveDefinite(assets: number[], rho: (a: number, b: number) => bigint): boolean {
  const n = assets.length;
  if (n > PSD_MAX_DIM) return false;
  if (n <= 1) return true;
  const a = (i: number, j: number): bigint => {
    if (i === j) return PSD_SCALE;
    let r = rho(assets[i], assets[j]);
    if (r > PPM) r = PPM;
    if (r < -PPM) r = -PPM;
    return r << PSD_PPM_SHIFT;
  };
  const w: bigint[][] = Array.from({ length: PSD_MAX_DIM }, () => new Array<bigint>(PSD_MAX_DIM).fill(0n));
  const l: bigint[][] = Array.from({ length: PSD_MAX_DIM }, () => new Array<bigint>(PSD_MAX_DIM).fill(0n));
  for (let j = 0; j < n; j++) {
    let acc = 0n;
    for (let k = 0; k < j; k++) {
      const t = w[j][k] * l[j][k];
      if (!inI128(t)) return false;
      acc = acc + t;
      if (!inI128(acc)) return false;
    }
    const dj = a(j, j) - shrCeil(acc, PSD_L_SHIFT);
    if (!inI128(dj)) return false;
    if (dj < PSD_MIN_PIVOT) return false;
    const r = (1n << (2n * PSD_L_SHIFT)) / dj;
    for (let i = j + 1; i < n; i++) {
      let acc2 = 0n;
      for (let k = 0; k < j; k++) {
        const t = w[i][k] * l[j][k];
        if (!inI128(t)) return false;
        acc2 = acc2 + t;
        if (!inI128(acc2)) return false;
      }
      const wij = a(i, j) - (acc2 >> PSD_L_SHIFT);
      if (!inI128(wij)) return false;
      const lij = wij * r;
      if (!inI128(lij)) return false;
      w[i][j] = wij;
      l[i][j] = lij >> PSD_L_SHIFT;
    }
  }
  return true;
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

  /** A4: base correlation sub-matrix over the distinct assets (sorted) is positive definite. */
  correlationIsPositiveDefinite(legs: PortfolioLeg[]): boolean {
    const assets: number[] = [];
    for (const l of legs.slice(0, MAX_LEGS)) if (!assets.includes(l.asset)) assets.push(l.asset);
    assets.sort((x, y) => x - y);
    return corrIsPositiveDefinite(assets, this.rho);
  }

  /** Parametric term, or Σ standalone risk when not positive definite (A4). */
  parametricOrFallback(legs: PortfolioLeg[]): [bigint, boolean] {
    if (legs.length > MAX_LEGS) fail("TooManyLegs");
    if (this.correlationIsPositiveDefinite(legs)) return [this.parametricRisk(legs), false];
    let s = 0n;
    for (const l of legs) s = checkedU64(s + this.standaloneRisk(l));
    return [s, true];
  }

  /** Validated asset parameters (InvalidAsset / UnsupportedAsset as in Rust). */
  assetRisk(a: number): AssetRisk {
    return this.asset(a);
  }

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
    return this.portfolioRiskReport(legs)[0];
  }

  portfolioRiskReport(legs: PortfolioLeg[]): [bigint, boolean] {
    const [p, fallback] = this.parametricOrFallback(legs);
    const s = this.scenarioRisk(legs);
    return [p > s ? p : s, fallback];
  }
}

/** Port of ch_math::risk hedge-break constants (M3). */
export const HEDGE_BREAK_MAX_BPS = 10_000;
export const HEDGE_BREAK_MIN_BPS = 2_500;
/** PROVISIONAL default, pending calibration; on-chain ch_core does not use it yet. */
export const HEDGE_BREAK_DEFAULT_BPS = 5_000;

export const hedgeBreakBpsIsValid = (bps: number): boolean =>
  bps === 0 || (bps >= HEDGE_BREAK_MIN_BPS && bps <= HEDGE_BREAK_MAX_BPS);

/**
 * Port of `StressedRiskModel::hedge_break_loss`: every leg moves adversely by
 * ceil(shock · h / 10_000) bps; each leg's loss is rounded up separately.
 */
export function hedgeBreakLoss(model: StressedRiskModel, legs: PortfolioLeg[], hedgeBreakBps: number): bigint {
  const h = BigInt(Math.min(hedgeBreakBps, HEDGE_BREAK_MAX_BPS));
  let loss = 0n;
  for (const leg of legs) {
    const shock = BigInt(model.assetRisk(leg.asset).shockBps);
    const m = divCeil(shock * h, 10_000n);
    loss = checkedU64(loss + checkedU64(divCeil(leg.notional * m, 10_000n)));
  }
  return loss;
}

/** Port of `ch_math::risk::HedgeBreakModel`. */
export class HedgeBreakModel implements RiskModel {
  constructor(
    readonly inner: StressedRiskModel,
    readonly hedgeBreakBps: number
  ) {}

  standaloneRisk(leg: PortfolioLeg): bigint {
    return this.inner.standaloneRisk(leg);
  }

  scenarioRisk(legs: PortfolioLeg[]): bigint {
    const s = this.inner.scenarioRisk(legs);
    const h = hedgeBreakLoss(this.inner, legs, this.hedgeBreakBps);
    return s > h ? s : h;
  }

  portfolioRisk(legs: PortfolioLeg[]): bigint {
    return this.portfolioRiskReport(legs)[0];
  }

  portfolioRiskReport(legs: PortfolioLeg[]): [bigint, boolean] {
    const [p, fallback] = this.inner.parametricOrFallback(legs);
    const s = this.scenarioRisk(legs);
    return [p > s ? p : s, fallback];
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

/** Port of `ch_math::view` (A1): bit of a venue index (0 for indices >= 64). */
export const venueBit = (venueIndex: number): bigint => (venueIndex >= 0 && venueIndex < 64 ? 1n << BigInt(venueIndex) : 0n);

/** Port of `view_still_accepted`. */
export const viewStillAccepted = (viewMask: bigint, venueIndex: number, acceptsNow: bigint): boolean =>
  (viewMask & ~(acceptsNow | venueBit(venueIndex)) & U64_MAX) === 0n;

export interface ViewCredit {
  joint: PortfolioCredit;
  legCredit: bigint[];
  viewMask: bigint[];
  viewRuns: number;
}

/** Port of `compute_view_credit`: min(joint, view) per leg; views cached by mask. */
export function computeViewCredit(
  legs: PortfolioLeg[],
  model: RiskModel,
  params: CreditParams,
  accepts: (venueIndex: number) => bigint
): ViewCredit {
  const joint = computePortfolioCredit(legs, model, params);
  let present = 0n;
  for (const l of legs) present |= venueBit(l.venueIndex);
  const cache = new Map<bigint, bigint[]>();
  let viewRuns = 0;
  const legCredit: bigint[] = [];
  const viewMask: bigint[] = [];
  legs.forEach((leg, i) => {
    const mask = ((accepts(leg.venueIndex) | venueBit(leg.venueIndex)) & U64_MAX) & present;
    viewMask.push(mask);
    let viewC: bigint;
    if (mask === present) {
      viewC = joint.legCredit[i];
    } else {
      let byLeg = cache.get(mask);
      if (!byLeg) {
        byLeg = new Array<bigint>(legs.length).fill(0n);
        const idx: number[] = [];
        legs.forEach((l, j) => {
          if ((mask & venueBit(l.venueIndex)) !== 0n) idx.push(j);
        });
        if (idx.length >= 2) {
          const r = computePortfolioCredit(idx.map((j) => legs[j]), model, params);
          viewRuns += 1;
          idx.forEach((j, t) => (byLeg![j] = r.legCredit[t]));
        }
        cache.set(mask, byLeg);
      }
      viewC = byLeg[i];
    }
    legCredit.push(joint.legCredit[i] < viewC ? joint.legCredit[i] : viewC);
  });
  return { joint, legCredit, viewMask, viewRuns };
}
