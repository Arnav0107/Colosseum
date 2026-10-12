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
 *  - credit is the engine's (with the A2 floor credit <= (sum IM - R_p)+);
 *    the NoFloor fields repeat everything without the floor (pre-A2
 *    reference, creditWithoutFloor) so the floor's cost stays visible.
 *
 * Pure and deterministic: same rows in, same numbers out.
 */
import { Book, creditFor, creditWithoutFloor, DayResult, Effective } from "./backtest";
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
  /** PRE-A2 reference: without the floor. */
  freedPctDailyNoFloor: number;
  breachesDailyNoFloor: number;
  breaches20mNoFloor: number;
  /** Dates (H = 1 day) that breach at this setting. */
  breachDates: string[];
}

interface Evaluated {
  credit: number;
  creditNoFloor: number;
}

const micro = (x: bigint) => Number(x) / 1e6;

/** Credit with and without the floor for one day's parameters at one setting (memoized by the caller). */
function evaluate(e: Effective, book: Book, credit: CurrentParams["credit"], haircutBps: number, hedgeBreakBps: number): Evaluated {
  const eff = { ...e, hedgeBreakBps };
  const params = { ...credit, haircut_bps: haircutBps };
  return {
    credit: micro(creditFor(eff, book, params, hedgeBreakBps > 0)),
    creditNoFloor: micro(creditWithoutFloor(eff, book, params, hedgeBreakBps > 0)),
  };
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
        let freedNoFloor = 0;
        let breaches = 0;
        let breachesNoFloor = 0;
        const dates: string[] = [];
        for (const r of rows) {
          const v = at(r);
          freed += v.credit / r.sumIm;
          freedNoFloor += v.creditNoFloor / r.sumIm;
          if (r.loss.pathLoss > r.sumIm - v.credit) {
            breaches++;
            dates.push(r.date);
          }
          if (r.loss.pathLoss > r.sumIm - v.creditNoFloor) breachesNoFloor++;
        }
        const n = Math.max(1, rows.length);
        return { freed: (100 * freed) / n, freedNoFloor: (100 * freedNoFloor) / n, breaches, breachesNoFloor, dates };
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
        freedPctDailyNoFloor: d.freedNoFloor,
        breachesDailyNoFloor: d.breachesNoFloor,
        breaches20mNoFloor: b.breachesNoFloor,
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
