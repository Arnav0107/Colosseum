use anchor_lang::prelude::*;

use crate::events::*;
use crate::state::*;
use crate::ClearinghouseError;

pub fn validate_config_params(p: &ConfigParams) -> Result<()> {
    let ok = (1..=MAX_VENUES as u8).contains(&p.max_venues)
        && p.haircut_bps <= 10_000
        && (1..=MAX_CREDIT_TTL_SLOTS).contains(&p.credit_ttl_slots)
        && p.snapshot_max_age_slots > 0
        && p.max_credit_bps_of_im <= 10_000
        && p.risk_model_max_age_slots > 0
        && p.max_price_age_secs > 0
        && p.max_conf_bps <= 10_000
        && p.max_move_bps <= 10_000;
    require!(ok, ClearinghouseError::InvalidConfigParams);
    Ok(())
}

pub fn venue_bit(venue_index: u8) -> Result<u64> {
    1u64.checked_shl(venue_index as u32)
        .ok_or_else(|| ClearinghouseError::InvalidVenueIndex.into())
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + GlobalConfig::INIT_SPACE,
        seeds = [SEED_CONFIG],
        bump
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        init,
        payer = admin,
        space = 8 + RiskModel::INIT_SPACE,
        seeds = [SEED_RISK_MODEL],
        bump
    )]
    pub risk_model: Box<Account<'info, RiskModel>>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

impl Initialize<'_> {
    pub fn process(&mut self, params: ConfigParams, bumps: &InitializeBumps) -> Result<()> {
        validate_config_params(&params)?;
        let slot = Clock::get()?.slot;

        let config = &mut self.config;
        config.admin = self.admin.key();
        config.proposed_admin = Pubkey::default();
        config.risk_epoch = 1;
        config.params = params;
        config.used_venues_bitmap = 0;
        config.active_venues_bitmap = 0;
        config.global_outstanding_credit = 0;
        config.venue_outstanding = [0; MAX_VENUES];
        config.asset_outstanding = [0; MAX_ASSETS];
        config.paused = false;
        config.bump = bumps.config;

        let model = &mut self.risk_model;
        model.version = 1;
        model.updated_slot = slot;
        model.corr = [0; CORR_LEN];
        model.bump = bumps.risk_model;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        has_one = admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,
    pub admin: Signer<'info>,
}

impl AdminOnly<'_> {
    pub fn update_params(&mut self, params: ConfigParams) -> Result<()> {
        validate_config_params(&params)?;
        self.config.params = params;
        let risk_epoch = self.config.bump_epoch()?;
        emit!(ParamsUpdated {
            admin: self.admin.key(),
            params,
            risk_epoch,
        });
        Ok(())
    }

    pub fn propose_admin(&mut self, new_admin: Pubkey) -> Result<()> {
        require!(
            new_admin != Pubkey::default(),
            ClearinghouseError::InvalidAdmin
        );
        self.config.proposed_admin = new_admin;
        emit!(AdminProposed {
            admin: self.admin.key(),
            proposed_admin: new_admin,
        });
        Ok(())
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<()> {
        self.config.paused = paused;
        emit!(PausedUpdated {
            admin: self.admin.key(),
            paused,
        });
        Ok(())
    }
}

#[derive(Accounts)]
pub struct AcceptAdmin<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = config.proposed_admin == proposed_admin.key() @ ClearinghouseError::Unauthorized,
        constraint = config.proposed_admin != Pubkey::default() @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,
    pub proposed_admin: Signer<'info>,
}

impl AcceptAdmin<'_> {
    pub fn process(&mut self) -> Result<()> {
        let old_admin = self.config.admin;
        self.config.admin = self.proposed_admin.key();
        self.config.proposed_admin = Pubkey::default();
        emit!(AdminUpdated {
            old_admin,
            new_admin: self.config.admin,
        });
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(venue_id: [u8; 32])]
pub struct RegisterVenue<'info> {
    #[account(
        mut,
        has_one = admin @ ClearinghouseError::Unauthorized,
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    // `init` (not `init_if_needed`): a venue id can be registered exactly once.
    #[account(
        init,
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

impl RegisterVenue<'_> {
    pub fn process(
        &mut self,
        venue_id: [u8; 32],
        venue_program_id: Pubkey,
        venue_authority: Pubkey,
        venue_index: u8,
        bumps: &RegisterVenueBumps,
    ) -> Result<()> {
        let config = &mut self.config;
        require!(
            venue_index < config.params.max_venues,
            ClearinghouseError::InvalidVenueIndex
        );
        let bit = venue_bit(venue_index)?;
        // The venue PDA is new, so a set bit belongs to a *different* venue.
        // Indices are never reused, even after deactivation (F8).
        require!(
            config.used_venues_bitmap & bit == 0,
            ClearinghouseError::VenueIndexAlreadyUsed
        );
        config.used_venues_bitmap |= bit;
        config.active_venues_bitmap |= bit;

        let venue = &mut self.venue_registration;
        venue.venue_id = venue_id;
        venue.venue_program_id = venue_program_id;
        venue.venue_authority = venue_authority;
        venue.venue_index = venue_index;
        venue.is_active = true;
        venue.bump = bumps.venue_registration;

        emit!(VenueRegistered {
            venue_id,
            venue_program_id,
            venue_authority,
            venue_index,
        });
        Ok(())
    }
}

#[derive(Accounts)]
pub struct SetVenueActive<'info> {
    #[account(
        mut,
        has_one = admin @ ClearinghouseError::Unauthorized,
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [SEED_VENUE, venue_registration.venue_id.as_ref()],
        bump = venue_registration.bump
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    pub admin: Signer<'info>,
}

impl SetVenueActive<'_> {
    pub fn process(&mut self, is_active: bool) -> Result<()> {
        let venue = &mut self.venue_registration;
        let bit = venue_bit(venue.venue_index)?;
        let config = &mut self.config;
        require!(
            config.used_venues_bitmap & bit != 0,
            ClearinghouseError::InvalidVenueIndex
        );
        venue.is_active = is_active;
        if is_active {
            config.active_venues_bitmap |= bit;
        } else {
            config.active_venues_bitmap &= !bit;
        }
        // Venue lifecycle changes invalidate every outstanding credit at once.
        let risk_epoch = config.bump_epoch()?;
        emit!(VenueActiveUpdated {
            venue_id: venue.venue_id,
            venue_index: venue.venue_index,
            is_active,
            risk_epoch,
        });
        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorrEntry {
    pub a: u8,
    pub b: u8,
    /// Scaled 1e6, in [-1e6, 1e6].
    pub rho: i32,
}

/// Upper bound on entries per update (keeps the transaction small).
pub const MAX_CORR_ENTRIES_PER_TX: usize = 128;

#[derive(Accounts)]
pub struct UpdateRiskModel<'info> {
    #[account(
        mut,
        has_one = admin @ ClearinghouseError::Unauthorized,
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(mut, seeds = [SEED_RISK_MODEL], bump = risk_model.bump)]
    pub risk_model: Box<Account<'info, RiskModel>>,

    pub admin: Signer<'info>,
}

impl UpdateRiskModel<'_> {
    /// Set some correlation pairs. An empty list re-attests the current model
    /// (refreshes `updated_slot`). Either way the risk epoch is bumped.
    pub fn update_correlations(&mut self, entries: Vec<CorrEntry>) -> Result<()> {
        require!(
            entries.len() <= MAX_CORR_ENTRIES_PER_TX,
            ClearinghouseError::InvalidCorrelationValue
        );
        let model = &mut self.risk_model;
        for e in &entries {
            require!(
                (-RHO_ONE..=RHO_ONE).contains(&e.rho),
                ClearinghouseError::InvalidCorrelationValue
            );
            let k = corr_index(e.a, e.b).ok_or(ClearinghouseError::InvalidAssetId)?;
            model.corr[k] = e.rho;
        }
        model.version = model
            .version
            .checked_add(1)
            .ok_or(ClearinghouseError::MathOverflow)?;
        model.updated_slot = Clock::get()?.slot;
        let risk_epoch = self.config.bump_epoch()?;
        emit!(RiskModelUpdated {
            version: model.version,
            entries: entries.len() as u16,
            risk_epoch,
        });
        Ok(())
    }
}

#[cfg(feature = "mock-oracle")]
#[derive(Accounts)]
#[instruction(asset_id: u8)]
pub struct SetMockPrice<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump,
        has_one = admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + crate::oracle::MockPrice::INIT_SPACE,
        seeds = [SEED_MOCK_PRICE, &[asset_id]],
        bump
    )]
    pub mock_price: Account<'info, crate::oracle::MockPrice>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[cfg(feature = "mock-oracle")]
impl SetMockPrice<'_> {
    pub fn process(
        &mut self,
        asset_id: u8,
        price_micro: u64,
        conf_micro: u64,
        publish_ts: i64,
        bumps: &SetMockPriceBumps,
    ) -> Result<()> {
        require!(
            (asset_id as usize) < MAX_ASSETS,
            ClearinghouseError::InvalidAssetId
        );
        let mock = &mut self.mock_price;
        mock.asset_id = asset_id;
        mock.price_micro = price_micro;
        mock.conf_micro = conf_micro;
        mock.publish_ts = publish_ts;
        mock.bump = bumps.mock_price;
        Ok(())
    }
}
