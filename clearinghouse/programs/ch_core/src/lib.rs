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
pub mod governance;
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

    // ------------------------------------------------------- admin / roles

    /// A7: the three operational roles are set here; production builds
    /// require all four keys (with the admin) to be distinct.
    pub fn initialize(
        ctx: Context<Initialize>,
        params: ConfigParams,
        risk_authority: Pubkey,
        venue_manager: Pubkey,
        guardian: Pubkey,
    ) -> Result<()> {
        ctx.accounts.process(
            params,
            [risk_authority, venue_manager, guardian],
            &ctx.bumps,
        )
    }

    pub fn propose_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
        ctx.accounts.propose_admin(new_admin)
    }

    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        ctx.accounts.process()
    }

    pub fn set_roles(
        ctx: Context<AdminOnly>,
        risk_authority: Pubkey,
        venue_manager: Pubkey,
        guardian: Pubkey,
    ) -> Result<()> {
        ctx.accounts
            .set_roles(risk_authority, venue_manager, guardian)
    }

    pub fn unpause(ctx: Context<AdminOnly>) -> Result<()> {
        ctx.accounts.unpause()
    }

    // ------------------------------------------------------------- guardian

    pub fn pause(ctx: Context<GuardianAction>) -> Result<()> {
        ctx.accounts.pause()
    }

    pub fn bump_risk_epoch(ctx: Context<GuardianAction>) -> Result<()> {
        ctx.accounts.bump_risk_epoch()
    }

    // -------------------------------------------------------- venue manager

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

    pub fn deactivate_venue(ctx: Context<VenueLifecycle>) -> Result<()> {
        ctx.accounts.deactivate()
    }

    pub fn request_venue_activation(ctx: Context<VenueLifecycle>) -> Result<()> {
        ctx.accounts.request_activation()
    }

    pub fn activate_venue(ctx: Context<VenueLifecycle>) -> Result<()> {
        ctx.accounts.activate()
    }

    // ---------------------------------------------------- venue (allowlist)

    /// A1: venues whose reports this venue accepts credit from (own bit
    /// implied). Signer: the venue's registered authority.
    pub fn set_venue_acceptance(ctx: Context<SetVenueAcceptance>, accepts_mask: u64) -> Result<()> {
        ctx.accounts.process(accepts_mask)
    }

    // ------------------------------------------------------- risk authority
    // Credit-reducing changes apply at once; others wait `timelock_slots`.

    pub fn update_params(ctx: Context<RiskGovernance>, params: ConfigParams) -> Result<()> {
        ctx.accounts.propose(RiskChange::Params(params))
    }

    pub fn update_correlations(
        ctx: Context<RiskGovernance>,
        entries: Vec<CorrEntry>,
    ) -> Result<()> {
        ctx.accounts.propose(RiskChange::Correlations(entries))
    }

    pub fn set_asset_risk(
        ctx: Context<RiskGovernance>,
        asset_id: u8,
        params: AssetRiskArgs,
    ) -> Result<()> {
        ctx.accounts
            .propose(RiskChange::AssetRisk { asset_id, params })
    }

    pub fn set_stress_params(
        ctx: Context<RiskGovernance>,
        corr_stress_ppm: u32,
        basis_decorr_ppm: u32,
    ) -> Result<()> {
        ctx.accounts.propose(RiskChange::Stress {
            corr_stress_ppm,
            basis_decorr_ppm,
        })
    }

    pub fn set_scenario(
        ctx: Context<RiskGovernance>,
        index: u8,
        kind: u8,
        moves: Vec<AssetMove>,
    ) -> Result<()> {
        ctx.accounts
            .propose(RiskChange::Scenario { index, kind, moves })
    }

    /// Pyth feed id for an asset. Setting or switching a feed waits for the
    /// timelock; clearing it (all zeros) applies at once.
    pub fn set_oracle_feed(
        ctx: Context<RiskGovernance>,
        asset_id: u8,
        feed_id: [u8; 32],
    ) -> Result<()> {
        ctx.accounts
            .propose(RiskChange::OracleFeed { asset_id, feed_id })
    }

    pub fn apply_pending_change(ctx: Context<RiskGovernance>) -> Result<()> {
        ctx.accounts.apply_pending()
    }

    pub fn cancel_pending_change(ctx: Context<RiskGovernance>) -> Result<()> {
        ctx.accounts.cancel_pending()
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

    /// Close consent + portfolio (all snapshots must be closed first).
    pub fn close_user_accounts(ctx: Context<CloseUserAccounts>) -> Result<()> {
        ctx.accounts.process()
    }

    // ---------------------------------------------------------------- venue

    /// A5: `mark_price_micro` (0 = not reported) is checked against the
    /// asset's oracle, passed as the first remaining account.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_position_snapshot<'info>(
        ctx: Context<'info, SubmitPositionSnapshot<'info>>,
        venue_id: [u8; 32],
        asset_id: u8,
        notional: u64,
        is_long: bool,
        initial_margin: u64,
        maintenance_margin: u64,
        mark_price_micro: u64,
    ) -> Result<()> {
        ctx.accounts.process(
            SnapshotArgs {
                venue_id,
                asset_id,
                notional,
                is_long,
                initial_margin,
                maintenance_margin,
                mark_price_micro,
            },
            &ctx.bumps,
            ctx.remaining_accounts,
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
