use anchor_lang::prelude::*;

#[event]
pub struct VenueRegistered {
    pub venue_id: [u8; 32],
    pub venue_program_id: Pubkey,
    pub venue_authority: Pubkey,
    pub venue_index: u8,
    /// 0 when active immediately (timelock 0).
    pub activation_eta_slot: u64,
}

#[event]
pub struct VenueActiveUpdated {
    pub venue_id: [u8; 32],
    pub venue_index: u8,
    pub is_active: bool,
    pub activation_eta_slot: u64,
    pub risk_epoch: u64,
}

#[event]
pub struct RolesUpdated {
    pub admin: Pubkey,
    pub risk_authority: Pubkey,
    pub venue_manager: Pubkey,
    pub guardian: Pubkey,
}

#[event]
pub struct EmergencyEpochBump {
    pub authority: Pubkey,
    pub risk_epoch: u64,
}

#[event]
pub struct ChangeQueued {
    pub proposer: Pubkey,
    pub eta_slot: u64,
}

#[event]
pub struct ChangeApplied {
    pub authority: Pubkey,
}

#[event]
pub struct ChangeCancelled {
    pub authority: Pubkey,
}

#[event]
pub struct UserAccountsClosed {
    pub user: Pubkey,
    pub portfolio_version: u64,
}

#[event]
pub struct ConsentUpdated {
    pub user: Pubkey,
    pub is_active: bool,
    pub authorized_venues_bitmap: u64,
    pub portfolio_version: u64,
}

#[event]
pub struct PositionSnapshotSubmitted {
    pub user: Pubkey,
    pub venue_index: u8,
    pub asset_id: u8,
    pub version: u64,
    pub notional: u64,
    pub is_long: bool,
    pub initial_margin: u64,
    pub maintenance_margin: u64,
    pub slot: u64,
    pub portfolio_version: u64,
}

#[event]
pub struct PositionSnapshotClosed {
    pub user: Pubkey,
    pub venue_index: u8,
    pub asset_id: u8,
    pub portfolio_version: u64,
}

#[event]
pub struct RiskModelUpdated {
    pub version: u64,
    pub entries: u16,
    pub risk_epoch: u64,
}

#[event]
pub struct PortfolioRefreshed {
    pub user: Pubkey,
    pub version: u64,
    pub risk_epoch: u64,
    pub num_legs: u8,
    pub sum_standalone_risk: u64,
    pub portfolio_risk: u64,
    pub hedge_benefit_ppm: u32,
    pub total_credit: u64,
    pub expires_slot: u64,
    /// `state::portfolio_flags` (A4: PSD fallback).
    pub flags: u8,
}

/// A1: a venue changed which venues' reports it accepts credit from.
#[event]
pub struct VenueAcceptanceUpdated {
    pub venue_index: u8,
    pub accepts_mask: u64,
}

/// Bit flags explaining an invalidation.
pub mod invalidation {
    pub const RELEASE_ONLY: u16 = 1 << 0;
    pub const EPOCH_MISMATCH: u16 = 1 << 1;
    pub const EXPIRED: u16 = 1 << 2;
    pub const VENUE_INACTIVE: u16 = 1 << 3;
    pub const CONSENT_WITHDRAWN: u16 = 1 << 4;
    pub const SNAPSHOT_STALE: u16 = 1 << 5;
    pub const PRICE_BREACH: u16 = 1 << 6;
    pub const ORACLE_UNUSABLE: u16 = 1 << 7;
    /// A1: a leg's view contains a venue its own venue no longer accepts.
    pub const ACCEPTANCE_REVOKED: u16 = 1 << 8;
}

#[event]
pub struct PortfolioInvalidated {
    pub user: Pubkey,
    pub version: u64,
    pub reasons: u16,
}

#[event]
pub struct ParamsUpdated {
    pub authority: Pubkey,
    pub params: crate::ConfigParams,
    pub risk_epoch: u64,
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
    pub authority: Pubkey,
    pub paused: bool,
}
