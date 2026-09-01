use alloy::primitives::Address;

/// Static metadata about a pool to poll, independent of where the address
/// came from (Dune, config, etc). `fee_bps` is the DEX's own swap fee
/// (e.g. 30 for a standard 0.30% Uniswap-V2-style pool) -- a caller-supplied
/// fact, not something this crate infers.
#[derive(Debug, Clone)]
pub struct PoolMeta {
    pub address: Address,
    pub dex: String,
    pub fee_bps: u32,
}

/// A pool's on-chain state as of one specific block. `token0`/`token1` are
/// always read live from the pool contract itself, never assumed from
/// config or a Dune symbol -- a pool's actual pairing is ground truth.
#[derive(Debug, Clone)]
pub struct PoolSnapshot {
    pub meta: PoolMeta,
    pub token0: Address,
    pub token1: Address,
    pub reserve0: u128,
    pub reserve1: u128,
    pub block_number: u64,
}

impl PoolSnapshot {
    /// True if `token` is one of this pool's two tokens.
    pub fn holds(&self, token: Address) -> bool {
        self.token0 == token || self.token1 == token
    }

    /// Reserve amount for `token`, if this pool holds it.
    pub fn reserve_of(&self, token: Address) -> Option<u128> {
        if token == self.token0 {
            Some(self.reserve0)
        } else if token == self.token1 {
            Some(self.reserve1)
        } else {
            None
        }
    }

    /// The token paired against `token` in this pool, if `token` is one of
    /// the two this pool actually holds.
    pub fn other_token(&self, token: Address) -> Option<Address> {
        if token == self.token0 {
            Some(self.token1)
        } else if token == self.token1 {
            Some(self.token0)
        } else {
            None
        }
    }
}
