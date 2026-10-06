// TODO(owner): Dev B
use anchor_lang::prelude::*;

declare_id!("5ntdRBb96dEHJDpBi4RwEjLK1RFxXuTpGQLj2YB5jKTR");

#[program]
pub mod ch_fund {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}

#[account]
#[derive(Default)]
pub struct PlaceholderFundState {
    pub is_initialized: bool,
}
