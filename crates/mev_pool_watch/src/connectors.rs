use crate::grouping::TokenPairKey;
use alloy::primitives::Address;
use std::collections::HashMap;

/// Known high-liquidity pools between major "quote" tokens (e.g. WBNB/BUSD),
/// used to close a 3-leg loop when two pools for the same target token are
/// paired against different quote tokens. Deliberately not baked into this
/// crate: these are real, fund-relevant addresses that must be verified by
/// the caller (Etherscan/BscScan, DeFiLlama, etc), not guessed -- see
/// ARCHITECTURE.md's "initial chain scope" and "multi-chain connector list"
/// (out of scope) notes. Scoped to whatever single chain a given poll cycle
/// targets; add a chain dimension before scanning more than one at once.
pub type ConnectorPools = HashMap<TokenPairKey, Address>;

/// A 3-leg candidate is only ever built when a connector for its quote-token
/// pair is present in this map -- if it's empty, `detect_candidates` simply
/// never emits 3-leg candidates, rather than fabricating a gap through an
/// external price feed.
pub fn empty_connectors() -> ConnectorPools {
    HashMap::new()
}
