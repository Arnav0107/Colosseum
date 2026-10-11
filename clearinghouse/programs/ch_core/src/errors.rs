use anchor_lang::prelude::*;

#[error_code]
pub enum ClearinghouseError {
    #[msg("Unauthorized access")]
    Unauthorized,
    #[msg("Venue is not active")]
    VenueInactive,
    #[msg("User consent missing or inactive")]
    UserConsentMissing,
    #[msg("Venue not authorized in user consent")]
    VenueNotAuthorized,
    #[msg("Invalid venue index")]
    InvalidVenueIndex,
    #[msg("Invalid asset ID")]
    InvalidAssetId,
    #[msg("Invalid correlation value")]
    InvalidCorrelationValue,
    #[msg("Risk model is stale")]
    RiskModelStale,
    #[msg("Price oracle is stale")]
    PriceStale,
    #[msg("Price oracle confidence interval too wide")]
    PriceUncertain,
    #[msg("Price cannot be zero or negative")]
    InvalidPrice,
    #[msg("Oracle backend not configured")]
    OracleNotConfigured,
    #[msg("Invalid oracle account")]
    InvalidOracleAccount,
    #[msg("Integer overflow in calculation")]
    MathOverflow,
    #[msg("Negative variance in portfolio netting")]
    NegativeVariance,
    #[msg("Snapshot is stale")]
    SnapshotStale,
    #[msg("Invalid snapshot account")]
    InvalidSnapshotAccount,
    #[msg("Snapshot notional cannot be zero")]
    ZeroNotional,
    #[msg("Margins must satisfy 0 < maintenance <= initial <= notional")]
    InvalidMargin,
    #[msg("Invalid configuration parameters")]
    InvalidConfigParams,
    #[msg("Venue index is already used")]
    VenueIndexAlreadyUsed,
    #[msg("Clearinghouse is paused")]
    ProgramPaused,
    #[msg("Invalid proposed admin address")]
    InvalidAdmin,
    #[msg("Portfolio already has the maximum number of legs")]
    PortfolioFull,
    #[msg("Duplicate leg (venue, asset) in portfolio")]
    DuplicateLeg,
    #[msg("Refresh must include exactly the portfolio's registered snapshots, then one oracle per leg")]
    IncompletePortfolio,
    #[msg("Too many legs")]
    TooManyLegs,
    #[msg("No invalidation condition holds")]
    InvalidationNotJustified,
    #[msg("Outstanding-credit counter would underflow")]
    CounterUnderflow,
    #[msg("Caller may not close this snapshot")]
    SnapshotCloseNotAllowed,
}

impl From<ch_math::portfolio::PortfolioError> for ClearinghouseError {
    fn from(e: ch_math::portfolio::PortfolioError) -> Self {
        use ch_math::portfolio::PortfolioError as P;
        match e {
            P::TooManyLegs => ClearinghouseError::TooManyLegs,
            P::DuplicateLeg => ClearinghouseError::DuplicateLeg,
            P::InvalidAsset => ClearinghouseError::InvalidAssetId,
            P::ZeroNotional => ClearinghouseError::ZeroNotional,
            P::InvalidMargin => ClearinghouseError::InvalidMargin,
            P::NegativeVariance => ClearinghouseError::NegativeVariance,
            P::Overflow => ClearinghouseError::MathOverflow,
        }
    }
}
