use alloy::primitives::{Address, B256, U256, b256};
use revm::primitives::Log;

pub const TRANSFER_SIG: B256 =
    b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");

pub struct Transfer {
    pub idx: usize,
    pub token: Address,
    pub from: Address,
    pub to: Address,
    pub amount: U256,
}

pub fn decode_transfers(logs: &[Log]) -> Vec<Transfer> {
    let mut transfers = Vec::new();
    for (i, log) in logs.iter().enumerate() {
        let topics = log.topics();
        // ERC-20 Transfer event has 3 topics: signature, from, to
        if topics.first() == Some(&TRANSFER_SIG) && topics.len() == 3 {
            let from = Address::from_word(topics[1]);
            let to = Address::from_word(topics[2]);
            let amount = U256::from_be_slice(log.data.data.as_ref());
            transfers.push(Transfer {
                idx: i,
                token: log.address,
                from,
                to,
                amount,
            });
        }
    }
    transfers
}
