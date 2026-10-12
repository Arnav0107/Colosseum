//! Positive-definiteness check of a correlation sub-matrix (A4).
//!
//! The parametric risk `√(xᵀρ*x)` is only meaningful when the base
//! correlation matrix is positive semidefinite: the direction-aware stress
//! makes every cross term at least as large as with `ρ`, so `xᵀρ*x ≥ xᵀρx`,
//! but if `ρ` itself is not PSD then "at least the base" can still be below
//! the true risk (docs/RISK_MODEL.md §3b). This module decides, with integer
//! arithmetic only, whether the sub-matrix of `ρ` over the assets present is
//! positive definite with margin; the risk model falls back to a `ρ`-free
//! risk when it is not.
//!
//! Method: fixed-point LDLᵀ (no square roots, Crout form) in i128, built so
//! that SBF pays for only one i128 division per pivot (8 at n = 8; the first
//! version, one division per term, cost ~140k CU, docs/PERFORMANCE.md):
//!
//! * entries `a_ij = ρ_ppm · 2^30` (scale `PSD_SCALE` = 1e6 · 2^30, no division);
//! * `w_ij = l_ij · d_j` at scale `PSD_SCALE`, `l_ij` at scale 2^`PSD_L_SHIFT`;
//! * sums of products are exact, then shifted (`>>` = floor) once per entry;
//! * `l_ij = (w_ij · r_j) >> PSD_L_SHIFT` with one reciprocal
//!   `r_j = 2^(2·PSD_L_SHIFT) / d_j` per pivot.
//!
//! A matrix is accepted iff every pivot `d_j ≥ PSD_MIN_PIVOT` (1 ppm of the
//! unit diagonal). The diagonal correction is rounded up, so pivots are never
//! overstated; overflow anywhere rejects (fail closed).
//!
//! * accepted ⇒ every pivot ≥ 1e-6, so the matrix is positive definite
//!   (rounding error ≪ 1e-6, see `psd_matches_float_classifier_near_singular`
//!   in tests/non_psd.rs);
//! * smallest eigenvalue ≥ ~1e-6 ⇒ accepted;
//! * exactly singular matrices (ρ = ±1 between different assets) are
//!   rejected, which errs on the safe side.

/// Binary part of the matrix scale: entries are `ρ_ppm << PSD_PPM_SHIFT`.
pub const PSD_PPM_SHIFT: u32 = 30;
/// Fixed-point scale of the matrix and the pivots: 1e6 · 2^30 (≈ 1.07e15).
pub const PSD_SCALE: i128 = 1_000_000 << PSD_PPM_SHIFT;
/// Smallest accepted pivot: 1 ppm of the unit diagonal (= 2^30).
pub const PSD_MIN_PIVOT: i128 = 1 << PSD_PPM_SHIFT;
/// `l_ij` are held at scale 2^PSD_L_SHIFT.
pub const PSD_L_SHIFT: u32 = 50;
/// Largest sub-matrix checked (one per distinct asset of a portfolio).
pub const PSD_MAX_DIM: usize = 8;

const PPM: i128 = 1_000_000;

/// ceil(x / 2^k) for any sign (arithmetic shifts floor).
fn shr_ceil(x: i128, k: u32) -> i128 {
    -((-x) >> k)
}

/// `true` iff the correlation sub-matrix over `assets` (distinct, any order;
/// callers pass them sorted so the result is independent of leg order) is
/// positive definite with every LDLᵀ pivot ≥ `PSD_MIN_PIVOT`. `rho(a, b)` is
/// the base correlation (ppm), clamped to [−1e6, 1e6]; the diagonal is 1.
/// At most `PSD_MAX_DIM` assets; more is rejected. 0 or 1 asset: `true`.
pub fn corr_is_positive_definite(assets: &[u8], rho: &dyn Fn(u8, u8) -> i64) -> bool {
    let n = assets.len();
    if n > PSD_MAX_DIM {
        return false;
    }
    if n <= 1 {
        return true;
    }
    let a = |i: usize, j: usize| -> i128 {
        if i == j {
            PSD_SCALE
        } else {
            (rho(assets[i], assets[j]) as i128).clamp(-PPM, PPM) << PSD_PPM_SHIFT
        }
    };
    let mut w = [[0i128; PSD_MAX_DIM]; PSD_MAX_DIM];
    let mut l = [[0i128; PSD_MAX_DIM]; PSD_MAX_DIM];
    for j in 0..n {
        // d_j = a_jj − Σ_k w_jk l_jk (scale S · 2^L, shifted back to S), the
        // correction rounded up so the pivot is never overstated.
        let mut acc: i128 = 0;
        for k in 0..j {
            let Some(v) = w[j][k]
                .checked_mul(l[j][k])
                .and_then(|t| acc.checked_add(t))
            else {
                return false;
            };
            acc = v;
        }
        let Some(dj) = a(j, j).checked_sub(shr_ceil(acc, PSD_L_SHIFT)) else {
            return false;
        };
        if dj < PSD_MIN_PIVOT {
            return false;
        }
        // One division per pivot: r_j = 2^(2L) / d_j (d_j ≥ 2^30, so r_j ≤ 2^70).
        let r = (1i128 << (2 * PSD_L_SHIFT)) / dj;
        // w_ij = a_ij − Σ_k w_ik l_jk;  l_ij = w_ij / d_j at scale 2^L  (i > j).
        for i in j + 1..n {
            let mut acc: i128 = 0;
            for k in 0..j {
                let Some(v) = w[i][k]
                    .checked_mul(l[j][k])
                    .and_then(|t| acc.checked_add(t))
                else {
                    return false;
                };
                acc = v;
            }
            let Some(wij) = a(i, j).checked_sub(acc >> PSD_L_SHIFT) else {
                return false;
            };
            let Some(lij) = wij.checked_mul(r) else {
                return false;
            };
            w[i][j] = wij;
            l[i][j] = lij >> PSD_L_SHIFT;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(m: &'static [(u8, u8, i64)]) -> impl Fn(u8, u8) -> i64 {
        move |a, b| {
            m.iter()
                .find(|(x, y, _)| (*x, *y) == (a.min(b), a.max(b)))
                .map(|t| t.2)
                .unwrap_or(0)
        }
    }

    #[test]
    fn identity_and_mild_correlations_are_accepted() {
        assert!(corr_is_positive_definite(&[], &|_, _| 0));
        assert!(corr_is_positive_definite(&[3], &|_, _| -1_000_000));
        assert!(corr_is_positive_definite(&[0, 1, 2], &|_, _| 0));
        assert!(corr_is_positive_definite(&[0, 1], &|_, _| 800_000));
        assert!(corr_is_positive_definite(&[0, 1], &|_, _| -999_000));
        let all: Vec<u8> = (0..8).collect();
        assert!(corr_is_positive_definite(&all, &|_, _| 500_000));
    }

    #[test]
    fn non_psd_and_singular_matrices_are_rejected() {
        // Three assets pairwise −0.9: eigenvalue 1 + 2(−0.9) = −0.8.
        assert!(!corr_is_positive_definite(&[0, 1, 2], &|_, _| -900_000));
        // ρ = ±1 between two different assets: singular (eigenvalue 0).
        assert!(!corr_is_positive_definite(&[0, 1], &|_, _| 1_000_000));
        assert!(!corr_is_positive_definite(&[0, 1], &|_, _| -1_000_000));
        // A(0,1) = A(1,2) = 0.9, A(0,2) = −0.9: indefinite.
        let m = table(&[(0, 1, 900_000), (1, 2, 900_000), (0, 2, -900_000)]);
        assert!(!corr_is_positive_definite(&[0, 1, 2], &m));
        // More than PSD_MAX_DIM assets: rejected.
        let nine: Vec<u8> = (0..9).collect();
        assert!(!corr_is_positive_definite(&nine, &|_, _| 0));
    }

    #[test]
    fn boundary_pivot_is_one_ppm() {
        // 2×2: pivots 1 and 1 − ρ². ρ = 0.9999995 → 1 − ρ² ≈ 1.0e-6 (just below
        // 1 ppm once rounded up); ρ = 0.999999 is 999,999 ppm → 1 − ρ² ≈ 2e-6.
        assert!(corr_is_positive_definite(&[0, 1], &|_, _| 999_999));
        assert!(!corr_is_positive_definite(&[0, 1], &|_, _| 1_000_000));
    }

    #[test]
    fn order_of_assets_does_not_change_the_answer_on_these_cases() {
        let m = table(&[(0, 1, 700_000), (1, 2, 600_000), (0, 2, 300_000)]);
        for p in [[0u8, 1, 2], [2, 1, 0], [1, 0, 2], [2, 0, 1]] {
            assert!(corr_is_positive_definite(&p, &m));
        }
        let bad = table(&[(0, 1, 900_000), (1, 2, 900_000), (0, 2, -900_000)]);
        for p in [[0u8, 1, 2], [2, 1, 0], [1, 0, 2], [2, 0, 1]] {
            assert!(!corr_is_positive_definite(&p, &bad));
        }
    }
}
