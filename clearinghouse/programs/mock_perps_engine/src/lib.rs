//! C1 reference venue: a minimal perpetuals margin engine that consumes
//! Clearinghouse trading-power credit. **Localnet / tests only.**
//!
//! What it is: collateral deposits, one position per asset (notional, side),
//! IM 20% / MM 10% of notional by default, a margin check on opening new risk
//! that applies `ch_client::effective_initial_margin(IM, MM, usable_credit)`
//! with full validation of the Clearinghouse accounts, liquidation on
//! maintenance margin only (credit never read), and a CPI to
//! `ch_core::submit_position_snapshot` (or `close_position_snapshot`) on
//! every position change.
//!
//! What it is not: an exchange. No order book, funding, fees, ADL, insurance
//! fund or liquidator reward. Prices come from ch_core's `MockPrice`
//! accounts. **Collateral is lamports counted 1:1 as micro-USD** (1 SOL of
//! lamports = $1,000 of test collateral). That is a stand-in for a
//! stablecoin: no SPL dependency was available offline. The venue (this
//! program) holds the collateral; Clearinghouse never does.
//!
//! The same source is built twice (`--features instance-a` / `instance-b`),
//! giving two program ids, so a user can hold a long on venue A and a short
//! on venue B. Each instance signs ch_core CPIs with its own
//! `[b"clearinghouse"]` PDA.
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use ch_client::{effective_initial_margin, usable_credit, LegCredit, PortfolioView};
use ch_core::cpi::accounts as cha;

#[cfg(all(feature = "instance-a", feature = "instance-b"))]
compile_error!("mock_perps_engine: enable only one of `instance-a` / `instance-b`");
#[cfg(not(any(feature = "instance-a", feature = "instance-b")))]
compile_error!("mock_perps_engine: build with `--features instance-a` or `--features instance-b`");

#[cfg(feature = "instance-a")]
declare_id!("Bcsu5MamSs98b6ALwDiBSiHQDkvST4GNFN6V9WKRc68o");
#[cfg(feature = "instance-b")]
declare_id!("B9SGSvTEquvzaZnDCie43wPi6Qyd7JA4QokPbsQvvPBM");

pub const ENGINE_SEED: &[u8] = b"engine";
pub const VAULT_SEED: &[u8] = b"vault";
pub const ACCOUNT_SEED: &[u8] = b"account";
/// The venue's signer for ch_core (registered as its `venue_authority`).
pub const VENUE_SIGNER_SEED: &[u8] = b"clearinghouse";
pub const MAX_POSITIONS: usize = 4;

pub mod margin;

#[program]
pub mod mock_perps_engine {
    use super::*;

    /// Create the engine config and its collateral vault.
    pub fn init_engine(
        ctx: Context<InitEngine>,
        venue_id: [u8; 32],
        venue_index: u8,
        im_bps: u16,
        mm_bps: u16,
    ) -> Result<()> {
        require!(
            mm_bps > 0 && mm_bps <= im_bps && im_bps <= 10_000,
            EngineError::InvalidMarginRates
        );
        let c = &mut ctx.accounts.engine;
        c.admin = ctx.accounts.admin.key();
        c.venue_id = venue_id;
        c.venue_index = venue_index;
        c.im_bps = im_bps;
        c.mm_bps = mm_bps;
        c.bump = ctx.bumps.engine;
        ctx.accounts.vault.bump = ctx.bumps.vault;
        Ok(())
    }

    /// A1: which venues' reports this venue accepts credit from (admin only),
    /// by CPI signed with the venue PDA.
    pub fn set_acceptance(ctx: Context<SetAcceptance>, accepts_mask: u64) -> Result<()> {
        let bump = [ctx.bumps.venue_signer];
        let seeds: &[&[u8]] = &[VENUE_SIGNER_SEED, &bump];
        let signer = &[seeds];
        let a = &ctx.accounts;
        ch_core::cpi::set_venue_acceptance(
            CpiContext::new_with_signer(
                ch_core::ID,
                cha::SetVenueAcceptance {
                    config: a.ch_config.to_account_info(),
                    venue_registration: a.venue_registration.to_account_info(),
                    venue_authority: a.venue_signer.to_account_info(),
                },
                signer,
            ),
            accepts_mask,
        )
    }

    /// Deposit collateral (lamports, counted 1:1 as micro-USD).
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        let acct = &mut ctx.accounts.margin_account;
        if acct.user == Pubkey::default() {
            acct.user = ctx.accounts.user.key();
            acct.bump = ctx.bumps.margin_account;
        }
        system_program::transfer(
            CpiContext::new(
                system_program::ID,
                system_program::Transfer {
                    from: ctx.accounts.user.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;
        acct.collateral = acct
            .collateral
            .checked_add(amount)
            .ok_or(EngineError::MathOverflow)?;
        Ok(())
    }

    /// Withdraw: equity after the withdrawal must still cover Σ IM *without*
    /// credit (`ch_client::withdrawable_equity`): credit is never withdrawable.
    pub fn withdraw<'info>(ctx: Context<'info, Withdraw<'info>>, amount: u64) -> Result<()> {
        let engine = &ctx.accounts.engine;
        let acct = &mut ctx.accounts.margin_account;
        let (equity, im, _mm) = margin::account_requirements(acct, engine, ctx.remaining_accounts)?;
        let free = ch_client::withdrawable_equity(equity.max(0) as u64, im);
        require!(amount <= free, EngineError::InsufficientMargin);
        require!(amount <= acct.collateral, EngineError::InsufficientMargin);
        acct.collateral -= amount;
        ctx.accounts.vault.sub_lamports(amount)?;
        ctx.accounts.user.add_lamports(amount)?;
        Ok(())
    }

    /// Open or increase a position (new risk). Reports the resulting position
    /// to ch_core, optionally refreshes the portfolio in the same instruction
    /// (pass the refresh accounts as remaining_accounts: every registered
    /// snapshot, then one oracle per leg), then requires
    /// equity ≥ Σ max(IM_i − usable_credit_i, MM_i) on this venue.
    /// Without refresh accounts the credit is 0 (the report invalidated it).
    pub fn open_position<'info>(
        ctx: Context<'info, Trade<'info>>,
        asset: u8,
        is_long: bool,
        notional_delta: u64,
    ) -> Result<()> {
        require!(notional_delta > 0, EngineError::ZeroSize);
        let price = margin::read_mock_price(&ctx.accounts.oracle, asset)?;
        {
            let acct = &mut ctx.accounts.margin_account;
            require_keys_eq!(
                acct.user,
                ctx.accounts.user.key(),
                EngineError::WrongAccount
            );
            margin::increase(acct, asset, is_long, notional_delta, price)?;
        }
        report(ctx.accounts, asset, price)?;
        if !ctx.remaining_accounts.is_empty() {
            refresh(ctx.accounts, ctx.remaining_accounts)?;
        }
        let required = margin_with_credit(ctx.accounts, price)?;
        let acct = &ctx.accounts.margin_account;
        let equity = margin::equity_at(acct, asset, price)?;
        require!(equity >= required as i128, EngineError::InsufficientMargin);
        emit!(MarginChecked {
            user: acct.user,
            asset,
            equity: equity.max(0) as u64,
            required,
        });
        Ok(())
    }

    /// Reduce or close a position (risk-reducing: no margin check). Realized
    /// P&L moves into collateral; a closed position closes its snapshot.
    pub fn reduce_position<'info>(
        ctx: Context<'info, Trade<'info>>,
        asset: u8,
        notional_delta: u64,
    ) -> Result<()> {
        let price = margin::read_mock_price(&ctx.accounts.oracle, asset)?;
        let closed = {
            let acct = &mut ctx.accounts.margin_account;
            require_keys_eq!(
                acct.user,
                ctx.accounts.user.key(),
                EngineError::WrongAccount
            );
            margin::decrease(acct, asset, notional_delta, price)?
        };
        if closed {
            close_snapshot(ctx.accounts)?;
        } else {
            report(ctx.accounts, asset, price)?;
        }
        Ok(())
    }

    /// Permissionless liquidation of one position when equity < Σ MM. Uses
    /// the venue's maintenance margin only; Clearinghouse credit is never
    /// read here, so credit cannot prevent or cause a liquidation.
    pub fn liquidate<'info>(ctx: Context<'info, Liquidate<'info>>, asset: u8) -> Result<()> {
        let (equity, mm, user) = {
            let engine = &ctx.accounts.engine;
            let acct = &mut ctx.accounts.margin_account;
            let (equity, _im, mm) =
                margin::account_requirements(acct, engine, ctx.remaining_accounts)?;
            require!(equity < mm as i128, EngineError::NotLiquidatable);
            let price = margin::read_mock_price(&ctx.accounts.oracle, asset)?;
            let size = acct
                .positions
                .iter()
                .find(|p| p.active && p.asset == asset)
                .map(|p| margin::notional(p.size_e6, price))
                .ok_or(EngineError::NoPosition)?;
            margin::decrease(acct, asset, size, price)?;
            (equity, mm, acct.user)
        };
        let a = &ctx.accounts;
        let bump = [ctx.bumps.venue_signer];
        let seeds: &[&[u8]] = &[VENUE_SIGNER_SEED, &bump];
        let signer = &[seeds];
        ch_core::cpi::close_position_snapshot(CpiContext::new_with_signer(
            ch_core::ID,
            cha::ClosePositionSnapshot {
                venue_registration: a.venue_registration.to_account_info(),
                user_consent: a.user_consent.to_account_info(),
                portfolio: a.portfolio.to_account_info(),
                pos_snapshot: a.pos_snapshot.to_account_info(),
                rent_receiver: a.rent_receiver.to_account_info(),
                closer: a.venue_signer.to_account_info(),
            },
            signer,
        ))?;
        emit!(Liquidated {
            user,
            asset,
            equity_before: equity.max(0) as u64,
            maintenance: mm,
        });
        Ok(())
    }
}

/// CPI `submit_position_snapshot` with the position as it is now, the venue's
/// own IM/MM (never reduced by credit), and the oracle price as mark (A5d).
fn report<'info>(a: &Trade<'info>, asset: u8, price: u64) -> Result<()> {
    let (notional, is_long) = a
        .margin_account
        .positions
        .iter()
        .find(|p| p.active && p.asset == asset)
        .map(|p| (margin::notional(p.size_e6, price), p.is_long))
        .ok_or(EngineError::NoPosition)?;
    let im = margin::bps_ceil(notional, a.engine.im_bps);
    let mm = margin::bps_ceil(notional, a.engine.mm_bps).max(1);
    let bump = [a.engine_signer_bump()?];
    let seeds: &[&[u8]] = &[VENUE_SIGNER_SEED, &bump];
    let signer = &[seeds];
    ch_core::cpi::submit_position_snapshot(
        CpiContext::new_with_signer(
            ch_core::ID,
            cha::SubmitPositionSnapshot {
                config: a.ch_config.to_account_info(),
                venue_registration: a.venue_registration.to_account_info(),
                user_consent: a.user_consent.to_account_info(),
                portfolio: a.portfolio.to_account_info(),
                pos_snapshot: a.pos_snapshot.to_account_info(),
                user: a.user.to_account_info(),
                venue_authority: a.venue_signer.to_account_info(),
                payer: a.user.to_account_info(),
                system_program: a.system_program.to_account_info(),
                risk_model: a.risk_model.to_account_info(),
            },
            signer,
        )
        .with_remaining_accounts(vec![a.oracle.to_account_info()]),
        a.engine.venue_id,
        asset,
        notional,
        is_long,
        im.min(notional),
        mm.min(im),
        price,
    )
}

fn close_snapshot<'info>(a: &Trade<'info>) -> Result<()> {
    let bump = [a.engine_signer_bump()?];
    let seeds: &[&[u8]] = &[VENUE_SIGNER_SEED, &bump];
    let signer = &[seeds];
    ch_core::cpi::close_position_snapshot(CpiContext::new_with_signer(
        ch_core::ID,
        cha::ClosePositionSnapshot {
            venue_registration: a.venue_registration.to_account_info(),
            user_consent: a.user_consent.to_account_info(),
            portfolio: a.portfolio.to_account_info(),
            pos_snapshot: a.pos_snapshot.to_account_info(),
            rent_receiver: a.user.to_account_info(),
            closer: a.venue_signer.to_account_info(),
        },
        signer,
    ))
}

/// CPI `refresh_portfolio` (permissionless) with the caller's remaining accounts.
fn refresh<'info>(a: &Trade<'info>, remaining: &[AccountInfo<'info>]) -> Result<()> {
    ch_core::cpi::refresh_portfolio(
        CpiContext::new(
            ch_core::ID,
            cha::RefreshPortfolio {
                config: a.ch_config.to_account_info(),
                risk_model: a.risk_model.to_account_info(),
                user_consent: a.user_consent.to_account_info(),
                portfolio: a.portfolio.to_account_info(),
            },
        )
        .with_remaining_accounts(remaining.to_vec()),
    )
}

/// Σ over this venue's positions of max(IM_i − usable_credit_i, MM_i), with
/// the Clearinghouse accounts validated here (owner, PDA, discriminator) and
/// `ch_client::usable_credit` applied (status, kind, epoch, expiry, leg,
/// allowlist view). Prices: the traded asset at `traded_price`; other assets
/// at their entry price (this engine trades one asset per instruction).
fn margin_with_credit(a: &Trade, traded_price: u64) -> Result<u64> {
    let idx = a.engine.venue_index;
    let (config_epoch, accepts) = config_view(&a.ch_config, idx)?;
    let pf = portfolio_view(&a.portfolio, &a.user.key())?;
    let view = PortfolioView {
        status: pf.status,
        credit_kind: pf.credit_kind,
        risk_epoch: pf.risk_epoch,
        expires_slot: pf.expires_slot,
        legs: &pf.legs[..pf.num_legs],
    };
    let now = Clock::get()?.slot;
    let mut required = 0u64;
    for p in a.margin_account.positions.iter().filter(|p| p.active) {
        let price = if Some(p.asset) == a.traded_asset() {
            traded_price
        } else {
            p.entry_price
        };
        let n = margin::notional(p.size_e6, price);
        let im = margin::bps_ceil(n, a.engine.im_bps);
        let mm = margin::bps_ceil(n, a.engine.mm_bps);
        let credit = usable_credit(&view, config_epoch, accepts, now, idx, p.asset);
        required = required
            .checked_add(effective_initial_margin(im, mm, credit))
            .ok_or(EngineError::MathOverflow)?;
    }
    Ok(required)
}

/// The fields of a ch_core `Portfolio` the venue rule needs (kept small: the
/// full account is deserialized in its own stack frame).
struct PortfolioFields {
    status: u8,
    credit_kind: u8,
    risk_epoch: u64,
    expires_slot: u64,
    legs: [LegCredit; ch_core::MAX_LEGS],
    num_legs: usize,
}

/// (risk_epoch, this venue's allowlist) from a validated `GlobalConfig`.
#[inline(never)]
fn config_view(info: &AccountInfo, venue_index: u8) -> Result<(u64, u64)> {
    let cfg = Box::new(load_ch_config(info)?);
    Ok((cfg.risk_epoch, cfg.accepts_of(venue_index)))
}

#[inline(never)]
fn portfolio_view(info: &AccountInfo, user: &Pubkey) -> Result<PortfolioFields> {
    let pf = Box::new(load_ch_portfolio(info, user)?);
    let mut legs = [LegCredit {
        venue_index: 0,
        asset_id: 0,
        credit: 0,
        view_mask: 0,
    }; ch_core::MAX_LEGS];
    for (out, l) in legs.iter_mut().zip(&pf.legs[..pf.num_legs as usize]) {
        *out = LegCredit {
            venue_index: l.venue_index,
            asset_id: l.asset_id,
            credit: l.credit,
            view_mask: l.view_mask,
        };
    }
    Ok(PortfolioFields {
        status: pf.status,
        credit_kind: pf.credit_kind,
        risk_epoch: pf.risk_epoch,
        expires_slot: pf.expires_slot,
        legs,
        num_legs: pf.num_legs as usize,
    })
}

/// `GlobalConfig`: owned by ch_core, PDA `[b"config"]`, Anchor discriminator.
fn load_ch_config(info: &AccountInfo) -> Result<ch_core::GlobalConfig> {
    require_keys_eq!(*info.owner, ch_core::ID, EngineError::WrongAccount);
    let (pda, _) = Pubkey::find_program_address(&[ch_core::SEED_CONFIG], &ch_core::ID);
    require_keys_eq!(info.key(), pda, EngineError::WrongAccount);
    let data = info.try_borrow_data()?;
    ch_core::GlobalConfig::try_deserialize(&mut &data[..])
        .map_err(|_| error!(EngineError::WrongAccount))
}

/// `Portfolio`: owned by ch_core, PDA `[b"portfolio", user]`, discriminator.
fn load_ch_portfolio(info: &AccountInfo, user: &Pubkey) -> Result<ch_core::Portfolio> {
    require_keys_eq!(*info.owner, ch_core::ID, EngineError::WrongAccount);
    let (pda, _) =
        Pubkey::find_program_address(&[ch_core::SEED_PORTFOLIO, user.as_ref()], &ch_core::ID);
    require_keys_eq!(info.key(), pda, EngineError::WrongAccount);
    let data = info.try_borrow_data()?;
    ch_core::Portfolio::try_deserialize(&mut &data[..])
        .map_err(|_| error!(EngineError::WrongAccount))
}

// ----------------------------------------------------------------- accounts

#[account]
#[derive(InitSpace)]
pub struct EngineConfig {
    pub admin: Pubkey,
    pub venue_id: [u8; 32],
    pub venue_index: u8,
    pub im_bps: u16,
    pub mm_bps: u16,
    pub bump: u8,
}

/// Holds the collateral lamports (program-owned, so withdrawals can debit it).
#[account]
#[derive(InitSpace)]
pub struct Vault {
    pub bump: u8,
}

#[derive(
    AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq, InitSpace,
)]
pub struct Position {
    pub active: bool,
    pub asset: u8,
    pub is_long: bool,
    /// Size in base units × 1e6.
    pub size_e6: u64,
    /// Average entry price, micro-USD.
    pub entry_price: u64,
}

#[account]
#[derive(InitSpace)]
pub struct MarginAccount {
    pub user: Pubkey,
    /// Micro-USD (lamports 1:1, see the crate docs).
    pub collateral: u64,
    /// Losses beyond collateral when a liquidation closes at a loss.
    pub bad_debt: u64,
    pub positions: [Position; MAX_POSITIONS],
    pub bump: u8,
}

#[derive(Accounts)]
pub struct InitEngine<'info> {
    #[account(init, payer = admin, space = 8 + EngineConfig::INIT_SPACE, seeds = [ENGINE_SEED], bump)]
    pub engine: Account<'info, EngineConfig>,
    #[account(init, payer = admin, space = 8 + Vault::INIT_SPACE, seeds = [VAULT_SEED], bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetAcceptance<'info> {
    #[account(seeds = [ENGINE_SEED], bump = engine.bump, has_one = admin @ EngineError::Unauthorized)]
    pub engine: Account<'info, EngineConfig>,
    pub admin: Signer<'info>,
    /// CHECK: validated by ch_core.
    #[account(mut)]
    pub ch_config: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub venue_registration: UncheckedAccount<'info>,
    /// CHECK: this program's signer PDA.
    #[account(seeds = [VENUE_SIGNER_SEED], bump)]
    pub venue_signer: UncheckedAccount<'info>,
    /// CHECK: must be ch_core.
    #[account(address = ch_core::ID)]
    pub ch_core_program: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + MarginAccount::INIT_SPACE,
        seeds = [ACCOUNT_SEED, user.key().as_ref()],
        bump
    )]
    pub margin_account: Account<'info, MarginAccount>,
    #[account(mut, seeds = [VAULT_SEED], bump = vault.bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(seeds = [ENGINE_SEED], bump = engine.bump)]
    pub engine: Account<'info, EngineConfig>,
    #[account(mut, seeds = [ACCOUNT_SEED, user.key().as_ref()], bump = margin_account.bump)]
    pub margin_account: Account<'info, MarginAccount>,
    #[account(mut, seeds = [VAULT_SEED], bump = vault.bump)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub user: Signer<'info>,
}

/// open_position / reduce_position.
#[derive(Accounts)]
pub struct Trade<'info> {
    #[account(seeds = [ENGINE_SEED], bump = engine.bump)]
    pub engine: Account<'info, EngineConfig>,
    #[account(mut, seeds = [ACCOUNT_SEED, user.key().as_ref()], bump = margin_account.bump)]
    pub margin_account: Account<'info, MarginAccount>,
    #[account(mut)]
    pub user: Signer<'info>,
    /// CHECK: ch_core MockPrice for the traded asset; checked in read_mock_price.
    pub oracle: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core and by load_ch_config. Writable: the
    /// in-instruction refresh updates the outstanding-credit counters.
    #[account(mut)]
    pub ch_config: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub risk_model: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub venue_registration: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub user_consent: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core and by load_ch_portfolio.
    #[account(mut)]
    pub portfolio: UncheckedAccount<'info>,
    /// CHECK: created / validated by ch_core.
    #[account(mut)]
    pub pos_snapshot: UncheckedAccount<'info>,
    /// CHECK: this program's signer PDA.
    #[account(seeds = [VENUE_SIGNER_SEED], bump)]
    pub venue_signer: UncheckedAccount<'info>,
    /// CHECK: must be ch_core.
    #[account(address = ch_core::ID)]
    pub ch_core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

impl Trade<'_> {
    fn engine_signer_bump(&self) -> Result<u8> {
        Ok(Pubkey::find_program_address(&[VENUE_SIGNER_SEED], &crate::ID).1)
    }

    /// The asset whose MockPrice was passed (read back from the account).
    fn traded_asset(&self) -> Option<u8> {
        let data = self.oracle.try_borrow_data().ok()?;
        ch_core::MockPrice::try_deserialize(&mut &data[..])
            .ok()
            .map(|m| m.asset_id)
    }
}

#[derive(Accounts)]
pub struct Liquidate<'info> {
    #[account(seeds = [ENGINE_SEED], bump = engine.bump)]
    pub engine: Account<'info, EngineConfig>,
    #[account(mut, seeds = [ACCOUNT_SEED, margin_account.user.as_ref()], bump = margin_account.bump)]
    pub margin_account: Account<'info, MarginAccount>,
    pub liquidator: Signer<'info>,
    /// CHECK: ch_core MockPrice for the asset; checked in read_mock_price.
    pub oracle: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub venue_registration: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    pub user_consent: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    #[account(mut)]
    pub portfolio: UncheckedAccount<'info>,
    /// CHECK: validated by ch_core.
    #[account(mut)]
    pub pos_snapshot: UncheckedAccount<'info>,
    /// CHECK: must equal the snapshot's rent payer (ch_core checks it).
    #[account(mut)]
    pub rent_receiver: UncheckedAccount<'info>,
    /// CHECK: this program's signer PDA.
    #[account(seeds = [VENUE_SIGNER_SEED], bump)]
    pub venue_signer: UncheckedAccount<'info>,
    /// CHECK: must be ch_core.
    #[account(address = ch_core::ID)]
    pub ch_core_program: UncheckedAccount<'info>,
}

#[event]
pub struct MarginChecked {
    pub user: Pubkey,
    pub asset: u8,
    pub equity: u64,
    pub required: u64,
}

#[event]
pub struct Liquidated {
    pub user: Pubkey,
    pub asset: u8,
    pub equity_before: u64,
    pub maintenance: u64,
}

#[error_code]
pub enum EngineError {
    #[msg("Equity does not cover the margin requirement")]
    InsufficientMargin,
    #[msg("Account is not liquidatable (equity >= maintenance margin)")]
    NotLiquidatable,
    #[msg("Invalid margin rates")]
    InvalidMarginRates,
    #[msg("Size must be positive")]
    ZeroSize,
    #[msg("No open position in this asset")]
    NoPosition,
    #[msg("A position in the opposite direction is open; reduce it first")]
    SideMismatch,
    #[msg("Too many positions")]
    TooManyPositions,
    #[msg("Wrong account")]
    WrongAccount,
    #[msg("Wrong or invalid price account")]
    InvalidPrice,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
