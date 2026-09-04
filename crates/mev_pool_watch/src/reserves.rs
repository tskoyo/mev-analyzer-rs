use crate::pool::{PoolMeta, PoolSnapshot};
use alloy::eips::BlockId;
use alloy::providers::Provider;
use mev_core::IUniswapV2Pair;

/// Fetches a snapshot for every pool in `pools`, all pinned to the same
/// block number (fetched once, up front). A poll cycle's readings must
/// never mix blocks -- comparing reserves fetched at different blocks can
/// manufacture a gap that's just ordinary price movement between two RPC
/// calls, not a real disagreement between pools.
///
/// A single pool's fetch failing does not abort the batch -- one bad RPC
/// call or a pool that reverts on `token0()`/`token1()` shouldn't take down
/// every other candidate in the cycle.
pub async fn fetch_snapshots_at_current_block<P>(
    provider: &P,
    pools: &[PoolMeta],
) -> eyre::Result<Vec<eyre::Result<PoolSnapshot>>>
where
    P: Provider + Clone,
{
    let block_number = provider.get_block_number().await?;
    let mut results = Vec::with_capacity(pools.len());
    for pool in pools {
        results.push(fetch_one(provider, pool, block_number).await);
    }
    Ok(results)
}

async fn fetch_one<P>(
    provider: &P,
    pool: &PoolMeta,
    block_number: u64,
) -> eyre::Result<PoolSnapshot>
where
    P: Provider + Clone,
{
    let pair = IUniswapV2Pair::new(pool.address, provider);
    let block = BlockId::from(block_number);

    let reserves = pair.getReserves().block(block).call().await?;
    let token0 = pair.token0().block(block).call().await?;
    let token1 = pair.token1().block(block).call().await?;

    Ok(PoolSnapshot {
        meta: pool.clone(),
        token0,
        token1,
        reserve0: reserves.reserve0.to::<u128>(),
        reserve1: reserves.reserve1.to::<u128>(),
        block_number,
    })
}
