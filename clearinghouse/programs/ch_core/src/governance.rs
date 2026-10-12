//! Governance rules, as pure functions (unit-tested natively):
//!
//! * absolute bounds (floors and ceilings) on every parameter;
//! * per-update step limits, so one change cannot move a parameter far;
//! * classification of a change as *credit-reducing* (applies immediately)
//!   or not (queued behind `timelock_slots`, cancellable by the guardian).
//!
//! A change is classified credit-reducing only when it provably cannot
//! increase any portfolio's credit. Everything else, including every
//! correlation and shock change (their effect depends on the direction of a
//! book's exposures), waits for the timelock.

use crate::state::*;
use crate::ClearinghouseError;
use anchor_lang::prelude::*;

/// Haircut floor: some haircut always applies to the computed hedge benefit.
pub const MIN_HAIRCUT_BPS: u16 = 500;
/// Per-leg cap ceiling as a fraction of IM (IM − MM applies as well).
pub const MAX_CREDIT_BPS_OF_IM: u16 = 9_000;
pub const MAX_SNAPSHOT_AGE_SLOTS: u64 = 216_000; // ~1 day
pub const MAX_RISK_MODEL_AGE_SLOTS: u64 = 6_480_000; // ~30 days
pub const MAX_PRICE_AGE_SECS: i64 = 3_600;
pub const MAX_CONF_BPS: u16 = 1_000;
pub const MAX_MOVE_BPS_GUARD: u16 = 2_000;
pub const MAX_TIMELOCK_SLOTS: u64 = 1_296_000; // ~6 days
/// Production builds: risk-increasing changes wait at least ~1 day. Localnet
/// and test builds (mock oracle) may use 0 so suites do not need to wait.
#[cfg(not(feature = "mock-oracle"))]
pub const MIN_TIMELOCK_SLOTS: u64 = 216_000;
#[cfg(feature = "mock-oracle")]
pub const MIN_TIMELOCK_SLOTS: u64 = 0;

/// Largest change of one correlation in one update (0.25).
pub const MAX_CORR_STEP_PPM: i32 = 250_000;
/// Largest haircut reduction in one update.
pub const MAX_HAIRCUT_DECREASE_BPS: u16 = 1_000;
/// Caps may at most double per update (or rise to this floor from below).
pub const CAP_STEP_FLOOR: u64 = 1_000_000_000; // $1,000

/// A7: production builds (`pyth` + `venue-cpi`) require the four role keys
/// to be distinct. Localnet and test builds may use one key for all roles.
#[cfg(all(feature = "pyth", feature = "venue-cpi"))]
pub const ROLES_MUST_BE_DISTINCT: bool = true;
#[cfg(not(all(feature = "pyth", feature = "venue-cpi")))]
pub const ROLES_MUST_BE_DISTINCT: bool = false;

/// The four keys are non-default and pairwise distinct.
pub fn roles_distinct(keys: &[Pubkey; 4]) -> bool {
    keys.iter().all(|k| *k != Pubkey::default())
        && (0..4).all(|i| (i + 1..4).all(|j| keys[i] != keys[j]))
}

/// A7: role check used by `initialize`, `set_roles` and `accept_admin`:
/// no default key ever; in production builds, four distinct keys.
pub fn check_roles(
    admin: Pubkey,
    risk_authority: Pubkey,
    venue_manager: Pubkey,
    guardian: Pubkey,
) -> Result<()> {
    let keys = [admin, risk_authority, venue_manager, guardian];
    require!(
        keys.iter().all(|k| *k != Pubkey::default()),
        ClearinghouseError::InvalidAdmin
    );
    if ROLES_MUST_BE_DISTINCT {
        require!(roles_distinct(&keys), ClearinghouseError::RolesNotDistinct);
    }
    Ok(())
}

/// Hedge break (A3): production builds may not disable the scenario (0).
#[cfg(not(feature = "mock-oracle"))]
pub const HEDGE_BREAK_MAY_BE_DISABLED: bool = false;
#[cfg(feature = "mock-oracle")]
pub const HEDGE_BREAK_MAY_BE_DISABLED: bool = true;

/// 0 (disabled, mock-oracle builds only) or within ch_math's
/// `[HEDGE_BREAK_MIN_BPS, HEDGE_BREAK_MAX_BPS]`.
pub fn hedge_break_ok(bps: u16) -> bool {
    ch_math::risk::hedge_break_bps_is_valid(bps) && (bps != 0 || HEDGE_BREAK_MAY_BE_DISABLED)
}

pub fn validate_config_params(p: &ConfigParams) -> Result<()> {
    let ok = (1..=MAX_VENUES as u8).contains(&p.max_venues)
        && (MIN_HAIRCUT_BPS..=10_000).contains(&p.haircut_bps)
        && (1..=MAX_CREDIT_TTL_SLOTS).contains(&p.credit_ttl_slots)
        && (1..=MAX_SNAPSHOT_AGE_SLOTS).contains(&p.snapshot_max_age_slots)
        && p.max_credit_bps_of_im <= MAX_CREDIT_BPS_OF_IM
        && (1..=MAX_RISK_MODEL_AGE_SLOTS).contains(&p.risk_model_max_age_slots)
        && (1..=MAX_PRICE_AGE_SECS).contains(&p.max_price_age_secs)
        && (1..=MAX_CONF_BPS).contains(&p.max_conf_bps)
        && (1..=MAX_MOVE_BPS_GUARD).contains(&p.max_move_bps)
        && (MIN_TIMELOCK_SLOTS..=MAX_TIMELOCK_SLOTS).contains(&p.timelock_slots)
        && hedge_break_ok(p.hedge_break_bps);
    require!(ok, ClearinghouseError::InvalidConfigParams);
    Ok(())
}

fn cap_step_ok(old: u64, new: u64) -> bool {
    new <= old.saturating_mul(2).max(CAP_STEP_FLOOR)
}

/// Step limits between the current and a proposed config.
pub fn check_params_step(old: &ConfigParams, new: &ConfigParams) -> Result<()> {
    let ok = old.haircut_bps.saturating_sub(new.haircut_bps) <= MAX_HAIRCUT_DECREASE_BPS
        && cap_step_ok(old.max_credit_per_user, new.max_credit_per_user)
        && cap_step_ok(old.max_credit_per_venue, new.max_credit_per_venue)
        && cap_step_ok(old.max_credit_per_asset, new.max_credit_per_asset)
        && cap_step_ok(old.global_credit_cap, new.global_credit_cap);
    require!(ok, ClearinghouseError::ChangeTooLarge);
    Ok(())
}

/// True iff every field moves in the credit-reducing (or neutral) direction.
pub fn params_reduce_credit(old: &ConfigParams, new: &ConfigParams) -> bool {
    new.max_venues <= old.max_venues
        && new.haircut_bps >= old.haircut_bps
        && new.credit_ttl_slots <= old.credit_ttl_slots
        && new.snapshot_max_age_slots <= old.snapshot_max_age_slots
        && new.max_credit_per_user <= old.max_credit_per_user
        && new.max_credit_bps_of_im <= old.max_credit_bps_of_im
        && new.max_credit_per_venue <= old.max_credit_per_venue
        && new.max_credit_per_asset <= old.max_credit_per_asset
        && new.global_credit_cap <= old.global_credit_cap
        && new.risk_model_max_age_slots <= old.risk_model_max_age_slots
        && new.max_price_age_secs <= old.max_price_age_secs
        && new.max_conf_bps <= old.max_conf_bps
        && new.max_move_bps <= old.max_move_bps
        // Shortening the timelock is never "reducing".
        && new.timelock_slots >= old.timelock_slots
        // A larger hedge-break fraction (or enabling it from 0) only adds
        // scenario loss; lowering or disabling it waits for the timelock.
        && new.hedge_break_bps >= old.hedge_break_bps
}

pub fn validate_asset_risk(p: &AssetRiskParams) -> Result<()> {
    let ok = if p.shock_bps == 0 {
        p.gap_bps == 0 && p.basis_bps == 0
    } else {
        (MIN_SHOCK_BPS..=MAX_MOVE_BPS).contains(&p.shock_bps)
            && (p.shock_bps..=MAX_MOVE_BPS).contains(&p.gap_bps)
            && (MIN_BASIS_BPS..=MAX_MOVE_BPS).contains(&p.basis_bps)
    };
    require!(ok, ClearinghouseError::InvalidRiskParams);
    Ok(())
}

/// A shock may change by at most a factor of 2 per update (enabling a
/// disabled asset or disabling one is not limited, but enabling is queued).
pub fn check_asset_step(old: &AssetRiskParams, new: &AssetRiskParams) -> Result<()> {
    let ok = old.shock_bps == 0
        || new.shock_bps == 0
        || ((new.shock_bps as u32) * 2 >= old.shock_bps as u32
            && (new.shock_bps as u32) <= (old.shock_bps as u32) * 2);
    require!(ok, ClearinghouseError::ChangeTooLarge);
    Ok(())
}

/// Credit-reducing asset changes: disabling the asset, or keeping the shock
/// while not lowering gap or basis (larger gaps only add scenario loss).
pub fn asset_change_reduces_credit(old: &AssetRiskParams, new: &AssetRiskParams) -> bool {
    new.shock_bps == 0
        || (old.shock_bps != 0
            && new.shock_bps == old.shock_bps
            && new.gap_bps >= old.gap_bps
            && new.basis_bps >= old.basis_bps)
}

pub fn validate_stress(corr_stress_ppm: u32, basis_decorr_ppm: u32) -> Result<()> {
    require!(
        (MIN_CORR_STRESS_PPM..=PPM_ONE).contains(&corr_stress_ppm)
            && (MIN_BASIS_DECORR_PPM..=PPM_ONE).contains(&basis_decorr_ppm),
        ClearinghouseError::InvalidRiskParams
    );
    Ok(())
}

/// More correlation stress and more basis decorrelation only add risk.
pub fn stress_change_reduces_credit(old: (u32, u32), new: (u32, u32)) -> bool {
    new.0 >= old.0 && new.1 >= old.1
}

/// |new − old| ≤ MAX_CORR_STEP_PPM for one pair.
pub fn check_corr_step(old: i32, new: i32) -> Result<()> {
    require!(
        (new as i64 - old as i64).abs() <= MAX_CORR_STEP_PPM as i64,
        ClearinghouseError::ChangeTooLarge
    );
    Ok(())
}

/// Adding a scenario to an unused slot can only raise scenario risk.
/// Replacing or clearing an existing scenario may lower it.
pub fn scenario_change_reduces_credit(old_kind: u8, new_kind: u8) -> bool {
    old_kind == ch_math::risk::scenario_kind::UNUSED
        && new_kind != ch_math::risk::scenario_kind::UNUSED
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ConfigParams {
        ConfigParams {
            max_venues: 16,
            haircut_bps: 2_000,
            credit_ttl_slots: 3_000,
            snapshot_max_age_slots: 10_000,
            max_credit_per_user: 100_000_000_000,
            max_credit_bps_of_im: 7_500,
            max_credit_per_venue: 1_000_000_000_000,
            max_credit_per_asset: 1_000_000_000_000,
            global_credit_cap: 10_000_000_000_000,
            risk_model_max_age_slots: 100_000,
            max_price_age_secs: 60,
            max_conf_bps: 100,
            max_move_bps: 500,
            timelock_slots: 216_000,
            hedge_break_bps: 5_000,
        }
    }

    #[test]
    fn floors_and_ceilings() {
        assert!(validate_config_params(&params()).is_ok());
        let cases: Vec<fn(&mut ConfigParams)> = vec![
            |p| p.haircut_bps = MIN_HAIRCUT_BPS - 1,
            |p| p.max_credit_bps_of_im = MAX_CREDIT_BPS_OF_IM + 1,
            |p| p.credit_ttl_slots = 0,
            |p| p.credit_ttl_slots = MAX_CREDIT_TTL_SLOTS + 1,
            |p| p.snapshot_max_age_slots = MAX_SNAPSHOT_AGE_SLOTS + 1,
            |p| p.max_price_age_secs = 0,
            |p| p.max_conf_bps = MAX_CONF_BPS + 1,
            |p| p.max_move_bps = 0,
            |p| p.max_move_bps = MAX_MOVE_BPS_GUARD + 1,
            |p| p.timelock_slots = MAX_TIMELOCK_SLOTS + 1,
            |p| p.max_venues = 0,
            |p| p.max_venues = 65,
            |p| p.hedge_break_bps = ch_math::risk::HEDGE_BREAK_MIN_BPS - 1,
            |p| p.hedge_break_bps = ch_math::risk::HEDGE_BREAK_MAX_BPS + 1,
        ];
        for mutate in cases {
            let mut p = params();
            mutate(&mut p);
            assert!(validate_config_params(&p).is_err(), "{p:?}");
        }
    }

    #[test]
    fn param_classification() {
        let old = params();
        let mut stricter = old;
        stricter.haircut_bps = 3_000;
        stricter.global_credit_cap /= 2;
        stricter.max_move_bps = 300;
        stricter.hedge_break_bps = 7_500;
        assert!(params_reduce_credit(&old, &stricter));

        for loosen in [
            |p: &mut ConfigParams| p.haircut_bps = 1_500,
            |p: &mut ConfigParams| p.global_credit_cap += 1,
            |p: &mut ConfigParams| p.max_move_bps += 1,
            |p: &mut ConfigParams| p.credit_ttl_slots += 1,
            |p: &mut ConfigParams| p.timelock_slots -= 1,
            |p: &mut ConfigParams| p.max_price_age_secs += 1,
            |p: &mut ConfigParams| p.hedge_break_bps -= 1,
        ] {
            let mut new = old;
            loosen(&mut new);
            assert!(!params_reduce_credit(&old, &new), "{new:?}");
        }
    }

    #[test]
    fn param_steps() {
        let old = params();
        let mut a = old;
        a.haircut_bps = old.haircut_bps - MAX_HAIRCUT_DECREASE_BPS;
        assert!(check_params_step(&old, &a).is_ok());
        a.haircut_bps -= 1;
        assert!(check_params_step(&old, &a).is_err());
        let mut b = old;
        b.global_credit_cap = old.global_credit_cap * 2;
        assert!(check_params_step(&old, &b).is_ok());
        b.global_credit_cap += 1;
        assert!(check_params_step(&old, &b).is_err());
        let mut c = old;
        c.max_credit_per_user = 0;
        let mut d = c;
        d.max_credit_per_user = CAP_STEP_FLOOR;
        assert!(
            check_params_step(&c, &d).is_ok(),
            "from zero up to the floor"
        );
    }

    #[test]
    fn asset_rules() {
        let on = AssetRiskParams {
            shock_bps: 1_000,
            gap_bps: 1_000,
            basis_bps: 100,
        };
        let off = AssetRiskParams::default();
        assert!(asset_change_reduces_credit(&on, &off), "disable");
        assert!(!asset_change_reduces_credit(&off, &on), "enable is queued");
        let mut wider = on;
        wider.gap_bps = 2_000;
        assert!(asset_change_reduces_credit(&on, &wider));
        let mut lower_shock = on;
        lower_shock.shock_bps = 900;
        lower_shock.gap_bps = 900;
        assert!(!asset_change_reduces_credit(&on, &lower_shock));
        let mut higher_shock = on;
        higher_shock.shock_bps = 1_100;
        higher_shock.gap_bps = 1_100;
        assert!(
            !asset_change_reduces_credit(&on, &higher_shock),
            "shock changes are not monotone in credit"
        );
        assert!(check_asset_step(&on, &lower_shock).is_ok());
        let mut halved = on;
        halved.shock_bps = 499;
        halved.gap_bps = 499;
        assert!(check_asset_step(&on, &halved).is_err());
        assert!(check_asset_step(&off, &on).is_ok());
    }

    #[test]
    fn stress_corr_and_scenario_rules() {
        assert!(stress_change_reduces_credit(
            (100_000, 20_000),
            (150_000, 20_000)
        ));
        assert!(!stress_change_reduces_credit(
            (100_000, 20_000),
            (90_000, 30_000)
        ));
        assert!(check_corr_step(800_000, 550_000).is_ok());
        assert!(check_corr_step(800_000, 549_999).is_err());
        assert!(check_corr_step(-1_000_000, -750_000).is_ok());
        use ch_math::risk::scenario_kind as k;
        assert!(scenario_change_reduces_credit(k::UNUSED, k::HYPOTHETICAL));
        assert!(!scenario_change_reduces_credit(k::HYPOTHETICAL, k::UNUSED));
        assert!(!scenario_change_reduces_credit(
            k::HYPOTHETICAL,
            k::HISTORICAL_REPLAY
        ));
    }

    #[test]
    fn hedge_break_bounds_and_production_floor() {
        let mut p = params();
        for ok in [
            ch_math::risk::HEDGE_BREAK_MIN_BPS,
            5_000,
            ch_math::risk::HEDGE_BREAK_MAX_BPS,
        ] {
            p.hedge_break_bps = ok;
            assert!(validate_config_params(&p).is_ok(), "{ok}");
        }
        p.hedge_break_bps = 0;
        // Disabling is allowed only in mock-oracle (localnet/test) builds.
        assert_eq!(
            validate_config_params(&p).is_ok(),
            HEDGE_BREAK_MAY_BE_DISABLED
        );
        let old = params();
        let mut enabled_from_zero = old;
        enabled_from_zero.hedge_break_bps = 2_500;
        let mut zero = old;
        zero.hedge_break_bps = 0;
        assert!(
            params_reduce_credit(&zero, &enabled_from_zero),
            "enabling reduces credit"
        );
        assert!(
            !params_reduce_credit(&old, &zero),
            "disabling waits for the timelock"
        );
    }

    #[test]
    fn roles_must_be_four_distinct_non_default_keys() {
        let k: Vec<Pubkey> = (1..=4u8).map(|b| Pubkey::new_from_array([b; 32])).collect();
        assert!(roles_distinct(&[k[0], k[1], k[2], k[3]]));
        for (i, j) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
            let mut r = [k[0], k[1], k[2], k[3]];
            r[j] = r[i];
            assert!(!roles_distinct(&r), "pair {i},{j}");
            assert_eq!(
                check_roles(r[0], r[1], r[2], r[3]).is_ok(),
                !ROLES_MUST_BE_DISTINCT
            );
        }
        assert!(!roles_distinct(&[k[0], k[1], k[2], Pubkey::default()]));
        assert!(check_roles(k[0], k[1], k[2], Pubkey::default()).is_err());
        assert!(check_roles(k[0], k[1], k[2], k[3]).is_ok());
    }
}
