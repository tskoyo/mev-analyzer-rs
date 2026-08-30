# Architecture

This document describes the full pipeline shape: how a candidate moves from a
Dune query to a REVM-confirmed arbitrage opportunity. See `CLAUDE.md` for the
hard rules and validation discipline that every stage below has to satisfy.

## Overview

```
Dune (SQL)  →  Rust RPC confirm  →  Safety scoring  →  REVM full sim
 shortlist      liquidity+loop+gap    (incl. cheap        (ground truth,
 + pool addrs   + profitability       buy/sell probe)      existing pattern)
```

Each stage is a gate: a candidate only advances if it survives, and each stage
is strictly cheaper than the one after it. REVM full simulation — the
expensive step — only runs on candidates that already passed profitability
*and* safety.

## Stage 1 — Dune

Query shape: `v2` (union of bought/sold trade rows) → `per_pool` (per-pool
aggregates, DEX-allowlisted, per-pool volume floor) → `per_chain` (`MIN`/`MAX`
pool volume → `pool_balance`, `distinct_addresses` per symbol-per-chain) →
`good_chains` (the actual filter: real pool count, pool balance, single
contract address, recency) → a pool-address-emitting final `SELECT`:

```sql
SELECT p.symbol, p.blockchain, p.token_address,
       p.project_contract_address AS pool_address,
       p.pool_volume, p.pool_traders, p.pool_last_trade
FROM per_pool p
JOIN good_chains g ON p.symbol = g.symbol AND p.blockchain = g.blockchain
ORDER BY p.symbol, p.blockchain, p.pool_volume DESC
```

This is the actual pipeline input — one row per real pool that survived every
filter. Never trusted for final numbers; only for "what to look at next."

Initial chain scope: whichever chain the live-RPC prototype already targets
(BNB chain, via `BNB_RPC_URL`) — keeps the Stage 2 connector-pool list small.
Extending to more chains is a matter of adding to that list, not a redesign.

## Stage 2 — Rust live RPC confirmation (`crates/mev_pool_watch`)

Takes Stage 1's pool-address rows, confirms real depth, detects loops, scores
profitability. Deliberately has no `revm` dependency — stays cheap and fast.

- **Pool reads are atomic per block.** Every poll cycle fetches the current
  block number once, then pins every `getReserves()`/`token0()`/`token1()`
  call to it. Readings from different blocks are never compared — a fake gap
  from ordinary price movement between two independent RPC calls is exactly
  the failure mode this prevents.
- **`token0`/`token1` are always read live**, never assumed from Dune or
  config — a pool's actual on-chain pairing is ground truth, not something to
  infer from a symbol.
- **Pools are grouped by token *address*, never symbol** — the same
  copycat-token protection Stage 1 applies, re-applied here since Stage 2 can
  receive pools for tokens Stage 1 didn't fully disambiguate.
- **Loop detection**: a 2-leg loop is any pair of pools sharing the exact same
  on-chain pair. A 3-leg loop requires a *connecting* pool (e.g. a target
  token paired against two different quote tokens, plus a pool between those
  two quote tokens) — looked up from a small static config list of known
  major-pair pools. No connector found → skip. A gap is never fabricated via
  an external USD price feed; it's only ever computed from the connecting
  pool's own on-chain rate.
- **Profitability**: reuses the constant-product, percentage-fee,
  ternary-search optimal-input math already proven out — generalized from a
  hardcoded 2-pool shape to an arbitrary N-leg chain so the same code serves
  both 2-leg and 3-leg candidates. Gas is the only flat cost; everything else
  scales with trade size.
- **Output**: an `ArbCandidate` — the loop's legs, its kind (2-leg/3-leg), the
  block it was observed at, the gap in bps, and an estimated optimal input and
  net profit. This is a lead, not a verdict — Stage 3/4 turn it into one.

## Stage 3 — Safety scoring (`crates/mev_safety`)

Runs on candidates that already passed Stage 2's profitability check. Exists
because trade-history aggregates (Stage 1) and live reserves (Stage 2) cannot
see whether a token is actually a honeypot, has an undisclosed transfer tax,
or is a fresh rug waiting to happen — only real execution semantics can.

- **Cheap buy/sell probe**: a single-token REVM simulation (buy then sell —
  not the full multi-leg loop) with a **three-way outcome**, not binary:
  - `Pass { tax_bps }` — sell succeeded. Any shortfall vs. the constant-product
    prediction is a real, quantified cost (a transfer tax), so it feeds back
    into Stage 2's profitability math as an extra fee% — it is *not* folded
    into the safety score, because it isn't a fuzzy risk, it's a known cost.
  - `TokenRejected(reason)` — the sell reverted with on-chain evidence
    pointing at the token's own logic (e.g. a blacklist/pause-style revert).
    Hard disqualify; no score offsets this.
  - `Inconclusive(reason)` — anything pointing at *our* harness instead of the
    token (RPC error, bad calldata, insufficient seeded balance/allowance, an
    ambiguous no-data revert). Never counted as a rejection — the candidate is
    held back and retried on the next cycle, not disqualified.
  - Before trusting any of the above on a real candidate, the harness
    self-validates against a known-safe token (WETH/USDC, or the project's
    validated reference tx's token) — the same discipline `CLAUDE.md` already
    requires for the REVM reference tx, applied here too.
- **Contract age** and **holder concentration** (excluding known pool/LP/
  router addresses — otherwise every token fails for holding its own
  liquidity) each produce a `[0,1]` sub-score, combined multiplicatively
  (`safety_score = age_score * holder_score`) — each is treated as an
  independent survival probability, not averaged, so a bad score on either
  one pulls the composite down.
- **Gate**: `risk_adjusted_value = safety_score * updated_net_profit` (the
  tax-corrected profit from the buy/sell probe). Only candidates clearing a
  minimum threshold proceed to Stage 4 — this is what lets a strong profit
  estimate offset a middling (but not disqualifying) safety signal.
- **Health check**: the `TokenRejected` rate across recent candidates is
  tracked. A spike is far more likely a harness regression than a sudden
  cluster of honeypots — the same "check your own plumbing first" discipline
  `CLAUDE.md` applies to REVM profit mismatches. Log/alert, don't silently
  keep rejecting.

## Stage 4 — REVM full simulation (`crates/mev_analysis`)

Ground truth. Replays the exact multi-leg swap sequence a surviving candidate
describes, forked at the pinned block, respecting `transaction_index` position
when simulating around a real transaction. Validated against the known
reference tx documented in `CLAUDE.md` before being trusted on new data.

## Out of scope (for now)

- **Trade execution.** This pipeline stops at a REVM-confirmed candidate — the
  decision and submission of an actual on-chain transaction is a separate,
  later concern.
- **Ownership/mint/blacklist bytecode inspection or third-party safety APIs**
  (GoPlus, honeypot.is, etc.) — a real future signal, not built yet; the
  buy/sell probe covers the most load-bearing case (can we actually sell)
  cheaply in the meantime.
- **Multi-chain connector list** beyond the initial chain scope.
- **A real CLI.** `mev_cli` has an unused `clap` dependency suggesting this
  was the intent — wiring stages 1-4 into an actual command-line tool is
  separate work.

## Known gaps

- No documented reference case exists for Stage 2 (a known real pool-pair gap
  at a specific block with an expected bps figure), unlike Stage 4's
  `0xb0a9...` reference tx. Worth adding to `CLAUDE.md`'s validation-discipline
  section once Stage 2 has run against real data a few times.
