use std::collections::HashMap;

use alloy::primitives::{Address, I256};

use crate::transfers::Transfer;

pub fn net_balance_changes(transfers: &[Transfer], addr: Address) -> HashMap<Address, I256> {
    let mut net: HashMap<Address, I256> = HashMap::new();
    for t in transfers {
        let amt = I256::try_from(t.amount).unwrap_or(I256::ZERO);
        if t.from == addr {
            *net.entry(t.token).or_insert(I256::ZERO) -= amt;
        }
        if t.to == addr {
            *net.entry(t.token).or_insert(I256::ZERO) += amt;
        }
    }
    net
}
