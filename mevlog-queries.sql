-- Query to get all swaps from Uniswap V2 along with their corresponding pair information
WITH v2 AS (
  SELECT
    s.block_number, s.tx_index, s.log_index,
    s.address AS pool_address,
    'sold' AS side,
    CASE WHEN u256_to_dec(s.amount0_in) <> '0' THEN p.token0 ELSE p.token1 END AS token_address,
    CASE WHEN u256_to_dec(s.amount0_in) <> '0' THEN s.amount0_in ELSE s.amount1_in END AS amount_raw
  FROM uniswap_v2_swaps s
  JOIN pair_created p ON p.pair = s.address
  UNION ALL
  SELECT
    s.block_number, s.tx_index, s.log_index,
    s.address AS pool_address,
    'bought' AS side,
    CASE WHEN u256_to_dec(s.amount0_out) <> '0' THEN p.token0 ELSE p.token1 END AS token_address,
    CASE WHEN u256_to_dec(s.amount0_out) <> '0' THEN s.amount0_out ELSE s.amount1_out END AS amount_raw
  FROM uniswap_v2_swaps s
  JOIN pair_created p ON p.pair = s.address
),
per_pool AS (
  SELECT
    token_address, pool_address,
    u256_sum(amount_raw) AS pool_volume_blob,
    COUNT(*) AS pool_trades,
    MAX(block_number) AS pool_last_trade_block
  FROM v2
  GROUP BY token_address, pool_address
),
per_token AS (
  SELECT
    token_address,
    COUNT(DISTINCT pool_address) AS real_pools,
    MIN(pool_volume_blob) AS min_pool_volume_blob,
    MAX(pool_volume_blob) AS max_pool_volume_blob,
    MAX(pool_last_trade_block) AS last_trade_block
  FROM per_pool
  GROUP BY token_address
),
good_tokens AS (
  SELECT *
  FROM per_token
  WHERE real_pools >= 2
    AND u256_to_dec(min_pool_volume_blob) <> '0'
    AND last_trade_block >= (SELECT MAX(block_number) - 5000 FROM blocks)  -- recency window in blocks, not a timestamp join (see MEV-11)
)
SELECT
  pp.token_address, pp.pool_address,
  u256_to_dec(pp.pool_volume_blob) AS pool_volume_raw,
  pp.pool_trades, pp.pool_last_trade_block,
  gt.real_pools,
  u256_to_dec(gt.min_pool_volume_blob) AS pool_balance_min,
  u256_to_dec(gt.max_pool_volume_blob) AS pool_balance_max
FROM per_pool pp
JOIN good_tokens gt ON gt.token_address = pp.token_address
ORDER BY pp.token_address, pp.pool_volume_blob DESC;