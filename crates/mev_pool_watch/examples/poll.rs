//! Bridges Stage 1 (Dune) into Stage 2 (live RPC confirmation): fetches
//! candidate pools from Dune across every chain it returns, and for each
//! chain you've configured an RPC endpoint for, polls live reserves
//! atomically per block and runs 2-leg/3-leg loop detection + profitability
//! math. Chains without a configured RPC are skipped, not silently dropped
//! or defaulted to some other chain -- a pool's reserves can only ever be
//! read from the chain it actually lives on.
//!
//! Generalized replacement for the old hardcoded-two-pool prototype
//! (mev_analysis/examples/arbitrage.rs) -- pool addresses now come from a
//! real Dune query instead of two `const Address`, and RPC dispatch follows
//! Dune's own `blockchain` column per candidate instead of one hardcoded
//! chain.
//!
//! Requires DUNE_API_KEY, DUNE_QUERY_ID, and one `{CHAIN}_RPC_URL` per chain
//! you want covered (e.g. `BNB_RPC_URL`, `ETHEREUM_RPC_URL` -- uppercased
//! from Dune's `blockchain` value) in the environment or a .env file.
//!
//! No connector pools are configured below, so this run will only ever
//! surface 2-leg candidates -- see `ConnectorPools`/ARCHITECTURE.md for why
//! those addresses must be verified by hand, not guessed, and note that a
//! connector is inherently chain-specific (WBNB/BUSD only means something
//! on bnb, WETH/USDC only on ethereum, etc).

use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use mev_dune::{CandidateQueryParams, DuneClient, PoolCandidate};
use mev_pool_watch::{
    ConnectorPools, PoolMeta, detect_candidates, fetch_decimals_for,
    fetch_snapshots_at_current_block,
};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();

    let query_id: u64 = std::env::var("DUNE_QUERY_ID")
        .map_err(|_| eyre::eyre!("set DUNE_QUERY_ID to the saved query's numeric id"))?
        .parse()?;

    let dune = DuneClient::from_env()?;
    let params = CandidateQueryParams::default().to_query_parameters();

    println!("fetching stage 1 candidates from dune...");
    let time_since_started = std::time::Instant::now();
    let candidates: Vec<PoolCandidate> = dune.run_query_with_params(query_id, &params).await?;
    let elapsed = time_since_started.elapsed();

    println!(
        "fetched {} candidates from dune in {:.2?}",
        candidates.len(),
        elapsed
    );
    println!("{} candidates across all chains\n", candidates.len());

    let mut by_chain: HashMap<String, Vec<PoolCandidate>> = HashMap::new();
    for c in candidates {
        by_chain.entry(c.blockchain.clone()).or_default().push(c);
    }

    let mut any_candidates = false;

    for (chain, rows) in &by_chain {
        let env_var = format!("{}_RPC_URL", chain.to_uppercase());
        let rpc_url = match std::env::var(&env_var) {
            Ok(url) => url,
            Err(_) => {
                println!(
                    "skipping {chain} ({} candidates): {env_var} not set",
                    rows.len()
                );
                continue;
            }
        };

        println!("--- {chain} ({} candidates) ---", rows.len());
        let provider = ProviderBuilder::new().connect_http(rpc_url.parse()?);
        println!("  connected to {chain} RPC, polling live reserves...");

        let mut by_token: HashMap<Address, Vec<&PoolCandidate>> = HashMap::new();
        for c in rows {
            by_token.entry(c.token_address).or_default().push(c);
        }

        for (token, token_rows) in &by_token {
            if token_rows.len() < 2 {
                continue; // need at least two pools to compare
            }

            let pools: Vec<PoolMeta> = token_rows
                .iter()
                .map(|r| PoolMeta {
                    address: r.pool_address,
                    dex: "unknown".to_string(),
                    fee_bps: 30,
                })
                .collect();

            let results = fetch_snapshots_at_current_block(&provider, &pools).await?;
            let snapshots: Vec<_> = results
                .into_iter()
                .filter_map(|r| match r {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("  pool fetch failed: {e}");
                        None
                    }
                })
                .collect();

            if snapshots.len() < 2 {
                continue;
            }

            let mut tokens: Vec<Address> = snapshots
                .iter()
                .flat_map(|s| [s.token0, s.token1])
                .collect();
            tokens.sort();
            tokens.dedup();
            let decimals = fetch_decimals_for(&provider, &tokens).await?;

            let connectors = connectors_for_chain(chain);

            let symbol = &token_rows[0].symbol;
            let found = detect_candidates(*token, &snapshots, &decimals, &connectors, 0.0);

            if found.is_empty() {
                continue;
            }
            any_candidates = true;

            println!("{symbol} ({token}):");
            for c in &found {
                println!(
                    "  {:?} gap={:.1}bps opt_in={:.6} gross={:.6} net={:.6} block={}",
                    c.loop_kind,
                    c.gap_bps,
                    c.estimated_optimal_input,
                    c.estimated_gross_profit,
                    c.estimated_net_profit,
                    c.block_number
                );
            }
        }
    }

    if !any_candidates {
        println!("\nno candidates detected this cycle");
    }

    Ok(())
}

/// Connector pools are inherently chain-specific -- a WBNB/BUSD address
/// means nothing on ethereum, and vice versa. None are populated yet (see
/// ARCHITECTURE.md's "multi-chain connector list" out-of-scope note); add
/// verified addresses per chain here once confirmed.
fn connectors_for_chain(_chain: &str) -> ConnectorPools {
    ConnectorPools::new()
}
