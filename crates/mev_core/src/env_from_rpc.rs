use alloy::consensus::Transaction;
use alloy::network::TransactionResponse;
use alloy::rpc::types::{Block, Transaction as RpcTransaction};
use revm::context::{BlockEnv, TxEnv};
use revm::context_interface::block::BlobExcessGasAndPrice;
use revm::primitives::{TxKind, U256};

pub fn block_env_from_rpc(block: &Block) -> BlockEnv {
    let header = &block.header;

    let blob_fee = header.blob_fee().unwrap_or_default() as u64;
    let blob_excess_gas_and_price = header
        .excess_blob_gas
        .map(|excess_blob_gas| BlobExcessGasAndPrice::new(excess_blob_gas, blob_fee));

    BlockEnv {
        number: U256::from(header.number),
        beneficiary: header.beneficiary,
        timestamp: U256::from(header.timestamp),
        gas_limit: header.gas_limit,
        basefee: header.base_fee_per_gas.unwrap_or_default(),
        prevrandao: Some(header.mix_hash),
        blob_excess_gas_and_price: blob_excess_gas_and_price,
        ..Default::default()
    }
}

pub fn tx_env_from_rpc(tx: &RpcTransaction) -> TxEnv {
    let kind = match tx.to() {
        Some(addr) => TxKind::Call(addr),
        None => TxKind::Create,
    };

    TxEnv {
        caller: tx.from(),
        kind,
        data: tx.input().clone(),
        value: tx.value(),
        gas_limit: tx.gas_limit(),
        gas_price: Transaction::gas_price(tx).unwrap_or_else(|| Transaction::max_fee_per_gas(tx)),
        gas_priority_fee: tx.max_priority_fee_per_gas(),
        nonce: tx.nonce(),
        chain_id: tx.chain_id(),
        access_list: tx.access_list().cloned().unwrap_or_default(),
        ..Default::default()
    }
}
