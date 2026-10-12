use anchor_lang::prelude::*;
use ch_math::portfolio::{
    cap_groups, compute_portfolio_credit, CreditParams, PortfolioCredit, PortfolioLeg,
};
use ch_math::risk::{AssetRisk, HedgeBreakModel, StressedRiskModel};

use crate::events::*;
use crate::oracle;
use crate::state::*;
use crate::ClearinghouseError;

/// Load a `PosSnapshot` from an arbitrary account: owner, discriminator,
/// PDA (from its own stored seeds and bump) and user binding are all checked.
fn load_snapshot(info: &AccountInfo, user: &Pubkey) -> Result<PosSnapshot> {
    require_keys_eq!(
        *info.owner,
        crate::ID,
        ClearinghouseError::InvalidSnapshotAccount
    );
    let data = info.try_borrow_data()?;
    let snap = PosSnapshot::try_deserialize(&mut &data[..])
        .map_err(|_| error!(ClearinghouseError::InvalidSnapshotAccount))?;
    let expected = Pubkey::create_program_address(
        &[
            SEED_SNAPSHOT,
            snap.user.as_ref(),
            snap.venue_id.as_ref(),
            &[snap.asset_id],
            &[snap.bump],
        ],
        &crate::ID,
    )
    .map_err(|_| error!(ClearinghouseError::InvalidSnapshotAccount))?;
    require_keys_eq!(
        expected,
        info.key(),
        ClearinghouseError::InvalidSnapshotAccount
    );
    require_keys_eq!(snap.user, *user, ClearinghouseError::InvalidSnapshotAccount);
    Ok(snap)
}

/// Run the stressed risk model and the credit engine. Separate frame: the
/// asset table lives on its own stack frame (SBF frames are 4 KiB).
#[inline(never)]
fn compute_credit(
    model: &RiskModel,
    legs: &[PortfolioLeg],
    p: &ConfigParams,
) -> Result<PortfolioCredit> {
    let assets: [AssetRisk; MAX_ASSETS] = core::array::from_fn(|a| AssetRisk {
        shock_bps: model.assets[a].shock_bps,
        gap_bps: model.assets[a].gap_bps,
        basis_bps: model.assets[a].basis_bps,
    });
    let rho = |a: u8, b: u8| model.rho(a, b);
    let risk = StressedRiskModel {
        assets: &assets,
        rho: &rho,
        corr_stress_ppm: model.corr_stress_ppm,
        basis_decorr_ppm: model.basis_decorr_ppm,
        // Unused slots are all-zero moves: zero loss, so they are harmless.
        scenarios: &model.scenarios,
    };
    let params = CreditParams {
        haircut_bps: p.haircut_bps,
        max_credit_per_user: p.max_credit_per_user,
        max_credit_bps_of_im: p.max_credit_bps_of_im,
        // TradingPowerOnly: never below the venue's maintenance margin.
        cap_at_im_minus_mm: true,
    };
    // Hedge break (A3): a ρ-independent scenario, R_p ≥ S · h / 10_000.
    // 0 (mock-oracle builds only) leaves the stressed model unchanged.
    let r = if p.hedge_break_bps > 0 {
        let hb = HedgeBreakModel {
            inner: risk,
            hedge_break_bps: p.hedge_break_bps,
        };
        compute_portfolio_credit(legs, &hb, &params)
    } else {
        compute_portfolio_credit(legs, &risk, &params)
    };
    r.map_err(|e| ClearinghouseError::from(e).into())
}

/// Recompute a user's credit from current on-chain state. Permissionless:
/// the result is a deterministic function of the snapshots, the risk model,
/// the config and the oracle prices, so anyone (the venue in the same
/// transaction as its snapshot, a keeper, the user) may call it.
///
/// remaining_accounts = [snapshot_0 .. snapshot_{n-1}, oracle_0 .. oracle_{n-1}]
/// where n = portfolio.num_registered, in any order of legs, and oracle_i is
/// the price account for snapshot_i's asset.
#[derive(Accounts)]
pub struct RefreshPortfolio<'info> {
    #[account(mut, seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(seeds = [SEED_RISK_MODEL], bump = risk_model.load()?.bump)]
    pub risk_model: AccountLoader<'info, RiskModel>,

    #[account(
        seeds = [SEED_CONSENT, portfolio.user.as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        mut,
        seeds = [SEED_PORTFOLIO, portfolio.user.as_ref()],
        bump = portfolio.bump
    )]
    pub portfolio: Box<Account<'info, Portfolio>>,
}

pub fn process_refresh_portfolio<'info>(
    accounts: &mut RefreshPortfolio<'info>,
    remaining: &[AccountInfo<'info>],
) -> Result<()> {
    let config = &mut accounts.config;
    let model = &*accounts.risk_model.load()?;
    let consent = &accounts.user_consent;
    let portfolio = &mut accounts.portfolio;
    let p = config.params;

    require!(!config.paused, ClearinghouseError::ProgramPaused);
    require!(consent.is_active, ClearinghouseError::UserConsentMissing);
    let clock = Clock::get()?;
    require!(
        clock.slot.saturating_sub(model.updated_slot) <= p.risk_model_max_age_slots,
        ClearinghouseError::RiskModelStale
    );

    let n = portfolio.num_registered as usize;
    require!(
        remaining.len() == 2 * n,
        ClearinghouseError::IncompletePortfolio
    );

    // Drop the previous computation's share before computing a new one, so a
    // repeated refresh never double-counts.
    portfolio.release(config)?;

    let mut legs = [PortfolioLeg {
        venue_index: 0,
        asset: 0,
        is_long: false,
        notional: 0,
        initial_margin: 0,
        maintenance_margin: 0,
    }; MAX_LEGS];
    let mut states = [LegState::default(); MAX_LEGS];
    let mut seen = [LegKey::default(); MAX_LEGS];

    for i in 0..n {
        let snap = load_snapshot(&remaining[i], &portfolio.user)?;
        let key = LegKey {
            venue_index: snap.venue_index,
            asset_id: snap.asset_id,
        };
        require!(
            portfolio.is_registered(key),
            ClearinghouseError::IncompletePortfolio
        );
        require!(!seen[..i].contains(&key), ClearinghouseError::DuplicateLeg);
        seen[i] = key;

        require!(
            config.venue_is_active(snap.venue_index),
            ClearinghouseError::VenueInactive
        );
        require!(
            consent.allows(snap.venue_index),
            ClearinghouseError::VenueNotAuthorized
        );
        require!(
            clock.slot.saturating_sub(snap.slot) <= p.snapshot_max_age_slots,
            ClearinghouseError::SnapshotStale
        );

        let reading = oracle::read_price(
            &remaining[n + i],
            snap.asset_id,
            &model.oracle_feeds[snap.asset_id as usize],
        )?;
        oracle::check_price(
            &reading,
            clock.unix_timestamp,
            p.max_price_age_secs,
            p.max_conf_bps,
        )?;

        legs[i] = PortfolioLeg {
            venue_index: snap.venue_index,
            asset: snap.asset_id,
            is_long: snap.is_long,
            notional: snap.notional,
            initial_margin: snap.initial_margin,
            maintenance_margin: snap.maintenance_margin,
        };
        states[i] = LegState {
            venue_index: snap.venue_index,
            asset_id: snap.asset_id,
            is_long: snap.is_long,
            snapshot_version: snap.version,
            snapshot_slot: snap.slot,
            notional: snap.notional,
            initial_margin: snap.initial_margin,
            maintenance_margin: snap.maintenance_margin,
            baseline_price: reading.price_micro,
            baseline_conf: reading.conf_micro,
            baseline_publish_ts: reading.publish_ts,
            credit: 0,
        };
    }

    let result = compute_credit(model, &legs[..n], &p)?;

    // Protocol-wide budgets: per venue, per asset, then global. Each step only
    // scales credits down, so all three hold at the end.
    let mut credit = result.leg_credit;
    let venue_headroom: [u64; MAX_VENUES] = core::array::from_fn(|v| {
        p.max_credit_per_venue
            .saturating_sub(config.venue_outstanding[v])
    });
    cap_groups(
        &mut credit[..n],
        &|i| legs[i].venue_index as usize,
        &venue_headroom,
    );
    let asset_headroom: [u64; MAX_ASSETS] = core::array::from_fn(|a| {
        p.max_credit_per_asset
            .saturating_sub(config.asset_outstanding[a])
    });
    cap_groups(
        &mut credit[..n],
        &|i| legs[i].asset as usize,
        &asset_headroom,
    );
    let global_headroom = [p
        .global_credit_cap
        .saturating_sub(config.global_outstanding_credit)];
    cap_groups(&mut credit[..n], &|_| 0, &global_headroom);

    let mut total = 0u64;
    for i in 0..n {
        states[i].credit = credit[i];
        total = total
            .checked_add(credit[i])
            .ok_or(ClearinghouseError::MathOverflow)?;
        let v = &mut config.venue_outstanding[legs[i].venue_index as usize];
        *v = v
            .checked_add(credit[i])
            .ok_or(ClearinghouseError::MathOverflow)?;
        let a = &mut config.asset_outstanding[legs[i].asset as usize];
        *a = a
            .checked_add(credit[i])
            .ok_or(ClearinghouseError::MathOverflow)?;
    }
    config.global_outstanding_credit = config
        .global_outstanding_credit
        .checked_add(total)
        .ok_or(ClearinghouseError::MathOverflow)?;

    portfolio.status = status::ACTIVE;
    portfolio.credit_kind = CREDIT_KIND_TRADING_POWER_ONLY;
    portfolio.version = portfolio
        .version
        .checked_add(1)
        .ok_or(ClearinghouseError::MathOverflow)?;
    portfolio.risk_epoch = config.risk_epoch;
    portfolio.risk_model_version = model.version;
    portfolio.computed_slot = clock.slot;
    portfolio.expires_slot = clock.slot.saturating_add(p.credit_ttl_slots);
    portfolio.total_credit = total;
    portfolio.counted = true;
    portfolio.counted_epoch = config.risk_epoch;
    portfolio.sum_standalone_risk = result.sum_standalone_risk;
    portfolio.portfolio_risk = result.portfolio_risk;
    portfolio.hedge_benefit_ppm = result.hedge_benefit_ppm;
    portfolio.num_legs = n as u8;
    portfolio.legs = [LegState::default(); MAX_LEGS];
    portfolio.legs[..n].copy_from_slice(&states[..n]);

    emit!(PortfolioRefreshed {
        user: portfolio.user,
        version: portfolio.version,
        risk_epoch: portfolio.risk_epoch,
        num_legs: n as u8,
        sum_standalone_risk: result.sum_standalone_risk,
        portfolio_risk: result.portfolio_risk,
        hedge_benefit_ppm: result.hedge_benefit_ppm,
        total_credit: total,
        expires_slot: portfolio.expires_slot,
    });
    Ok(())
}

/// Permissionless: mark a portfolio's credit unusable and release its share of
/// the outstanding-credit counters, if (and only if) some condition holds.
/// Works while paused.
///
/// remaining_accounts: either empty, or exactly one oracle account per leg
/// (in leg order) to check the price baseline.
#[derive(Accounts)]
pub struct InvalidatePortfolio<'info> {
    #[account(mut, seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    /// Source of the governed oracle feed per asset.
    #[account(seeds = [SEED_RISK_MODEL], bump = risk_model.load()?.bump)]
    pub risk_model: AccountLoader<'info, RiskModel>,

    #[account(
        seeds = [SEED_CONSENT, portfolio.user.as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        mut,
        seeds = [SEED_PORTFOLIO, portfolio.user.as_ref()],
        bump = portfolio.bump
    )]
    pub portfolio: Box<Account<'info, Portfolio>>,
}

pub fn process_invalidate_portfolio<'info>(
    accounts: &mut InvalidatePortfolio<'info>,
    remaining: &[AccountInfo<'info>],
) -> Result<()> {
    use crate::events::invalidation as r;
    let config = &mut accounts.config;
    let consent = &accounts.user_consent;
    let portfolio = &mut accounts.portfolio;
    let p = config.params;
    let clock = Clock::get()?;
    let n = portfolio.num_legs as usize;

    let mut reasons: u16 = 0;
    if portfolio.status != status::ACTIVE {
        if portfolio.counted {
            reasons |= r::RELEASE_ONLY;
        }
    } else {
        if portfolio.risk_epoch != config.risk_epoch {
            reasons |= r::EPOCH_MISMATCH;
        }
        if clock.slot > portfolio.expires_slot {
            reasons |= r::EXPIRED;
        }
        for leg in &portfolio.legs[..n] {
            if !config.venue_is_active(leg.venue_index) {
                reasons |= r::VENUE_INACTIVE;
            }
            if !consent.allows(leg.venue_index) {
                reasons |= r::CONSENT_WITHDRAWN;
            }
            if clock.slot.saturating_sub(leg.snapshot_slot) > p.snapshot_max_age_slots {
                reasons |= r::SNAPSHOT_STALE;
            }
        }
        if !remaining.is_empty() {
            require!(
                remaining.len() == n,
                ClearinghouseError::IncompletePortfolio
            );
            for (leg, info) in portfolio.legs[..n].iter().zip(remaining) {
                // A wrong account is an error, never a trip.
                let feed = accounts.risk_model.load()?.oracle_feeds[leg.asset_id as usize];
                let reading = oracle::read_price(info, leg.asset_id, &feed)?;
                // Only a price published after the baseline is evidence.
                // Otherwise anyone could pick an older genuine update (stale,
                // wide, or far from the baseline) and trip correct credit.
                // An older reading says nothing about this leg; skip it.
                if reading.publish_ts <= leg.baseline_publish_ts {
                    continue;
                }
                if oracle::check_price(
                    &reading,
                    clock.unix_timestamp,
                    p.max_price_age_secs,
                    p.max_conf_bps,
                )
                .is_err()
                {
                    reasons |= r::ORACLE_UNUSABLE;
                } else if ch_math::price_move_bps(leg.baseline_price, reading.price_micro)
                    > p.max_move_bps as u64
                {
                    // Compared with the baseline stored at computation time,
                    // never with a snapshot that a venue can rewrite (F5).
                    reasons |= r::PRICE_BREACH;
                }
            }
        }
    }
    require!(reasons != 0, ClearinghouseError::InvalidationNotJustified);

    portfolio.release(config)?;
    portfolio.mark_invalid();

    emit!(PortfolioInvalidated {
        user: portfolio.user,
        version: portfolio.version,
        reasons,
    });
    Ok(())
}
