use anchor_lang::prelude::*;

pub mod oracle;
pub use oracle::*;

declare_id!("2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg");

pub const SEED_CONFIG: &[u8] = b"config";
pub const SEED_VENUE: &[u8] = b"venue";
pub const SEED_CONSENT: &[u8] = b"consent";
pub const SEED_SNAPSHOT: &[u8] = b"snapshot";
pub const SEED_CREDIT: &[u8] = b"credit";
pub const SEED_CORRELATIONS: &[u8] = b"correlations";
pub const SEED_MOCK_PRICE: &[u8] = b"mock_price";

pub const MAX_CREDIT_TTL_SLOTS: u64 = 3_000;
pub const DEFAULT_HAIRCUT_BPS: u16 = 2_000;

#[allow(clippy::too_many_arguments)]
pub fn validate_config_params(
    haircut_bps: u16,
    max_credit_bps_of_required: u16,
    credit_ttl_slots: u64,
    snapshot_max_age_slots: u64,
    max_venues: u8,
    max_price_age_secs: i64,
    max_conf_bps: u16,
    max_move_bps: u16,
) -> Result<()> {
    require!(
        haircut_bps <= 10_000,
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        max_credit_bps_of_required <= 10_000,
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        (1..=MAX_CREDIT_TTL_SLOTS).contains(&credit_ttl_slots),
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        snapshot_max_age_slots > 0,
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        (1..=64).contains(&max_venues),
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        max_price_age_secs > 0,
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        max_conf_bps <= 10_000,
        ClearinghouseError::InvalidConfigParams
    );
    require!(
        max_move_bps <= 10_000,
        ClearinghouseError::InvalidConfigParams
    );
    Ok(())
}

#[program]
pub mod ch_core {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, params: InitConfigParams) -> Result<()> {
        validate_config_params(
            params.haircut_bps,
            params.max_credit_bps_of_required,
            params.credit_ttl_slots,
            params.snapshot_max_age_slots,
            params.max_venues,
            params.max_price_age_secs,
            params.max_conf_bps,
            params.max_move_bps,
        )?;

        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.keeper_authority = params.keeper_authority;
        config.default_fund_program = params.default_fund_program;
        config.proposed_admin = Pubkey::default();
        config.max_venues = params.max_venues;
        config.haircut_bps = params.haircut_bps;
        config.credit_ttl_slots = params.credit_ttl_slots;
        config.snapshot_max_age_slots = params.snapshot_max_age_slots;
        config.max_credit_per_user = params.max_credit_per_user;
        config.max_credit_bps_of_required = params.max_credit_bps_of_required;
        config.corr_min_interval_slots = params.corr_min_interval_slots;
        config.corr_max_age_slots = params.corr_max_age_slots;
        config.max_price_age_secs = params.max_price_age_secs;
        config.max_conf_bps = params.max_conf_bps;
        config.max_move_bps = params.max_move_bps;
        config.used_venues_bitmap = 0;
        config.paused = false;
        config.bump = ctx.bumps.config;
        config.reserved = [0u8; 31];
        Ok(())
    }

    pub fn register_venue(
        ctx: Context<RegisterVenue>,
        venue_id: [u8; 32],
        venue_program_id: Pubkey,
        venue_authority: Pubkey,
        venue_index: u8,
        weight_bps: u16,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(
            venue_index < config.max_venues,
            ClearinghouseError::InvalidVenueIndex
        );

        let venue_bit = 1u64
            .checked_shl(venue_index as u32)
            .ok_or(ClearinghouseError::InvalidVenueIndex)?;

        let is_used = (config.used_venues_bitmap & venue_bit) != 0;
        let venue = &mut ctx.accounts.venue_registration;
        if is_used {
            require!(
                venue.venue_id == venue_id,
                ClearinghouseError::VenueIndexAlreadyUsed
            );
        } else {
            config.used_venues_bitmap |= venue_bit;
        }

        venue.venue_id = venue_id;
        venue.venue_program_id = venue_program_id;
        venue.venue_authority = venue_authority;
        venue.venue_index = venue_index;
        venue.is_active = true;
        venue.weight_bps = weight_bps;
        venue.bump = ctx.bumps.venue_registration;

        emit!(VenueRegistered {
            venue_id,
            venue_program_id,
            venue_authority,
            venue_index,
            weight_bps,
        });

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

        emit!(ConsentUpdated {
            user: ctx.accounts.user.key(),
            is_active,
            authorized_venues_bitmap,
        });

        Ok(())
    }

    pub fn submit_position_snapshot(
        ctx: Context<SubmitPositionSnapshot>,
        venue_id: [u8; 32],
        asset_id: u8,
        notional_value: u64,
        is_long: bool,
        required_margin: u64,
    ) -> Result<()> {
        require!(
            !ctx.accounts.config.paused,
            ClearinghouseError::ProgramPaused
        );
        require!(
            ctx.accounts.venue_registration.is_active,
            ClearinghouseError::VenueInactive
        );
        require!(
            ctx.accounts.user_consent.is_active,
            ClearinghouseError::UserConsentMissing
        );
        require!(asset_id < 8, ClearinghouseError::InvalidAssetId);

        let venue_bit = 1u64
            .checked_shl(ctx.accounts.venue_registration.venue_index as u32)
            .ok_or(ClearinghouseError::InvalidVenueIndex)?;
        require!(
            (ctx.accounts.user_consent.authorized_venues_bitmap & venue_bit) != 0,
            ClearinghouseError::VenueNotAuthorized
        );

        let clock = Clock::get()?;

        // Read and validate price from oracle
        let reading = oracle::read_price(&ctx.accounts.price_oracle.to_account_info(), asset_id)?;
        oracle::check_price(
            &reading,
            clock.unix_timestamp,
            ctx.accounts.config.max_price_age_secs,
            ctx.accounts.config.max_conf_bps,
        )?;

        let snapshot = &mut ctx.accounts.pos_snapshot;
        snapshot.user = ctx.accounts.user.key();
        snapshot.venue_id = venue_id;
        snapshot.asset_id = asset_id;
        snapshot.notional_value = notional_value;
        snapshot.is_long = is_long;
        snapshot.required_margin = required_margin;
        snapshot.snapshot_price = reading.price_micro;
        snapshot.slot = clock.slot;
        snapshot.timestamp = clock.unix_timestamp;
        snapshot.bump = ctx.bumps.pos_snapshot;

        emit!(PositionSnapshotSubmitted {
            user: ctx.accounts.user.key(),
            venue_id,
            asset_id,
            notional_value,
            is_long,
            required_margin,
            snapshot_price: reading.price_micro,
            slot: clock.slot,
        });

        Ok(())
    }

    #[allow(clippy::needless_range_loop)]
    pub fn update_correlations(
        ctx: Context<UpdateCorrelations>,
        correlations: [[i64; 8]; 8],
    ) -> Result<()> {
        let clock = Clock::get()?;
        let config = &ctx.accounts.config;
        let matrix = &mut ctx.accounts.correlation_matrix;

        for i in 0..8 {
            for j in 0..8 {
                let val = correlations[i][j];
                require!(
                    (-1_000_000..=1_000_000).contains(&val),
                    ClearinghouseError::InvalidCorrelationValue
                );
                require!(
                    correlations[i][j] == correlations[j][i],
                    ClearinghouseError::AsymmetricCorrelationMatrix
                );
            }
            require!(
                correlations[i][i] == 1_000_000,
                ClearinghouseError::InvalidCorrelationDiagonal
            );
        }

        if matrix.updated_slot != 0 {
            require!(
                clock.slot.saturating_sub(matrix.updated_slot) >= config.corr_min_interval_slots,
                ClearinghouseError::CorrelationUpdateTooFrequent
            );
        }

        matrix.oracle_authority = ctx.accounts.oracle_authority.key();
        matrix.updated_at = clock.unix_timestamp;
        matrix.updated_slot = clock.slot;
        matrix.correlations = correlations;
        matrix.bump = ctx.bumps.correlation_matrix;

        emit!(CorrelationsUpdated {
            oracle_authority: ctx.accounts.oracle_authority.key(),
            slot: clock.slot,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    pub fn compute_credit(ctx: Context<ComputeCredit>) -> Result<()> {
        let config = &ctx.accounts.config;
        require!(!config.paused, ClearinghouseError::ProgramPaused);
        let clock = Clock::get()?;

        // 0. Require distinct venues and indices
        require!(
            ctx.accounts.venue_reg_a.key() != ctx.accounts.venue_reg_b.key(),
            ClearinghouseError::DuplicateVenue
        );
        require!(
            ctx.accounts.venue_reg_a.venue_index != ctx.accounts.venue_reg_b.venue_index,
            ClearinghouseError::DuplicateVenue
        );

        // 1. Consent bit checks
        let bit_a = 1u64
            .checked_shl(ctx.accounts.venue_reg_a.venue_index as u32)
            .ok_or(ClearinghouseError::InvalidVenueIndex)?;
        let bit_b = 1u64
            .checked_shl(ctx.accounts.venue_reg_b.venue_index as u32)
            .ok_or(ClearinghouseError::InvalidVenueIndex)?;
        require!(
            (ctx.accounts.user_consent.authorized_venues_bitmap & bit_a) != 0,
            ClearinghouseError::VenueNotAuthorized
        );
        require!(
            (ctx.accounts.user_consent.authorized_venues_bitmap & bit_b) != 0,
            ClearinghouseError::VenueNotAuthorized
        );

        // Snapshots staleness check
        require!(
            clock.slot.saturating_sub(ctx.accounts.snapshot_a.slot)
                <= config.snapshot_max_age_slots,
            ClearinghouseError::SnapshotStale
        );
        require!(
            clock.slot.saturating_sub(ctx.accounts.snapshot_b.slot)
                <= config.snapshot_max_age_slots,
            ClearinghouseError::SnapshotStale
        );

        // Snapshots notional check
        require!(
            ctx.accounts.snapshot_a.notional_value > 0
                && ctx.accounts.snapshot_b.notional_value > 0,
            ClearinghouseError::ZeroNotional
        );

        // Required margin check: must be > 0 and <= notional
        require!(
            ctx.accounts.snapshot_a.required_margin > 0
                && ctx.accounts.snapshot_a.required_margin
                    <= ctx.accounts.snapshot_a.notional_value,
            ClearinghouseError::InvalidMargin
        );
        require!(
            ctx.accounts.snapshot_b.required_margin > 0
                && ctx.accounts.snapshot_b.required_margin
                    <= ctx.accounts.snapshot_b.notional_value,
            ClearinghouseError::InvalidMargin
        );

        // Correlation matrix staleness check
        require!(
            ctx.accounts.correlation_matrix.updated_slot > 0
                && clock
                    .slot
                    .saturating_sub(ctx.accounts.correlation_matrix.updated_slot)
                    <= config.corr_max_age_slots,
            ClearinghouseError::CorrelationStale
        );

        // 2. Price guard per leg
        let snap_a = &ctx.accounts.snapshot_a;
        let snap_b = &ctx.accounts.snapshot_b;

        require!(
            snap_a.asset_id < 8 && snap_b.asset_id < 8,
            ClearinghouseError::InvalidAssetId
        );

        let reading_a =
            oracle::read_price(&ctx.accounts.price_a.to_account_info(), snap_a.asset_id)?;
        oracle::check_price(
            &reading_a,
            clock.unix_timestamp,
            config.max_price_age_secs,
            config.max_conf_bps,
        )?;
        let move_a = ch_math::price_move_bps(snap_a.snapshot_price, reading_a.price_micro);
        require!(
            move_a <= config.max_move_bps as u64,
            ClearinghouseError::PriceMoved
        );

        let reading_b =
            oracle::read_price(&ctx.accounts.price_b.to_account_info(), snap_b.asset_id)?;
        oracle::check_price(
            &reading_b,
            clock.unix_timestamp,
            config.max_price_age_secs,
            config.max_conf_bps,
        )?;
        let move_b = ch_math::price_move_bps(snap_b.snapshot_price, reading_b.price_micro);
        require!(
            move_b <= config.max_move_bps as u64,
            ClearinghouseError::PriceMoved
        );

        // 3. Legs
        let sign_a: i64 = if snap_a.is_long { 1 } else { -1 };
        let signed_margin_a = i64::try_from(snap_a.required_margin)
            .map_err(|_| ClearinghouseError::MathOverflow)?
            .checked_mul(sign_a)
            .ok_or(ClearinghouseError::MathOverflow)?;

        let sign_b: i64 = if snap_b.is_long { 1 } else { -1 };
        let signed_margin_b = i64::try_from(snap_b.required_margin)
            .map_err(|_| ClearinghouseError::MathOverflow)?
            .checked_mul(sign_b)
            .ok_or(ClearinghouseError::MathOverflow)?;

        let legs = [
            ch_math::Leg {
                asset: snap_a.asset_id,
                signed_required_margin: signed_margin_a,
            },
            ch_math::Leg {
                asset: snap_b.asset_id,
                signed_required_margin: signed_margin_b,
            },
        ];

        // 4. Combined risk & credit total
        let matrix = &ctx.accounts.correlation_matrix;
        let rho = |a: u8, b: u8| -> i64 { matrix.correlations[a as usize][b as usize] };

        let combined = ch_math::combined_risk(&legs, &rho).map_err(|e| match e {
            ch_math::MathError::NegativeVariance => ClearinghouseError::NegativeVariance,
            ch_math::MathError::Overflow => ClearinghouseError::MathOverflow,
        })?;

        let ra = snap_a.required_margin;
        let rb = snap_b.required_margin;
        let sum_required = ra.saturating_add(rb);

        let total = ch_math::credit_total(sum_required, combined, config.haircut_bps)
            .min(config.max_credit_per_user);

        // 5. Pro-rata split and individual caps
        let shares = ch_math::split_pro_rata(total, [ra, rb]);

        let max_credit_a =
            (ra as u128).saturating_mul(config.max_credit_bps_of_required as u128) / 10_000;
        let max_credit_b =
            (rb as u128).saturating_mul(config.max_credit_bps_of_required as u128) / 10_000;

        let credit_a_final = shares[0].min(max_credit_a as u64);
        let credit_b_final = shares[1].min(max_credit_b as u64);

        // 6. Write MarginCredit accounts
        let valid_until_slot = clock.slot.saturating_add(config.credit_ttl_slots);

        let credit_a = &mut ctx.accounts.credit_a;
        credit_a.user = ctx.accounts.user.key();
        credit_a.venue_id = ctx.accounts.venue_reg_a.venue_id;
        credit_a.credit_amount = credit_a_final;
        credit_a.valid_until_slot = valid_until_slot;
        credit_a.update_epoch = clock.epoch;
        credit_a.bump = ctx.bumps.credit_a;

        let credit_b = &mut ctx.accounts.credit_b;
        credit_b.user = ctx.accounts.user.key();
        credit_b.venue_id = ctx.accounts.venue_reg_b.venue_id;
        credit_b.credit_amount = credit_b_final;
        credit_b.valid_until_slot = valid_until_slot;
        credit_b.update_epoch = clock.epoch;
        credit_b.bump = ctx.bumps.credit_b;

        emit!(CreditComputed {
            user: ctx.accounts.user.key(),
            combined,
            sum_required,
            credit_a: credit_a_final,
            credit_b: credit_b_final,
            valid_until_slot,
        });

        Ok(())
    }

    pub fn revoke_credit(ctx: Context<RevokeCredit>) -> Result<()> {
        let credit_a = &mut ctx.accounts.credit_a;
        credit_a.credit_amount = 0;
        credit_a.valid_until_slot = 0;

        let credit_b = &mut ctx.accounts.credit_b;
        credit_b.credit_amount = 0;
        credit_b.valid_until_slot = 0;

        emit!(CreditRevoked {
            user: ctx.accounts.user.key(),
            venue_id_a: ctx.accounts.venue_reg_a.venue_id,
            venue_id_b: ctx.accounts.venue_reg_b.venue_id,
            reason: 0,
        });

        Ok(())
    }

    pub fn revoke_if_unsafe(ctx: Context<RevokeIfUnsafe>) -> Result<()> {
        let clock = Clock::get()?;
        let config = &ctx.accounts.config;
        let snap_a = &ctx.accounts.snapshot_a;
        let snap_b = &ctx.accounts.snapshot_b;

        let tripped_a = check_leg_guard_tripped(
            &ctx.accounts.price_oracle_a.to_account_info(),
            snap_a,
            clock.unix_timestamp,
            config.max_price_age_secs,
            config.max_conf_bps,
            config.max_move_bps,
        )?;

        let tripped_b = check_leg_guard_tripped(
            &ctx.accounts.price_oracle_b.to_account_info(),
            snap_b,
            clock.unix_timestamp,
            config.max_price_age_secs,
            config.max_conf_bps,
            config.max_move_bps,
        )?;

        require!(tripped_a || tripped_b, ClearinghouseError::GuardNotTripped);

        let credit_a = &mut ctx.accounts.credit_a;
        credit_a.credit_amount = 0;
        credit_a.valid_until_slot = 0;

        let credit_b = &mut ctx.accounts.credit_b;
        credit_b.credit_amount = 0;
        credit_b.valid_until_slot = 0;

        emit!(CreditRevoked {
            user: ctx.accounts.user.key(),
            venue_id_a: ctx.accounts.venue_reg_a.venue_id,
            venue_id_b: ctx.accounts.venue_reg_b.venue_id,
            reason: 1,
        });

        Ok(())
    }

    pub fn invalidate_snapshot(ctx: Context<InvalidateSnapshot>) -> Result<()> {
        let snapshot = &mut ctx.accounts.pos_snapshot;
        snapshot.notional_value = 0;
        snapshot.required_margin = 0;
        Ok(())
    }

    pub fn revoke_if_basis_gone(ctx: Context<RevokeIfBasisGone>) -> Result<()> {
        let clock = Clock::get()?;
        let config = &ctx.accounts.config;
        let consent = &ctx.accounts.user_consent;
        let venue_a = &ctx.accounts.venue_reg_a;
        let venue_b = &ctx.accounts.venue_reg_b;
        let snap_a = &ctx.accounts.snapshot_a;
        let snap_b = &ctx.accounts.snapshot_b;

        // Branch 1: consent inactive or bit cleared
        let bit_a = 1u64
            .checked_shl(venue_a.venue_index as u32)
            .ok_or(ClearinghouseError::InvalidVenueIndex)?;
        let bit_b = 1u64
            .checked_shl(venue_b.venue_index as u32)
            .ok_or(ClearinghouseError::InvalidVenueIndex)?;
        let consent_gone = !consent.is_active
            || (consent.authorized_venues_bitmap & bit_a) == 0
            || (consent.authorized_venues_bitmap & bit_b) == 0;

        // Branch 2: either venue inactive
        let venue_inactive = !venue_a.is_active || !venue_b.is_active;

        // Branch 3: either snapshot invalidated or older than snapshot_max_age_slots
        let snap_stale = clock.slot.saturating_sub(snap_a.slot) > config.snapshot_max_age_slots
            || clock.slot.saturating_sub(snap_b.slot) > config.snapshot_max_age_slots;

        // Branch 4: either snapshot has notional 0
        let zero_notional = snap_a.notional_value == 0 || snap_b.notional_value == 0;

        let basis_gone = consent_gone || venue_inactive || snap_stale || zero_notional;
        require!(basis_gone, ClearinghouseError::BasisNotGone);

        let credit_a = &mut ctx.accounts.credit_a;
        credit_a.credit_amount = 0;
        credit_a.valid_until_slot = 0;

        let credit_b = &mut ctx.accounts.credit_b;
        credit_b.credit_amount = 0;
        credit_b.valid_until_slot = 0;

        emit!(CreditRevoked {
            user: ctx.accounts.user.key(),
            venue_id_a: venue_a.venue_id,
            venue_id_b: venue_b.venue_id,
            reason: 2,
        });

        Ok(())
    }

    #[cfg(feature = "mock-oracle")]
    pub fn set_mock_price(
        ctx: Context<SetMockPrice>,
        asset_id: u8,
        price_micro: u64,
        conf_micro: u64,
        publish_ts: i64,
    ) -> Result<()> {
        require!(asset_id < 8, ClearinghouseError::InvalidAssetId);
        let mock = &mut ctx.accounts.mock_price;
        mock.asset_id = asset_id;
        mock.price_micro = price_micro;
        mock.conf_micro = conf_micro;
        mock.publish_ts = publish_ts;
        mock.bump = ctx.bumps.mock_price;
        Ok(())
    }

    pub fn update_params(ctx: Context<UpdateParams>, params: UpdateConfigParams) -> Result<()> {
        validate_config_params(
            params.haircut_bps,
            params.max_credit_bps_of_required,
            params.credit_ttl_slots,
            params.snapshot_max_age_slots,
            params.max_venues,
            params.max_price_age_secs,
            params.max_conf_bps,
            params.max_move_bps,
        )?;

        let config = &mut ctx.accounts.config;
        config.max_venues = params.max_venues;
        config.haircut_bps = params.haircut_bps;
        config.credit_ttl_slots = params.credit_ttl_slots;
        config.snapshot_max_age_slots = params.snapshot_max_age_slots;
        config.max_credit_per_user = params.max_credit_per_user;
        config.max_credit_bps_of_required = params.max_credit_bps_of_required;
        config.corr_min_interval_slots = params.corr_min_interval_slots;
        config.corr_max_age_slots = params.corr_max_age_slots;
        config.max_price_age_secs = params.max_price_age_secs;
        config.max_conf_bps = params.max_conf_bps;
        config.max_move_bps = params.max_move_bps;

        emit!(ParamsUpdated {
            admin: ctx.accounts.admin.key(),
            max_venues: params.max_venues,
            haircut_bps: params.haircut_bps,
            credit_ttl_slots: params.credit_ttl_slots,
            snapshot_max_age_slots: params.snapshot_max_age_slots,
            max_credit_per_user: params.max_credit_per_user,
            max_credit_bps_of_required: params.max_credit_bps_of_required,
            corr_min_interval_slots: params.corr_min_interval_slots,
            corr_max_age_slots: params.corr_max_age_slots,
            max_price_age_secs: params.max_price_age_secs,
            max_conf_bps: params.max_conf_bps,
            max_move_bps: params.max_move_bps,
        });

        Ok(())
    }

    pub fn set_keeper(ctx: Context<SetKeeper>, new_keeper: Pubkey) -> Result<()> {
        let old_keeper = ctx.accounts.config.keeper_authority;
        ctx.accounts.config.keeper_authority = new_keeper;

        emit!(KeeperUpdated {
            admin: ctx.accounts.admin.key(),
            old_keeper,
            new_keeper,
        });

        Ok(())
    }

    pub fn propose_admin(ctx: Context<ProposeAdmin>, new_admin: Pubkey) -> Result<()> {
        require!(
            new_admin != Pubkey::default(),
            ClearinghouseError::InvalidAdmin
        );
        ctx.accounts.config.proposed_admin = new_admin;

        emit!(AdminProposed {
            admin: ctx.accounts.admin.key(),
            proposed_admin: new_admin,
        });

        Ok(())
    }

    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        let old_admin = ctx.accounts.config.admin;
        let new_admin = ctx.accounts.proposed_admin.key();
        ctx.accounts.config.admin = new_admin;
        ctx.accounts.config.proposed_admin = Pubkey::default();

        emit!(AdminUpdated {
            old_admin,
            new_admin,
        });

        Ok(())
    }

    pub fn set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
        ctx.accounts.config.paused = paused;

        emit!(PausedUpdated {
            admin: ctx.accounts.admin.key(),
            paused,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Account Contexts
// -----------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct InitConfigParams {
    pub keeper_authority: Pubkey,
    pub default_fund_program: Pubkey,
    pub max_venues: u8,
    pub haircut_bps: u16,
    pub credit_ttl_slots: u64,
    pub snapshot_max_age_slots: u64,
    pub max_credit_per_user: u64,
    pub max_credit_bps_of_required: u16,
    pub corr_min_interval_slots: u64,
    pub corr_max_age_slots: u64,
    pub max_price_age_secs: i64,
    pub max_conf_bps: u16,
    pub max_move_bps: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct UpdateConfigParams {
    pub max_venues: u8,
    pub haircut_bps: u16,
    pub credit_ttl_slots: u64,
    pub snapshot_max_age_slots: u64,
    pub max_credit_per_user: u64,
    pub max_credit_bps_of_required: u16,
    pub corr_min_interval_slots: u64,
    pub corr_max_age_slots: u64,
    pub max_price_age_secs: i64,
    pub max_conf_bps: u16,
    pub max_move_bps: u16,
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
    pub config: Account<'info, GlobalConfig>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
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
pub struct UpdateParams<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        has_one = admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetKeeper<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        has_one = admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct ProposeAdmin<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        has_one = admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
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
    pub config: Account<'info, GlobalConfig>,
    pub proposed_admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetPaused<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        has_one = admin @ ClearinghouseError::Unauthorized
    )]
    pub config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
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
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Account<'info, GlobalConfig>,

    #[account(
        seeds = [SEED_VENUE, venue_id.as_ref()],
        bump = venue_registration.bump,
        constraint = venue_registration.venue_authority == venue_authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    #[account(
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Account<'info, UserConsent>,

    #[account(
        init_if_needed,
        payer = venue_authority,
        space = 8 + PosSnapshot::INIT_SPACE,
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_id.as_ref()],
        bump
    )]
    pub pos_snapshot: Account<'info, PosSnapshot>,

    /// Price oracle account corresponding to the reported asset
    /// CHECK: Checked by oracle::read_price
    pub price_oracle: UncheckedAccount<'info>,

    /// CHECK: Target user address for the position snapshot
    pub user: UncheckedAccount<'info>,

    // TODO(security): venue_authority is currently a direct Signer stand-in for a CPI-signed venue PDA
    #[account(mut)]
    pub venue_authority: Signer<'info>,

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

#[derive(Accounts)]
pub struct ComputeCredit<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = config.keeper_authority == keeper.key() @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    /// CHECK: Target user
    pub user: UncheckedAccount<'info>,

    #[account(
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump = user_consent.bump,
        constraint = user_consent.is_active @ ClearinghouseError::UserConsentMissing
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_a.venue_id.as_ref()],
        bump = venue_reg_a.bump,
        constraint = venue_reg_a.is_active @ ClearinghouseError::VenueInactive
    )]
    pub venue_reg_a: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_b.venue_id.as_ref()],
        bump = venue_reg_b.bump,
        constraint = venue_reg_b.is_active @ ClearinghouseError::VenueInactive,
        constraint = venue_reg_a.key() != venue_reg_b.key() && venue_reg_a.venue_index != venue_reg_b.venue_index @ ClearinghouseError::DuplicateVenue
    )]
    pub venue_reg_b: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump = snapshot_a.bump,
        constraint = snapshot_a.user == user.key() && snapshot_a.venue_id == venue_reg_a.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub snapshot_a: Box<Account<'info, PosSnapshot>>,

    #[account(
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump = snapshot_b.bump,
        constraint = snapshot_b.user == user.key() && snapshot_b.venue_id == venue_reg_b.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub snapshot_b: Box<Account<'info, PosSnapshot>>,

    #[account(
        seeds = [SEED_CORRELATIONS],
        bump = correlation_matrix.bump
    )]
    pub correlation_matrix: Box<Account<'info, CorrelationMatrix>>,

    /// CHECK: Oracle for asset A
    pub price_a: UncheckedAccount<'info>,

    /// CHECK: Oracle for asset B
    pub price_b: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = keeper,
        space = 8 + MarginCredit::INIT_SPACE,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump
    )]
    pub credit_a: Box<Account<'info, MarginCredit>>,

    #[account(
        init_if_needed,
        payer = keeper,
        space = 8 + MarginCredit::INIT_SPACE,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump
    )]
    pub credit_b: Box<Account<'info, MarginCredit>>,

    #[account(mut)]
    pub keeper: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevokeCredit<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = config.keeper_authority == authority.key() || config.admin == authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    /// CHECK: Target user
    pub user: UncheckedAccount<'info>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_a.venue_id.as_ref()],
        bump = venue_reg_a.bump
    )]
    pub venue_reg_a: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_b.venue_id.as_ref()],
        bump = venue_reg_b.bump
    )]
    pub venue_reg_b: Box<Account<'info, VenueRegistration>>,

    #[account(
        mut,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump = credit_a.bump
    )]
    pub credit_a: Box<Account<'info, MarginCredit>>,

    #[account(
        mut,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump = credit_b.bump
    )]
    pub credit_b: Box<Account<'info, MarginCredit>>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct RevokeIfUnsafe<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    /// CHECK: Target user
    pub user: UncheckedAccount<'info>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_a.venue_id.as_ref()],
        bump = venue_reg_a.bump
    )]
    pub venue_reg_a: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_b.venue_id.as_ref()],
        bump = venue_reg_b.bump
    )]
    pub venue_reg_b: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump = snapshot_a.bump,
        constraint = snapshot_a.user == user.key() && snapshot_a.venue_id == venue_reg_a.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub snapshot_a: Box<Account<'info, PosSnapshot>>,

    #[account(
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump = snapshot_b.bump,
        constraint = snapshot_b.user == user.key() && snapshot_b.venue_id == venue_reg_b.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub snapshot_b: Box<Account<'info, PosSnapshot>>,

    /// CHECK: Oracle for snapshot A asset
    pub price_oracle_a: UncheckedAccount<'info>,

    /// CHECK: Oracle for snapshot B asset
    pub price_oracle_b: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump = credit_a.bump
    )]
    pub credit_a: Box<Account<'info, MarginCredit>>,

    #[account(
        mut,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump = credit_b.bump
    )]
    pub credit_b: Box<Account<'info, MarginCredit>>,
}

#[derive(Accounts)]
pub struct InvalidateSnapshot<'info> {
    #[account(
        seeds = [SEED_VENUE, venue_registration.venue_id.as_ref()],
        bump = venue_registration.bump,
        constraint = venue_registration.venue_authority == venue_authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    #[account(
        mut,
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_registration.venue_id.as_ref()],
        bump = pos_snapshot.bump,
        constraint = pos_snapshot.user == user.key() && pos_snapshot.venue_id == venue_registration.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub pos_snapshot: Account<'info, PosSnapshot>,

    /// CHECK: Target user
    pub user: UncheckedAccount<'info>,

    pub venue_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct RevokeIfBasisGone<'info> {
    #[account(
        seeds = [SEED_CONFIG],
        bump = config.bump
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    /// CHECK: Target user
    pub user: UncheckedAccount<'info>,

    #[account(
        seeds = [SEED_CONSENT, user.key().as_ref()],
        bump = user_consent.bump
    )]
    pub user_consent: Box<Account<'info, UserConsent>>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_a.venue_id.as_ref()],
        bump = venue_reg_a.bump
    )]
    pub venue_reg_a: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_VENUE, venue_reg_b.venue_id.as_ref()],
        bump = venue_reg_b.bump
    )]
    pub venue_reg_b: Box<Account<'info, VenueRegistration>>,

    #[account(
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump = snapshot_a.bump,
        constraint = snapshot_a.user == user.key() && snapshot_a.venue_id == venue_reg_a.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub snapshot_a: Box<Account<'info, PosSnapshot>>,

    #[account(
        seeds = [SEED_SNAPSHOT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump = snapshot_b.bump,
        constraint = snapshot_b.user == user.key() && snapshot_b.venue_id == venue_reg_b.venue_id @ ClearinghouseError::InvalidSnapshotOwner
    )]
    pub snapshot_b: Box<Account<'info, PosSnapshot>>,

    #[account(
        mut,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_a.venue_id.as_ref()],
        bump = credit_a.bump
    )]
    pub credit_a: Box<Account<'info, MarginCredit>>,

    #[account(
        mut,
        seeds = [SEED_CREDIT, user.key().as_ref(), venue_reg_b.venue_id.as_ref()],
        bump = credit_b.bump
    )]
    pub credit_b: Box<Account<'info, MarginCredit>>,
}

fn check_leg_guard_tripped(
    oracle_info: &AccountInfo,
    snapshot: &PosSnapshot,
    now_ts: i64,
    max_price_age_secs: i64,
    max_conf_bps: u16,
    max_move_bps: u16,
) -> Result<bool> {
    let reading = oracle::read_price(oracle_info, snapshot.asset_id)
        .map_err(|_| ClearinghouseError::InvalidOracleAccount)?;

    if reading.price_micro == 0 {
        return Ok(true);
    }

    match oracle::check_price(&reading, now_ts, max_price_age_secs, max_conf_bps) {
        Err(e) => {
            let target_stale: anchor_lang::error::Error = ClearinghouseError::PriceStale.into();
            let target_unc: anchor_lang::error::Error = ClearinghouseError::PriceUncertain.into();
            let target_invalid: anchor_lang::error::Error = ClearinghouseError::InvalidPrice.into();
            if e == target_stale || e == target_unc || e == target_invalid {
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Ok(()) => {
            let move_bps = ch_math::price_move_bps(snapshot.snapshot_price, reading.price_micro);
            Ok(move_bps > max_move_bps as u64)
        }
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
    pub config: Account<'info, GlobalConfig>,

    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + MockPrice::INIT_SPACE,
        seeds = [SEED_MOCK_PRICE, &[asset_id]],
        bump
    )]
    pub mock_price: Account<'info, MockPrice>,

    #[account(mut)]
    pub admin: Signer<'info>,

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
    pub proposed_admin: Pubkey,
    pub max_venues: u8,
    pub haircut_bps: u16,
    pub credit_ttl_slots: u64,
    pub snapshot_max_age_slots: u64,
    pub max_credit_per_user: u64,
    pub max_credit_bps_of_required: u16,
    pub corr_min_interval_slots: u64,
    pub corr_max_age_slots: u64,
    pub max_price_age_secs: i64,
    pub max_conf_bps: u16,
    pub max_move_bps: u16,
    pub used_venues_bitmap: u64,
    pub paused: bool,
    pub bump: u8,
    pub reserved: [u8; 31],
}

#[account]
#[derive(InitSpace)]
pub struct VenueRegistration {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub venue_authority: Pubkey,
    pub venue_index: u8,
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
    pub asset_id: u8,
    pub notional_value: u64,
    pub is_long: bool,
    pub required_margin: u64,
    pub snapshot_price: u64,
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
    pub updated_slot: u64,
    pub correlations: [[i64; 8]; 8],
    pub bump: u8,
}

// -----------------------------------------------------------------------------
// Events
// -----------------------------------------------------------------------------

#[event]
pub struct VenueRegistered {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub venue_authority: Pubkey,
    pub venue_index: u8,
    pub weight_bps: u16,
}

#[event]
pub struct ConsentUpdated {
    pub user: Pubkey,
    pub is_active: bool,
    pub authorized_venues_bitmap: u64,
}

#[event]
pub struct PositionSnapshotSubmitted {
    pub user: Pubkey,
    pub venue_id: [u8; 32],
    pub asset_id: u8,
    pub notional_value: u64,
    pub is_long: bool,
    pub required_margin: u64,
    pub snapshot_price: u64,
    pub slot: u64,
}

#[event]
pub struct CorrelationsUpdated {
    pub oracle_authority: Pubkey,
    pub slot: u64,
    pub timestamp: i64,
}

#[event]
pub struct CreditComputed {
    pub user: Pubkey,
    pub combined: u64,
    pub sum_required: u64,
    pub credit_a: u64,
    pub credit_b: u64,
    pub valid_until_slot: u64,
}

#[event]
pub struct CreditRevoked {
    pub user: Pubkey,
    pub venue_id_a: [u8; 32],
    pub venue_id_b: [u8; 32],
    pub reason: u8, // 0 = manual keeper/admin, 1 = unsafe price guard
}

#[event]
pub struct ParamsUpdated {
    pub admin: Pubkey,
    pub max_venues: u8,
    pub haircut_bps: u16,
    pub credit_ttl_slots: u64,
    pub snapshot_max_age_slots: u64,
    pub max_credit_per_user: u64,
    pub max_credit_bps_of_required: u16,
    pub corr_min_interval_slots: u64,
    pub corr_max_age_slots: u64,
    pub max_price_age_secs: i64,
    pub max_conf_bps: u16,
    pub max_move_bps: u16,
}

#[event]
pub struct KeeperUpdated {
    pub admin: Pubkey,
    pub old_keeper: Pubkey,
    pub new_keeper: Pubkey,
}

#[event]
pub struct AdminProposed {
    pub admin: Pubkey,
    pub proposed_admin: Pubkey,
}

#[event]
pub struct AdminUpdated {
    pub old_admin: Pubkey,
    pub new_admin: Pubkey,
}

#[event]
pub struct PausedUpdated {
    pub admin: Pubkey,
    pub paused: bool,
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
    #[msg("Venue not authorized in user consent")]
    VenueNotAuthorized,
    #[msg("Invalid venue index")]
    InvalidVenueIndex,
    #[msg("Invalid asset ID")]
    InvalidAssetId,
    #[msg("Invalid correlation value")]
    InvalidCorrelationValue,
    #[msg("Correlation matrix must be symmetric")]
    AsymmetricCorrelationMatrix,
    #[msg("Correlation diagonal must be exactly 1_000_000")]
    InvalidCorrelationDiagonal,
    #[msg("Correlation update too frequent")]
    CorrelationUpdateTooFrequent,
    #[msg("Price oracle is stale")]
    PriceStale,
    #[msg("Price oracle confidence interval too wide")]
    PriceUncertain,
    #[msg("Price moved beyond max tolerance")]
    PriceMoved,
    #[msg("Price cannot be zero or negative")]
    InvalidPrice,
    #[msg("Oracle backend not configured")]
    OracleNotConfigured,
    #[msg("Invalid oracle account")]
    InvalidOracleAccount,
    #[msg("Integer overflow in calculation")]
    MathOverflow,
    #[msg("Negative variance in portfolio netting")]
    NegativeVariance,
    #[msg("Snapshot is stale")]
    SnapshotStale,
    #[msg("Price guard was not tripped")]
    GuardNotTripped,
    #[msg("Invalid snapshot ownership")]
    InvalidSnapshotOwner,
    #[msg("Snapshot notional cannot be zero")]
    ZeroNotional,
    #[msg("Basis is not gone")]
    BasisNotGone,
    #[msg("Correlation matrix is stale")]
    CorrelationStale,
    #[msg("Invalid configuration parameters")]
    InvalidConfigParams,
    #[msg("Venue index is already used")]
    VenueIndexAlreadyUsed,
    #[msg("Venues in pair must be distinct")]
    DuplicateVenue,
    #[msg("Required margin must be non-zero and not exceed notional")]
    InvalidMargin,
    #[msg("Clearinghouse is paused")]
    ProgramPaused,
    #[msg("Invalid proposed admin address")]
    InvalidAdmin,
}
