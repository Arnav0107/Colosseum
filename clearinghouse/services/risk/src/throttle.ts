/**
 * B3 evaluation: the regime-aware credit throttle, off-chain.
 *
 * Design (docs/design/CHANGES_FOR_APPROVAL.md, B3): at refresh, each leg's
 * move m = |price − ema| / ema (bps, rounded up) scales credit linearly from
 * 1 (m ≤ start) to 0 (m ≥ full); the portfolio scale is the minimum over its
 * legs. On-chain the EMA would be Pyth's own `ema_price`; this backtest has
 * no Pyth data, so it uses an exponential average of 20-minute closes with a
 * 1-hour time constant as a stand-in (labelled in BACKTEST.md). Confidence
 * widening is NOT modelled (no confidence data).
 *
 * Integer math mirrors what ch_math would do: ppm scale, floors.
 */
import { Bar, BAR_MS } from "./data";

export const PPM = 1_000_000;
/** Pyth's EMA horizon is about an hour; one 20-minute bar weighs 1 − e^(−20/60). */
export const EMA_ALPHA = 1 - Math.exp(-BAR_MS / (60 * 60 * 1000));

export interface ThrottleParams {
  /** Below this move from the EMA (bps): full credit. 0 disables the throttle. */
  startBps: number;
  /** At or above this move (bps): no credit. */
  fullBps: number;
}

export const DEFAULT_THROTTLE: ThrottleParams = { startBps: 100, fullBps: 300 };

/** EMA of closes of bars strictly before time `t` (null if none). */
export function emaBefore(bars: Bar[], t: number): number | null {
  let ema: number | null = null;
  for (const b of bars) {
    if (b.t + BAR_MS > t) break;
    ema = ema === null ? b.c : ema + EMA_ALPHA * (b.c - ema);
  }
  return ema;
}

/** Precompute EMA after each bar (index-aligned); emaAt[i] uses bars 0..i. */
export function emaSeries(bars: Bar[]): number[] {
  const out: number[] = [];
  let ema = bars[0]?.c ?? 0;
  for (const b of bars) {
    ema = out.length === 0 ? b.c : ema + EMA_ALPHA * (b.c - ema);
    out.push(ema);
  }
  return out;
}

/** |price − ema| / ema in bps, rounded up (as price_move_bps). */
export function moveBps(ema: number, price: number): number {
  return Math.ceil((Math.abs(price - ema) * 10_000) / ema - 1e-12);
}

/** lin(m; start, full) in ppm, floored: 1e6 if m ≤ start, 0 if m ≥ full. */
export function linPpm(m: number, p: ThrottleParams): number {
  if (p.startBps === 0) return PPM;
  if (m <= p.startBps) return PPM;
  if (m >= p.fullBps) return 0;
  return Math.floor((PPM * (p.fullBps - m)) / (p.fullBps - p.startBps));
}

/** Portfolio scale = min over legs. */
export function scalePpm(moves: number[], p: ThrottleParams): number {
  return moves.reduce((s, m) => Math.min(s, linPpm(m, p)), PPM);
}
