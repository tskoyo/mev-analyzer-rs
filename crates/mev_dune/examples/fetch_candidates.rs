use mev_dune::{CandidateQueryParams, DuneClient, PoolCandidate};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();

    let query_id: u64 = std::env::var("DUNE_QUERY_ID")
        .map_err(|_| eyre::eyre!("set DUNE_QUERY_ID to the saved query's numeric id"))?
        .parse()?;

    let client = DuneClient::from_env()?;
    let params = CandidateQueryParams::default().to_query_parameters();

    println!("running dune query {query_id}...");
    let candidates: Vec<PoolCandidate> = client.run_query_with_params(query_id, &params).await?;

    println!("{} pool candidates:\n", candidates.len());
    for c in &candidates {
        println!(
            "{:>8} {:<10} pool={} token={} vol=${:.0} traders={}",
            c.symbol, c.blockchain, c.pool_address, c.token_address, c.pool_volume, c.pool_traders
        );
    }

    Ok(())
}
