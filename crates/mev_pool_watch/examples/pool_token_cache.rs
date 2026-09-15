use alloy::providers::ProviderBuilder;
use mev_pool_watch::PoolTokenCache;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let db_path = std::env::var("POOL_TOKEN_CACHE_DB_PATH")
        .unwrap_or_else(|_| "pool_token_cache.db".to_string());

    let cache = PoolTokenCache::open(db_path.as_str()).await?;

    let rpc_url = std::env::var("ETHEREUM_RPC_URL").expect("ETHEREUM RPC URL NOT FOUND!");
    let provider = ProviderBuilder::new().connect(&rpc_url).await?;
    let pool = "0x9c4fe5ffd9a9fc5678cfbd93aa2d4fd684b67c4c".parse()?;

    let result = cache.resolve(&provider, pool).await?;

    println!("Result: {:?}", result);

    return Ok(());
}
