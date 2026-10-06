// TODO(owner): Dev A
use anchor_lang::prelude::*;

declare_id!("2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg");

pub const SEED_CONFIG: &[u8] = b"config";
pub const SEED_VENUE: &[u8] = b"venue";
pub const SEED_CONSENT: &[u8] = b"consent";
pub const SEED_SNAPSHOT: &[u8] = b"snapshot";
pub const SEED_CREDIT: &[u8] = b"credit";
pub const SEED_CORRELATIONS: &[u8] = b"correlations";

#[program]
pub mod ch_core {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        keeper_authority: Pubkey,
        default_fund_program: Pubkey,
        max_venues: u8,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.keeper_authority = keeper_authority;
        config.default_fund_program = default_fund_program;
        config.max_venues = max_venues;
        config.bump = ctx.bumps.config;
        config.reserved = [0u8; 64];
        Ok(())
    }

    pub fn register_venue(
        ctx: Context<RegisterVenue>,
        venue_id: [u8; 32],
        venue_program_id: Pubkey,
        weight_bps: u16,
    ) -> Result<()> {
        let venue = &mut ctx.accounts.venue_registration;
        venue.venue_id = venue_id;
        venue.venue_program_id = venue_program_id;
        venue.is_active = true;
        venue.weight_bps = weight_bps;
        venue.bump = ctx.bumps.venue_registration;
        Ok(())
    }

    pub fn update_user_consent(
        ctx: Context<UpdateUserConsent>,
        is_active: bool,
        authorized_venues_bitmap: u64,
    ) -> Result<()> {
        let consent = &mut ctx.accounts.user_consent;
        consent.user = ctx.accounts.user.key();
        consent.is_active = is_active;
        consent.authorized_venues_bitmap = authorized_venues_bitmap;
        consent.bump = ctx.bumps.user_consent;
        Ok(())
    }

    pub fn submit_position_snapshot(
        ctx: Context<SubmitPositionSnapshot>,
        venue_id: [u8; 32],
        notional_value: u64,
        is_long: bool,
        maintenance_margin: u64,
    ) -> Result<()> {
        require!(ctx.accounts.venue_registration.is_active, ClearinghouseError::VenueInactive);
        require!(ctx.accounts.user_consent.is_active, ClearinghouseError::UserConsentMissing);

        let clock = Clock::get()?;
        let snapshot = &mut ctx.accounts.pos_snapshot;
        snapshot.user = ctx.accounts.user.key();
        snapshot.venue_id = venue_id;
        snapshot.notional_value = notional_value;
        snapshot.is_long = is_long;
        snapshot.maintenance_margin = maintenance_margin;
        snapshot.slot = clock.slot;
        snapshot.timestamp = clock.unix_timestamp;
        snapshot.bump = ctx.bumps.pos_snapshot;
        Ok(())
    }

    pub fn publish_margin_credit(
        ctx: Context<PublishMarginCredit>,
        credit_amount: u64,
        valid_until_slot: u64,
    ) -> Result<()> {
        let clock = Clock::get()?;
        let credit = &mut ctx.accounts.margin_credit;
        credit.user = ctx.accounts.user.key();
        credit.venue_id = ctx.accounts.venue_registration.venue_id;
        credit.credit_amount = credit_amount;
        credit.valid_until_slot = valid_until_slot;
        credit.update_epoch = clock.epoch;
        credit.bump = ctx.bumps.margin_credit;
        Ok(())
    }

    pub fn update_correlations(
        ctx: Context<UpdateCorrelations>,
        correlations: [[i64; 8]; 8],
    ) -> Result<()> {
        let clock = Clock::get()?;
        let matrix = &mut ctx.accounts.correlation_matrix;
        matrix.oracle_authority = ctx.accounts.oracle_authority.key();
        matrix.updated_at = clock.unix_timestamp;
        matrix.correlations = correlations;
        matrix.bump = ctx.bumps.correlation_matrix;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Account Contexts
// -----------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + GlobalConfig::INIT_SPACE,
        seeds = [SEED_CONFIG],
        bump
    )]
    pub config: Account<'info, GlobalConfig>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(venue_id: [u8; 32])]
pub struct RegisterVenue<'info> {
    #[account(
        has_one = admin @ ClearinghouseError::Unauthorized,
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Account<'info, GlobalConfig>,

    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + VenueRegistration::INIT_SPACE,
        seeds = [SEED_VENUE, venue_id.as_ref()],
        bump
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateUserConsent<'info> {
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + UserConsent::INIT_SPACE,
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump
    )]
    pub user_consent: Account<'info, UserConsent>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(venue_id: [u8; 32])]
pub struct SubmitPositionSnapshot<'info> {
    #[account(
        seeds = [SEED_VENUE, venue_id.as_ref()],
        bump = venue_registration.bump
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    #[account(
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Account<'info, UserConsent>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + PosSnapshot::INIT_SPACE,
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_id.as_ref()],
        bump
    )]
    pub pos_snapshot: Account<'info, PosSnapshot>,

    /// CHECK: Target user address for the position snapshot
    pub user: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PublishMarginCredit<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = config.keeper_authority == keeper.key() @ ClearinghouseError::Unauthorized
    )]
    pub config: Account<'info, GlobalConfig>,

    #[account(
        seeds = [SEED_VENUE, venue_registration.venue_id.as_ref()],
        bump = venue_registration.bump
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    #[account(
        init_if_needed,
        payer = keeper,
        space = 8 + MarginCredit::INIT_SPACE,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_registration.venue_id.as_ref()],
        bump
    )]
    pub margin_credit: Account<'info, MarginCredit>,

    /// CHECK: Target user
    pub user: UncheckedAccount<'info>,

    #[account(mut)]
    pub keeper: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateCorrelations<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = config.keeper_authority == oracle_authority.key() || config.admin == oracle_authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub config: Account<'info, GlobalConfig>,

    #[account(
        init_if_needed,
        payer = oracle_authority,
        space = 8 + CorrelationMatrix::INIT_SPACE,
        seeds = [SEED_CORRELATIONS],
        bump
    )]
    pub correlation_matrix: Account<'info, CorrelationMatrix>,

    #[account(mut)]
    pub oracle_authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// State Accounts
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct GlobalConfig {
    pub admin: Pubkey,
    pub keeper_authority: Pubkey,
    pub default_fund_program: Pubkey,
    pub max_venues: u8,
    pub bump: u8,
    pub reserved: [u8; 64],
}

#[account]
#[derive(InitSpace)]
pub struct VenueRegistration {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub is_active: bool,
    pub weight_bps: u16,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct UserConsent {
    pub user: Pubkey,
    pub is_active: bool,
    pub authorized_venues_bitmap: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct PosSnapshot {
    pub user: Pubkey,
    pub venue_id: [u8; 32],
    pub notional_value: u64,
    pub is_long: bool,
    pub maintenance_margin: u64,
    pub slot: u64,
    pub timestamp: i64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct MarginCredit {
    pub user: Pubkey,
    pub venue_id: [u8; 32],
    pub credit_amount: u64,
    pub valid_until_slot: u64,
    pub update_epoch: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct CorrelationMatrix {
    pub oracle_authority: Pubkey,
    pub updated_at: i64,
    pub correlations: [[i64; 8]; 8],
    pub bump: u8,
}

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

#[error_code]
pub enum ClearinghouseError {
    #[msg("Unauthorized access")]
    Unauthorized,
    #[msg("Venue is not active")]
    VenueInactive,
    #[msg("User consent missing or inactive")]
    UserConsentMissing,
    #[msg("Invalid credit calculation")]
    InvalidCreditCalculation,
}
