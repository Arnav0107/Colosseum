/**
 * Positive-semidefiniteness check and nearest-correlation-matrix repair.
 *
 * Repair is Higham (2002) alternating projections with Dykstra's correction:
 * the nearest (Frobenius) symmetric PSD matrix with unit diagonal. A repair is
 * always returned as a report (`PsdReport.repaired`, the distance moved and the
 * eigenvalues before/after); callers must surface it and never apply it silently.
 */
export type Matrix = number[][];

const clone = (m: Matrix): Matrix => m.map((r) => [...r]);

/** Eigenvalues and eigenvectors (columns) of a symmetric matrix, cyclic Jacobi. */
export function symmetricEigen(m: Matrix): { values: number[]; vectors: Matrix } {
  const n = m.length;
  const a = clone(m);
  const v: Matrix = Array.from({ length: n }, (_, i) => Array.from({ length: n }, (_, j) => (i === j ? 1 : 0)));
  for (let sweep = 0; sweep < 100; sweep++) {
    let off = 0;
    for (let p = 0; p < n; p++) for (let q = p + 1; q < n; q++) off += a[p][q] ** 2;
    if (off < 1e-24) break;
    for (let p = 0; p < n; p++) {
      for (let q = p + 1; q < n; q++) {
        if (Math.abs(a[p][q]) < 1e-300) continue;
        const theta = (a[q][q] - a[p][p]) / (2 * a[p][q]);
        const t = Math.sign(theta || 1) / (Math.abs(theta) + Math.sqrt(theta * theta + 1));
        const c = 1 / Math.sqrt(t * t + 1);
        const s = t * c;
        for (let k = 0; k < n; k++) {
          const akp = a[k][p];
          const akq = a[k][q];
          a[k][p] = c * akp - s * akq;
          a[k][q] = s * akp + c * akq;
        }
        for (let k = 0; k < n; k++) {
          const apk = a[p][k];
          const aqk = a[q][k];
          a[p][k] = c * apk - s * aqk;
          a[q][k] = s * apk + c * aqk;
        }
        for (let k = 0; k < n; k++) {
          const vkp = v[k][p];
          const vkq = v[k][q];
          v[k][p] = c * vkp - s * vkq;
          v[k][q] = s * vkp + c * vkq;
        }
      }
    }
  }
  return { values: a.map((r, i) => r[i]), vectors: v };
}

export const minEigenvalue = (m: Matrix): number => Math.min(...symmetricEigen(m).values);

/** Tolerance: eigenvalues ≥ −PSD_TOL count as non-negative (float round-off). */
export const PSD_TOL = 1e-12;

export const isPsd = (m: Matrix): boolean => minEigenvalue(m) >= -PSD_TOL;

function projectPsd(m: Matrix): Matrix {
  const { values, vectors } = symmetricEigen(m);
  const n = m.length;
  const out: Matrix = Array.from({ length: n }, () => new Array(n).fill(0));
  for (let k = 0; k < n; k++) {
    const lam = Math.max(values[k], 0);
    if (lam === 0) continue;
    for (let i = 0; i < n; i++) for (let j = 0; j < n; j++) out[i][j] += lam * vectors[i][k] * vectors[j][k];
  }
  return out;
}

const frobenius = (a: Matrix, b: Matrix): number => Math.sqrt(a.reduce((s, r, i) => s + r.reduce((t, x, j) => t + (x - b[i][j]) ** 2, 0), 0));

/** Higham's alternating projections onto {PSD} ∩ {unit diagonal}. */
export function nearestCorrelation(m: Matrix, maxIter = 1_000): Matrix {
  const n = m.length;
  let y = clone(m);
  let ds: Matrix = Array.from({ length: n }, () => new Array(n).fill(0));
  for (let it = 0; it < maxIter; it++) {
    const r = y.map((row, i) => row.map((x, j) => x - ds[i][j]));
    const x = projectPsd(r);
    ds = x.map((row, i) => row.map((v, j) => v - r[i][j]));
    const next = x.map((row, i) => row.map((v, j) => (i === j ? 1 : v)));
    const moved = frobenius(next, y);
    y = next;
    if (moved < 1e-12 && minEigenvalue(y) >= -PSD_TOL) break;
  }
  // Symmetrise against round-off and clip into [-1, 1].
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      const s = Math.max(-1, Math.min(1, (y[i][j] + y[j][i]) / 2));
      y[i][j] = s;
      y[j][i] = s;
    }
    y[i][i] = 1;
  }
  return y;
}

export interface PsdReport {
  psd: boolean;
  minEigenvalueBefore: number;
  repaired: boolean;
  /** Only when repaired. */
  minEigenvalueAfter?: number;
  frobeniusDistance?: number;
  method?: string;
}

/** Check `m`; if it is not PSD, repair it and say so. */
export function checkAndRepair(m: Matrix): { matrix: Matrix; report: PsdReport } {
  const before = minEigenvalue(m);
  if (before >= -PSD_TOL) return { matrix: m, report: { psd: true, minEigenvalueBefore: before, repaired: false } };
  const fixed = nearestCorrelation(m);
  return {
    matrix: fixed,
    report: {
      psd: false,
      minEigenvalueBefore: before,
      repaired: true,
      minEigenvalueAfter: minEigenvalue(fixed),
      frobeniusDistance: frobenius(fixed, m),
      method: "Higham (2002) nearest correlation matrix, alternating projections with Dykstra correction",
    },
  };
}
