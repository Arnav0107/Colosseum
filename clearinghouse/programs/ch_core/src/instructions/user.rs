use anchor_lang::prelude::*;

use crate::events::*;
use crate::state::*;
use crate::ClearinghouseError;

/// Grant or change consent. Creates the user's consent and portfolio accounts
/// on first use and invalidates any credit in the same instruction: a consent
/// change can never leave stale credit behind.
#[derive(Accounts)]
pub struct UpdateUserConsent<'info> {
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + UserConsent::INIT_SPACE,
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        init_if_needed,
        payer = user,
        space = 8 + Portfolio::INIT_SPACE,
        seeds = [SEED_PORTFOLIO, user.key().as_ref()],
        bump
    )]
    pub portfolio: Box<Account<'info, Portfolio>>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

impl UpdateUserConsent<'_> {
    pub fn process(
        &mut self,
        is_active: bool,
        authorized_venues_bitmap: u64,
        bumps: &UpdateUserConsentBumps,
    ) -> Result<()> {
        let user = self.user.key();

        let consent = &mut self.user_consent;
        consent.user = user;
        consent.is_active = is_active;
        consent.authorized_venues_bitmap = authorized_venues_bitmap;
        consent.bump = bumps.user_consent;

        let portfolio = &mut self.portfolio;
        if portfolio.user == Pubkey::default() {
            // Freshly created.
            portfolio.user = user;
            portfolio.status = status::EMPTY;
            portfolio.credit_kind = CREDIT_KIND_TRADING_POWER_ONLY;
            portfolio.bump = bumps.portfolio;
        }
        portfolio.mark_invalid();

        emit!(ConsentUpdated {
            user,
            is_active,
            authorized_venues_bitmap,
            portfolio_version: portfolio.version,
        });
        Ok(())
    }
}

/// Close the user's consent and portfolio and return their rent to the user.
///
/// Requires every position snapshot to be closed first (revoke consent for a
/// venue, then `close_position_snapshot`), so no snapshot can outlive the
/// portfolio that registers it. Releases any outstanding-credit share, so
/// the global, venue and asset counters stay exact. Allowed while paused: it
/// only removes credit.
///
/// Re-creating the accounts later (`update_user_consent`) starts a fresh,
/// EMPTY portfolio with no legs and no credit; nothing from before the close
/// carries over.
#[derive(Accounts)]
pub struct CloseUserAccounts<'info> {
    #[account(mut, seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        close = user,
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump = user_consent.bump,
        has_one = user @ ClearinghouseError::Unauthorized
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        mut,
        close = user,
        seeds = [SEED_PORTFOLIO, user.key().as_ref()],
        bump = portfolio.bump,
        has_one = user @ ClearinghouseError::Unauthorized
    )]
    pub portfolio: Box<Account<'info, Portfolio>>,

    #[account(mut)]
    pub user: Signer<'info>,
}

impl CloseUserAccounts<'_> {
    pub fn process(&mut self) -> Result<()> {
        let portfolio = &mut self.portfolio;
        require!(
            portfolio.num_registered == 0,
            ClearinghouseError::SnapshotsStillOpen
        );
        portfolio.release(&mut self.config)?;
        portfolio.mark_invalid();
        emit!(UserAccountsClosed {
            user: self.user.key(),
            portfolio_version: portfolio.version,
        });
        Ok(())
    }
}
