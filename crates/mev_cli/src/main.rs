#[derive(Clone)]
struct Pool {
    name: &'static str,
    usdc: f64, // reserve of USDC
    weth: f64, // reserve of WETH
    fee: f64,  // swap fee taken off the input, e.g. 0.003 = 0.30%
}

impl Pool {
    /// Spot price of 1 WETH in USDC is just the ratio of reserves. This single
    /// number is what a bot mirrors across many pools. When two pools holding the
    /// same pair disagree on it, THAT disagreement is the arbitrage opportunity.
    /// This is "detection": not magic, just comparing ratios.
    fn weth_price(&self) -> f64 {
        self.usdc / self.weth
    }

    /// The exact constant-product swap the pool contract runs: put `amount_in` of
    /// the input token in, receive some output token out. Fee is taken off the
    /// input first, then x*y=k determines the output. Crucially: the more you put
    /// in, the worse your marginal rate gets — because your own trade moves the
    /// reserves. That self-inflicted price movement is "price impact" / slippage.
    fn amount_out(&self, amount_in: f64, reserve_in: f64, reserve_out: f64) -> f64 {
        let in_with_fee = amount_in * (1.0 - self.fee);
        (in_with_fee * reserve_out) / (reserve_in + in_with_fee)
    }

    fn usdc_to_weth(&self, usdc_in: f64) -> f64 {
        self.amount_out(usdc_in, self.usdc, self.weth)
    }
    fn weth_to_usdc(&self, weth_in: f64) -> f64 {
        self.amount_out(weth_in, self.weth, self.usdc)
    }
}

/// The cyclic arb itself: start with `usdc_in`, buy WETH where it's cheap, sell
/// that WETH where it's expensive, end holding USDC again. Returns final USDC.
/// On-chain this is ONE transaction to ONE contract doing both legs atomically.
fn run_arb(usdc_in: f64, cheap: &Pool, expensive: &Pool) -> f64 {
    let weth = cheap.usdc_to_weth(usdc_in); // leg 1: USDC -> WETH  (cheap pool)
    expensive.weth_to_usdc(weth) // leg 2: WETH -> USDC  (expensive pool)
}

/// Gross profit in USDC for a given input size, before gas.
fn gross_profit(usdc_in: f64, cheap: &Pool, expensive: &Pool) -> f64 {
    run_arb(usdc_in, cheap, expensive) - usdc_in
}

/// Find the input size that MAXIMIZES profit. This is the part naive explanations
/// skip entirely. You do NOT trade as much as possible: past a point, your own
/// price impact eats the spread faster than you capture it, and profit falls.
/// The profit curve is concave (rises, peaks, falls), so we ternary-search it.
/// The peak is where marginal spread captured == marginal price impact paid.
fn optimal_input(cheap: &Pool, expensive: &Pool, mut lo: f64, mut hi: f64) -> f64 {
    for _ in 0..200 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if gross_profit(m1, cheap, expensive) < gross_profit(m2, cheap, expensive) {
            lo = m1;
        } else {
            hi = m2;
        }
    }
    (lo + hi) / 2.0
}

/// Gas cost of executing the arb, expressed in USDC so it sits in the same units
/// as profit. This single term is what turns most "opportunities" into traps.
fn gas_cost_usdc(gas_used: f64, gas_price_gwei: f64, eth_price_usdc: f64) -> f64 {
    let eth_spent = gas_used * gas_price_gwei * 1e-9; // gwei * gas -> ETH
    eth_spent * eth_price_usdc
}

fn analyze(label: &str, cheap: &Pool, expensive: &Pool, gas_used: f64, gas_price_gwei: f64) {
    let eth_price = expensive.weth_price(); // good-enough numeraire for gas conversion
    println!("=== {label} ===");
    println!(
        "  {}: 1 WETH = {:>9.2} USDC",
        cheap.name,
        cheap.weth_price()
    );
    println!(
        "  {}: 1 WETH = {:>9.2} USDC",
        expensive.name,
        expensive.weth_price()
    );
    let spread = expensive.weth_price() - cheap.weth_price();
    println!(
        "  price gap : {:>9.2} USDC/WETH  ({:.3}%)  <- this is what 'detection' spots",
        spread,
        100.0 * spread / cheap.weth_price()
    );

    println!(
        "\ngross profit vs. trade size (watch it rise, peak, then FALL — that's price impact):"
    );
    for &dx in &[5_000.0, 25_000.0, 75_000.0, 150_000.0, 300_000.0, 600_000.0] {
        println!(
            "     put in {:>10.0} USDC  ->  gross profit {:>11.2} USDC",
            dx,
            gross_profit(dx, cheap, expensive)
        );
    }

    let best_in = optimal_input(cheap, expensive, 0.0, cheap.usdc.min(expensive.usdc));
    let gross = gross_profit(best_in, cheap, expensive);
    let gas = gas_cost_usdc(gas_used, gas_price_gwei, eth_price);
    let net = gross - gas;

    println!(
        "\n  OPTIMAL trade size : {:>11.2} USDC   <- solved, not guessed",
        best_in
    );
    println!("  gross profit       : {:>11.2} USDC", gross);
    println!(
        "  gas cost           : {:>11.2} USDC   ({:.0} gas @ {:.0} gwei, ETH=${:.0})",
        gas, gas_used, gas_price_gwei, eth_price
    );
    println!("----------------------------------");
    println!("NET profit: {:>11.2} USDC", net);
    if net > 0.0 {
        println!("DECISION: EXECUTE ✅   submit the transaction");
    } else {
        println!("DECISION: SKIP ❌      the on-chain require(profit>0) would REVERT;");
        println!("submitting anyway only burns the gas above for nothing");
    }
    println!();
}

fn main() {
    println!("\nARBITRAGE UNDER THE HOOD — the off-chain math, before any EVM\n");

    // SCENARIO 1: a fat price gap. WETH is meaningfully cheaper on Pool A.
    // A clear, profitable arb even after gas.
    let a = Pool {
        name: "Pool A (Uniswap)",
        usdc: 3_000_000.0,
        weth: 1000.0,
        fee: 0.003,
    };
    let b = Pool {
        name: "Pool B (Sushi)",
        usdc: 3_150_000.0,
        weth: 1000.0,
        fee: 0.003,
    };
    analyze(
        "SCENARIO 1: fat gap — genuinely profitable",
        &a,
        &b,
        180_000.0,
        10.0,
    );

    // SCENARIO 2: a real but THIN gap, during a high-gas moment. There IS a
    // positive-gross arb here — a naive bot fires on it and loses money, because
    // gas exceeds the profit. This is failure-mode #2 from the project doc, in
    // miniature: the calculation that SHOULD stop you is the net-of-gas check.
    let c = Pool {
        name: "Pool C (Uniswap)",
        usdc: 3_000_000.0,
        weth: 1000.0,
        fee: 0.003,
    };
    let d = Pool {
        name: "Pool D (Sushi)",
        usdc: 3_025_000.0,
        weth: 1000.0,
        fee: 0.003,
    };
    analyze(
        "SCENARIO 2: thin gap + high gas — the trap that bleeds naive bots",
        &c,
        &d,
        180_000.0,
        45.0,
    );
}
