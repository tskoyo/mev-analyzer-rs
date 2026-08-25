# mev-analyzer-rs

## What this project is

A pipeline for finding real (not apparent) arbitrage opportunities on Ethereum-family
chains, and simulating them precisely enough to trust the numbers.

Three stages, each doing what it's good at:

1. **Dune (SQL)** — cheap, slow, wide discovery. Scan `dex.trades` across chains to
   shortlist tokens with multiple active pools on the same chain. Never trusted for
   final numbers — only for "what should we look at."
2. **Rust (live RPC)** — cheap, fast, narrow confirmation. Read live pool reserves
   via `getReserves()` to check whether a Dune candidate is actually deep enough to
   matter, before spending simulation budget on it.
3. **REVM (exact)** — expensive, precise, ground truth. Fork mainnet state at a
   specific block and actually execute swaps to get the real output amount —
   including effects no formula captures (transfer taxes, proxy quirks, exact gas).

Nothing is trusted until it's confirmed at the REVM layer. Dune volume numbers in
particular are not to be treated as evidence of anything — see rules below.

## Workspace layout

- `crates/mev-analysis` — Dune queries, candidate filtering, orchestration
- `crates/<pool-watch or similar>` — live reserve polling, `getReserves()` calls,
  price-gap logging
- `crates/<revm-sim or similar>` — transaction/trade simulation via REVM + AlloyDB

(Update this section with real crate names as they solidify — don't let it drift
from the actual `Cargo.toml` workspace members.)

## Hard rules

These aren't style preferences. Each one corresponds to a bug or false conclusion
that already happened once in this project. Do not relearn them.

- **Volume ≠ depth.** A token doing $1M/month through a $3k pool is not liquid.
  Never rank or filter candidates by volume alone. Always check live reserves
  before treating a candidate as real.
- **An average of two pool sizes is meaningless.** A $200k pool and a $2k pool
  average to a fake "$101k per pool." Use `MIN(pool_volume)` /
  `pool_balance = MIN/MAX` to catch lopsided pairs, not `AVG`.
- **A high pool count for a token is a red flag, not a good sign**, unless
  `distinct_addresses = 1` for that symbol on that chain. Multiple contracts
  sharing a display name (scams, copycats) will otherwise get summed together
  into one fake "token."
- **`taker` ≠ `tx_from`.** `tx_from` is who paid gas; `taker` is who the trade was
  actually for. On Ethereum V2 trades, roughly 75% of rows have `taker != tx_from`
  (routers, aggregators, bots in between). Use `taker` for counting real distinct
  traders.
- **Hourly (or coarser) price comparison across two pools is not evidence of a
  gap.** Two pools trading at different times within the same hour will show a
  fake spread purely from the asset's price moving. Only compare prices from
  trades in the same minute (or tighter) as genuine evidence of disagreement.
  Check overlap frequency before building anything on top of a "gap."
- **Different pool pairings (e.g. TOKEN/WBNB vs TOKEN/BUSD) require a 3-leg
  loop, not 2.** Confirm `token0`/`token1` match across both pools before
  assuming a simple two-swap round trip. If they don't match, the round-trip
  fee cost goes up (extra leg) and USD-denominated comparisons become
  quote-currency-feed-dependent — compare in token terms via the connecting
  pool's own rate, not via external USD prices.
- **Fees are a percentage, not a flat cost.** Bigger trade size does not fix a
  negative (gap% − fee%) spread — both scale together and the sign never
  changes. Only gas is flat; only price impact is genuinely nonlinear with size.
- **`in a single-transaction simulation, isolating a tx from its block position
  will not reproduce its exact real-world result`** if `transaction_index > 0`.
  Prior transactions in the same block can move the pools your tx depends on.
  A close-but-not-exact simulated profit (e.g. $7.85 vs a known real $12.84) is
  most likely this, not a code bug — check `tx.transaction_index()` before
  assuming the sim harness is broken.

## Validation discipline

Every new piece of the pipeline gets checked against a known-good number before
it's trusted with anything new:

- Known reference transaction:
  `0xb0a944ff492a157eef35d31fdceb9b680e3d25082cea9ba68cdbf54b0463af24`
  (Ethereum, block 25759160) — a flash-loan arb: Morpho → Curve (USDC→USDat) →
  M^0 wrap → Uniswap V3 (wM→USDC) → repay. **Real profit: 12.841880 USDC.**
  Bot contract: `0x97974dAc5C287E03B12Da9410FebC0CcaBB033B1` (this is the
  address to read balances on — not `tx.from()`, which is just the relayer/EOA
  that submitted the transaction).
- Any new SQL query that computes profit/arb detection: run it against this tx
  first, confirm ~12.84 comes out, before trusting it on the full dataset.
- Any new REVM simulation path: replay this tx, confirm the balance delta on
  the bot address. If it's off but reasonably close (same order of magnitude,
  correct sign), check `transaction_index` before assuming the harness is wrong.
- Prices/reserves: sanity-check against a second source (DeFiLlama, the
  protocol's own dashboard, a manual Etherscan read) before trusting a new
  data path.
