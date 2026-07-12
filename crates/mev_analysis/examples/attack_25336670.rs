use std::collections::HashMap;

use alloy::eips::{BlockId, BlockNumberOrTag};
use alloy::network::TransactionResponse;
use alloy::primitives::utils::format_units;
use alloy::primitives::{Address, B256};
use alloy::providers::{Provider, ProviderBuilder};
use dotenv::dotenv;
use eyre::{Result, eyre};
use mev_analysis::{decode_transfers, fetch_token_meta, net_balance_changes};
use mev_core::{block_env_from_rpc, tx_env_from_rpc};
use revm::context::ContextTr;
use revm::database::{AlloyDB, CacheDB};
use revm::database_interface::{DatabaseCommit, WrapDatabaseAsync};
use revm::{Context, ExecuteEvm, MainBuilder, MainContext};

const TARGET_TX: &str = "0x27f702b2f5d713ef57c76c4242e29eb550ee8f3fb6661295a144f1d3c7e1158f";
const TARGET_BLOCK: u64 = 25336670;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let rpc_url = std::env::var("ALCHEMY_RPC_URL").expect("ALCHEMY_RPC_URL must be set in .env");
    let provider = ProviderBuilder::new().connect(&rpc_url).await?;
    let bot: Address = "0xE08D97e151473A848C3d9CA3f323Cb720472D015".parse()?;

    // Pull the full block: we need every preceding tx to replay (in case any
    // of them moved the pools our target tx trades against) and the header
    // fields BlockEnv needs (basefee, timestamp, coinbase, ...).
    let block = provider
        .get_block_by_number(BlockNumberOrTag::Number(TARGET_BLOCK))
        .full()
        .await?
        .ok_or_else(|| eyre!("block {TARGET_BLOCK} not found"))?;

    let target_hash: B256 = TARGET_TX
        .parse()
        .map_err(|_| eyre!("invalid target tx hash {TARGET_TX}"))?;

    let txs = block
        .transactions
        .as_transactions()
        .ok_or_else(|| eyre!("block was not fetched with full transactions"))?;

    let target_index = txs
        .iter()
        .position(|tx| tx.tx_hash() == target_hash)
        .ok_or_else(|| eyre!("tx {TARGET_TX} not found in block {TARGET_BLOCK}"))?;

    println!(
        "{target_hash} is #{target_index} of {} txs in block {TARGET_BLOCK}",
        txs.len()
    );

    // eth_getStorageAt/eth_getBalance at block N return state AFTER block N
    // fully executed. Since our target tx is IN block N, we fork one block
    // earlier to get the state right before block N's first tx runs.
    let fork_point = BlockId::number(TARGET_BLOCK - 1);
    let alloy_db = WrapDatabaseAsync::new(AlloyDB::new(provider, fork_point))
        .ok_or_else(|| eyre!("failed to build AlloyDB"))?;
    let cache_db = CacheDB::new(alloy_db);

    let mut evm = Context::mainnet()
        .with_db(cache_db)
        .modify_block_chained(|b| *b = block_env_from_rpc(&block))
        .build_mainnet();

    // Replay every tx ahead of ours in the block, folding each one's state
    // changes back into the shared CacheDB before moving to the next, so the
    // pools our target tx touches are in their exact pre-attack state.
    for (i, tx) in txs[..target_index].iter().enumerate() {
        let result = evm.transact(tx_env_from_rpc(tx))?;
        if !result.result.is_success() {
            println!(
                "warning: preceding tx #{i} ({:?}) did not succeed",
                tx.tx_hash()
            );
        }
        evm.db_mut().commit(result.state);
    }

    // Now execute the attack tx itself on top of the replayed state.
    let target_tx = &txs[target_index];
    let result = evm.transact(tx_env_from_rpc(target_tx))?;

    println!("--- execution result ---");
    println!("success: {}", result.result.is_success());
    println!("gas used: {}", result.result.tx_gas_used());
    println!("logs: {}", result.result.logs().len());

    let transfers = decode_transfers(result.result.logs());
    drop(result);

    let net = net_balance_changes(&transfers, bot);

    let mut meta: HashMap<Address, (String, u8)> = HashMap::new();
    for t in &transfers {
        if meta.contains_key(&t.token) {
            continue;
        }
        let token = t.token;
        let (symbol, decimals) =
            fetch_token_meta(|tx| evm.transact(tx).ok().map(|r| r.result), token);
        meta.insert(token, (symbol, decimals));
    }

    // --- print with names + human-readable amounts ---
    println!("--- ERC-20 transfers ---");
    for t in &transfers {
        let (symbol, decimals) = &meta[&t.token];
        let human = format_units(t.amount, *decimals).unwrap_or_else(|_| t.amount.to_string());
        println!(
            "#{:>2}  {} -> {}  {} {:>2}",
            t.idx, t.from, t.to, human, symbol
        );
    }

    println!("--- net balance change for bot {bot} ---");
    for (token, delta) in &net {
        let (symbol, decimals) = &meta[token];
        // format_units works on unsigned; handle the sign yourself
        let sign = if delta.is_negative() { "-" } else { "+" };
        let mag = delta.unsigned_abs();
        let human = format_units(mag, *decimals).unwrap_or_else(|_| mag.to_string());
        println!("  {sign}{human} {symbol}");
    }

    Ok(())
}
