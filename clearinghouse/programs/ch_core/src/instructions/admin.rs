use anchor_lang::prelude::*;

use crate::events::*;
use crate::governance::{check_roles, validate_config_params};
use crate::state::*;
use crate::ClearinghouseError;

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
        space = RISK_MODEL_SPACE,
        seeds = [SEED_RISK_MODEL],
        bump
    )]
    pub risk_model: AccountLoader<'info, RiskModel>,

    #[account(
        init,
        payer = admin,
        space = PENDING_CHANGE_SPACE,
        seeds = [SEED_PENDING_CHANGE],
        bump
    )]
    pub pending_change: Box<Account<'info, PendingChange>>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

impl Initialize<'_> {
    pub fn process(
        &mut self,
        params: ConfigParams,
        roles: [Pubkey; 3],
        bumps: &InitializeBumps,
    ) -> Result<()> {
        validate_config_params(&params)?;
        let slot = Clock::get()?.slot;
        let admin = self.admin.key();
        let [risk_authority, venue_manager, guardian] = roles;
        // A7: production builds require four distinct role keys.
        check_roles(admin, risk_authority, venue_manager, guardian)?;

        let config = &mut self.config;
        config.admin = admin;
        config.proposed_admin = Pubkey::default();
        config.risk_authority = risk_authority;
        config.venue_manager = venue_manager;
        config.guardian = guardian;
        config.risk_epoch = 1;
        config.params = params;
        config.used_venues_bitmap = 0;
        config.active_venues_bitmap = 0;
        config.global_outstanding_credit = 0;
        config.venue_outstanding = [0; MAX_VENUES];
        config.asset_outstanding = [0; MAX_ASSETS];
        config.paused = false;
        config.bump = bumps.config;
        // A1 safe default: every venue accepts only its own reports.
        config.venue_accepts = [0; MAX_VENUES];

        let mut model = self.risk_model.load_init()?;
        model.version = 1;
        model.updated_slot = slot;
        model.corr = [0; CORR_LEN];
        // Every asset starts unsupported: refresh fails closed until the
        // risk authority sets its parameters.
        model.assets = [AssetRiskParams::default(); MAX_ASSETS];
        model.corr_stress_ppm = DEFAULT_CORR_STRESS_PPM;
        model.basis_decorr_ppm = DEFAULT_BASIS_DECORR_PPM;
        model.scenario_kind = [0; MAX_SCENARIOS];
        model.scenarios = [[0; MAX_ASSETS]; MAX_SCENARIOS];
        model.bump = bumps.risk_model;
        model.padding = [0; 7];

        let pending = &mut self.pending_change;
        pending.change = RiskChange::None;
        pending.proposer = Pubkey::default();
        pending.proposed_slot = 0;
        pending.eta_slot = 0;
        pending.bump = bumps.pending_change;
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

    /// Rotate the operational roles. Every role is independently rotatable.
    pub fn set_roles(
        &mut self,
        risk_authority: Pubkey,
        venue_manager: Pubkey,
        guardian: Pubkey,
    ) -> Result<()> {
        check_roles(self.config.admin, risk_authority, venue_manager, guardian)?;
        let c = &mut self.config;
        c.risk_authority = risk_authority;
        c.venue_manager = venue_manager;
        c.guardian = guardian;
        emit!(RolesUpdated {
            admin: self.admin.key(),
            risk_authority,
            venue_manager,
            guardian,
        });
        Ok(())
    }

    /// Only the admin can unpause: resuming credit creation increases risk.
    pub fn unpause(&mut self) -> Result<()> {
        self.config.paused = false;
        emit!(PausedUpdated {
            authority: self.admin.key(),
            paused: false,
        });
        Ok(())
    }
}

/// Guardian (or admin) actions. Each one can only reduce usable credit.
#[derive(Accounts)]
pub struct GuardianAction<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = authority.key() == config.guardian
            || authority.key() == config.admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,
    pub authority: Signer<'info>,
}

impl GuardianAction<'_> {
    pub fn pause(&mut self) -> Result<()> {
        self.config.paused = true;
        emit!(PausedUpdated {
            authority: self.authority.key(),
            paused: true,
        });
        Ok(())
    }

    /// Emergency: make every outstanding credit unusable at once.
    pub fn bump_risk_epoch(&mut self) -> Result<()> {
        let risk_epoch = self.config.bump_epoch()?;
        emit!(EmergencyEpochBump {
            authority: self.authority.key(),
            risk_epoch,
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
        let c = &self.config;
        check_roles(
            self.proposed_admin.key(),
            c.risk_authority,
            c.venue_manager,
            c.guardian,
        )?;
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
