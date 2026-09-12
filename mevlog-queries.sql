-- Query to get all swaps from Uniswap V2 along with their corresponding pair information
SELECT
   s.block_number,
   s.tx_index,
   s.log_index,
   hex(s.address) AS pool,
   hex(pc.token0) AS token0,
   hex(pc.token1) AS token1,
   hex(s.sender) AS sender,
   hex(s.to_address) AS to_address,
   s.amount0_in,
   s.amount1_in,
   s.amount0_out,
   s.amount1_out
FROM uniswap_v2_swaps s
JOIN pair_created pc ON pc.pair = s.address
