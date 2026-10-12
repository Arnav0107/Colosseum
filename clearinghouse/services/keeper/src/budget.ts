/**
 * Compute budget for `refresh_portfolio` (A1).
 *
 * With counterparty allowlists, a refresh runs the credit engine once for the
 * joint book plus once per distinct "view" (the legs of the venues a venue
 * accepts) that is neither the full set nor a single venue's lone leg. Most
 * books need no extra run and fit the default 200,000 CU. Measured in
 * crates/ch_svm_tests/tests/perf.rs: 8 legs on 8 venues with 8 distinct
 * 7-leg views cost 739,513 CU against 158,896 for the joint run alone, about
 * 72.6k CU per extra run. This mirrors the view grouping of
 * `ch_math::view::compute_view_credit` (the cache key is the view mask).
 */
export const DEFAULT_CU_LIMIT = 200_000;
export const MAX_CU_LIMIT = 1_400_000;
/** Measured ~72.6k per 7-leg view; rounded up. */
export const CU_PER_VIEW_RUN = 80_000;

const bit = (v: number): bigint => (v >= 0 && v < 64 ? 1n << BigInt(v) : 0n);

/** Number of extra engine runs `compute_view_credit` makes for these legs. */
export function extraViewRuns(legVenues: number[], venueAccepts: bigint[]): number {
  let present = 0n;
  for (const v of legVenues) present |= bit(v);
  const seen = new Set<bigint>();
  let runs = 0;
  for (const v of legVenues) {
    const mask = ((venueAccepts[v] ?? 0n) | bit(v)) & present;
    if (mask === present || seen.has(mask)) continue;
    seen.add(mask);
    const n = legVenues.filter((x) => (mask & bit(x)) !== 0n).length;
    if (n >= 2) runs += 1;
  }
  return runs;
}

/** Units to request, or null when the default limit suffices. */
export function computeUnitLimitFor(legVenues: number[], venueAccepts: bigint[]): number | null {
  const runs = extraViewRuns(legVenues, venueAccepts);
  if (runs === 0) return null;
  return Math.min(MAX_CU_LIMIT, DEFAULT_CU_LIMIT + CU_PER_VIEW_RUN * runs);
}
