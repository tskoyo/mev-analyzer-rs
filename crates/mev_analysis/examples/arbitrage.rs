use alloy::{
    primitives::{Address, address},
    providers::ProviderBuilder,
    sol,
};
use chrono::Local;
use std::time::Duration;

const UNISWAP_V2_WETH_USDC: Address = address!("B4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc");
const SUSHISWAP_WETH_USDC: Address = address!("397FF1542f962076d0BFE58eA045FfA2d347ACa0");

const TOKEN0_DECIMALS: f64 = 1e6; // USDC
const TOKEN1_DECIMALS: f64 = 1e18; // WETH

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

// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();

    let rpc_url = std::env::var("RPC_URL")
        .map_err(|_| eyre::eyre!("set RPC_URL, e.g. RPC_URL=https://eth.llamarpc.com"))?;

    println!("Rpc url is: {}", rpc_url);

    let provider = ProviderBuilder::new().connect_http(rpc_url.parse()?);

    let uniswap = IUniswapV2Pair::new(UNISWAP_V2_WETH_USDC, &provider);
    let sushi = IUniswapV2Pair::new(SUSHISWAP_WETH_USDC, &provider);

    println!("watching WETH/USDC on Uniswap V2 vs SushiSwap");
    println!("logging gaps of {LOG_THRESHOLD_BPS} bp or more\n");

    loop {
        match check_once(&uniswap, &sushi).await {
            Ok(()) => {}
            // A single failed RPC call should not kill the process.
            Err(e) => eprintln!("[{}] rpc error: {e}", now()),
        }
        tokio::time::sleep(Duration::from_secs(POLL_SECONDS)).await;
    }
}

async fn check_once<P>(
    uniswap: &IUniswapV2Pair::IUniswapV2PairInstance<P>,
    sushi: &IUniswapV2Pair::IUniswapV2PairInstance<P>,
) -> eyre::Result<()>
where
    P: alloy::providers::Provider + Clone,
{
    let u = uniswap.getReserves().call().await?;
    let s = sushi.getReserves().call().await?;

    let uni_price = price_from_reserves(u.reserve0.to::<u128>(), u.reserve1.to::<u128>());
    let sushi_price = price_from_reserves(s.reserve0.to::<u128>(), s.reserve1.to::<u128>());

    let cheaper = uni_price.min(sushi_price);
    let gap_bps = ((uni_price - sushi_price).abs() / cheaper) * 10_000.0;

    if gap_bps >= LOG_THRESHOLD_BPS {
        let (buy_on, sell_on) = if uni_price < sushi_price {
            ("Uniswap", "Sushi")
        } else {
            ("Sushi", "Uniswap")
        };

        let verdict = if gap_bps > ROUND_TRIP_COST_BPS {
            "ABOVE fee cost"
        } else {
            "below fee cost"
        };

        println!(
            "[{}] uni ${:>10.2} | sushi ${:>10.2} | gap {:>6.1} bp | buy {buy_on} sell {sell_on} | {verdict}",
            now(),
            uni_price,
            sushi_price,
            gap_bps,
        );
    }

    Ok(())
}

fn price_from_reserves(reserve0: u128, reserve1: u128) -> f64 {
    let usdc = reserve0 as f64 / TOKEN0_DECIMALS;
    let weth = reserve1 as f64 / TOKEN1_DECIMALS;
    usdc / weth
}

fn now() -> String {
    Local::now().format("%H:%M:%S").to_string()
}
