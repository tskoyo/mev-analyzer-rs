//! Bridges Stage 1 (Dune) into Stage 2 (live RPC confirmation): fetches
//! candidate pools from Dune, polls their live reserves atomically per
//! block, and runs 2-leg/3-leg loop detection + profitability math.
//!
//! Generalized replacement for the old hardcoded-two-pool prototype
//! (mev_analysis/examples/arbitrage.rs) -- pool addresses now come from a
//! real Dune query instead of two `const Address`.
//!
//! Requires DUNE_API_KEY, DUNE_QUERY_ID, and BNB_RPC_URL (Stage 1's initial
//! chain scope -- see ARCHITECTURE.md) in the environment or a .env file.
//!
//! No connector pools are configured below, so this run will only ever
//! surface 2-leg candidates -- see `ConnectorPools`/ARCHITECTURE.md for why
//! those addresses must be verified by hand, not guessed.

use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use mev_dune::{CandidateQueryParams, DuneClient, PoolCandidate};
use mev_pool_watch::{
    ConnectorPools, PoolMeta, detect_candidates, fetch_decimals_for,
    fetch_snapshots_at_current_block,
};
use std::collections::HashMap;

const CHAIN: &str = "bnb";

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();

    let query_id: u64 = std::env::var("DUNE_QUERY_ID")
        .map_err(|_| eyre::eyre!("set DUNE_QUERY_ID to the saved query's numeric id"))?
        .parse()?;
    let rpc_url = std::env::var("BNB_RPC_URL")
        .map_err(|_| eyre::eyre!("set BNB_RPC_URL, e.g. https://bsc-dataseed.binance.org"))?;

    let dune = DuneClient::from_env()?;
    let params = CandidateQueryParams::default().to_query_parameters();

    println!("fetching stage 1 candidates from dune...");
    let candidates: Vec<PoolCandidate> = dune.run_query_with_params(query_id, &params).await?;

    let candidates: Vec<PoolCandidate> = candidates
        .into_iter()
        .filter(|c| c.blockchain == CHAIN)
        .collect();
    println!("{} candidates on {CHAIN}\n", candidates.len());

    let provider = ProviderBuilder::new().connect_http(rpc_url.parse()?);

    // Key on token_address, not symbol -- Dune's distinct_addresses = 1
    // filter already guarantees one contract per symbol/chain, but keying
    // on the address here costs nothing and removes any doubt.
    let mut by_token: HashMap<Address, Vec<PoolCandidate>> = HashMap::new();
    for c in candidates {
        by_token.entry(c.token_address).or_default().push(c);
    }

    let mut any_candidates = false;

    for (token, rows) in &by_token {
        if rows.len() < 2 {
            continue; // need at least two pools to compare
        }

        let pools: Vec<PoolMeta> = rows
            .iter()
            .map(|r| PoolMeta {
                address: r.pool_address,
                // Dune's output doesn't carry the DEX/project name yet
                // (dune/candidates.sql's final SELECT would need a `project`
                // column added), so fee_bps is a flat 30 (0.30%) guess for
                // every pool here -- fine for Stage 2's approximate model,
                // but a real per-DEX fee lookup is a worthwhile follow-up.
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

        // No connectors configured -- see module doc. Extend this map with
        // verified addresses to enable 3-leg detection.
        let connectors = ConnectorPools::new();

        let symbol = &rows[0].symbol;
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

    if !any_candidates {
        println!("no candidates detected this cycle");
    }

    Ok(())
}
