# Venue integration

How a perpetuals venue (an on-chain program) reports positions to
`ch_core` and applies credit. Read [`THREAT_MODEL.md`](THREAT_MODEL.md) first.
You rely only on the venues **you** allowlist (section 2a): by default your
users' credit is computed from your own reports alone.

## 1. Registration (governance side)

The protocol's `venue_manager` calls `register_venue(venue_id, your_program_id,
venue_authority, venue_index)`. In production builds (`venue-cpi`):

- `venue_authority` **must** be `PDA([b"clearinghouse"], your_program_id)`.
  No private key exists for it, so only your program can sign reports.
- The venue starts **inactive**. After `timelock_slots` (≥ 216,000, about a
  day), the venue manager calls `activate_venue`. Activation and deactivation
  both bump the global `risk_epoch` (all existing credit becomes unusable and
  must be refreshed).
- A `venue_id` and a `venue_index` can be registered only once, ever.

## 2. Reporting positions (your program, by CPI)

Call `ch_core::submit_position_snapshot` from your program whenever a user's
position in an asset changes, signed with your `[b"clearinghouse"]` PDA.
The reference is `programs/mock_perps_a/src/lib.rs` (mock, ~120 lines).

| # | Account | Writable | Signer |
| --- | --- | --- | --- |
| 0 | `config` = PDA `[b"config"]` | | |
| 1 | `venue_registration` = PDA `[b"venue", venue_id]` | | |
| 2 | `user_consent` = PDA `[b"consent", user]` | | |
| 3 | `portfolio` = PDA `[b"portfolio", user]` | ✓ | |
| 4 | `pos_snapshot` = PDA `[b"snapshot", user, venue_id, [asset_id]]` | ✓ | |
| 5 | `user` | | |
| 6 | `venue_authority` = your `[b"clearinghouse"]` PDA | | ✓ (`invoke_signed`) |
| 7 | `payer` (pays snapshot rent the first time; gets it back on close) | ✓ | ✓ |
| 8 | system program | | |
| 9 | `risk_model` = PDA `[b"risk_model"]` (A5d: oracle feed source) | | |
| 10 | *(optional)* the asset's oracle account, only when you send a mark price | | |

Data: discriminator `[232, 58, 29, 242, 23, 228, 111, 118]` followed by Borsh
`(venue_id: [u8;32], asset_id: u8, notional: u64, is_long: bool,
initial_margin: u64, maintenance_margin: u64, mark_price_micro: u64)`, all
amounts in micro-USD. `mark_price_micro = 0` means "no mark"; otherwise pass
the asset's oracle account (Pyth `PriceUpdateV2` for the governed feed) as
account 10. The mark must be within `max_move_bps` of it.

Rules the program enforces (the report fails otherwise):
- the venue is active, and the user's consent is active and includes your
  venue index;
- `notional > 0` and `0 < MM ≤ IM ≤ notional`. A closed position is **not**
  reported as zero: close the snapshot (section 4);
- at most 8 legs per user across all venues, and asset ids < 32 with a configured
  risk model (otherwise refresh fails);
- A5 sanity bounds: `notional ≤ max_leg_notional`; this user's total notional
  on your venue ≤ `max_venue_notional_per_user`; growth within a window ≤
  `max(base · (1 + max_notional_growth_bps), base + notional_growth_floor)`
  (a new position starts from base 0). **A report that breaks a bound fails,
  and with it your transaction.** Size these parameters for your markets
  with governance, and handle the error (for example, by splitting a large
  increase across windows).

Every report invalidates the user's credit **in the same instruction**. Report
the requirement you would charge *without* credit: IM and MM are your own
numbers.

## 2a. Choosing whose reports you accept (A1)

Call `ch_core::set_venue_acceptance(accepts_mask)` from your program, signed
by the same `[b"clearinghouse"]` PDA (accounts: `config` writable, your
`venue_registration`, your PDA as signer; discriminator
`[189, 181, 155, 149, 251, 202, 67, 39]`, data `u64` mask). Bit `j` = venue
index `j`. Your own index is always implied. Gate this behind your own
governance.

- **Default 0:** you accept only your own reports, so a hedge held on another
  venue earns no credit at your venue.
- Adding a venue takes effect at the user's next refresh. Removing one makes
  every credit that used it unusable **immediately** (`usable_credit`
  checks each leg's `view_mask` against your current list).
- Your credit for a leg is never more than what your accepted venues' legs
  alone justify, and never more than the joint computation over all legs.

## 3. Applying credit

Before accepting a risk-increasing action (opening or increasing a position,
withdrawing), read the user's `Portfolio` and the `GlobalConfig`, and verify
both accounts yourself:
- owner == `ch_core` program id;
- address == PDA `[b"portfolio", user]` / `[b"config"]`;
- Anchor discriminator.

Then use `crates/ch_client`:

```rust
use ch_client::{usable_credit, effective_initial_margin, withdrawable_equity, PortfolioView};

let credit = usable_credit(
    &view,
    config.risk_epoch,
    config.venue_accepts[my_venue_index as usize], // A1: your current allowlist
    clock.slot,
    my_venue_index,
    asset_id,
);
let required_im = effective_initial_margin(my_im, my_mm, credit); // max(IM − credit, MM)
// Liquidation: use your own MM, untouched by credit.
// Withdrawals: withdrawable_equity(equity, my_im_without_credit). Credit is never withdrawable.
```

`usable_credit` returns 0 unless the portfolio is Active, the credit kind is
TradingPowerOnly, `portfolio.risk_epoch == config.risk_epoch`, the
current slot is ≤ `expires_slot`, and the leg's `view_mask` only contains
venues you still accept. Fill `LegCredit.view_mask` from the portfolio's
`legs[i].view_mask`. Do not implement your own variant.

Do **not**:
- apply credit to maintenance margin or liquidation thresholds;
- count credit as collateral or as withdrawable equity;
- cache `(portfolio.version, credit)` across slots. Read it live. The
  version restarts if a user closes and re-creates their accounts.

When credit disappears (input change, expiry, epoch bump), the user's
requirement goes back to your own IM. Because credit never took it below MM,
losing credit alone never makes an account liquidatable. It only blocks
further risk-increasing actions until the user adds margin or credit is
refreshed.

## 4. Closing positions

When a position is fully closed, call `close_position_snapshot` (signed by
your PDA): accounts `venue_registration`, `user_consent`, `portfolio`,
`pos_snapshot`, `rent_receiver` (= the snapshot's `rent_payer`), `closer`.
This unregisters the leg and invalidates credit. A user can close a
snapshot themselves only after revoking consent for your venue or after the
venue is deactivated.

## 5. Keeping credit fresh

Credit lives at most `credit_ttl_slots` (≤ 3,000 slots). Anyone can call
`refresh_portfolio`. You, the user, or a keeper (`services/keeper`) can pay
for it. A venue that wants its users to keep credit should run or fund a
keeper. A failed refresh is safe: credit simply stays unusable.

## 6. Checklist

- [ ] Registered with your program's `[b"clearinghouse"]` PDA as authority.
- [ ] Allowlist (`set_venue_acceptance`) set deliberately, behind your own governance.
- [ ] Report on every change, and close on full exit. Never report zero.
- [ ] Report IM and MM without credit.
- [ ] Verify the owner, PDA and discriminator of `Portfolio` and `GlobalConfig`
      before reading them.
- [ ] `usable_credit` + `effective_initial_margin` for opening risk only.
- [ ] MM and liquidation untouched; credit never withdrawable.
- [ ] No caching across slots or versions.
- [ ] A plan for when credit disappears (it will, on any input change or epoch bump).
