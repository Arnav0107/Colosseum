/** Small, deterministic statistics helpers (off-chain; floats are fine here). */

/** Quantile, linear interpolation between order statistics (Hyndman–Fan type 7). */
export function quantile(xs: number[], q: number): number {
  if (xs.length === 0) throw new Error("quantile of empty sample");
  if (!(q >= 0 && q <= 1)) throw new Error(`quantile level ${q} not in [0, 1]`);
  const s = [...xs].sort((a, b) => a - b);
  const h = (s.length - 1) * q;
  const lo = Math.floor(h);
  const hi = Math.ceil(h);
  return s[lo] + (h - lo) * (s[hi] - s[lo]);
}

export function mean(xs: number[]): number {
  return xs.reduce((a, b) => a + b, 0) / xs.length;
}

export function pearson(x: number[], y: number[]): number {
  if (x.length !== y.length || x.length < 2) throw new Error("pearson: need two equal-length samples of size ≥ 2");
  const mx = mean(x);
  const my = mean(y);
  let sxy = 0;
  let sxx = 0;
  let syy = 0;
  for (let i = 0; i < x.length; i++) {
    sxy += (x[i] - mx) * (y[i] - my);
    sxx += (x[i] - mx) ** 2;
    syy += (y[i] - my) ** 2;
  }
  if (sxx === 0 || syy === 0) throw new Error("pearson: zero variance");
  return sxy / Math.sqrt(sxx * syy);
}

/** OLS slope of y on x. */
export function beta(y: number[], x: number[]): number {
  const mx = mean(x);
  const my = mean(y);
  let sxy = 0;
  let sxx = 0;
  for (let i = 0; i < x.length; i++) {
    sxy += (x[i] - mx) * (y[i] - my);
    sxx += (x[i] - mx) ** 2;
  }
  return sxy / sxx;
}
