//! Shared harness for instruction-level tests of `ch_core`.
//!
//! Tests run the *compiled SBF program* (`target/deploy/ch_core.so`, built
//! with `--features mock-oracle`) inside LiteSVM, so account constraints, PDA
//! derivation, signer checks, CPIs to the system program and error codes are
//! all exercised exactly as on a validator.
//!
//! If the `.so` is missing the harness panics with the build command. It never
//! skips: a silently skipped on-chain test is how F11 happened.

use anchor_lang::prelude::Pubkey;
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use litesvm::types::{FailedTransactionMetadata, TransactionMetadata};
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;

pub use anchor_lang::solana_program::instruction::Instruction;
pub use ch_core;
pub use solana_keypair::Keypair as Kp;
pub use solana_signer::Signer as _;

pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Path of the program binary under test.
pub fn program_so_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("CH_CORE_SO") {
        return p.into();
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/ch_core.so")
}

/// Convert any 32-byte address type to Anchor's `Pubkey`.
pub fn pk(bytes: [u8; 32]) -> Pubkey {
    Pubkey::new_from_array(bytes)
}

pub fn kp_pubkey(kp: &Keypair) -> Pubkey {
    pk(kp.pubkey().to_bytes())
}

pub struct Env {
    pub svm: LiteSVM,
    pub payer: Keypair,
}

/// Failed metadata is boxed: it is large and clippy (rightly) objects to large `Err` variants.
pub type TxResult = Result<TransactionMetadata, Box<FailedTransactionMetadata>>;

impl Env {
    pub fn new() -> Self {
        let so = program_so_path();
        let bytes = std::fs::read(&so).unwrap_or_else(|e| {
            panic!(
                "cannot read {} ({e}). Build it first: \
                 `cargo build-sbf --manifest-path programs/ch_core/Cargo.toml --features mock-oracle`",
                so.display()
            )
        });
        let mut svm = LiteSVM::new();
        svm.add_program(ch_core::ID.to_bytes(), &bytes)
            .expect("load ch_core.so");
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000 * LAMPORTS_PER_SOL)
            .expect("airdrop payer");
        let mut env = Self { svm, payer };
        env.set_clock(1_000, 1_700_000_000);
        env
    }

    pub fn funded(&mut self, sol: u64) -> Keypair {
        let kp = Keypair::new();
        self.svm
            .airdrop(&kp.pubkey(), sol * LAMPORTS_PER_SOL)
            .expect("airdrop");
        kp
    }

    /// Set both the slot and the unix timestamp seen by the program.
    pub fn set_clock(&mut self, slot: u64, unix_timestamp: i64) {
        self.svm.warp_to_slot(slot);
        let mut clock: solana_clock::Clock = self.svm.get_sysvar();
        clock.slot = slot;
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar(&clock);
    }

    pub fn clock(&self) -> solana_clock::Clock {
        self.svm.get_sysvar()
    }

    /// Advance slot and time together (400ms per slot, rounded to whole seconds).
    pub fn advance_slots(&mut self, slots: u64) {
        let c = self.clock();
        let secs = (slots * 400 / 1000) as i64;
        self.set_clock(c.slot + slots, c.unix_timestamp + secs);
    }

    /// Send one transaction (payer = `self.payer`) with extra signers.
    pub fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> TxResult {
        self.svm.expire_blockhash();
        let mut all: Vec<&Keypair> = vec![&self.payer];
        all.extend_from_slice(signers);
        let ixs: Vec<_> = ixs.to_vec();
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.payer.pubkey()),
            &all,
            self.svm.latest_blockhash(),
        );
        self.svm.send_transaction(tx).map_err(Box::new)
    }

    pub fn account<T: AccountDeserialize>(&self, address: &Pubkey) -> Option<T> {
        let acc = self.svm.get_account(&address.to_bytes().into())?;
        T::try_deserialize(&mut acc.data.as_slice()).ok()
    }

    pub fn exists(&self, address: &Pubkey) -> bool {
        self.svm
            .get_account(&address.to_bytes().into())
            .map(|a| a.lamports > 0)
            .unwrap_or(false)
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an Anchor instruction from its `accounts::*` and `instruction::*` structs.
pub fn ix(accounts: impl ToAccountMetas, data: impl InstructionData) -> Instruction {
    Instruction {
        program_id: ch_core::ID,
        accounts: accounts.to_account_metas(None),
        data: data.data(),
    }
}

/// The custom error code a failed transaction returned, if any.
pub fn custom_code(res: &TxResult) -> Option<u32> {
    match res {
        Err(f) => match &f.err {
            TransactionError::InstructionError(
                _,
                solana_instruction_error::InstructionError::Custom(c),
            ) => Some(*c),
            _ => None,
        },
        Ok(_) => None,
    }
}

/// Assert the transaction failed with exactly `code` (Anchor/custom error number).
/// Panics with the logs otherwise, including when the transaction succeeded.
#[track_caller]
pub fn assert_code(res: &TxResult, code: u32) {
    match res {
        Ok(meta) => panic!(
            "expected failure with code {code}, but the transaction succeeded.\nlogs:\n  {}",
            meta.logs.join("\n  ")
        ),
        Err(f) => {
            let got = custom_code(res);
            assert_eq!(
                got,
                Some(code),
                "wrong failure: {:?}\nlogs:\n  {}",
                f.err,
                f.meta.logs.join("\n  ")
            );
        }
    }
}

/// Assert failure with a `ClearinghouseError` variant.
#[track_caller]
pub fn assert_ch_err(res: &TxResult, e: ch_core::ClearinghouseError) {
    assert_code(res, u32::from(e));
}

/// Assert failure with an Anchor framework error (e.g. ConstraintSeeds).
#[track_caller]
pub fn assert_anchor_err(res: &TxResult, e: anchor_lang::error::ErrorCode) {
    assert_code(res, u32::from(e));
}

/// Assert success, printing logs on failure. Returns compute units consumed.
#[track_caller]
pub fn assert_ok(res: &TxResult) -> u64 {
    match res {
        Ok(meta) => meta.compute_units_consumed,
        Err(f) => panic!(
            "transaction failed: {:?}\nlogs:\n  {}",
            f.err,
            f.meta.logs.join("\n  ")
        ),
    }
}

/// PDA helpers and standard fixtures for the current program API.
pub mod setup {
    use super::*;
    use ch_core::{accounts as acc, instruction as ins};

    pub fn config_pda() -> Pubkey {
        Pubkey::find_program_address(&[ch_core::SEED_CONFIG], &ch_core::ID).0
    }

    pub fn venue_pda(venue_id: &[u8; 32]) -> Pubkey {
        Pubkey::find_program_address(&[ch_core::SEED_VENUE, venue_id], &ch_core::ID).0
    }

    pub fn venue_id(name: &str) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[..name.len()].copy_from_slice(name.as_bytes());
        id
    }

    pub fn default_params(keeper: Pubkey) -> ch_core::InitConfigParams {
        ch_core::InitConfigParams {
            keeper_authority: keeper,
            default_fund_program: Pubkey::new_unique(),
            max_venues: 10,
            haircut_bps: 2_000,
            credit_ttl_slots: 3_000,
            snapshot_max_age_slots: 10_000,
            max_credit_per_user: 100_000_000_000,
            max_credit_bps_of_required: 7_500,
            corr_min_interval_slots: 0,
            corr_max_age_slots: 10_000,
            max_price_age_secs: 60,
            max_conf_bps: 100,
            max_move_bps: 500,
        }
    }

    pub fn initialize(env: &mut Env, admin: &Keypair) -> TxResult {
        let keeper = Pubkey::new_unique();
        let i = ix(
            acc::Initialize {
                config: config_pda(),
                admin: kp_pubkey(admin),
                system_program: anchor_lang::system_program::ID,
            },
            ins::Initialize {
                params: default_params(keeper),
            },
        );
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
                weight_bps: 10_000,
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
}
