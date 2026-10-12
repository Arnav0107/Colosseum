//! A8: seeded random-sequence (stateful) invariant test on the compiled
//! program. Design: docs/design/A8_A9_TEST_PLAN.md.
//!
//!     cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test stateful
//!     CH_STATEFUL_SEEDS=7,42 CH_STATEFUL_STEPS=1000 cargo test ... --test stateful -- --nocapture
//!
//! Each step draws one action (valid or deliberately invalid). An action may
//! only fail with an error from its whitelist; any other outcome (including
//! CounterUnderflow or MathOverflow) fails the test. After EVERY step all
//! invariants are checked by reading every account. A failure message names
//! the seed, the step, the last actions and the command that reproduces it.

use ch_core::{status, ClearinghouseError as E, CREDIT_KIND_TRADING_POWER_ONLY};
use ch_svm_tests::setup::*;
use ch_svm_tests::*;
use std::collections::BTreeSet;

const N_VENUES: usize = 4;
const N_USERS: usize = 3;
const ASSETS: [u8; 3] = [SOL, ETH, BTC];
/// Fixed seed list run in CI (32 seeds x 300 steps).
const CI_SEEDS: [u64; 32] = [
    1, 2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89,
    97, 101, 103, 107, 109, 113, 127,
];
const CI_STEPS: usize = 300;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

fn code(e: E) -> u32 {
    u32::from(e)
}

struct Sim {
    w: World,
    users: Vec<Kp>,
    /// (user, venue, asset) with an open snapshot.
    open: BTreeSet<(usize, usize, u8)>,
    history: Vec<String>,
    seed: u64,
    step: usize,
}

impl Sim {
    fn new(seed: u64) -> Self {
        let mut w = World::new(N_VENUES as u8);
        let users = (0..N_USERS).map(|_| w.user()).collect();
        Self {
            w,
            users,
            open: BTreeSet::new(),
            history: Vec::new(),
            seed,
            step: 0,
        }
    }

    fn fail(&self, what: &str) -> ! {
        let tail: Vec<_> = self.history.iter().rev().take(20).rev().cloned().collect();
        panic!(
            "stateful invariant violated: {what}\nseed {} step {}\nlast actions:\n  {}\nreproduce: CH_STATEFUL_SEEDS={} cargo test --manifest-path crates/ch_svm_tests/Cargo.toml --test stateful -- --nocapture",
            self.seed,
            self.step,
            tail.join("\n  "),
            self.seed
        );
    }

    /// Accept Ok or one of `allowed` errors; anything else is a violation.
    fn expect(&mut self, label: String, res: &TxResult, allowed: &[E]) -> bool {
        let ok = res.is_ok();
        let c = custom_code(res);
        let outcome = if ok {
            "ok".to_string()
        } else {
            format!("err {c:?}")
        };
        self.history.push(format!("{label} -> {outcome}"));
        if ok {
            return true;
        }
        match c {
            Some(c) if allowed.iter().any(|e| code(*e) == c) => false,
            _ => {
                let logs = res
                    .as_ref()
                    .err()
                    .map(|f| f.meta.logs.join("\n"))
                    .unwrap_or_default();
                self.fail(&format!("{label}: unexpected failure {c:?}\n{logs}"))
            }
        }
    }

    /// Must fail with exactly `e`.
    fn expect_err(&mut self, label: String, res: &TxResult, e: E) {
        let c = custom_code(res);
        self.history
            .push(format!("{label} -> {c:?} (expected {e:?})"));
        if c != Some(code(e)) {
            self.fail(&format!("{label}: expected {e:?}, got {c:?}"));
        }
    }

    fn act(&mut self, r: &mut Rng) {
        let u = r.below(N_USERS as u64) as usize;
        let v = r.below(N_VENUES as u64) as usize;
        let a = ASSETS[r.below(3) as usize];
        let user = self.users[u].insecure_clone();
        match r.below(20) {
            0..=3 => {
                let long = r.chance(50);
                let im = (1 + r.below(20)) * 1_000_000_000; // $1k..$20k
                let res = self.w.submit(&user, v, a, long, im);
                let ok = self.expect(
                    format!("submit u{u} v{v} a{a} long={long} im={im}"),
                    &res,
                    &[
                        E::VenueInactive,
                        E::VenueNotAuthorized,
                        E::UserConsentMissing,
                        E::PortfolioFull,
                        E::NotionalGrowthTooFast,
                    ],
                );
                if ok {
                    self.open.insert((u, v, a));
                }
            }
            4 => {
                let mine: Vec<_> = self.open.iter().filter(|k| k.0 == u).copied().collect();
                if mine.is_empty() {
                    return;
                }
                let (_, v, a) = mine[r.below(mine.len() as u64) as usize];
                let auth = self.w.venues[v].authority.insecure_clone();
                let up = kp_pubkey(&user);
                let i = self.w.close_ix(&up, v, a, &kp_pubkey(&auth));
                let res = self.w.env.send(&[i], &[&auth]);
                if self.expect(format!("close u{u} v{v} a{a}"), &res, &[]) {
                    self.open.remove(&(u, v, a));
                }
            }
            5 => {
                let active = r.chance(85);
                let bitmap = if r.chance(70) {
                    (1 << N_VENUES) - 1
                } else {
                    r.below(1 << N_VENUES)
                };
                let res = self.w.set_consent(&user, active, bitmap);
                self.expect(
                    format!("consent u{u} active={active} bitmap={bitmap:#b}"),
                    &res,
                    &[],
                );
            }
            6..=11 => {
                let mut res = self.w.refresh(&user);
                // Books with partial allowlist views can exceed the default
                // 200k CU (docs/PERFORMANCE.md); like the keeper, retry with a
                // ComputeBudget limit. Out-of-CU is the only error without a
                // program error code, and is never a partial state change.
                let out_of_cu = res.as_ref().err().is_some_and(|f| {
                    f.meta.logs.iter().any(|l| {
                        l.contains("exceeded CUs meter")
                            || l.contains("Computational budget exceeded")
                    })
                });
                if out_of_cu {
                    self.history
                        .push(format!("refresh u{u} -> out of CU at 200k, retry"));
                    res = self.w.refresh_with_cu_limit(&user, 1_400_000);
                }
                self.expect(
                    format!("refresh u{u}"),
                    &res,
                    &[
                        E::ProgramPaused,
                        E::SnapshotStale,
                        E::VenueInactive,
                        E::VenueNotAuthorized,
                        E::UserConsentMissing,
                        E::PriceStale,
                        E::RiskModelStale,
                    ],
                );
            }
            12 => {
                let with_oracles = r.chance(50);
                let res = self.w.invalidate(&user, with_oracles);
                self.expect(
                    format!("invalidate u{u} oracles={with_oracles}"),
                    &res,
                    &[E::InvalidationNotJustified, E::PriceStale],
                );
            }
            13 => {
                if !r.chance(40) {
                    return;
                }
                let admin = self.w.admin_pk();
                let (label, res) = match r.below(4) {
                    0 => ("pause", self.w.admin_send(pause_ix(&admin))),
                    1 => ("unpause", self.w.admin_send(unpause_ix(&admin))),
                    2 => ("bump epoch", self.w.admin_send(bump_epoch_ix(&admin))),
                    _ => {
                        // Invalid: a user cannot bump the epoch.
                        let i = bump_epoch_ix(&kp_pubkey(&user));
                        let res = self.w.env.send(&[i], &[&user]);
                        self.expect_err(format!("bump epoch by user u{u}"), &res, E::Unauthorized);
                        return;
                    }
                };
                self.expect(label.to_string(), &res, &[]);
            }
            14 => {
                if !r.chance(50) {
                    return;
                }
                let admin = self.w.admin_pk();
                let id = self.w.venues[v].id;
                let active = r.chance(60);
                let i = set_venue_active_ix(&admin, &id, active);
                let res = self.w.admin_send(i);
                // Requesting activation of an active venue: InvalidVenueIndex.
                self.expect(
                    format!("venue v{v} active={active}"),
                    &res,
                    &[E::InvalidVenueIndex],
                );
                if active {
                    let res = self.w.admin_send(activate_venue_ix(&admin, &id));
                    // Already active (no ETA pending), or completed by the request (timelock 0).
                    self.expect(
                        format!("activate v{v}"),
                        &res,
                        &[
                            E::InvalidVenueIndex,
                            E::NoPendingChange,
                            E::TimelockNotElapsed,
                        ],
                    );
                }
            }
            15 => {
                let admin = self.w.admin_pk();
                let mut p = self.w.config().params;
                let haircut = 1_500 + 500 * r.below(4) as u16; // 15%..30%
                p.haircut_bps = haircut;
                let res = self.w.admin_send(update_params_ix(&admin, p));
                self.expect(format!("haircut {haircut}"), &res, &[E::ChangeTooLarge]);
            }
            16 => {
                if r.chance(80) {
                    let mask = if r.chance(50) {
                        u64::MAX
                    } else {
                        r.below(1 << N_VENUES)
                    };
                    let res = self.w.set_acceptance(v, mask);
                    self.expect(format!("accept v{v} mask={mask:#b}"), &res, &[]);
                } else {
                    // Invalid: another venue's authority.
                    let other = (v + 1) % N_VENUES;
                    let auth = self.w.venues[other].authority.insecure_clone();
                    let i = set_acceptance_ix(&self.w.venues[v].id, &kp_pubkey(&auth), 0);
                    let res = self.w.env.send(&[i], &[&auth]);
                    self.expect_err(format!("accept v{v} by v{other}"), &res, E::Unauthorized);
                }
            }
            _ => {
                if r.chance(50) {
                    // Mostly short warps; one in ten crosses the credit TTL.
                    let slots = if r.chance(10) {
                        1 + r.below(4_000)
                    } else {
                        1 + r.below(300)
                    };
                    self.w.env.advance_slots(slots);
                    self.w.refresh_prices();
                    self.history.push(format!("warp {slots} slots"));
                } else {
                    let base = [150_000_000u64, 3_000_000_000, 65_000_000_000][r.below(3) as usize];
                    let asset = [SOL, ETH, BTC][[150_000_000u64, 3_000_000_000, 65_000_000_000]
                        .iter()
                        .position(|x| *x == base)
                        .unwrap()];
                    let pct = r.below(31) as i64 - 15; // -15%..+15%
                    let price = (base as i64 + base as i64 * pct / 100) as u64;
                    self.w.set_price(asset, price);
                    self.history.push(format!("price a{asset} {pct:+}%"));
                }
            }
        }
    }

    fn check(&self) {
        let c = self.w.config();
        let now = self.w.env.clock().slot;
        let p = c.params;
        let (mut global, mut venue, mut asset) = (0u128, [0u128; 64], [0u128; 32]);
        for user in &self.users {
            let pf = self.w.portfolio(user);
            if pf.credit_kind != CREDIT_KIND_TRADING_POWER_ONLY {
                self.fail("credit kind is not TradingPowerOnly");
            }
            let legs = &pf.legs[..pf.num_legs as usize];
            let total: u128 = legs.iter().map(|l| l.credit as u128).sum();
            if pf.counted && pf.counted_epoch == c.risk_epoch {
                global += pf.total_credit as u128;
                for l in legs {
                    venue[l.venue_index as usize] += l.credit as u128;
                    asset[l.asset_id as usize] += l.credit as u128;
                }
            }
            if pf.counted && total != pf.total_credit as u128 {
                self.fail("portfolio total != sum of leg credits");
            }
            if total > p.max_credit_per_user as u128 {
                self.fail("per-user cap exceeded");
            }
            let sum_im: u128 = legs.iter().map(|l| l.initial_margin as u128).sum();
            if pf.status == status::ACTIVE
                && total > sum_im.saturating_sub(pf.portfolio_risk as u128)
            {
                self.fail("A2 floor violated: credit > (sum IM - R_p)+");
            }
            for l in legs {
                if l.credit > l.initial_margin - l.maintenance_margin {
                    self.fail("leg credit > IM - MM");
                }
                if l.credit as u128
                    > l.initial_margin as u128 * p.max_credit_bps_of_im as u128 / 10_000
                {
                    self.fail("leg credit > bps-of-IM cap");
                }
                let usable = pf.usable_credit(
                    c.risk_epoch,
                    c.accepts_of(l.venue_index),
                    now,
                    l.venue_index,
                    l.asset_id,
                );
                let must_be_zero = pf.status != status::ACTIVE
                    || pf.risk_epoch != c.risk_epoch
                    || now > pf.expires_slot
                    || !ch_math::view::view_still_accepted(
                        l.view_mask,
                        l.venue_index,
                        c.accepts_of(l.venue_index),
                    );
                if must_be_zero && usable != 0 {
                    self.fail(
                        "usable credit while inactive / stale epoch / expired / not allowlisted",
                    );
                }
                if usable > 0 && !c.venue_is_active(l.venue_index) {
                    self.fail("usable credit at an inactive venue");
                }
                if usable > 0
                    && l.view_mask & !(c.accepts_of(l.venue_index) | (1u64 << l.venue_index)) != 0
                {
                    self.fail("usable credit from a venue not allowlisted");
                }
            }
        }
        if global != c.global_outstanding_credit as u128 {
            self.fail(&format!(
                "global counter {} != sum of counted credits {global}",
                c.global_outstanding_credit
            ));
        }
        for i in 0..64 {
            if venue[i] != c.venue_outstanding[i] as u128 {
                self.fail(&format!("venue counter {i} mismatch"));
            }
        }
        for i in 0..32 {
            if asset[i] != c.asset_outstanding[i] as u128 {
                self.fail(&format!("asset counter {i} mismatch"));
            }
        }
        if c.global_outstanding_credit > p.global_credit_cap
            || c.venue_outstanding
                .iter()
                .any(|x| *x > p.max_credit_per_venue)
            || c.asset_outstanding
                .iter()
                .any(|x| *x > p.max_credit_per_asset)
        {
            self.fail("an outstanding-credit cap is exceeded");
        }
    }
}

fn seeds() -> (Vec<u64>, usize) {
    let seeds = std::env::var("CH_STATEFUL_SEEDS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().parse().expect("seed"))
                .collect()
        })
        .unwrap_or_else(|| CI_SEEDS.to_vec());
    let steps = std::env::var("CH_STATEFUL_STEPS")
        .ok()
        .map(|s| s.parse().expect("steps"))
        .unwrap_or(CI_STEPS);
    (seeds, steps)
}

#[test]
fn random_action_sequences_preserve_every_invariant() {
    let (seeds, steps) = seeds();
    let (mut ok_actions, mut credited_checks) = (0usize, 0usize);
    for seed in seeds {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut s = Sim::new(seed);
        s.check();
        for step in 0..steps {
            s.step = step;
            s.act(&mut r);
            s.check();
            if s.history.last().is_some_and(|h| h.ends_with("-> ok")) {
                ok_actions += 1;
            }
            if s.users.iter().any(|u| s.w.usable_total(u) > 0) {
                credited_checks += 1;
            }
        }
    }
    println!(
        "stateful: {ok_actions} successful actions; {credited_checks} steps with usable credit"
    );
    // The generator must reach states with live credit, or the invariants are vacuous.
    assert!(
        credited_checks > 1_500,
        "too few steps with usable credit: {credited_checks}"
    );
}
