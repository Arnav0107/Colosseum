//! Counterparty allowlists (A1): credit per venue "view".
//!
//! Credit is computed jointly over all of a user's legs. Without allowlists a
//! venue that misreports (for example a fictitious offsetting leg) creates
//! credit that *other* venues apply. Here every venue `v` names the venues
//! whose reports it accepts, `accepts[v]` (its own bit is always implied). For
//! each leg on venue `v`:
//!
//! ```text
//!   view(v)  = legs of venues in (accepts[v] | bit v), restricted to the venues present
//!   c_view_i = credit of leg i computed over view(v) alone
//!   c_i      = min(c_joint_i, c_view_i)
//! ```
//!
//! * `c_view_i` depends only on legs `v` accepts, so a non-accepted venue can
//!   never raise `c_i` (it can only lower `c_joint_i`, which is safe);
//! * `c_i ≤ c_joint_i` keeps every guarantee of the joint computation: one
//!   hedge is never used twice (F3), and every cap, the floor and the
//!   per-user cap hold because they hold for the joint credits.
//!
//! Views with one leg are 0 without computing; a view equal to the full set
//! reuses the joint result; identical views are computed once.

use crate::portfolio::{
    compute_portfolio_credit, CreditParams, PortfolioCredit, PortfolioError, PortfolioLeg,
    RiskModel, MAX_LEGS,
};

/// Venue indices that can appear in an allowlist (bits of a u64).
pub const MAX_VIEW_VENUES: usize = 64;

pub fn venue_bit(venue_index: u8) -> u64 {
    1u64.checked_shl(venue_index as u32).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewCredit {
    /// The joint computation over every leg (risk numbers, caps, floor).
    pub joint: PortfolioCredit,
    /// Final per-leg credit: min(joint, view), aligned with the input legs.
    pub leg_credit: [u64; MAX_LEGS],
    /// Venues whose legs entered the view of each leg (always includes the
    /// leg's own venue). Venues must check `view_mask ⊆ accepts | own bit`.
    pub view_mask: [u64; MAX_LEGS],
    /// Number of engine runs besides the joint one (for cost accounting).
    pub view_runs: u8,
}

impl ViewCredit {
    pub fn total(&self) -> u64 {
        self.leg_credit[..self.joint.num_legs]
            .iter()
            .fold(0u64, |a, c| a.saturating_add(*c))
    }
}

/// The joint computation, then one computation per distinct accepted view.
/// `accepts(v)` is venue v's allowlist (its own bit is added here). Pure,
/// deterministic and independent of leg order.
pub fn compute_view_credit(
    legs: &[PortfolioLeg],
    model: &dyn RiskModel,
    params: &CreditParams,
    accepts: &dyn Fn(u8) -> u64,
) -> Result<ViewCredit, PortfolioError> {
    let joint = compute_portfolio_credit(legs, model, params)?;
    let n = legs.len();
    let mut present = 0u64;
    for l in legs {
        present |= venue_bit(l.venue_index);
    }
    let mut leg_credit = [0u64; MAX_LEGS];
    let mut view_mask = [0u64; MAX_LEGS];
    // Cache of computed views: (mask, credit per input leg index).
    let mut cache: [(u64, [u64; MAX_LEGS]); MAX_LEGS] = [(0, [0; MAX_LEGS]); MAX_LEGS];
    let mut cached = 0usize;
    let mut view_runs = 0u8;
    for i in 0..n {
        let v = legs[i].venue_index;
        let own = venue_bit(v);
        let mask = (accepts(v) | own) & present;
        view_mask[i] = mask;
        let view_c = if mask == present {
            joint.leg_credit[i]
        } else if let Some((_, c)) = cache[..cached].iter().find(|(m, _)| *m == mask) {
            c[i]
        } else {
            let mut sub = [legs[0]; MAX_LEGS];
            let mut idx = [0usize; MAX_LEGS];
            let mut k = 0usize;
            for (j, l) in legs.iter().enumerate() {
                if mask & venue_bit(l.venue_index) != 0 {
                    sub[k] = *l;
                    idx[k] = j;
                    k += 1;
                }
            }
            let mut by_leg = [0u64; MAX_LEGS];
            if k >= 2 {
                let r = compute_portfolio_credit(&sub[..k], model, params)?;
                view_runs += 1;
                for t in 0..k {
                    by_leg[idx[t]] = r.leg_credit[t];
                }
            }
            cache[cached] = (mask, by_leg);
            cached += 1;
            by_leg[i]
        };
        leg_credit[i] = joint.leg_credit[i].min(view_c);
    }
    Ok(ViewCredit {
        joint,
        leg_credit,
        view_mask,
        view_runs,
    })
}

/// The venue-side validity rule for a leg's view (mirrors `ch_client`): the
/// credit may be used only while every venue in its view is still accepted.
pub fn view_still_accepted(view_mask: u64, venue_index: u8, accepts_now: u64) -> bool {
    view_mask & !(accepts_now | venue_bit(venue_index)) == 0
}
