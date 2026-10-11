//! Clearinghouse core program.
//!
//! One `Portfolio` per user holds credit computed jointly over every
//! position (snapshot) the user has consented to share. Credit is
//! *trading-power only*: a venue that integrates it may lower its
//! initial-margin requirement for risk-increasing actions, and must check
//! `Portfolio::usable_credit` (status, epoch, expiry, leg) every time. It is
//! not collateral, not withdrawable, and does not move gains between venues.
//! See docs/ARCHITECTURE.md and docs/RISK_MODEL.md.

use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod oracle;
pub mod state;

pub use errors::*;
pub use events::*;
pub use instructions::*;
pub use oracle::*;
pub use state::*;

declare_id!("2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg");

#[program]
pub mod ch_core {
    use super::*;

    // ---------------------------------------------------------------- admin

    pub fn initialize(ctx: Context<Initialize>, params: ConfigParams) -> Result<()> {
        ctx.accounts.process(params, &ctx.bumps)
    }

    pub fn update_params(ctx: Context<AdminOnly>, params: ConfigParams) -> Result<()> {
        ctx.accounts.update_params(params)
    }

    pub fn propose_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
        ctx.accounts.propose_admin(new_admin)
    }

    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        ctx.accounts.process()
    }

    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        ctx.accounts.set_paused(paused)
    }

    pub fn register_venue(
        ctx: Context<RegisterVenue>,
        venue_id: [u8; 32],
        venue_program_id: Pubkey,
        venue_authority: Pubkey,
        venue_index: u8,
    ) -> Result<()> {
        ctx.accounts.process(
            venue_id,
            venue_program_id,
            venue_authority,
            venue_index,
            &ctx.bumps,
        )
    }

    pub fn set_venue_active(ctx: Context<SetVenueActive>, is_active: bool) -> Result<()> {
        ctx.accounts.process(is_active)
    }

    pub fn update_correlations(
        ctx: Context<UpdateRiskModel>,
        entries: Vec<CorrEntry>,
    ) -> Result<()> {
        ctx.accounts.update_correlations(entries)
    }

    #[cfg(feature = "mock-oracle")]
    pub fn set_mock_price(
        ctx: Context<SetMockPrice>,
        asset_id: u8,
        price_micro: u64,
        conf_micro: u64,
        publish_ts: i64,
    ) -> Result<()> {
        ctx.accounts
            .process(asset_id, price_micro, conf_micro, publish_ts, &ctx.bumps)
    }

    // ----------------------------------------------------------------- user

    pub fn update_user_consent(
        ctx: Context<UpdateUserConsent>,
        is_active: bool,
        authorized_venues_bitmap: u64,
    ) -> Result<()> {
        ctx.accounts
            .process(is_active, authorized_venues_bitmap, &ctx.bumps)
    }

    // ---------------------------------------------------------------- venue

    #[allow(clippy::too_many_arguments)]
    pub fn submit_position_snapshot(
        ctx: Context<SubmitPositionSnapshot>,
        venue_id: [u8; 32],
        asset_id: u8,
        notional: u64,
        is_long: bool,
        initial_margin: u64,
        maintenance_margin: u64,
    ) -> Result<()> {
        ctx.accounts.process(
            SnapshotArgs {
                venue_id,
                asset_id,
                notional,
                is_long,
                initial_margin,
                maintenance_margin,
            },
            &ctx.bumps,
        )
    }

    pub fn close_position_snapshot(ctx: Context<ClosePositionSnapshot>) -> Result<()> {
        ctx.accounts.process()
    }

    // ------------------------------------------------------- permissionless

    pub fn refresh_portfolio<'info>(ctx: Context<'info, RefreshPortfolio<'info>>) -> Result<()> {
        instructions::portfolio::process_refresh_portfolio(ctx.accounts, ctx.remaining_accounts)
    }

    pub fn invalidate_portfolio<'info>(
        ctx: Context<'info, InvalidatePortfolio<'info>>,
    ) -> Result<()> {
        instructions::portfolio::process_invalidate_portfolio(ctx.accounts, ctx.remaining_accounts)
    }
}
