/**
 * Efficiency-vs-safety frontier (B1).
 *
 * Re-uses the backtest rows (parameters in force per day, realized losses)
 * and recomputes credit for every (haircut_bps, hedge_break_bps) pair. The
 * calibrated risk parameters in force on each day are unchanged; only the two
 * swept parameters differ. For each setting:
 *  - freed: mean over evaluations of credit / Σ IM (margin freed);
 *  - breaches at H = 1 day and at H = 20 minutes (bar-level), and the rate per
 *    evaluation day (H = 20 min: breaches / (windows / 72));
 *  - the same with the proposed floor credit ≤ (Σ IM − R_p)⁺ (A2), computed as
 *    min(credit, (Σ IM − R_p)⁺) on the final credit, like the PROPOSAL column of
 *    the backtest (the engine change applies it before the per-leg caps; for this
 *    two-leg book with equal IM the two agree up to flooring).
 *
 * Pure and deterministic: same rows in, same numbers out.
 */
import { HedgeBreakModel, RiskModel } from "../../../testbench/src/portfolio";
import { bookLegs, Book, creditFor, DayResult, Effective, stressedModel } from "./backtest";
import { CurrentParams } from "./calibrate";

export const HAIRCUTS_BPS = [500, 1_000, 1_500, 2_000, 2_500, 3_000, 4_000, 5_000, 6_000, 7_000];
export const HEDGE_BREAKS_BPS = [0, 2_500, 3_000, 4_000, 5_000, 6_000, 7_000, 8_000];
/** 20-minute windows per day. */
export const WINDOWS_PER_DAY = 72;

export interface FrontierPoint {
  haircutBps: number;
  hedgeBreakBps: number;
  /** Mean credit / Σ IM over the H = 1 day evaluations, in percent. */
  freedPctDaily: number;
  /** Same over the H = 20 min windows. */
  freedPct20m: number;
  breachesDaily: number;
  daysDaily: number;
  breaches20m: number;
  windows20m: number;
  /** Breaches per evaluation day. */
  breachRateDaily: number;
  breachRate20m: number;
  /** With the proposed floor credit ≤ (Σ IM − R_p)⁺. */
  freedPctDailyFloor: number;
  breachesDailyFloor: number;
  breaches20mFloor: number;
  /** Dates (H = 1 day) that breach at this setting. */
  breachDates: string[];
}

interface Evaluated {
  credit: number;
  creditFloor: number;
}

const micro = (x: bigint) => Number(x) / 1e6;

function model(e: Effective, hedgeBreakBps: number): RiskModel {
  const base = stressedModel(e);
  return hedgeBreakBps > 0 ? new HedgeBreakModel(base, hedgeBreakBps) : base;
}

/** Credit and floored credit for one day's parameters at one setting (memoized by the caller). */
function evaluate(e: Effective, book: Book, credit: CurrentParams["credit"], haircutBps: number, hedgeBreakBps: number): Evaluated {
  const eff = { ...e, hedgeBreakBps };
  const c = micro(creditFor(eff, book, { ...credit, haircut_bps: haircutBps }, hedgeBreakBps > 0));
  const legs = bookLegs(book);
  const sumIm = micro(legs.reduce((s, l) => s + l.initialMargin, 0n));
  const rp = micro(model(e, hedgeBreakBps).portfolioRisk(legs));
  return { credit: c, creditFloor: Math.min(c, Math.max(0, sumIm - rp)) };
}

export function frontier(
  daily: DayResult[],
  bar: DayResult[],
  current: CurrentParams,
  book: Book,
  haircuts: number[] = HAIRCUTS_BPS,
  hedgeBreaks: number[] = HEDGE_BREAKS_BPS
): FrontierPoint[] {
  const out: FrontierPoint[] = [];
  for (const hb of hedgeBreaks) {
    for (const h of haircuts) {
      // Parameters change at most once per day, so memoize by their content.
      const memo = new Map<string, Evaluated>();
      const at = (r: DayResult) => {
        const k = JSON.stringify(r.params);
        let v = memo.get(k);
        if (!v) {
          v = evaluate(r.params, book, current.credit, h, hb);
          memo.set(k, v);
        }
        return v;
      };
      const stats = (rows: DayResult[]) => {
        let freed = 0;
        let freedFloor = 0;
        let breaches = 0;
        let breachesFloor = 0;
        const dates: string[] = [];
        for (const r of rows) {
          const v = at(r);
          freed += v.credit / r.sumIm;
          freedFloor += v.creditFloor / r.sumIm;
          if (r.loss.pathLoss > r.sumIm - v.credit) {
            breaches++;
            dates.push(r.date);
          }
          if (r.loss.pathLoss > r.sumIm - v.creditFloor) breachesFloor++;
        }
        const n = Math.max(1, rows.length);
        return { freed: (100 * freed) / n, freedFloor: (100 * freedFloor) / n, breaches, breachesFloor, dates };
      };
      const d = stats(daily);
      const b = stats(bar);
      out.push({
        haircutBps: h,
        hedgeBreakBps: hb,
        freedPctDaily: d.freed,
        freedPct20m: b.freed,
        breachesDaily: d.breaches,
        daysDaily: daily.length,
        breaches20m: b.breaches,
        windows20m: bar.length,
        breachRateDaily: d.breaches / Math.max(1, daily.length),
        breachRate20m: b.breaches / Math.max(1, bar.length / WINDOWS_PER_DAY),
        freedPctDailyFloor: d.freedFloor,
        breachesDailyFloor: d.breachesFloor,
        breaches20mFloor: b.breachesFloor,
        breachDates: d.dates,
      });
    }
  }
  return out;
}

/**
 * Points not dominated on (freed at H = 1 day: higher is better, breaches at
 * H = 1 day: fewer is better, breaches at H = 20 min: fewer is better).
 */
export function paretoFront(points: FrontierPoint[]): FrontierPoint[] {
  const dominates = (a: FrontierPoint, b: FrontierPoint) =>
    a.freedPctDaily >= b.freedPctDaily &&
    a.breachesDaily <= b.breachesDaily &&
    a.breaches20m <= b.breaches20m &&
    (a.freedPctDaily > b.freedPctDaily || a.breachesDaily < b.breachesDaily || a.breaches20m < b.breaches20m);
  return points.filter((p) => !points.some((q) => dominates(q, p)));
}
