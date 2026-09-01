use crate::pool::PoolSnapshot;
use alloy::primitives::Address;
use std::collections::HashMap;

/// An unordered token pair, used as a grouping key. Two pools with the same
/// key hold the exact same pair and can be compared directly (a 2-leg
/// loop); pools with different keys need a connecting pool (a 3-leg loop).
///
/// Keyed strictly on token *address*, never on a display symbol -- a
/// high pool count for a symbol is a red flag, not a good sign, unless it
/// resolves to a single contract address. Copycat/scam tokens sharing a
/// symbol must never collapse into the same group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenPairKey(Address, Address);

impl TokenPairKey {
    pub fn new(a: Address, b: Address) -> Self {
        if a < b { Self(a, b) } else { Self(b, a) }
    }
}

/// Groups snapshots by the actual on-chain token pair they hold.
pub fn group_by_pair(snapshots: &[PoolSnapshot]) -> HashMap<TokenPairKey, Vec<PoolSnapshot>> {
    let mut groups: HashMap<TokenPairKey, Vec<PoolSnapshot>> = HashMap::new();
    for snap in snapshots {
        let key = TokenPairKey::new(snap.token0, snap.token1);
        groups.entry(key).or_default().push(snap.clone());
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    fn snapshot(pool: Address, token0: Address, token1: Address) -> PoolSnapshot {
        PoolSnapshot {
            meta: crate::pool::PoolMeta {
                address: pool,
                dex: "test".to_string(),
                fee_bps: 30,
            },
            token0,
            token1,
            reserve0: 1_000,
            reserve1: 1_000,
            block_number: 1,
        }
    }

    #[test]
    fn same_symbol_different_token_address_lands_in_different_groups() {
        // Two pools that might share a display symbol (e.g. both call
        // themselves "MEOW") but are backed by different contracts must
        // never be grouped together -- that's the copycat-token trap.
        let real_meow = address!("0000000000000000000000000000000000000001");
        let scam_meow = address!("0000000000000000000000000000000000000002");
        let weth = address!("00000000000000000000000000000000000003ee");

        let pool_a = address!("0000000000000000000000000000000000000a01");
        let pool_b = address!("0000000000000000000000000000000000000b02");

        let snaps = vec![
            snapshot(pool_a, real_meow, weth),
            snapshot(pool_b, scam_meow, weth),
        ];

        let groups = group_by_pair(&snaps);
        assert_eq!(groups.len(), 2, "different token addresses must not merge into one group");
    }

    #[test]
    fn same_pair_different_pools_land_in_the_same_group() {
        let token_a = address!("0000000000000000000000000000000000000001");
        let token_b = address!("0000000000000000000000000000000000000002");
        let pool_1 = address!("0000000000000000000000000000000000000a01");
        let pool_2 = address!("0000000000000000000000000000000000000b02");

        let snaps = vec![
            snapshot(pool_1, token_a, token_b),
            snapshot(pool_2, token_b, token_a), // reversed order, still same pair
        ];

        let groups = group_by_pair(&snaps);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.values().next().unwrap().len(), 2);
    }
}
