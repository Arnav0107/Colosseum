//! MOCK venue B (identical to mock_perps_a, different program id) for tests and localnet only. Not a perpetuals exchange: it has no
//! positions, collateral or liquidation engine. It exists to prove the
//! production venue-authentication path (F10): a snapshot reaches ch_core only
//! through CPI from the registered venue program, signed by that program's
//! `[b"clearinghouse"]` PDA.
//!
//! It deliberately does not depend on the ch_core crate (ch_core requires an
//! explicit oracle/venue-auth feature choice); the instruction is built from
//! its Anchor discriminator and Borsh-encoded arguments, which the LiteSVM
//! tests check against ch_core's own definitions.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;

declare_id!("9XiLmi2tEuxsaLzChVD8PT5csrZrapTphEdY8iUdwN58");

pub const CH_CORE_ID: Pubkey = pubkey!("2XcTzg5kuZBxaY7FXGHHDbVCtsUxeoMtgJnNpXevsBbg");
/// sha256("global:submit_position_snapshot")[..8]
pub const SUBMIT_POSITION_SNAPSHOT_DISCRIMINATOR: [u8; 8] = [232, 58, 29, 242, 23, 228, 111, 118];
pub const VENUE_SIGNER_SEED: &[u8] = b"clearinghouse";

#[program]
pub mod mock_perps_b {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    pub fn report_position(
        ctx: Context<ReportPosition>,
        venue_id: [u8; 32],
        asset_id: u8,
        notional: u64,
        is_long: bool,
        initial_margin: u64,
        maintenance_margin: u64,
    ) -> Result<()> {
        let a = &ctx.accounts;
        let mut data = SUBMIT_POSITION_SNAPSHOT_DISCRIMINATOR.to_vec();
        (
            venue_id,
            asset_id,
            notional,
            is_long,
            initial_margin,
            maintenance_margin,
        )
            .serialize(&mut data)?;
        let ix = Instruction {
            program_id: CH_CORE_ID,
            accounts: vec![
                AccountMeta::new_readonly(a.config.key(), false),
                AccountMeta::new_readonly(a.venue_registration.key(), false),
                AccountMeta::new_readonly(a.user_consent.key(), false),
                AccountMeta::new(a.portfolio.key(), false),
                AccountMeta::new(a.pos_snapshot.key(), false),
                AccountMeta::new_readonly(a.user.key(), false),
                AccountMeta::new_readonly(a.venue_signer.key(), true),
                AccountMeta::new(a.payer.key(), true),
                AccountMeta::new_readonly(a.system_program.key(), false),
            ],
            data,
        };
        invoke_signed(
            &ix,
            &[
                a.config.to_account_info(),
                a.venue_registration.to_account_info(),
                a.user_consent.to_account_info(),
                a.portfolio.to_account_info(),
                a.pos_snapshot.to_account_info(),
                a.user.to_account_info(),
                a.venue_signer.to_account_info(),
                a.payer.to_account_info(),
                a.system_program.to_account_info(),
                a.ch_core_program.to_account_info(),
            ],
            &[&[VENUE_SIGNER_SEED, &[ctx.bumps.venue_signer]]],
        )?;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct ReportPosition<'info> {
    /// CHECK: validated by ch_core.
    pub config: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub venue_registration: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub user_consent: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    #[account(mut)]
    pub portfolio: UncheckedAccount<'info>,
    /// CHECK: created/validated by ch_core.
    #[account(mut)]
    pub pos_snapshot: UncheckedAccount<'info>,
    /// CHECK: the user whose position is reported; bound by ch_core's seeds.
    pub user: UncheckedAccount<'info>,
    /// CHECK: this program's signer PDA; signs the CPI via invoke_signed.
    #[account(seeds = [VENUE_SIGNER_SEED], bump)]
    pub venue_signer: UncheckedAccount<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: must be ch_core.
    #[account(address = CH_CORE_ID)]
    pub ch_core_program: UncheckedAccount<'info>,
}
