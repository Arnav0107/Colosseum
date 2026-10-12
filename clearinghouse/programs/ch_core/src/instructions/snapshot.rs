use anchor_lang::prelude::*;

use crate::events::*;
use crate::state::*;
use crate::ClearinghouseError;

/// A venue reports one position (user, venue, asset). Every write bumps the
/// snapshot version and invalidates the user's portfolio in the same
/// instruction (F2): a shrunk, flipped or closed position can never keep the
/// credit computed for the old one.
///
/// Allowed while paused: it only ever removes usable credit.
#[derive(Accounts)]
#[instruction(venue_id: [u8; 32], asset_id: u8)]
pub struct SubmitPositionSnapshot<'info> {
    #[account(seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        seeds = [SEED_VENUE, venue_id.as_ref()],
        bump = venue_registration.bump,
        constraint = venue_registration.venue_authority == venue_authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub venue_registration: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        mut,
        seeds = [SEED_PORTFOLIO, user.key().as_ref()],
        bump = portfolio.bump
    )]
    pub portfolio: Box<Account<'info, Portfolio>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + PosSnapshot::INIT_SPACE,
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_id.as_ref(), &[asset_id]],
        bump
    )]
    pub pos_snapshot: Box<Account<'info, PosSnapshot>>,

    /// CHECK: the user whose position this is; bound through the consent,
    /// portfolio and snapshot PDA seeds.
    pub user: UncheckedAccount<'info>,

    /// With `venue-cpi` (production) this is the venue program's
    /// [b"clearinghouse"] PDA (enforced at registration), so only that program
    /// can sign, through CPI. With `dev-venue-signer` it may be a keypair.
    pub venue_authority: Signer<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,

    /// A5d: source of the governed oracle feed for the mark-price check.
    #[account(seeds = [SEED_RISK_MODEL], bump = risk_model.load()?.bump)]
    pub risk_model: AccountLoader<'info, RiskModel>,
}

pub struct SnapshotArgs {
    pub venue_id: [u8; 32],
    pub asset_id: u8,
    pub notional: u64,
    pub is_long: bool,
    pub initial_margin: u64,
    pub maintenance_margin: u64,
    /// A5d: venue's mark price in micro-USD; 0 = not reported.
    pub mark_price_micro: u64,
}

impl<'info> SubmitPositionSnapshot<'info> {
    /// remaining_accounts: empty, or (when `mark_price_micro > 0`) the
    /// asset's oracle account first.
    pub fn process(
        &mut self,
        a: SnapshotArgs,
        bumps: &SubmitPositionSnapshotBumps,
        remaining: &[AccountInfo<'info>],
    ) -> Result<()> {
        let venue = &self.venue_registration;
        require!(venue.is_active, ClearinghouseError::VenueInactive);
        require!(
            self.config.venue_is_active(venue.venue_index),
            ClearinghouseError::VenueInactive
        );
        require!(
            self.user_consent.is_active,
            ClearinghouseError::UserConsentMissing
        );
        require!(
            self.user_consent.allows(venue.venue_index),
            ClearinghouseError::VenueNotAuthorized
        );
        require!(
            (a.asset_id as usize) < MAX_ASSETS,
            ClearinghouseError::InvalidAssetId
        );
        // A closed position is removed with close_position_snapshot, never
        // reported as notional 0.
        require!(a.notional > 0, ClearinghouseError::ZeroNotional);
        require!(
            a.maintenance_margin > 0
                && a.maintenance_margin <= a.initial_margin
                && a.initial_margin <= a.notional,
            ClearinghouseError::InvalidMargin
        );

        let p = self.config.params;
        // A5a: per-leg notional cap.
        require!(
            a.notional <= p.max_leg_notional,
            ClearinghouseError::LegNotionalCapExceeded
        );

        let clock = Clock::get()?;

        // A5d: optional mark price, checked against the governed oracle.
        if a.mark_price_micro > 0 {
            let info = remaining
                .first()
                .ok_or(ClearinghouseError::MarkPriceOracleMissing)?;
            let feed = self.risk_model.load()?.oracle_feeds[a.asset_id as usize];
            let reading = crate::oracle::read_price(info, a.asset_id, &feed)?;
            crate::oracle::check_price(
                &reading,
                clock.unix_timestamp,
                p.max_price_age_secs,
                p.max_conf_bps,
            )?;
            require!(
                ch_math::price_move_bps(reading.price_micro, a.mark_price_micro)
                    <= p.max_move_bps as u64,
                ClearinghouseError::MarkPriceDeviation
            );
        }

        let user = self.user.key();
        let key = LegKey {
            venue_index: venue.venue_index,
            asset_id: a.asset_id,
        };

        let snapshot = &mut self.pos_snapshot;
        let is_new = snapshot.version == 0;
        if is_new {
            snapshot.user = user;
            snapshot.venue_id = a.venue_id;
            snapshot.venue_index = venue.venue_index;
            snapshot.asset_id = a.asset_id;
            snapshot.rent_payer = self.payer.key();
            snapshot.bump = bumps.pos_snapshot;
        } else {
            // init_if_needed re-entry: the seeds already bind these, but check
            // the stored identity anyway (defence in depth).
            require!(
                snapshot.user == user
                    && snapshot.venue_id == a.venue_id
                    && snapshot.asset_id == a.asset_id
                    && snapshot.venue_index == venue.venue_index,
                ClearinghouseError::InvalidSnapshotAccount
            );
        }
        // A5c: growth limit within a window (a new snapshot starts from 0, so
        // close-and-reopen gains nothing).
        if is_new {
            snapshot.window_start_slot = clock.slot;
            snapshot.window_base_notional = 0;
        } else if clock.slot.saturating_sub(snapshot.window_start_slot)
            >= p.notional_growth_window_slots
        {
            snapshot.window_start_slot = clock.slot;
            snapshot.window_base_notional = snapshot.notional;
        }
        if p.max_notional_growth_bps > 0 {
            let base = snapshot.window_base_notional as u128;
            let by_rate = base + base * p.max_notional_growth_bps as u128 / 10_000;
            let by_floor = base + p.notional_growth_floor as u128;
            require!(
                a.notional as u128 <= by_rate.max(by_floor),
                ClearinghouseError::NotionalGrowthTooFast
            );
        }
        snapshot.mark_price = a.mark_price_micro;
        snapshot.version = snapshot
            .version
            .checked_add(1)
            .ok_or(ClearinghouseError::MathOverflow)?;
        snapshot.is_long = a.is_long;
        snapshot.notional = a.notional;
        snapshot.initial_margin = a.initial_margin;
        snapshot.maintenance_margin = a.maintenance_margin;
        snapshot.slot = clock.slot;
        snapshot.timestamp = clock.unix_timestamp;

        let portfolio = &mut self.portfolio;
        portfolio.register(key)?;
        portfolio.set_registered_notional(key, a.notional);
        // A5b: this user's total notional on this venue.
        require!(
            portfolio.venue_notional(key.venue_index) <= p.max_venue_notional_per_user as u128,
            ClearinghouseError::VenueNotionalCapExceeded
        );
        portfolio.mark_invalid();

        emit!(PositionSnapshotSubmitted {
            user,
            venue_index: key.venue_index,
            asset_id: key.asset_id,
            version: snapshot.version,
            notional: a.notional,
            is_long: a.is_long,
            initial_margin: a.initial_margin,
            maintenance_margin: a.maintenance_margin,
            slot: clock.slot,
            portfolio_version: portfolio.version,
        });
        Ok(())
    }
}

/// Remove a position. Callable by the venue authority at any time, or by the
/// user once their consent no longer covers the venue or the venue is
/// inactive (so a user is never stuck with a leg they cannot refresh).
/// Invalidates the portfolio in the same instruction; rent goes back to
/// whoever paid it.
#[derive(Accounts)]
pub struct ClosePositionSnapshot<'info> {
    #[account(
        seeds = [SEED_VENUE, venue_registration.venue_id.as_ref()],
        bump = venue_registration.bump
    )]
    pub venue_registration: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_CONSENT, pos_snapshot.user.as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        mut,
        seeds = [SEED_PORTFOLIO, pos_snapshot.user.as_ref()],
        bump = portfolio.bump
    )]
    pub portfolio: Box<Account<'info, Portfolio>>,

    #[account(
        mut,
        close = rent_receiver,
        seeds = [
            SEED_SNAPSHOT,
            pos_snapshot.user.as_ref(),
            venue_registration.venue_id.as_ref(),
            &[pos_snapshot.asset_id]
        ],
        bump = pos_snapshot.bump
    )]
    pub pos_snapshot: Box<Account<'info, PosSnapshot>>,

    /// CHECK: must be the account that paid the snapshot's rent.
    #[account(mut, address = pos_snapshot.rent_payer @ ClearinghouseError::Unauthorized)]
    pub rent_receiver: UncheckedAccount<'info>,

    pub closer: Signer<'info>,
}

impl ClosePositionSnapshot<'_> {
    pub fn process(&mut self) -> Result<()> {
        let venue = &self.venue_registration;
        let snapshot = &self.pos_snapshot;
        let closer = self.closer.key();
        let by_venue = closer == venue.venue_authority;
        let by_user = closer == snapshot.user
            && (!venue.is_active || !self.user_consent.allows(venue.venue_index));
        require!(
            by_venue || by_user,
            ClearinghouseError::SnapshotCloseNotAllowed
        );

        let key = LegKey {
            venue_index: snapshot.venue_index,
            asset_id: snapshot.asset_id,
        };
        let portfolio = &mut self.portfolio;
        portfolio.unregister(key);
        portfolio.mark_invalid();

        emit!(PositionSnapshotClosed {
            user: snapshot.user,
            venue_index: key.venue_index,
            asset_id: key.asset_id,
            portfolio_version: portfolio.version,
        });
        Ok(())
    }
}
