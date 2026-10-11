use anchor_lang::prelude::*;

use crate::events::*;
use crate::governance::*;
use crate::state::*;
use crate::ClearinghouseError;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorrEntry {
    pub a: u8,
    pub b: u8,
    /// Scaled 1e6, in [-1e6, 1e6].
    pub rho: i32,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetMove {
    pub asset: u8,
    /// Signed move in bps, in [-10_000, 10_000].
    pub move_bps: i16,
}

/// Upper bound on correlation entries per change (fits a transaction and the
/// pending-change account).
pub const MAX_CORR_ENTRIES_PER_TX: usize = 128;

/// Every risk-authority change goes through here.
#[derive(Accounts)]
pub struct RiskGovernance<'info> {
    #[account(mut, seeds = [SEED_CONFIG], bump = config.bump)]
    pub config: Box<Account<'info, GlobalConfig>>,

    #[account(mut, seeds = [SEED_RISK_MODEL], bump = risk_model.load()?.bump)]
    pub risk_model: AccountLoader<'info, RiskModel>,

    #[account(mut, seeds = [SEED_PENDING_CHANGE], bump = pending_change.bump)]
    pub pending_change: Box<Account<'info, PendingChange>>,

    pub authority: Signer<'info>,
}

fn scenario_vector(kind: u8, moves: &[AssetMove]) -> Result<[i16; MAX_ASSETS]> {
    use ch_math::risk::scenario_kind as k;
    require!(
        kind == k::HISTORICAL_REPLAY
            || kind == k::HYPOTHETICAL
            || (kind == k::UNUSED && moves.is_empty()),
        ClearinghouseError::InvalidScenario
    );
    require!(
        moves.len() <= MAX_ASSETS,
        ClearinghouseError::InvalidScenario
    );
    let mut v = [0i16; MAX_ASSETS];
    for m in moves {
        require!(
            (m.asset as usize) < MAX_ASSETS,
            ClearinghouseError::InvalidAssetId
        );
        require!(
            m.move_bps.unsigned_abs() <= MAX_MOVE_BPS,
            ClearinghouseError::InvalidRiskParams
        );
        v[m.asset as usize] = m.move_bps;
    }
    Ok(v)
}

impl RiskGovernance<'_> {
    /// Validate `change` against bounds and step limits *relative to the
    /// current state*, and say whether it can only reduce credit.
    fn check(&self, change: &RiskChange) -> Result<bool> {
        let config = &self.config;
        let model = self.risk_model.load()?;
        Ok(match change {
            RiskChange::None => return err!(ClearinghouseError::NoPendingChange),
            RiskChange::Params(p) => {
                validate_config_params(p)?;
                check_params_step(&config.params, p)?;
                params_reduce_credit(&config.params, p)
            }
            RiskChange::Correlations(entries) => {
                require!(
                    entries.len() <= MAX_CORR_ENTRIES_PER_TX,
                    ClearinghouseError::InvalidCorrelationValue
                );
                for e in entries {
                    require!(
                        (-RHO_ONE..=RHO_ONE).contains(&e.rho),
                        ClearinghouseError::InvalidCorrelationValue
                    );
                    let k = corr_index(e.a, e.b).ok_or(ClearinghouseError::InvalidAssetId)?;
                    check_corr_step(model.corr[k], e.rho)?;
                }
                // Not monotone in credit (depends on each book's direction).
                // An empty list re-attests the model and extends its
                // freshness window, which is not credit-reducing either.
                false
            }
            RiskChange::AssetRisk { asset_id, params } => {
                require!(
                    (*asset_id as usize) < MAX_ASSETS,
                    ClearinghouseError::InvalidAssetId
                );
                let new = AssetRiskParams::from(*params);
                validate_asset_risk(&new)?;
                let old = model.assets[*asset_id as usize];
                check_asset_step(&old, &new)?;
                asset_change_reduces_credit(&old, &new)
            }
            RiskChange::Stress {
                corr_stress_ppm,
                basis_decorr_ppm,
            } => {
                validate_stress(*corr_stress_ppm, *basis_decorr_ppm)?;
                stress_change_reduces_credit(
                    (model.corr_stress_ppm, model.basis_decorr_ppm),
                    (*corr_stress_ppm, *basis_decorr_ppm),
                )
            }
            RiskChange::Scenario { index, kind, moves } => {
                require!(
                    (*index as usize) < MAX_SCENARIOS,
                    ClearinghouseError::InvalidScenario
                );
                scenario_vector(*kind, moves)?;
                scenario_change_reduces_credit(model.scenario_kind[*index as usize], *kind)
            }
            RiskChange::OracleFeed { asset_id, feed_id } => {
                require!(
                    (*asset_id as usize) < MAX_ASSETS,
                    ClearinghouseError::InvalidAssetId
                );
                // Removing a feed makes the asset unpriceable (no credit);
                // setting or switching one is never treated as reducing.
                *feed_id == [0u8; 32]
            }
        })
    }

    /// Apply an already-checked change, bump versions and the risk epoch.
    fn apply(&mut self, change: &RiskChange) -> Result<()> {
        let slot = Clock::get()?.slot;
        if let RiskChange::Params(p) = change {
            self.config.params = *p;
            let risk_epoch = self.config.bump_epoch()?;
            emit!(ParamsUpdated {
                authority: self.authority.key(),
                params: *p,
                risk_epoch,
            });
            return Ok(());
        }
        let (version, entries) = {
            let mut model = self.risk_model.load_mut()?;
            let entries: u16 = match change {
                RiskChange::Correlations(es) => {
                    for e in es {
                        let k = corr_index(e.a, e.b).ok_or(ClearinghouseError::InvalidAssetId)?;
                        model.corr[k] = e.rho;
                    }
                    es.len() as u16
                }
                RiskChange::AssetRisk { asset_id, params } => {
                    model.assets[*asset_id as usize] = AssetRiskParams::from(*params);
                    1
                }
                RiskChange::Stress {
                    corr_stress_ppm,
                    basis_decorr_ppm,
                } => {
                    model.corr_stress_ppm = *corr_stress_ppm;
                    model.basis_decorr_ppm = *basis_decorr_ppm;
                    1
                }
                RiskChange::Scenario { index, kind, moves } => {
                    model.scenarios[*index as usize] = scenario_vector(*kind, moves)?;
                    model.scenario_kind[*index as usize] = *kind;
                    moves.len() as u16
                }
                RiskChange::OracleFeed { asset_id, feed_id } => {
                    model.oracle_feeds[*asset_id as usize] = *feed_id;
                    1
                }
                RiskChange::None | RiskChange::Params(_) => 0,
            };
            model.version = model
                .version
                .checked_add(1)
                .ok_or(ClearinghouseError::MathOverflow)?;
            model.updated_slot = slot;
            (model.version, entries)
        };
        let risk_epoch = self.config.bump_epoch()?;
        emit!(RiskModelUpdated {
            version,
            entries,
            risk_epoch,
        });
        Ok(())
    }

    /// Risk authority: apply now if the change can only reduce credit (or the
    /// timelock is 0), otherwise queue it behind `timelock_slots`.
    pub fn propose(&mut self, change: RiskChange) -> Result<()> {
        require_keys_eq!(
            self.authority.key(),
            self.config.risk_authority,
            ClearinghouseError::Unauthorized
        );
        let reduces = self.check(&change)?;
        let delay = self.config.params.timelock_slots;
        if reduces || delay == 0 {
            return self.apply(&change);
        }
        require!(
            self.pending_change.change == RiskChange::None,
            ClearinghouseError::PendingChangeExists
        );
        let slot = Clock::get()?.slot;
        let p = &mut self.pending_change;
        p.change = change;
        p.proposer = self.authority.key();
        p.proposed_slot = slot;
        p.eta_slot = slot.saturating_add(delay);
        emit!(ChangeQueued {
            proposer: p.proposer,
            eta_slot: p.eta_slot,
        });
        Ok(())
    }

    /// Risk authority or admin, after the timelock. Re-validated against the
    /// state at apply time (bounds and step limits).
    pub fn apply_pending(&mut self) -> Result<()> {
        let who = self.authority.key();
        require!(
            who == self.config.risk_authority || who == self.config.admin,
            ClearinghouseError::Unauthorized
        );
        let change = self.pending_change.change.clone();
        require!(
            change != RiskChange::None,
            ClearinghouseError::NoPendingChange
        );
        require!(
            Clock::get()?.slot >= self.pending_change.eta_slot,
            ClearinghouseError::TimelockNotElapsed
        );
        self.check(&change)?;
        self.pending_change.change = RiskChange::None;
        self.pending_change.eta_slot = 0;
        self.apply(&change)?;
        emit!(ChangeApplied { authority: who });
        Ok(())
    }

    /// Risk authority, guardian or admin: drop the pending change.
    pub fn cancel_pending(&mut self) -> Result<()> {
        let who = self.authority.key();
        let c = &self.config;
        require!(
            who == c.risk_authority || who == c.guardian || who == c.admin,
            ClearinghouseError::Unauthorized
        );
        require!(
            self.pending_change.change != RiskChange::None,
            ClearinghouseError::NoPendingChange
        );
        self.pending_change.change = RiskChange::None;
        self.pending_change.eta_slot = 0;
        emit!(ChangeCancelled { authority: who });
        Ok(())
    }
}
