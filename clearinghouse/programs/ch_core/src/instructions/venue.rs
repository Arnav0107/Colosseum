use anchor_lang::prelude::*;

use crate::events::*;
use crate::state::*;
use crate::ClearinghouseError;

/// Seed under the *venue's* program id for the PDA that signs snapshot
/// writes via CPI: `find_program_address(&[VENUE_SIGNER_SEED], venue_program_id)`.
pub const VENUE_SIGNER_SEED: &[u8] = b"clearinghouse";

// Venue authentication must be chosen explicitly (F10). `venue-cpi`: only the
// registered venue program can sign (through its PDA). `dev-venue-signer`:
// any keypair (localnet/tests only).
#[cfg(all(feature = "venue-cpi", feature = "dev-venue-signer"))]
compile_error!("ch_core: enable only one of `venue-cpi` / `dev-venue-signer`");
#[cfg(not(any(feature = "venue-cpi", feature = "dev-venue-signer")))]
compile_error!(
    "ch_core: no venue authentication selected. Build with `--features venue-cpi` \
     (production) or `--features dev-venue-signer` (localnet/tests only)."
);

/// The signer a venue program uses for CPI into ch_core.
pub fn venue_signer_pda(venue_program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[VENUE_SIGNER_SEED], venue_program_id).0
}

pub fn venue_bit(venue_index: u8) -> Result<u64> {
    1u64.checked_shl(venue_index as u32)
        .ok_or_else(|| ClearinghouseError::InvalidVenueIndex.into())
}

#[derive(Accounts)]
#[instruction(venue_id: [u8; 32])]
pub struct RegisterVenue<'info> {
    #[account(
        mut,
        seeds = [SEED_CONFIG],
        bump = config.bump,
        constraint = config.venue_manager == authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub config: Box<Account<'info, GlobalConfig>>,

    // `init` (not `init_if_needed`): a venue id can be registered exactly once.
    #[account(
        init,
        payer = payer,
        space = 8 + VenueRegistration::INIT_SPACE,
        seeds = [SEED_VENUE, venue_id.as_ref()],
        bump
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    pub authority: Signer<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

impl RegisterVenue<'_> {
    /// A new venue is *inactive* until its activation timelock has elapsed
    /// (immediately if `timelock_slots` is 0): a venue's reports can create
    /// credit, so adding one is a credit-increasing change.
    pub fn process(
        &mut self,
        venue_id: [u8; 32],
        venue_program_id: Pubkey,
        venue_authority: Pubkey,
        venue_index: u8,
        bumps: &RegisterVenueBumps,
    ) -> Result<()> {
        #[cfg(feature = "venue-cpi")]
        require_keys_eq!(
            venue_authority,
            venue_signer_pda(&venue_program_id),
            ClearinghouseError::VenueAuthorityNotProgramPda
        );

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

        let slot = Clock::get()?.slot;
        let venue = &mut self.venue_registration;
        venue.venue_id = venue_id;
        venue.venue_program_id = venue_program_id;
        venue.venue_authority = venue_authority;
        venue.venue_index = venue_index;
        venue.bump = bumps.venue_registration;
        venue.is_active = false;
        venue.activation_eta_slot = slot.saturating_add(config.params.timelock_slots);
        if config.params.timelock_slots == 0 {
            venue.is_active = true;
            venue.activation_eta_slot = 0;
            config.active_venues_bitmap |= bit;
        }

        emit!(VenueRegistered {
            venue_id,
            venue_program_id,
            venue_authority,
            venue_index,
            activation_eta_slot: venue.activation_eta_slot,
        });
        Ok(())
    }
}

#[derive(Accounts)]
pub struct VenueLifecycle<'info> {
    #[account(mut, seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [SEED_VENUE, venue_registration.venue_id.as_ref()],
        bump = venue_registration.bump
    )]
    pub venue_registration: Account<'info, VenueRegistration>,

    pub authority: Signer<'info>,
}

impl VenueLifecycle<'_> {
    fn emit_status(&self, risk_epoch: u64) {
        let v = &self.venue_registration;
        emit!(VenueActiveUpdated {
            venue_id: v.venue_id,
            venue_index: v.venue_index,
            is_active: v.is_active,
            activation_eta_slot: v.activation_eta_slot,
            risk_epoch,
        });
    }

    /// Immediate. Venue manager or guardian. Bumps the risk epoch, so every
    /// credit (including those touching this venue) becomes unusable.
    pub fn deactivate(&mut self) -> Result<()> {
        let who = self.authority.key();
        require!(
            who == self.config.venue_manager || who == self.config.guardian,
            ClearinghouseError::Unauthorized
        );
        let bit = venue_bit(self.venue_registration.venue_index)?;
        let venue = &mut self.venue_registration;
        venue.is_active = false;
        venue.activation_eta_slot = 0;
        self.config.active_venues_bitmap &= !bit;
        let risk_epoch = self.config.bump_epoch()?;
        self.emit_status(risk_epoch);
        Ok(())
    }

    /// Venue manager: start the activation timelock for an inactive venue
    /// (completes immediately if `timelock_slots` is 0).
    pub fn request_activation(&mut self) -> Result<()> {
        require_keys_eq!(
            self.authority.key(),
            self.config.venue_manager,
            ClearinghouseError::Unauthorized
        );
        require!(
            !self.venue_registration.is_active,
            ClearinghouseError::InvalidVenueIndex
        );
        let slot = Clock::get()?.slot;
        // max(1): 0 means "nothing requested".
        self.venue_registration.activation_eta_slot = slot
            .saturating_add(self.config.params.timelock_slots)
            .max(1);
        if self.config.params.timelock_slots == 0 {
            return self.activate();
        }
        self.emit_status(self.config.risk_epoch);
        Ok(())
    }

    /// Venue manager: complete a requested activation once its timelock elapsed.
    pub fn activate(&mut self) -> Result<()> {
        require_keys_eq!(
            self.authority.key(),
            self.config.venue_manager,
            ClearinghouseError::Unauthorized
        );
        let eta = self.venue_registration.activation_eta_slot;
        require!(eta != 0, ClearinghouseError::NoPendingChange);
        require!(
            Clock::get()?.slot >= eta,
            ClearinghouseError::TimelockNotElapsed
        );
        let bit = venue_bit(self.venue_registration.venue_index)?;
        let venue = &mut self.venue_registration;
        venue.is_active = true;
        venue.activation_eta_slot = 0;
        self.config.active_venues_bitmap |= bit;
        let risk_epoch = self.config.bump_epoch()?;
        self.emit_status(risk_epoch);
        Ok(())
    }
}

/// A1: a venue chooses which venues' reports it accepts credit from. Signed by
/// the venue's own authority (production: its program PDA, by CPI). Removing a
/// venue makes every credit that used it unusable in the same instruction
/// (`usable_credit` checks each leg's `view_mask`); adding one takes effect at
/// the next refresh. Allowed while paused: it never creates credit by itself.
#[derive(Accounts)]
pub struct SetVenueAcceptance<'info> {
    #[account(mut, seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(
        seeds = [SEED_VENUE, venue_registration.venue_id.as_ref()],
        bump = venue_registration.bump,
        constraint = venue_registration.venue_authority == venue_authority.key() @ ClearinghouseError::Unauthorized
    )]
    pub venue_registration: Box<Account<'info, VenueRegistration>>,

    pub venue_authority: Signer<'info>,
}

impl SetVenueAcceptance<'_> {
    pub fn process(&mut self, accepts_mask: u64) -> Result<()> {
        let idx = self.venue_registration.venue_index;
        require!(
            (idx as usize) < MAX_VENUES,
            ClearinghouseError::InvalidVenueIndex
        );
        // The own bit is implied; store it cleared so the mask is canonical.
        let mask = accepts_mask & !venue_bit(idx)?;
        self.config.venue_accepts[idx as usize] = mask;
        emit!(VenueAcceptanceUpdated {
            venue_index: idx,
            accepts_mask: mask,
        });
        Ok(())
    }
}
