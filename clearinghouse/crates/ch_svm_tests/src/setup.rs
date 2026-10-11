//! PDA helpers and a `World` fixture for the portfolio API.

use super::*;
use ch_core::{accounts as acc, instruction as ins, ConfigParams, CorrEntry};

pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[ch_core::SEED_CONFIG], &ch_core::ID).0
}

pub fn risk_model_pda() -> Pubkey {
    Pubkey::find_program_address(&[ch_core::SEED_RISK_MODEL], &ch_core::ID).0
}

pub fn venue_pda(venue_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[ch_core::SEED_VENUE, venue_id], &ch_core::ID).0
}

pub fn consent_pda(user: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ch_core::SEED_CONSENT, user.as_ref()], &ch_core::ID).0
}

pub fn portfolio_pda(user: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ch_core::SEED_PORTFOLIO, user.as_ref()], &ch_core::ID).0
}

pub fn snapshot_pda(user: &Pubkey, venue_id: &[u8; 32], asset: u8) -> Pubkey {
    Pubkey::find_program_address(
        &[ch_core::SEED_SNAPSHOT, user.as_ref(), venue_id, &[asset]],
        &ch_core::ID,
    )
    .0
}

pub fn mock_price_pda(asset: u8) -> Pubkey {
    Pubkey::find_program_address(&[ch_core::SEED_MOCK_PRICE, &[asset]], &ch_core::ID).0
}

pub fn venue_id(name: &str) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[..name.len()].copy_from_slice(name.as_bytes());
    id
}

/// The parameters the testbench and these tests start from.
pub fn default_params() -> ConfigParams {
    ConfigParams {
        max_venues: 16,
        haircut_bps: 2_000,
        credit_ttl_slots: 3_000,
        snapshot_max_age_slots: 10_000,
        max_credit_per_user: 100_000_000_000,
        max_credit_bps_of_im: 7_500,
        max_credit_per_venue: 1_000_000_000_000,
        max_credit_per_asset: 1_000_000_000_000,
        global_credit_cap: 10_000_000_000_000,
        risk_model_max_age_slots: 100_000,
        max_price_age_secs: 60,
        max_conf_bps: 100,
        max_move_bps: 500,
    }
}

pub fn initialize_ix(admin: &Pubkey, params: ConfigParams) -> Instruction {
    ix(
        acc::Initialize {
            config: config_pda(),
            risk_model: risk_model_pda(),
            admin: *admin,
            system_program: anchor_lang::system_program::ID,
        },
        ins::Initialize { params },
    )
}

pub fn initialize(env: &mut Env, admin: &Kp) -> TxResult {
    let i = initialize_ix(&kp_pubkey(admin), default_params());
    env.send(&[i], &[admin])
}

pub fn register_venue_ix(
    admin: &Pubkey,
    venue_pda_override: Option<Pubkey>,
    venue_id: [u8; 32],
    authority: Pubkey,
    index: u8,
) -> Instruction {
    ix(
        acc::RegisterVenue {
            config: config_pda(),
            venue_registration: venue_pda_override.unwrap_or_else(|| venue_pda(&venue_id)),
            admin: *admin,
            system_program: anchor_lang::system_program::ID,
        },
        ins::RegisterVenue {
            venue_id,
            venue_program_id: Pubkey::new_unique(),
            venue_authority: authority,
            venue_index: index,
        },
    )
}

pub fn set_venue_active_ix(admin: &Pubkey, venue_id: &[u8; 32], active: bool) -> Instruction {
    ix(
        acc::SetVenueActive {
            config: config_pda(),
            venue_registration: venue_pda(venue_id),
            admin: *admin,
        },
        ins::SetVenueActive { is_active: active },
    )
}

pub fn admin_only(admin: &Pubkey) -> acc::AdminOnly {
    acc::AdminOnly {
        config: config_pda(),
        admin: *admin,
    }
}

pub struct Venue {
    pub id: [u8; 32],
    pub index: u8,
    pub authority: Kp,
}

/// Initialized protocol with `n` active venues, mock prices for assets 0..4
/// and a correlation of +0.8 between assets 0 and 1.
pub struct World {
    pub env: Env,
    pub admin: Kp,
    pub venues: Vec<Venue>,
}

pub const SOL: u8 = 0;
pub const ETH: u8 = 1;
pub const BTC: u8 = 2;

impl World {
    pub fn new(n_venues: u8) -> Self {
        Self::with_params(n_venues, default_params())
    }

    pub fn with_params(n_venues: u8, params: ConfigParams) -> Self {
        let mut env = Env::new();
        let admin = env.funded(100);
        let admin_pk = kp_pubkey(&admin);
        assert_ok(&env.send(&[initialize_ix(&admin_pk, params)], &[&admin]));
        let mut venues = Vec::new();
        for i in 0..n_venues {
            let authority = env.funded(10);
            let id = venue_id(&format!("venue-{i}"));
            let r = env.send(
                &[register_venue_ix(
                    &admin_pk,
                    None,
                    id,
                    kp_pubkey(&authority),
                    i,
                )],
                &[&admin],
            );
            assert_ok(&r);
            venues.push(Venue {
                id,
                index: i,
                authority,
            });
        }
        let mut w = Self { env, admin, venues };
        w.set_corr(&[(SOL, ETH, 800_000)]);
        for (asset, price) in [
            (SOL, 150_000_000u64),
            (ETH, 3_000_000_000),
            (BTC, 65_000_000_000),
            (3, 1_000_000),
        ] {
            w.set_price(asset, price);
        }
        w
    }

    pub fn admin_pk(&self) -> Pubkey {
        kp_pubkey(&self.admin)
    }

    /// Price with a 1 bp confidence, published "now".
    pub fn set_price(&mut self, asset: u8, price: u64) {
        let ts = self.env.clock().unix_timestamp;
        self.set_price_full(asset, price, price / 10_000, ts);
    }

    pub fn set_price_full(&mut self, asset: u8, price: u64, conf: u64, publish_ts: i64) {
        let admin = self.admin_pk();
        let i = ix(
            acc::SetMockPrice {
                config: config_pda(),
                mock_price: mock_price_pda(asset),
                admin,
                system_program: anchor_lang::system_program::ID,
            },
            ins::SetMockPrice {
                asset_id: asset,
                price_micro: price,
                conf_micro: conf,
                publish_ts,
            },
        );
        let admin_kp = self.admin.insecure_clone();
        assert_ok(&self.env.send(&[i], &[&admin_kp]));
    }

    /// Re-publish every price at the current clock (keeps them fresh after warps).
    pub fn refresh_prices(&mut self) {
        for asset in [SOL, ETH, BTC, 3] {
            let acc = self
                .env
                .svm
                .get_account(&mock_price_pda(asset).to_bytes().into());
            if let Some(acc) = acc {
                let mp = ch_core::MockPrice::try_deserialize(&mut acc.data.as_slice()).unwrap();
                self.set_price_full(
                    asset,
                    mp.price_micro,
                    mp.conf_micro,
                    self.env.clock().unix_timestamp,
                );
            }
        }
    }

    pub fn set_corr_ix(&self, entries: &[(u8, u8, i32)]) -> Instruction {
        ix(
            acc::UpdateRiskModel {
                config: config_pda(),
                risk_model: risk_model_pda(),
                admin: self.admin_pk(),
            },
            ins::UpdateCorrelations {
                entries: entries
                    .iter()
                    .map(|&(a, b, rho)| CorrEntry { a, b, rho })
                    .collect(),
            },
        )
    }

    pub fn set_corr(&mut self, entries: &[(u8, u8, i32)]) {
        let i = self.set_corr_ix(entries);
        let admin_kp = self.admin.insecure_clone();
        assert_ok(&self.env.send(&[i], &[&admin_kp]));
    }

    pub fn consent_ix(user: &Pubkey, active: bool, bitmap: u64) -> Instruction {
        ix(
            acc::UpdateUserConsent {
                user_consent: consent_pda(user),
                portfolio: portfolio_pda(user),
                user: *user,
                system_program: anchor_lang::system_program::ID,
            },
            ins::UpdateUserConsent {
                is_active: active,
                authorized_venues_bitmap: bitmap,
            },
        )
    }

    /// A funded user who consents to every venue.
    pub fn user(&mut self) -> Kp {
        let user = self.env.funded(10);
        let i = Self::consent_ix(&kp_pubkey(&user), true, u64::MAX);
        assert_ok(&self.env.send(&[i], &[&user]));
        user
    }

    pub fn set_consent(&mut self, user: &Kp, active: bool, bitmap: u64) -> TxResult {
        let i = Self::consent_ix(&kp_pubkey(user), active, bitmap);
        self.env.send(&[i], &[user])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_ix(
        &self,
        user: &Pubkey,
        venue: usize,
        asset: u8,
        notional: u64,
        is_long: bool,
        im: u64,
        mm: u64,
    ) -> Instruction {
        let v = &self.venues[venue];
        ix(
            acc::SubmitPositionSnapshot {
                config: config_pda(),
                venue_registration: venue_pda(&v.id),
                user_consent: consent_pda(user),
                portfolio: portfolio_pda(user),
                pos_snapshot: snapshot_pda(user, &v.id, asset),
                user: *user,
                venue_authority: kp_pubkey(&v.authority),
                payer: kp_pubkey(&v.authority),
                system_program: anchor_lang::system_program::ID,
            },
            ins::SubmitPositionSnapshot {
                venue_id: v.id,
                asset_id: asset,
                notional,
                is_long,
                initial_margin: im,
                maintenance_margin: mm,
            },
        )
    }

    /// Venue `venue` reports a position: notional = 5 x IM, MM = IM / 2.
    pub fn submit(
        &mut self,
        user: &Kp,
        venue: usize,
        asset: u8,
        is_long: bool,
        im: u64,
    ) -> TxResult {
        self.submit_full(user, venue, asset, 5 * im, is_long, im, im / 2)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_full(
        &mut self,
        user: &Kp,
        venue: usize,
        asset: u8,
        notional: u64,
        is_long: bool,
        im: u64,
        mm: u64,
    ) -> TxResult {
        let i = self.submit_ix(&kp_pubkey(user), venue, asset, notional, is_long, im, mm);
        let auth = self.venues[venue].authority.insecure_clone();
        self.env.send(&[i], &[&auth])
    }

    pub fn close_ix(&self, user: &Pubkey, venue: usize, asset: u8, closer: &Pubkey) -> Instruction {
        let v = &self.venues[venue];
        ix(
            acc::ClosePositionSnapshot {
                venue_registration: venue_pda(&v.id),
                user_consent: consent_pda(user),
                portfolio: portfolio_pda(user),
                pos_snapshot: snapshot_pda(user, &v.id, asset),
                rent_receiver: kp_pubkey(&v.authority),
                closer: *closer,
            },
            ins::ClosePositionSnapshot {},
        )
    }

    pub fn portfolio(&self, user: &Kp) -> ch_core::Portfolio {
        self.env
            .account(&portfolio_pda(&kp_pubkey(user)))
            .expect("portfolio exists")
    }

    pub fn config(&self) -> ch_core::GlobalConfig {
        self.env.account(&config_pda()).expect("config exists")
    }

    pub fn venue_id_by_index(&self, index: u8) -> [u8; 32] {
        self.venues
            .iter()
            .find(|v| v.index == index)
            .expect("venue")
            .id
    }

    /// Remaining accounts for refresh: registered snapshots then oracles,
    /// in registration order (or a caller-supplied permutation).
    pub fn refresh_metas(&self, user: &Pubkey, order: Option<&[usize]>) -> Vec<Pubkey> {
        let p: ch_core::Portfolio = self.env.account(&portfolio_pda(user)).expect("portfolio");
        let keys: Vec<_> = p.registered[..p.num_registered as usize].to_vec();
        let idx: Vec<usize> = match order {
            Some(o) => o.to_vec(),
            None => (0..keys.len()).collect(),
        };
        let mut out: Vec<Pubkey> = idx
            .iter()
            .map(|&i| {
                snapshot_pda(
                    user,
                    &self.venue_id_by_index(keys[i].venue_index),
                    keys[i].asset_id,
                )
            })
            .collect();
        out.extend(idx.iter().map(|&i| mock_price_pda(keys[i].asset_id)));
        out
    }

    pub fn refresh_ix_with(&self, user: &Pubkey, remaining: &[Pubkey]) -> Instruction {
        let mut i = ix(
            acc::RefreshPortfolio {
                config: config_pda(),
                risk_model: risk_model_pda(),
                user_consent: consent_pda(user),
                portfolio: portfolio_pda(user),
            },
            ins::RefreshPortfolio {},
        );
        i.accounts.extend(remaining.iter().map(|k| {
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*k, false)
        }));
        i
    }

    pub fn refresh_ix(&self, user: &Pubkey) -> Instruction {
        let metas = self.refresh_metas(user, None);
        self.refresh_ix_with(user, &metas)
    }

    pub fn refresh(&mut self, user: &Kp) -> TxResult {
        let i = self.refresh_ix(&kp_pubkey(user));
        self.env.send(&[i], &[])
    }

    pub fn invalidate_ix(&self, user: &Pubkey, with_oracles: bool) -> Instruction {
        let mut i = ix(
            acc::InvalidatePortfolio {
                config: config_pda(),
                user_consent: consent_pda(user),
                portfolio: portfolio_pda(user),
            },
            ins::InvalidatePortfolio {},
        );
        if with_oracles {
            let p: ch_core::Portfolio = self.env.account(&portfolio_pda(user)).expect("portfolio");
            i.accounts
                .extend(p.legs[..p.num_legs as usize].iter().map(|l| {
                    anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                        mock_price_pda(l.asset_id),
                        false,
                    )
                }));
        }
        i
    }

    pub fn invalidate(&mut self, user: &Kp, with_oracles: bool) -> TxResult {
        let i = self.invalidate_ix(&kp_pubkey(user), with_oracles);
        self.env.send(&[i], &[])
    }

    /// What a venue would accept right now for (venue index, asset).
    pub fn usable(&self, user: &Kp, venue_index: u8, asset: u8) -> u64 {
        let p = self.portfolio(user);
        let c = self.config();
        p.usable_credit(c.risk_epoch, self.env.clock().slot, venue_index, asset)
    }

    pub fn usable_total(&self, user: &Kp) -> u64 {
        let p = self.portfolio(user);
        p.legs[..p.num_legs as usize]
            .iter()
            .map(|l| self.usable(user, l.venue_index, l.asset_id))
            .sum()
    }
}
