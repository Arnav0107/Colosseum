/**
 * Crash-replay backtest for a long SOL / short ETH book.
 *
 * For each UTC day d:
 *  - parameters: the on-chain parameters in force on d. Governance runs the
 *    calibration once a day on data fully closed before the cutoff
 *    d − 1 day (the production timelock is ≥ 216,000 slots ≈ 1 day, so a
 *    change proposed from data up to d − 1 day applies at d at the earliest),
 *    clamped to the bounds and step limits relative to the previous day's
 *    parameters. Before enough data exists, the initial (localnet) parameters
 *    stay in force. The look-ahead guard is asserted in code.
 *  - credit: the TypeScript port of ch_math (bit-identical to Rust through the
 *    shared vectors), with the same model ch_core runs: the stressed model,
 *    the hedge-break scenario at the governed hedge_break_bps (A3) and the
 *    floor (A2). Reference columns repeat it without the hedge break
 *    (pre-A3) and without the floor (pre-A2).
 *  - post-credit requirement = Σ IM − credit;
 *    breach = realized worst portfolio loss over the horizon > requirement;
 *    baseline (no Clearinghouse) breach = loss > Σ IM.
 *
 * HORIZON CAVEAT: credit lives ~20 minutes, the primary horizon here is 1 day.
 * That is a conservative proxy, not a validation at the credit's own horizon.
 */
import {
  computePortfolioCredit,
  hedgedCredit,
  HedgeBreakModel,
  PortfolioLeg,
  RiskModel,
  StressedRiskModel,
  totalCredit,
} from "../../../testbench/src/portfolio";
import { AssetRiskArgs } from "./bounds";
import { AssetSeries, calibrate, Calibration, CurrentParams, DEFAULT_METHOD, InsufficientData, Method } from "./calibrate";
import { Bar, BAR_MS, DAY_MS } from "./data";

export const USD = 1_000_000n;
export const SOL = 0;
export const ETH = 1;

export interface Book {
  /** USD per leg at the day's opening price. */
  notionalUsd: number;
  imBps: number;
  mmBps: number;
}

/** The worked example: $50k each side, IM 20%, MM 10%. */
export const DEFAULT_BOOK: Book = { notionalUsd: 50_000, imBps: 2_000, mmBps: 1_000 };

/** Parameters in force on a day (what the chain would hold). */
export interface Effective {
  sol: AssetRiskArgs;
  eth: AssetRiskArgs;
  rho: number;
  corrStressPpm: number;
  basisDecorrPpm: number;
  hedgeBreakBps: number;
}

export function effectiveFrom(p: CurrentParams): Effective {
  const a = (id: number) => p.assets.find((x) => x.asset_id === id)!.params;
  return {
    sol: { ...a(SOL) },
    eth: { ...a(ETH) },
    rho: p.correlations.find((c) => c.a === SOL && c.b === ETH)!.rho,
    corrStressPpm: p.corr_stress_ppm,
    basisDecorrPpm: p.basis_decorr_ppm,
    hedgeBreakBps: p.hedge_break_bps,
  };
}

export function asCurrent(e: Effective, base: CurrentParams): CurrentParams {
  return {
    ...base,
    assets: [
      { asset_id: SOL, name: "SOL", params: { ...e.sol } },
      { asset_id: ETH, name: "ETH", params: { ...e.eth } },
    ],
    correlations: [{ a: SOL, b: ETH, rho: e.rho }],
    corr_stress_ppm: e.corrStressPpm,
    basis_decorr_ppm: e.basisDecorrPpm,
    hedge_break_bps: e.hedgeBreakBps,
  };
}

function applyCalibration(c: Calibration, prev: Effective): Effective {
  const asset = (id: number) => c.payloads.set_asset_risk.find((x) => x.asset_id === id)!.params;
  return {
    sol: asset(SOL),
    eth: asset(ETH),
    rho: c.payloads.update_correlations.entries.find((e) => e.a === SOL && e.b === ETH)!.rho,
    corrStressPpm: c.payloads.set_stress_params.corr_stress_ppm,
    basisDecorrPpm: c.payloads.set_stress_params.basis_decorr_ppm,
    // On-chain governance parameter (A3), held at the recommended default;
    // the calibration's figure is a recommendation, not applied here.
    hedgeBreakBps: prev.hedgeBreakBps,
  };
}

const usd = (x: number) => BigInt(Math.round(x * 1e6));

export function bookLegs(book: Book): PortfolioLeg[] {
  const n = usd(book.notionalUsd);
  const im = (n * BigInt(book.imBps)) / 10_000n;
  const mm = (n * BigInt(book.mmBps)) / 10_000n;
  return [
    { venueIndex: 0, asset: SOL, isLong: true, notional: n, initialMargin: im, maintenanceMargin: mm },
    { venueIndex: 1, asset: ETH, isLong: false, notional: n, initialMargin: im, maintenanceMargin: mm },
  ];
}

export function stressedModel(e: Effective): StressedRiskModel {
  const assets = Array.from({ length: 32 }, () => ({ shockBps: 0, gapBps: 0, basisBps: 0 }));
  assets[SOL] = { shockBps: e.sol.shock_bps, gapBps: e.sol.gap_bps, basisBps: e.sol.basis_bps };
  assets[ETH] = { shockBps: e.eth.shock_bps, gapBps: e.eth.gap_bps, basisBps: e.eth.basis_bps };
  const rho = (a: number, b: number): bigint => ((a === SOL && b === ETH) || (a === ETH && b === SOL) ? BigInt(e.rho) : 0n);
  // On-chain scenario slots are all-zero moves unless governance sets one; none are set here.
  return new StressedRiskModel(assets, rho, e.corrStressPpm, e.basisDecorrPpm, []);
}

export function creditFor(e: Effective, book: Book, params: CurrentParams["credit"], withHedgeBreak: boolean): bigint {
  const base = stressedModel(e);
  const model: RiskModel = withHedgeBreak && e.hedgeBreakBps > 0 ? new HedgeBreakModel(base, e.hedgeBreakBps) : base;
  const c = computePortfolioCredit(bookLegs(book), model, {
    haircutBps: params.haircut_bps,
    maxCreditPerUser: BigInt(params.max_credit_per_user),
    maxCreditBpsOfIm: params.max_credit_bps_of_im,
    capAtImMinusMm: true,
  });
  return totalCredit(c);
}

/**
 * PRE-A2 REFERENCE ONLY (not what ch_core computes since A2): the same credit
 * without the floor credit <= (sum IM - R_p)+, so before/after can be reported.
 * Mirrors computePortfolioCredit step by step minus the floor line.
 */
export function creditWithoutFloor(e: Effective, book: Book, params: CurrentParams["credit"], withHedgeBreak: boolean): bigint {
  const base = stressedModel(e);
  const model: RiskModel = withHedgeBreak && e.hedgeBreakBps > 0 ? new HedgeBreakModel(base, e.hedgeBreakBps) : base;
  const legs = bookLegs(book);
  const s = legs.reduce((a, l) => a + model.standaloneRisk(l), 0n);
  const sumIm = legs.reduce((a, l) => a + l.initialMargin, 0n);
  let total = hedgedCredit(sumIm, s, model.portfolioRisk(legs), params.haircut_bps);
  const perUser = BigInt(params.max_credit_per_user);
  if (total > perUser) total = perUser;
  let out = 0n;
  for (const l of legs) {
    if (total === 0n) break;
    const share = (total * l.initialMargin) / sumIm;
    let cap = (l.initialMargin * BigInt(params.max_credit_bps_of_im)) / 10_000n;
    const room = l.initialMargin - l.maintenanceMargin;
    if (room < cap) cap = room;
    out += share < cap ? share : cap;
  }
  return out;
}

/** Bars of one asset in [from, to), checked to be the full, gap-free set. */
function window(bars: Bar[], from: number, to: number): Bar[] {
  const w = bars.filter((b) => b.t >= from && b.t < to);
  const expected = (to - from) / BAR_MS;
  if (w.length !== expected) throw new Error(`window ${new Date(from).toISOString()}: ${w.length} of ${expected} bars`);
  return w;
}

export interface RealizedLoss {
  /** Long-SOL low and short-ETH high of the same bar combined: conservative. */
  pathLoss: number;
  /** Same, from bar closes only. */
  closeLoss: number;
  solMove: number;
  ethMove: number;
}

/** Worst mark-to-market loss of the book over [from, to), from the opening prices at `from`. */
export function realizedLoss(sol: Bar[], eth: Bar[], from: number, to: number, book: Book): RealizedLoss {
  const s = window(sol, from, to);
  const e = window(eth, from, to);
  const n = book.notionalUsd;
  const s0 = s[0].o;
  const e0 = e[0].o;
  let pathLoss = 0;
  let closeLoss = 0;
  for (let k = 0; k < s.length; k++) {
    pathLoss = Math.max(pathLoss, n * ((s0 - s[k].l) / s0) + n * ((e[k].h - e0) / e0));
    closeLoss = Math.max(closeLoss, n * ((s0 - s[k].c) / s0) + n * ((e[k].c - e0) / e0));
  }
  return { pathLoss, closeLoss, solMove: s[s.length - 1].c / s0 - 1, ethMove: e[e.length - 1].c / e0 - 1 };
}

export interface DayResult {
  date: string;
  /** Data used for the parameters in force: bars closed strictly before this instant. */
  dataCutoff: string;
  paramsSource: "initial" | "calibrated" | "carried (insufficient data)";
  params: Effective;
  sumIm: number;
  /** R_p of the model in force (what the model believes the book can lose). */
  portfolioRisk: number;
  credit: number;
  creditNoHedgeBreak: number;
  /** PRE-A2 REFERENCE: credit without the floor (what ch_core gave before A2). */
  creditNoFloor: number;
  postCredit: number;
  postCreditNoHedgeBreak: number;
  loss: RealizedLoss;
  breachWithCh: boolean;
  breachWithChNoHedgeBreak: boolean;
  breachBaseline: boolean;
  breachNoFloor: boolean;
  marginOfSafety: number;
}

export interface Summary {
  label: string;
  days: number;
  breachesWithCh: number;
  breachesWithChNoHedgeBreak: number;
  breachesBaseline: number;
  breachesNoFloor: number;
  worstMarginOfSafety: number;
  worstMarginDate: string;
}

export function summarize(label: string, rows: DayResult[]): Summary {
  let worst = rows[0];
  for (const r of rows) if (r.marginOfSafety < worst.marginOfSafety) worst = r;
  return {
    label,
    days: rows.length,
    breachesWithCh: rows.filter((r) => r.breachWithCh).length,
    breachesWithChNoHedgeBreak: rows.filter((r) => r.breachWithChNoHedgeBreak).length,
    breachesBaseline: rows.filter((r) => r.breachBaseline).length,
    breachesNoFloor: rows.filter((r) => r.breachNoFloor).length,
    worstMarginOfSafety: worst.marginOfSafety,
    worstMarginDate: worst.date,
  };
}

const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
const micro = (x: bigint) => Number(x) / 1e6;

export interface BacktestOptions {
  book: Book;
  method: Method;
  horizonMs: number;
  /** Delay between the data cutoff and the day the parameters apply. */
  timelockMs: number;
  /** Spacing of evaluation start times (parameters still change once per UTC day). */
  stepMs: number;
}

export const DEFAULT_OPTIONS: BacktestOptions = { book: DEFAULT_BOOK, method: DEFAULT_METHOD, horizonMs: DAY_MS, timelockMs: DAY_MS, stepMs: DAY_MS };

/**
 * Days: every UTC midnight d in the data with a full horizon [d, d + H) of
 * bars. Parameters for d use only bars that closed before d − timelock.
 */
export function runBacktest(sol: Bar[], eth: Bar[], initial: CurrentParams, opts: BacktestOptions = DEFAULT_OPTIONS): DayResult[] {
  const firstDay = Math.ceil(sol[0].t / DAY_MS) * DAY_MS;
  const lastT = sol[sol.length - 1].t + BAR_MS;
  const have = new Set(sol.map((b) => b.t));
  const rows: DayResult[] = [];
  let eff = effectiveFrom(initial);
  let source: DayResult["paramsSource"] = "initial";
  let paramsDay = -1;
  for (let d = firstDay; d + opts.horizonMs <= lastT; d += opts.stepMs) {
    if (!have.has(d) || !have.has(d + opts.horizonMs - BAR_MS)) continue; // not in the data (gap between samples)
    let ok = true;
    for (let t = d; t < d + opts.horizonMs; t += BAR_MS) if (!have.has(t)) ok = false;
    if (!ok) continue;

    // Governance acts once per UTC day: parameters for any start time on day D
    // come from data closed before D − timelock.
    const dayStart = Math.floor(d / DAY_MS) * DAY_MS;
    const cutoff = dayStart - opts.timelockMs;
    const known: AssetSeries[] = [
      { name: "SOL", assetId: SOL, bars: sol.filter((b) => b.t + BAR_MS <= cutoff) },
      { name: "ETH", assetId: ETH, bars: eth.filter((b) => b.t + BAR_MS <= cutoff) },
    ];
    // No look-ahead: the calibration may only see bars closed before the cutoff,
    // and the cutoff must precede the first bar whose loss is measured.
    if (!(cutoff <= d)) throw new Error("look-ahead: cutoff after evaluation start");
    for (const s of known) {
      for (const b of s.bars) if (b.t + BAR_MS > cutoff) throw new Error(`look-ahead: ${s.name} ${new Date(b.t).toISOString()} used for ${day(d)}`);
    }
    if (dayStart !== paramsDay && known[0].bars.length > 0) {
      try {
        eff = applyCalibration(calibrate(known, asCurrent(eff, initial), opts.method, cutoff), eff);
        source = "calibrated";
      } catch (e) {
        if (!(e instanceof InsufficientData)) throw e;
        if (source === "calibrated") source = "carried (insufficient data)";
      }
    }
    paramsDay = dayStart;

    const sumIm = micro(bookLegs(opts.book).reduce((s, l) => s + l.initialMargin, 0n));
    const credit = micro(creditFor(eff, opts.book, initial.credit, true));
    const creditHb = micro(creditFor(eff, opts.book, initial.credit, false));
    const loss = realizedLoss(sol, eth, d, d + opts.horizonMs, opts.book);
    const base = stressedModel(eff);
    const onChain: RiskModel = eff.hedgeBreakBps > 0 ? new HedgeBreakModel(base, eff.hedgeBreakBps) : base;
    const portfolioRisk = micro(onChain.portfolioRisk(bookLegs(opts.book)));
    const creditNoFloor = micro(creditWithoutFloor(eff, opts.book, initial.credit, true));
    const postCredit = sumIm - credit;
    const postCreditHb = sumIm - creditHb;
    rows.push({
      date: opts.horizonMs === DAY_MS ? day(d) : new Date(d).toISOString(),
      dataCutoff: new Date(cutoff).toISOString(),
      paramsSource: source,
      params: { ...eff, sol: { ...eff.sol }, eth: { ...eff.eth } },
      sumIm,
      portfolioRisk,
      credit,
      creditNoHedgeBreak: creditHb,
      creditNoFloor,
      postCredit,
      postCreditNoHedgeBreak: postCreditHb,
      loss,
      breachWithCh: loss.pathLoss > postCredit,
      breachWithChNoHedgeBreak: loss.pathLoss > postCreditHb,
      breachBaseline: loss.pathLoss > sumIm,
      breachNoFloor: loss.pathLoss > sumIm - creditNoFloor,
      marginOfSafety: postCredit - loss.pathLoss,
    });
  }
  return rows;
}

/** A labelled HYPOTHETICAL one-day decorrelation: both legs lose at once. */
export interface Hypothetical {
  label: string;
  solMove: number;
  ethMove: number;
  params: string;
  sumIm: number;
  credit: number;
  postCredit: number;
  creditNoHedgeBreak: number;
  postCreditNoHedgeBreak: number;
  loss: number;
  breachWithCh: boolean;
  breachWithChNoHedgeBreak: boolean;
  breachBaseline: boolean;
}

export function hypothetical(label: string, solMove: number, ethMove: number, eff: Effective, paramsLabel: string, initial: CurrentParams, book: Book = DEFAULT_BOOK): Hypothetical {
  const sumIm = micro(bookLegs(book).reduce((s, l) => s + l.initialMargin, 0n));
  const credit = micro(creditFor(eff, book, initial.credit, true));
  const creditHb = micro(creditFor(eff, book, initial.credit, false));
  const loss = Math.max(0, book.notionalUsd * -solMove + book.notionalUsd * ethMove);
  return {
    label,
    solMove,
    ethMove,
    params: paramsLabel,
    sumIm,
    credit,
    postCredit: sumIm - credit,
    creditNoHedgeBreak: creditHb,
    postCreditNoHedgeBreak: sumIm - creditHb,
    loss,
    breachWithCh: loss > sumIm - credit,
    breachWithChNoHedgeBreak: loss > sumIm - creditHb,
    breachBaseline: loss > sumIm,
  };
}
