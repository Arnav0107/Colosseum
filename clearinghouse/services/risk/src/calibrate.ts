/**
 * Risk-parameter calibration from committed 20-minute klines.
 *
 * Definitions (all stated in the output's `method` block):
 *  - shock_bps: the q-quantile, over every horizon window in the sample, of the
 *    largest adverse excursion in either direction from the window's opening
 *    price, using bar highs and lows (path-wise, not close-to-close).
 *  - gap_bps: the q-quantile of the asset's idiosyncratic horizon move
 *    |R_a − β_ab·R_b| (β from 20-minute log returns), maximised over partners;
 *    the on-chain bound then forces gap ≥ shock.
 *  - basis_bps / basis_decorr_ppm: NOT estimated (single-venue spot data has no
 *    venue basis); the current value is kept and the output says so.
 *  - ρ: median of rolling Pearson correlations of 20-minute log returns.
 *  - corr_stress_ppm (δ): median − low quantile of the rolling correlations,
 *    so ρ − δ covers that quantile.
 *  - hedge_break_bps (M3, off-chain only): the q-quantile, over windows and
 *    both long/short orientations of each pair, of the largest simultaneous
 *    adverse move of both legs (closes), as a fraction of the sum of their
 *    proposed shocks (equal-notional pair).
 *
 * Then: PSD check (Higham repair, reported), clamp to the on-chain bounds and
 * step limits (every clamp reported), round, re-check PSD (fail closed).
 */
import { AssetRiskArgs, assetChangeReducesCredit, Clamp, clampAssetRisk, clampCorr, clampHedgeBreak, clampStress, stressChangeReducesCredit } from "./bounds";
import { Bar, BAR_MS, segments } from "./data";
import { checkAndRepair, isPsd, Matrix, PsdReport } from "./psd";
import { beta, pearson, quantile } from "./stats";

export interface AssetSeries {
  name: string;
  assetId: number;
  bars: Bar[];
}

export interface CorrEntry {
  a: number;
  b: number;
  rho: number;
}

/** What is on chain now (or assumed to be). Same field names as the instructions. */
export interface CurrentParams {
  label: string;
  assets: { asset_id: number; name: string; params: AssetRiskArgs }[];
  correlations: CorrEntry[];
  corr_stress_ppm: number;
  basis_decorr_ppm: number;
  /** Off-chain only (ch_core does not use it yet); 0 = disabled. */
  hedge_break_bps: number;
  credit: { haircut_bps: number; max_credit_per_user: string; max_credit_bps_of_im: number };
}

export interface Method {
  horizonBars: number;
  quantile: number;
  corrWindowBars: number;
  corrStressQuantile: number;
}

/** Horizon 1 day (72 × 20 min), 99% quantile, 3-day rolling correlation, 5% low quantile. */
export const DEFAULT_METHOD: Method = { horizonBars: 72, quantile: 0.99, corrWindowBars: 216, corrStressQuantile: 0.05 };

export class InsufficientData extends Error {}

/** Throws if any bar is not fully closed before `cutoffMs` (look-ahead guard). */
export function assertNoLookAhead(series: AssetSeries[], cutoffMs: number): void {
  for (const s of series) {
    for (const b of s.bars) {
      if (b.t + BAR_MS > cutoffMs) {
        throw new Error(`look-ahead: ${s.name} bar ${new Date(b.t).toISOString()} closes after cutoff ${new Date(cutoffMs).toISOString()}`);
      }
    }
  }
}

const r6 = (x: number) => Math.round(x * 1e6) / 1e6;
const toBps = (frac: number) => Math.ceil(frac * 10_000 - 1e-9);

interface Aligned {
  t: number[];
  bars: Bar[][]; // [asset][i]
}

function alignAll(series: AssetSeries[]): Aligned {
  const maps = series.map((s) => new Map(s.bars.map((b) => [b.t, b])));
  const t = series[0].bars.map((b) => b.t).filter((x) => maps.every((m) => m.has(x)));
  return { t, bars: maps.map((m) => t.map((x) => m.get(x)!)) };
}

/** Window starts i whose horizon [i, i+H) lies inside one gap-free segment. */
function windowStarts(t: number[], h: number): number[] {
  const out: number[] = [];
  for (const [s, e] of segments(t)) for (let i = s; i + h <= e; i++) out.push(i);
  return out;
}

function logReturns(t: number[], bars: Bar[]): { idx: number[]; r: number[] } {
  const idx: number[] = [];
  const r: number[] = [];
  for (const [s, e] of segments(t)) {
    for (let k = s + 1; k < e; k++) {
      idx.push(k);
      r.push(Math.log(bars[k].c / bars[k - 1].c));
    }
  }
  return { idx, r };
}

export interface Calibration {
  method: Method & { description: Record<string, string> };
  sample: { bars: number; first: string; last: string; segments: number; horizonWindows: number };
  estimates: {
    assets: { asset_id: number; name: string; shock_raw_bps: number; gap_raw_bps: number; basis: string }[];
    correlations: { a: number; b: number; full_sample: number; rolling: { count: number; min: number; p05: number; median: number; max: number } }[];
    corr_stress_raw_ppm: number;
    hedge_break_raw_bps: number;
  };
  psd: PsdReport;
  clamps: Clamp[];
  payloads: {
    update_correlations: { entries: CorrEntry[] };
    set_asset_risk: { asset_id: number; params: AssetRiskArgs }[];
    set_stress_params: { corr_stress_ppm: number; basis_decorr_ppm: number };
  };
  offchain: { hedge_break_bps: number; note: string };
  diff: DiffRow[];
}

export interface DiffRow {
  field: string;
  current: number;
  proposed: number;
  delta: number;
  applies: "unchanged" | "at once (credit-reducing)" | "after timelock";
}

export function calibrate(series: AssetSeries[], current: CurrentParams, method: Method = DEFAULT_METHOD, cutoffMs?: number): Calibration {
  if (cutoffMs !== undefined) assertNoLookAhead(series, cutoffMs);
  if (series.length < 2) throw new Error("need at least two assets");
  const { t, bars } = alignAll(series);
  const H = method.horizonBars;
  const starts = windowStarts(t, H);
  if (starts.length === 0 || t.length < H + method.corrWindowBars) {
    throw new InsufficientData(`need ≥ ${H + method.corrWindowBars} aligned bars in a gap-free run; have ${t.length}`);
  }
  const q = method.quantile;
  const clamps: Clamp[] = [];
  const cur = new Map(current.assets.map((a) => [a.asset_id, a.params]));

  // Per-asset path excursions and horizon returns.
  const excursion = bars.map((b) =>
    starts.map((i) => {
      const o = b[i].o;
      let down = 0;
      let up = 0;
      for (let k = i; k < i + H; k++) {
        down = Math.max(down, (o - b[k].l) / o);
        up = Math.max(up, (b[k].h - o) / o);
      }
      return Math.max(down, up);
    })
  );
  const horizonRet = bars.map((b) => starts.map((i) => b[i + H - 1].c / b[i].o - 1));
  const rets = bars.map((b) => logReturns(t, b).r);

  const shockRaw = excursion.map((x) => toBps(quantile(x, q)));
  const gapRaw = series.map((_, a) => {
    let worst = 0;
    series.forEach((__, b) => {
      if (a === b) return;
      const bt = beta(rets[a], rets[b]);
      const resid = horizonRet[a].map((ra, w) => Math.abs(ra - bt * horizonRet[b][w]));
      worst = Math.max(worst, quantile(resid, q));
    });
    return toBps(worst);
  });

  const proposedAssets = series.map((s, a) => {
    const now = cur.get(s.assetId) ?? null;
    const basis = now ? now.basis_bps : 100;
    return clampAssetRisk(s.name, { shock_bps: shockRaw[a], gap_bps: gapRaw[a], basis_bps: basis }, now, clamps);
  });

  // Correlations: rolling windows of 20-minute log returns inside each segment.
  const W = method.corrWindowBars;
  const corrStats: Calibration["estimates"]["correlations"] = [];
  const n = series.length;
  const raw: Matrix = Array.from({ length: n }, (_, i) => Array.from({ length: n }, (_, j) => (i === j ? 1 : 0)));
  const segOfRet: number[] = [];
  {
    let s = 0;
    for (const [a, e] of segments(t)) {
      for (let k = a + 1; k < e; k++) segOfRet.push(s);
      s++;
    }
  }
  let deltaRaw = 0;
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      const rolling: number[] = [];
      for (let k = 0; k + W <= rets[i].length; k++) {
        if (segOfRet[k] !== segOfRet[k + W - 1]) continue;
        rolling.push(pearson(rets[i].slice(k, k + W), rets[j].slice(k, k + W)));
      }
      if (rolling.length === 0) throw new InsufficientData("no full rolling-correlation window");
      const med = quantile(rolling, 0.5);
      const lo = quantile(rolling, method.corrStressQuantile);
      raw[i][j] = raw[j][i] = med;
      deltaRaw = Math.max(deltaRaw, med - lo);
      corrStats.push({
        a: series[i].assetId,
        b: series[j].assetId,
        full_sample: r6(pearson(rets[i], rets[j])),
        rolling: { count: rolling.length, min: r6(Math.min(...rolling)), p05: r6(lo), median: r6(med), max: r6(Math.max(...rolling)) },
      });
    }
  }

  const { matrix, report } = checkAndRepair(raw);
  const currentRho = (a: number, b: number) => current.correlations.find((c) => (c.a === a && c.b === b) || (c.a === b && c.b === a))?.rho ?? null;
  const entries: CorrEntry[] = [];
  const clampedMatrix: Matrix = matrix.map((r) => [...r]);
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      const a = series[i].assetId;
      const b = series[j].assetId;
      const rho = clampCorr(`rho(${series[i].name},${series[j].name})`, Math.round(matrix[i][j] * 1e6), currentRho(a, b), clamps);
      entries.push({ a: Math.min(a, b), b: Math.max(a, b), rho });
      clampedMatrix[i][j] = clampedMatrix[j][i] = rho / 1e6;
    }
  }
  if (!isPsd(clampedMatrix)) {
    // Clamping (step limits) moved the matrix out of the PSD cone. Never ship it.
    throw new Error("correlation matrix is not PSD after clamping to the on-chain step limits; no payload produced");
  }

  const stress = clampStress({ corr_stress_ppm: Math.ceil(deltaRaw * 1e6), basis_decorr_ppm: current.basis_decorr_ppm }, clamps);

  // Hedge break: simultaneous adverse move of both legs, as a fraction of the proposed shocks.
  const ratios: number[] = [];
  for (let a = 0; a < n; a++) {
    for (let b = 0; b < n; b++) {
      if (a === b) continue; // orientation: long a, short b
      const denom = (proposedAssets[a].shock_bps + proposedAssets[b].shock_bps) / 10_000;
      for (const i of starts) {
        let worst = 0;
        for (let k = i; k < i + H; k++) {
          const la = 1 - bars[a][k].c / bars[a][i].o;
          const sb = bars[b][k].c / bars[b][i].o - 1;
          if (la > 0 && sb > 0) worst = Math.max(worst, la + sb);
        }
        ratios.push(worst / denom);
      }
    }
  }
  const hedgeBreakRaw = toBps(quantile(ratios, q));
  const hedgeBreak = clampHedgeBreak(hedgeBreakRaw, clamps);

  const payloads = {
    update_correlations: { entries },
    set_asset_risk: series.map((s, a) => ({ asset_id: s.assetId, params: proposedAssets[a] })),
    set_stress_params: stress,
  };

  return {
    method: {
      ...method,
      description: {
        horizon: `${method.horizonBars} × 20-minute bars = ${(method.horizonBars * BAR_MS) / 3_600_000} h`,
        shock_bps: `${q * 100}% quantile of the largest adverse excursion (high/low) from the window open, either direction`,
        gap_bps: `${q * 100}% quantile of |R_a − β·R_b| over the horizon (β from 20-minute log returns); on-chain bound forces gap ≥ shock`,
        basis_bps: "not estimated: single-venue spot data has no venue basis; current value kept",
        rho: `median of rolling ${method.corrWindowBars}-bar Pearson correlations of 20-minute log returns`,
        corr_stress_ppm: `median − ${method.corrStressQuantile * 100}% quantile of the rolling correlations`,
        basis_decorr_ppm: "not estimated (no multi-venue data); current value kept",
        hedge_break_bps: `${q * 100}% quantile of the largest simultaneous adverse move of both legs (closes) / (shock_a + shock_b)`,
      },
    },
    sample: {
      bars: t.length,
      first: new Date(t[0]).toISOString(),
      last: new Date(t[t.length - 1]).toISOString(),
      segments: segments(t).length,
      horizonWindows: starts.length,
    },
    estimates: {
      assets: series.map((s, a) => ({
        asset_id: s.assetId,
        name: s.name,
        shock_raw_bps: shockRaw[a],
        gap_raw_bps: gapRaw[a],
        basis: "not estimated (kept current)",
      })),
      correlations: corrStats,
      corr_stress_raw_ppm: Math.ceil(deltaRaw * 1e6),
      hedge_break_raw_bps: hedgeBreakRaw,
    },
    psd: report,
    clamps,
    payloads,
    offchain: {
      hedge_break_bps: hedgeBreak,
      note: "ch_core does not read hedge_break_bps yet (M3); recommendation only, no governance instruction exists",
    },
    diff: diffAgainst(current, payloads, hedgeBreak, series),
  };
}

function diffAgainst(current: CurrentParams, p: Calibration["payloads"], hedgeBreak: number, series: AssetSeries[]): DiffRow[] {
  const rows: DiffRow[] = [];
  const row = (field: string, cur: number, next: number, reduces: boolean) =>
    rows.push({ field, current: cur, proposed: next, delta: next - cur, applies: cur === next ? "unchanged" : reduces ? "at once (credit-reducing)" : "after timelock" });
  for (const a of p.set_asset_risk) {
    const name = series.find((s) => s.assetId === a.asset_id)!.name;
    const old = current.assets.find((x) => x.asset_id === a.asset_id)?.params ?? { shock_bps: 0, gap_bps: 0, basis_bps: 0 };
    const reduces = assetChangeReducesCredit(old, a.params);
    row(`${name}.shock_bps`, old.shock_bps, a.params.shock_bps, reduces);
    row(`${name}.gap_bps`, old.gap_bps, a.params.gap_bps, reduces);
    row(`${name}.basis_bps`, old.basis_bps, a.params.basis_bps, reduces);
  }
  for (const e of p.update_correlations.entries) {
    const old = current.correlations.find((c) => c.a === e.a && c.b === e.b)?.rho ?? 0;
    const name = (id: number) => series.find((s) => s.assetId === id)!.name;
    row(`rho(${name(e.a)},${name(e.b)}) ppm`, old, e.rho, false); // correlation changes always queue
  }
  const sr = stressChangeReducesCredit(
    [current.corr_stress_ppm, current.basis_decorr_ppm],
    [p.set_stress_params.corr_stress_ppm, p.set_stress_params.basis_decorr_ppm]
  );
  row("corr_stress_ppm", current.corr_stress_ppm, p.set_stress_params.corr_stress_ppm, sr);
  row("basis_decorr_ppm", current.basis_decorr_ppm, p.set_stress_params.basis_decorr_ppm, sr);
  rows.push({
    field: "hedge_break_bps (off-chain)",
    current: current.hedge_break_bps,
    proposed: hedgeBreak,
    delta: hedgeBreak - current.hedge_break_bps,
    applies: current.hedge_break_bps === hedgeBreak ? "unchanged" : "after timelock",
  });
  return rows;
}

/** Human-readable report: sample, PSD result, every clamp, and the diff. */
export function renderReport(c: Calibration): string {
  const out: string[] = [];
  out.push(`Sample: ${c.sample.bars} aligned 20-minute bars, ${c.sample.first} .. ${c.sample.last}, ${c.sample.segments} gap-free segment(s), ${c.sample.horizonWindows} horizon windows`);
  out.push(`Horizon: ${c.method.description.horizon}; quantile ${c.method.quantile}`);
  if (c.psd.repaired) {
    out.push(
      `PSD: FAILED (min eigenvalue ${c.psd.minEigenvalueBefore.toExponential(3)}). REPAIRED with ${c.psd.method}; ` +
        `Frobenius distance ${c.psd.frobeniusDistance!.toFixed(6)}, min eigenvalue after ${c.psd.minEigenvalueAfter!.toExponential(3)}`
    );
  } else {
    out.push(`PSD: ok (min eigenvalue ${c.psd.minEigenvalueBefore.toFixed(6)}); no repair`);
  }
  out.push(c.clamps.length ? "Clamps applied:" : "Clamps applied: none");
  for (const k of c.clamps) out.push(`  ${k.field}: ${k.raw} -> ${k.value} (${k.rule})`);
  out.push("");
  out.push("| field | current | proposed | delta | applies |");
  out.push("| --- | ---: | ---: | ---: | --- |");
  for (const r of c.diff) out.push(`| ${r.field} | ${r.current} | ${r.proposed} | ${r.delta > 0 ? "+" : ""}${r.delta} | ${r.applies} |`);
  return out.join("\n");
}
