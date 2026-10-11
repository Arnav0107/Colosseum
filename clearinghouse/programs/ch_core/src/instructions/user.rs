use anchor_lang::prelude::*;

use crate::events::*;
use crate::state::*;

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
