/**
 * Independent re-computation of a portfolio's credit from on-chain inputs.
 *
 * Everything the computation needs is read from chain: the legs as stored in
 * the portfolio at refresh time, the config parameters and counters, and the
 * risk model. Nothing is assumed by the UI (F12). The engine is the TS port
 * in ../portfolio.ts, kept bit-identical with Rust by vectors.test.ts.
 */
import {
  capGroups,
  computePortfolioCredit,
  CreditParams,
  MAX_ASSETS,
  PortfolioCredit,
  PortfolioError,
  PortfolioLeg,
  HedgeBreakModel,
  RiskModel,
  StressedRiskModel,
} from "../portfolio";
import { ConfigParamsView, ConfigView, LegStateView, PortfolioView, RiskModelView, STATUS } from "./views";

export const MAX_VENUES = 64;

/** Same layout as `ch_core::state::corr_index`. */
export function corrIndex(a: number, b: number): number | null {
  const [i, j] = a < b ? [a, b] : [b, a];
  if (i === j || j >= MAX_ASSETS) return null;
  return (i * (2 * MAX_ASSETS - i - 1)) / 2 + (j - i - 1);
}

export function modelFromView(rm: RiskModelView): StressedRiskModel {
  const rho = (a: number, b: number): bigint => {
    if (a === b) return 1_000_000n;
    const k = corrIndex(a, b);
    return BigInt(k === null ? 0 : rm.corr[k] ?? 0);
  };
  const scenarios = rm.scenarios.filter((_, i) => (rm.scenarioKind[i] ?? 0) !== 0);
  return new StressedRiskModel(rm.assets, rho, rm.corrStressPpm, rm.basisDecorrPpm, scenarios);
}

/** The model ch_core runs: the stressed model, wrapped in the hedge-break scenario when enabled (A3). */
export function riskModelFor(rm: RiskModelView, p: ConfigParamsView): RiskModel {
  const base = modelFromView(rm);
  return p.hedgeBreakBps > 0 ? new HedgeBreakModel(base, p.hedgeBreakBps) : base;
}

export function creditParams(p: ConfigParamsView): CreditParams {
  return {
    haircutBps: p.haircutBps,
    maxCreditPerUser: p.maxCreditPerUser,
    maxCreditBpsOfIm: p.maxCreditBpsOfIm,
    // Always on in ch_core: credit never takes a leg below maintenance margin.
    capAtImMinusMm: true,
  };
}

export function legInput(l: LegStateView): PortfolioLeg {
  return {
    venueIndex: l.venueIndex,
    asset: l.assetId,
    isLong: l.isLong,
    notional: l.notional,
    initialMargin: l.initialMargin,
    maintenanceMargin: l.maintenanceMargin,
  };
}

const sat = (a: bigint, b: bigint): bigint => (a > b ? a - b : 0n);

/**
 * Outstanding-credit counters as they were just before this portfolio's last
 * refresh added its own share. Exact when no other portfolio changed the
 * counters since (true on the single-user testbench).
 */
export function countersBeforeOwnShare(cfg: ConfigView, pf: PortfolioView) {
  const venue = [...cfg.venueOutstanding];
  const asset = [...cfg.assetOutstanding];
  let global = cfg.globalOutstandingCredit;
  if (pf.counted && pf.countedEpoch === cfg.riskEpoch) {
    for (const l of pf.legs) {
      venue[l.venueIndex] = sat(venue[l.venueIndex] ?? 0n, l.credit);
      asset[l.assetId] = sat(asset[l.assetId] ?? 0n, l.credit);
    }
    global = sat(global, pf.totalCredit);
  }
  return { venue, asset, global };
}

/** Per-venue, then per-asset, then global budget, as in refresh_portfolio. */
export function applyBudgets(
  legs: PortfolioLeg[],
  credits: bigint[],
  p: ConfigParamsView,
  outstanding: { venue: bigint[]; asset: bigint[]; global: bigint }
): bigint[] {
  const venueHeadroom = Array.from({ length: MAX_VENUES }, (_, v) => sat(p.maxCreditPerVenue, outstanding.venue[v] ?? 0n));
  const assetHeadroom = Array.from({ length: MAX_ASSETS }, (_, a) => sat(p.maxCreditPerAsset, outstanding.asset[a] ?? 0n));
  let c = capGroups(credits, (i) => legs[i].venueIndex, venueHeadroom);
  c = capGroups(c, (i) => legs[i].asset, assetHeadroom);
  return capGroups(c, () => 0, [sat(p.globalCreditCap, outstanding.global)]);
}

/**
 * What a venue applies: max(IM − credit, MM). Mirrors
 * `ch_client::effective_initial_margin`, including its guard: inconsistent
 * inputs (MM > IM) ignore the credit and return IM.
 */
export function effectiveInitialMargin(im: bigint, mm: bigint, credit: bigint): bigint {
  if (mm > im) return im;
  const reduced = sat(im, credit);
  return reduced > mm ? reduced : mm;
}

/** Per-leg ceiling on credit: min(IM · max_credit_bps_of_im, IM − MM). */
export function legCap(im: bigint, mm: bigint, maxCreditBpsOfIm: number): bigint {
  const bpsCap = (im * BigInt(maxCreditBpsOfIm)) / 10_000n;
  const room = sat(im, mm);
  return bpsCap < room ? bpsCap : room;
}

export type Verdict =
  | { kind: "nothing"; reason: string }
  | { kind: "stale"; reason: string }
  | { kind: "engine-error"; reason: string }
  | { kind: "match"; engine: PortfolioCredit; expected: bigint[] }
  | { kind: "mismatch"; engine: PortfolioCredit; expected: bigint[]; differences: string[] };

export function verifyPortfolio(cfg: ConfigView, rm: RiskModelView, pf: PortfolioView): Verdict {
  if (pf.status !== STATUS.ACTIVE || pf.legs.length === 0) {
    return { kind: "nothing", reason: "No active credit to verify. Refresh the portfolio first." };
  }
  if (pf.riskEpoch !== cfg.riskEpoch || pf.riskModelVersion !== rm.version) {
    return {
      kind: "stale",
      reason: "Parameters or the risk model changed since this credit was computed (epoch/version differ). Venues already treat it as unusable.",
    };
  }
  const legs = pf.legs.map(legInput);
  let engine: PortfolioCredit;
  try {
    engine = computePortfolioCredit(legs, riskModelFor(rm, cfg.params), creditParams(cfg.params));
  } catch (e) {
    const reason = e instanceof PortfolioError ? e.code : String(e);
    return { kind: "engine-error", reason: `The off-chain engine rejected the stored legs: ${reason}` };
  }
  const expected = applyBudgets(legs, engine.legCredit, cfg.params, countersBeforeOwnShare(cfg, pf));
  const differences: string[] = [];
  const cmp = (what: string, chain: bigint | number, local: bigint | number) => {
    if (BigInt(chain) !== BigInt(local)) differences.push(`${what}: on-chain ${chain}, expected ${local}`);
  };
  cmp("sum of standalone risk S", pf.sumStandaloneRisk, engine.sumStandaloneRisk);
  cmp("portfolio risk R_p", pf.portfolioRisk, engine.portfolioRisk);
  cmp("hedge benefit (ppm)", pf.hedgeBenefitPpm, engine.hedgeBenefitPpm);
  pf.legs.forEach((l, i) => cmp(`leg ${i + 1} credit`, l.credit, expected[i]));
  cmp("total credit", pf.totalCredit, expected.reduce((a, b) => a + b, 0n));
  return differences.length === 0
    ? { kind: "match", engine, expected }
    : { kind: "mismatch", engine, expected, differences };
}

/** Mirror of `Portfolio::usable_credit`: what a venue would accept right now. */
export function usableCredit(cfg: ConfigView, pf: PortfolioView, nowSlot: bigint, venueIndex: number, assetId: number): bigint {
  if (pf.status !== STATUS.ACTIVE || pf.creditKind !== CREDIT_KIND_TRADING_POWER_ONLY || pf.riskEpoch !== cfg.riskEpoch || nowSlot > pf.expiresSlot) {
    return 0n;
  }
  return pf.legs.find((l) => l.venueIndex === venueIndex && l.assetId === assetId)?.credit ?? 0n;
}

export const CREDIT_KIND_TRADING_POWER_ONLY = 1;
