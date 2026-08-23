use alloy::{
    consensus::Transaction,
    eips::BlockId,
    network::TransactionResponse,
    providers::{Provider, ProviderBuilder},
};
use anyhow::{Result, anyhow};
use revm::{
    Context, ExecuteCommitEvm, MainBuilder, MainContext,
    database::CacheDB,
    primitives::{Address, U256, address},
};
use revm::{context::TxEnv, database::WrapDatabaseAsync};
use revm::{
    database::AlloyDB,
    primitives::{TxKind, b256},
};

const BLOCK_NUMBER: u64 = 25759160 - 1;
const USDC: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

macro_rules! read_balance {
    ($evm:expr, $token:expr, $who:expr) => {{
        let mut data = vec![0x70u8, 0xa0, 0x82, 0x31]; // balanceOf(address)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice($who.as_slice());

        let call = TxEnv {
            caller: Address::ZERO,
            kind: TxKind::Call($token),
            data: data.into(),
            value: U256::ZERO,
            gas_limit: 200_000,
            gas_price: 0,
            ..Default::default()
        };

        let out = $evm.transact_commit(call)?; // note: transact, NOT transact_commit
        let bytes = out
            .output()
            .ok_or_else(|| anyhow!("balanceOf returned nothing"))?;
        U256::from_be_slice(bytes.as_ref())
    }};
}

#[tokio::main]
async fn main() -> Result<()> {
    let rpc_url = "https://mainnet.infura.io/v3/c60b0bb42f8a4c6481ecd229eddaca27";

    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    let block_number = BlockId::number(BLOCK_NUMBER);

    let tx_hash = b256!("b0a944ff492a157eef35d31fdceb9b680e3d25082cea9ba68cdbf54b0463af24");
    let tx = provider
        .get_transaction_by_hash(tx_hash)
        .await?
        .ok_or_else(|| anyhow!("tx not found"))?;

    let alloy_db = WrapDatabaseAsync::new(AlloyDB::new(provider, block_number))
        .ok_or_else(|| anyhow!("failed to build AlloyDB"))?;

    let cache_db = CacheDB::new(alloy_db);

    let mut context = Context::mainnet().with_db(cache_db);
    context.cfg.disable_nonce_check = true;
    let mut evm = context.build_mainnet();

    let arb_tx = TxEnv {
        caller: tx.from(),
        kind: TxKind::Call(tx.to().unwrap()),
        data: tx.input().clone(),
        value: tx.value(),
        gas_limit: tx.gas_limit(),
        gas_price: 0,
        ..Default::default()
    };

    let bot = tx.from();

    let before = read_balance!(evm, USDC, bot);
    let result = evm.transact_commit(arb_tx)?;
    let after = read_balance!(evm, USDC, bot);

    println!("success: {}", result.is_success());
    println!(
        "Profit: {} USDC",
        (after - before).to::<u128>() as f64 / 1e6
    );

    Ok(())
}
