// TODO(owner): Dev B
use anchor_lang::prelude::*;

declare_id!("s8B6EBe2kCoX7LMSYGW9MpouEjxxBkibyCWSEKGf4uh");

#[program]
pub mod mock_perps_a {
    use super::*;

    pub fn initialize(_ctx: Context<Initialize>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}

#[account]
#[derive(Default)]
pub struct PlaceholderMockPerpsAState {
    pub is_initialized: bool,
}
