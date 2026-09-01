//! Generalizes `mev_cli`'s two-pool profit/sizing math to an arbitrary
//! N-leg chain, so the same code serves 2-leg and 3-leg candidates. The
//! formula itself is unchanged: fee is a percentage taken off the input
//! before the constant-product swap (scales with size), gas is the only
//! flat cost, and the profit curve is concave in trade size, so a ternary
//! search finds the peak rather than guessing.

/// One swap in a loop: the pool's reserves for the token going in and the
/// token coming out, plus that pool's own fee.
#[derive(Debug, Clone, Copy)]
pub struct AmmLeg {
    pub reserve_in: f64,
    pub reserve_out: f64,
    pub fee_bps: u32,
}

impl AmmLeg {
    pub fn amount_out(&self, amount_in: f64) -> f64 {
        let fee = self.fee_bps as f64 / 10_000.0;
        let in_with_fee = amount_in * (1.0 - fee);
        (in_with_fee * self.reserve_out) / (self.reserve_in + in_with_fee)
    }
}

/// Runs `amount_in` through every leg in order, returning what you end up
/// with after the last swap.
pub fn run_loop(amount_in: f64, legs: &[AmmLeg]) -> f64 {
    legs.iter().fold(amount_in, |amount, leg| leg.amount_out(amount))
}

/// Gross profit for a given input size, before gas.
pub fn gross_profit(amount_in: f64, legs: &[AmmLeg]) -> f64 {
    run_loop(amount_in, legs) - amount_in
}

/// Finds the input size that maximizes gross profit via ternary search --
/// the profit curve rises, peaks, then falls as price impact overtakes the
/// spread being captured, so the peak isn't "as much as possible."
pub fn optimal_input(legs: &[AmmLeg], mut lo: f64, mut hi: f64) -> f64 {
    for _ in 0..200 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if gross_profit(m1, legs) < gross_profit(m2, legs) {
            lo = m1;
        } else {
            hi = m2;
        }
    }
    (lo + hi) / 2.0
}

/// Gross profit minus gas, both in the loop's start-token units. Gas is
/// accepted pre-converted rather than computed here, since converting a
/// flat native-gas cost into "start token" units needs a price reference
/// specific to whichever token this loop starts in -- that's a unit
/// conversion the caller owns, not a re-introduction of USD-based pricing
/// into the gap itself.
pub fn net_profit(amount_in: f64, legs: &[AmmLeg], gas_cost_in_start_token: f64) -> f64 {
    gross_profit(amount_in, legs) - gas_cost_in_start_token
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ports mev_cli's two manual scenarios into real assertions -- turns
    // the console demo into an automated regression guard for the
    // percentage-fee hard rule (bigger size doesn't fix a negative spread).

    fn two_pool_loop(usdc_a: f64, weth_a: f64, usdc_b: f64, weth_b: f64, fee_bps: u32) -> [AmmLeg; 2] {
        [
            AmmLeg { reserve_in: usdc_a, reserve_out: weth_a, fee_bps },
            AmmLeg { reserve_in: weth_b, reserve_out: usdc_b, fee_bps },
        ]
    }

    #[test]
    fn fat_gap_low_gas_is_genuinely_profitable() {
        // Pool A (cheap): 3,000,000 USDC / 1,000 WETH
        // Pool B (expensive): 3,150,000 USDC / 1,000 WETH
        let legs = two_pool_loop(3_000_000.0, 1_000.0, 3_150_000.0, 1_000.0, 30);
        let best_in = optimal_input(&legs, 0.0, 3_000_000.0f64.min(3_150_000.0));
        let net = net_profit(best_in, &legs, gas_cost_usdc(180_000.0, 10.0, 3_150.0));
        assert!(net > 0.0, "expected a genuinely profitable arb, got net={net}");
    }

    #[test]
    fn thin_gap_high_gas_is_a_trap() {
        // Pool C (cheap): 3,000,000 USDC / 1,000 WETH
        // Pool D (expensive): 3,025,000 USDC / 1,000 WETH -- real but thin gap
        let legs = two_pool_loop(3_000_000.0, 1_000.0, 3_025_000.0, 1_000.0, 30);
        let best_in = optimal_input(&legs, 0.0, 3_000_000.0f64.min(3_025_000.0));
        // High-gas moment: gas exceeds the profit even though gross > 0.
        let net = net_profit(best_in, &legs, gas_cost_usdc(180_000.0, 45.0, 3_025.0));
        assert!(net < 0.0, "expected gas to erase the thin gap, got net={net}");
    }

    fn gas_cost_usdc(gas_used: f64, gas_price_gwei: f64, eth_price_usdc: f64) -> f64 {
        let eth_spent = gas_used * gas_price_gwei * 1e-9;
        eth_spent * eth_price_usdc
    }
}
