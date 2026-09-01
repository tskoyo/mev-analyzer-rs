use alloy::primitives::Address;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopKind {
    TwoLeg,
    ThreeLeg { connector: Address },
}

/// One swap in a candidate's path, in on-chain (raw, un-normalized) units --
/// exactly what's needed to build the calldata sequence for Stage 3/4.
#[derive(Debug, Clone)]
pub struct PoolLeg {
    pub pool: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub reserve_in: u128,
    pub reserve_out: u128,
}

/// A detected, priced, but not-yet-verified arbitrage loop -- a lead, not a
/// verdict. Everything here is an estimate from Stage 2's approximate
/// spot-price model; Stage 3 (safety) and Stage 4 (REVM) turn it into one.
#[derive(Debug, Clone)]
pub struct ArbCandidate {
    pub legs: Vec<PoolLeg>,
    pub loop_kind: LoopKind,
    /// The block every leg's reserves were read at -- pins the fork point
    /// Stage 4 must replay against to reproduce this candidate exactly.
    pub block_number: u64,
    pub gap_bps: f64,
    /// Decimal-normalized ("human units"), same convention as `economics`.
    pub estimated_optimal_input: f64,
    pub estimated_gross_profit: f64,
    pub estimated_net_profit: f64,
}
