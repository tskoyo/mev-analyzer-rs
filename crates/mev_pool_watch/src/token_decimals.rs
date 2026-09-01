use alloy::primitives::Address;
use alloy::providers::Provider;
use mev_core::ERC20;
use std::collections::HashMap;

/// Fetches `decimals()` for a deduplicated set of tokens, live. Only 2-leg
/// gaps get away without this (decimals cancel out of a same-pair ratio);
/// 3-leg gaps and all profitability math need real per-token decimals since
/// they compare across different tokens.
pub async fn fetch_decimals_for<P>(
    provider: &P,
    tokens: &[Address],
) -> eyre::Result<HashMap<Address, u8>>
where
    P: Provider + Clone,
{
    let mut out = HashMap::new();
    for &token in tokens {
        if out.contains_key(&token) {
            continue;
        }
        let erc20 = ERC20::new(token, provider);
        let decimals = erc20.decimals().call().await?;
        out.insert(token, decimals);
    }
    Ok(out)
}
