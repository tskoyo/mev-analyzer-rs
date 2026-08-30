use alloy::primitives::Address;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PoolCandidate {
    pub symbol: String,
    pub blockchain: String,
    pub token_address: Address,
    pub pool_address: Address,
    pub pool_volume: f64,
    pub pool_traders: u64,
    pub pool_last_trade: String,
}
