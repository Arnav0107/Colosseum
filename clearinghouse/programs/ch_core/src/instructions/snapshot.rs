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
}

pub struct SnapshotArgs {
    pub venue_id: [u8; 32],
    pub asset_id: u8,
    pub notional: u64,
    pub is_long: bool,
    pub initial_margin: u64,
    pub maintenance_margin: u64,
}

impl SubmitPositionSnapshot<'_> {
    pub fn process(&mut self, a: SnapshotArgs, bumps: &SubmitPositionSnapshotBumps) -> Result<()> {
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

        let clock = Clock::get()?;
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
