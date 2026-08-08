use std::collections::HashMap;

use alloy::primitives::{Address, I256};

use crate::transfers::Transfer;

pub fn net_balance_changes(transfers: &[Transfer], addr: Address) -> HashMap<Address, I256> {
    let mut net: HashMap<Address, I256> = HashMap::new();
    for t in transfers {
        if t.from != addr && t.to != addr {
            continue;
        }
        let amt = I256::try_from(t.amount).unwrap_or(I256::ZERO);
        if t.from == addr {
            *net.entry(t.token).or_default() -= amt;
        }
        if t.to == addr {
            *net.entry(t.token).or_default() += amt;
        }
    }
    net
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{U256, address};

    fn transfer(token: Address, from: Address, to: Address, amount: u64) -> Transfer {
        Transfer {
            idx: 0,
            token,
            from,
            to,
            amount: U256::from(amount),
        }
    }

    #[test]
    fn nets_incoming_and_outgoing_per_token() {
        let addr = address!("0000000000000000000000000000000000000001");
        let other = address!("0000000000000000000000000000000000000002");
        let token_a = address!("00000000000000000000000000000000000000aa");
        let token_b = address!("00000000000000000000000000000000000000bb");

        let transfers = vec![
            transfer(token_a, other, addr, 100),  // +100 A
            transfer(token_a, addr, other, 30),   // -30 A
            transfer(token_b, addr, other, 5),    // -5 B
            transfer(token_a, other, other, 999), // unrelated, ignored
        ];

        let net = net_balance_changes(&transfers, addr);

        assert_eq!(net.get(&token_a), Some(&I256::try_from(70).unwrap()));
        assert_eq!(net.get(&token_b), Some(&I256::try_from(-5).unwrap()));
        assert_eq!(net.len(), 2);
    }

    #[test]
    fn self_transfer_nets_to_zero() {
        let addr = address!("0000000000000000000000000000000000000001");
        let token = address!("00000000000000000000000000000000000000aa");

        let transfers = vec![transfer(token, addr, addr, 42)];

        let net = net_balance_changes(&transfers, addr);

        assert_eq!(net.get(&token), Some(&I256::ZERO));
    }
}
