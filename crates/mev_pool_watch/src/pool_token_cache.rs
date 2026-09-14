use alloy::primitives::Address;
use alloy::providers::Provider;
use dashmap::DashMap;
use mev_core::IUniswapV2Pair;
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::str::FromStr;

/// Permanent cache of a pool's `token0`/`token1`, resolved once via a direct
/// on-chain call and never invalidated -- a pool's token pairing is fixed at
/// deployment, unlike reserves or prices.
/// NOTE: This applies only to UniswapV2-style pools, not UniswapV3 or other AMM designs.
///
/// This exists because mevlog's `pair_created` table only covers pools whose
/// `PairCreated` event happened to fall inside its indexed block range: a
/// pool created before indexing started never backfills there, no matter how
/// long indexing keeps running (see MEV-18). Deliberately a separate SQLite
/// file, not mevlog's own DB -- `mevlog update-custom-tables` drops and
/// rebuilds every custom table from scratch, which would silently wipe
/// anything written there.
pub struct PoolTokenCache {
    db: SqlitePool,
    tokens: DashMap<Address, (Address, Address)>,
}

impl PoolTokenCache {
    /// Opens (creating if needed) the SQLite file at `db_path` and loads
    /// every previously-resolved pool into memory for O(1) lookups.
    pub async fn open(db_path: &str) -> eyre::Result<Self> {
        let options =
            SqliteConnectOptions::from_str(&format!("sqlite://{db_path}"))?.create_if_missing(true);
        let db = SqlitePool::connect_with(options).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pool_tokens (
                pool_address TEXT PRIMARY KEY,
                token0 TEXT NOT NULL,
                token1 TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await?;

        let tokens = DashMap::new();
        let rows = sqlx::query("SELECT pool_address, token0, token1 FROM pool_tokens")
            .fetch_all(&db)
            .await?;

        for row in rows {
            let pool: String = row.try_get("pool_address")?;
            let token0: String = row.try_get("token0")?;
            let token1: String = row.try_get("token1")?;
            tokens.insert(pool.parse()?, (token0.parse()?, token1.parse()?));
        }

        Ok(Self { db, tokens })
    }

    /// In-memory lookup only -- never touches RPC or disk.
    pub fn get(&self, pool: Address) -> Option<(Address, Address)> {
        self.tokens.get(&pool).map(|entry| *entry)
    }

    /// Cache-first resolution. A hit returns immediately with no I/O at all.
    /// On a miss, calls `token0()`/`token1()` directly on the pool contract
    /// (two cheap `eth_call`s, no log scanning) and writes the result
    /// through to both the SQLite file and the in-memory map before
    /// returning it, so every future call for this pool -- this run or after
    /// a restart -- is free.
    pub async fn resolve<P>(&self, provider: &P, pool: Address) -> eyre::Result<(Address, Address)>
    where
        P: Provider + Clone,
    {
        if let Some(tokens) = self.get(pool) {
            return Ok(tokens);
        }

        let pair = IUniswapV2Pair::new(pool, provider);
        let token0 = pair.token0().call().await?;
        let token1 = pair.token1().call().await?;

        sqlx::query(
            "INSERT OR IGNORE INTO pool_tokens (pool_address, token0, token1) VALUES (?, ?, ?)",
        )
        .bind(pool.to_string())
        .bind(token0.to_string())
        .bind(token1.to_string())
        .execute(&self.db)
        .await?;

        self.tokens.insert(pool, (token0, token1));
        Ok((token0, token1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address::from([byte; 20])
    }

    #[tokio::test]
    async fn resolved_pools_survive_a_reopen() {
        let dir = std::env::temp_dir().join(format!("pool-token-cache-test-{}", uuid()));
        let db_path = dir.to_str().unwrap().to_string();

        let cache = PoolTokenCache::open(&db_path).await.unwrap();
        let pool = addr(1);
        assert_eq!(cache.get(pool), None);

        sqlx::query("INSERT INTO pool_tokens (pool_address, token0, token1) VALUES (?, ?, ?)")
            .bind(pool.to_string())
            .bind(addr(2).to_string())
            .bind(addr(3).to_string())
            .execute(&cache.db)
            .await
            .unwrap();

        // A fresh `open` must load what a prior instance (or a prior
        // process) already persisted -- that's the entire point of writing
        // through to SQLite instead of only caching in memory.
        let reopened = PoolTokenCache::open(&db_path).await.unwrap();
        assert_eq!(reopened.get(pool), Some((addr(2), addr(3))));

        std::fs::remove_file(&db_path).ok();
    }

    fn uuid() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}
