/**
 * On-chain governance bounds, mirrored from programs/ch_core/src/state.rs and
 * programs/ch_core/src/governance.rs. `bounds.test.ts` re-reads the Rust
 * sources and fails if any value here drifts from them.
 */
import { HEDGE_BREAK_MAX_BPS, HEDGE_BREAK_MIN_BPS } from "../../../testbench/src/portfolio";

export const MIN_SHOCK_BPS = 100;
export const MAX_MOVE_BPS = 10_000;
export const MIN_BASIS_BPS = 10;
export const MIN_CORR_STRESS_PPM = 50_000;
export const MIN_BASIS_DECORR_PPM = 10_000;
export const PPM_ONE = 1_000_000;
export const RHO_ONE = 1_000_000;
/** governance.rs: |Δρ| per update. */
export const MAX_CORR_STEP_PPM = 250_000;
export { HEDGE_BREAK_MAX_BPS, HEDGE_BREAK_MIN_BPS };

export interface AssetRiskArgs {
  shock_bps: number;
  gap_bps: number;
  basis_bps: number;
}

/** One adjustment made to a raw estimate. Every clamp is reported, never silent. */
export interface Clamp {
  field: string;
  raw: number;
  value: number;
  rule: string;
}

const clampTo = (field: string, raw: number, lo: number, hi: number, rule: string, out: Clamp[]): number => {
  const v = Math.min(hi, Math.max(lo, raw));
  if (v !== raw) out.push({ field, raw, value: v, rule });
  return v;
};

/**
 * `validate_asset_risk` and `check_asset_step`: shock in [100, 10,000] and
 * within [ceil(old/2), 2·old] of the current shock (unless the asset is
 * currently disabled); gap in [shock, 10,000]; basis in [10, 10,000].
 */
export function clampAssetRisk(name: string, raw: AssetRiskArgs, current: AssetRiskArgs | null, out: Clamp[]): AssetRiskArgs {
  let shock = clampTo(`${name}.shock_bps`, raw.shock_bps, MIN_SHOCK_BPS, MAX_MOVE_BPS, "bounds [100, 10000]", out);
  if (current && current.shock_bps > 0) {
    shock = clampTo(
      `${name}.shock_bps`,
      shock,
      Math.ceil(current.shock_bps / 2),
      current.shock_bps * 2,
      `step limit: within [old/2, 2·old] of ${current.shock_bps}`,
      out
    );
  }
  const gap = clampTo(`${name}.gap_bps`, raw.gap_bps, shock, MAX_MOVE_BPS, "bounds [shock_bps, 10000]", out);
  const basis = clampTo(`${name}.basis_bps`, raw.basis_bps, MIN_BASIS_BPS, MAX_MOVE_BPS, "bounds [10, 10000]", out);
  return { shock_bps: shock, gap_bps: gap, basis_bps: basis };
}

/** ρ in [−1e6, 1e6] and `check_corr_step`: |new − old| ≤ 250,000. */
export function clampCorr(field: string, raw: number, current: number | null, out: Clamp[]): number {
  let r = clampTo(field, raw, -RHO_ONE, RHO_ONE, "bounds [-1e6, 1e6]", out);
  if (current !== null) {
    r = clampTo(field, r, current - MAX_CORR_STEP_PPM, current + MAX_CORR_STEP_PPM, `step limit: |Δρ| ≤ 250000 from ${current}`, out);
  }
  return r;
}

/** `validate_stress`: δ in [50,000, 1e6], β in [10,000, 1e6]. No step limit. */
export function clampStress(raw: { corr_stress_ppm: number; basis_decorr_ppm: number }, out: Clamp[]) {
  return {
    corr_stress_ppm: clampTo("corr_stress_ppm", raw.corr_stress_ppm, MIN_CORR_STRESS_PPM, PPM_ONE, "bounds [50000, 1e6]", out),
    basis_decorr_ppm: clampTo("basis_decorr_ppm", raw.basis_decorr_ppm, MIN_BASIS_DECORR_PPM, PPM_ONE, "bounds [10000, 1e6]", out),
  };
}

/** `hedge_break_bps_is_valid`: [2,500, 10,000] when enabled (0 disables it). */
export function clampHedgeBreak(raw: number, out: Clamp[]): number {
  return clampTo("hedge_break_bps", raw, HEDGE_BREAK_MIN_BPS, HEDGE_BREAK_MAX_BPS, "bounds [2500, 10000]", out);
}

/** governance.rs `asset_change_reduces_credit`: applies at once if true, else queued. */
export function assetChangeReducesCredit(old: AssetRiskArgs, next: AssetRiskArgs): boolean {
  return (
    next.shock_bps === 0 ||
    (old.shock_bps !== 0 && next.shock_bps === old.shock_bps && next.gap_bps >= old.gap_bps && next.basis_bps >= old.basis_bps)
  );
}

/** governance.rs `stress_change_reduces_credit`. */
export function stressChangeReducesCredit(old: [number, number], next: [number, number]): boolean {
  return next[0] >= old[0] && next[1] >= old[1];
}
