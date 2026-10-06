// TODO(owner): Dev B
use anchor_lang::prelude::*;

declare_id!("9XiLmi2tEuxsaLzChVD8PT5csrZrapTphEdY8iUdwN58");

#[program]
pub mod mock_perps_b {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}

#[account]
#[derive(Default)]
pub struct PlaceholderMockPerpsBState {
    pub is_initialized: bool,
}
