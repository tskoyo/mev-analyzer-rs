use alloy::{
    primitives::{Address, address},
    providers::ProviderBuilder,
    sol,
};
use chrono::Local;
use std::time::Duration;

const POOL_1: Address = address!("1cea83ec5e48d9157fcae27a19807bef79195ce1");
const POOL_2: Address = address!("647bc907d520c3f63be38d01dbd979f5606bec48");

const TOKEN_WITH_SIX_DECIMALS: f64 = 1e6; // USDC
const TOKEN_WITH_EIGHTEEN_DECIMALS: f64 = 1e18; // WETH

const POLL_SECONDS: u64 = 12;

const LOG_THRESHOLD_BPS: f64 = 5.0;

const ROUND_TRIP_COST_BPS: f64 = 60.0;

sol! {
    #[sol(rpc)]
    interface IUniswapV2Pair {
        function getReserves() external view returns (
            uint112 reserve0,
            uint112 reserve1,
            uint32 blockTimestampLast
        );
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();

    let rpc_url = std::env::var("BNB_RPC_URL")
        .map_err(|_| eyre::eyre!("set RPC_URL, e.g. RPC_URL=https://eth.llamarpc.com"))?;

    let provider = ProviderBuilder::new().connect_http(rpc_url.parse()?);

    let pool_1_pair = IUniswapV2Pair::new(POOL_1, &provider);
    let pool_2_pair = IUniswapV2Pair::new(POOL_2, &provider);

    println!("watching pools");
    println!("logging gaps of {LOG_THRESHOLD_BPS} bp or more\n");

    loop {
        match check_once(&pool_1_pair, &pool_2_pair).await {
            Ok(()) => {}
            // A single failed RPC call should not kill the process.
            Err(e) => eprintln!("[{}] rpc error: {e}", now()),
        }
        tokio::time::sleep(Duration::from_secs(POLL_SECONDS)).await;
    }
}

async fn check_once<P>(
    pair_1_instance: &IUniswapV2Pair::IUniswapV2PairInstance<P>,
    pair_2_instance: &IUniswapV2Pair::IUniswapV2PairInstance<P>,
) -> eyre::Result<()>
where
    P: alloy::providers::Provider + Clone,
{
    match pair_1_instance.getReserves().call().await {
        Ok(r) => println!("pair_1 ok: {} / {}", r.reserve0, r.reserve1),
        Err(e) => println!("pair_1 FAILED: {e}"),
    }

    match pair_2_instance.getReserves().call().await {
        Ok(r) => println!("pair_2 ok: {} / {}", r.reserve0, r.reserve1),
        Err(e) => println!("pair_2 FAILED: {e}"),
    }

    let u = pair_1_instance.getReserves().call().await?;
    let s = pair_2_instance.getReserves().call().await?;

    let pair_1_price = price_from_reserves(u.reserve0.to::<u128>(), u.reserve1.to::<u128>());
    let pair_2_price = price_from_reserves(s.reserve0.to::<u128>(), s.reserve1.to::<u128>());

    println!(
        "  pair_1 pool: {:.0} USDC / {:.2} WETH",
        u.reserve0.to::<u128>() as f64 / 1e6,
        u.reserve1.to::<u128>() as f64 / 1e18
    );
    println!(
        " pair_2 pool: {:.0} USDC / {:.2} WETH",
        s.reserve0.to::<u128>() as f64 / 1e6,
        s.reserve1.to::<u128>() as f64 / 1e18
    );

    let cheaper = pair_1_price.min(pair_2_price);
    let gap_bps = ((pair_1_price - pair_2_price).abs() / cheaper) * 10_000.0;

    if gap_bps >= LOG_THRESHOLD_BPS {
        let (buy_on, sell_on) = if pair_1_price < pair_2_price {
            ("Uniswap", "Swaap")
        } else {
            ("Swaap", "Uniswap")
        };

        let verdict = if gap_bps > ROUND_TRIP_COST_BPS {
            "ABOVE fee cost"
        } else {
            "below fee cost"
        };

        println!(
            "[{}] pair_1 ${:>10.2} | pair_2 ${:>10.2} | gap {:>6.1} bp | buy {buy_on} sell {sell_on} | {verdict}",
            now(),
            pair_1_price,
            pair_2_price,
            gap_bps,
        );
    }

    Ok(())
}

fn price_from_reserves(reserve0: u128, reserve1: u128) -> f64 {
    let usdc = reserve0 as f64 / TOKEN_WITH_SIX_DECIMALS;
    let weth = reserve1 as f64 / TOKEN_WITH_EIGHTEEN_DECIMALS;
    usdc / weth
}

fn now() -> String {
    Local::now().format("%H:%M:%S").to_string()
}
