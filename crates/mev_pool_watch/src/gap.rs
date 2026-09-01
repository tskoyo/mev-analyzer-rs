use crate::grouping::TokenPairKey;
use crate::pool::PoolSnapshot;
use alloy::primitives::Address;
use std::collections::HashMap;

/// Bps gap between two pools holding the identical token pair. Both
/// snapshots must be from the same block -- comparing reserves fetched at
/// different blocks can manufacture a gap that's just ordinary price
/// movement between two RPC calls, not a real disagreement.
///
/// Decimals cancel out of this ratio (both pools hold the same two tokens),
/// so raw reserves are fine here -- only cross-token (3-leg) comparisons
/// need real decimals.
pub fn two_leg_gap_bps(a: &PoolSnapshot, b: &PoolSnapshot) -> eyre::Result<f64> {
    if a.block_number != b.block_number {
        return Err(eyre::eyre!(
            "cannot compare pools from different blocks ({} vs {})",
            a.block_number,
            b.block_number
        ));
    }
    if TokenPairKey::new(a.token0, a.token1) != TokenPairKey::new(b.token0, b.token1) {
        return Err(eyre::eyre!("pools do not hold the same token pair"));
    }

    let price_a = a.reserve1 as f64 / a.reserve0 as f64;
    let price_b = b.reserve1 as f64 / b.reserve0 as f64;
    let lower = price_a.min(price_b);
    if lower <= 0.0 {
        return Err(eyre::eyre!("non-positive price"));
    }

    Ok(((price_a - price_b).abs() / lower) * 10_000.0)
}

/// Bps gap between the *direct* price of `target` in terms of `y` (from
/// `leg_ay`) and the *implied* price of `target` in terms of `y` via `x`
/// (from `leg_ax` and the `x`/`y` connector pool). A real disagreement here
/// is exactly the evidence a 3-leg loop needs -- and it's computed entirely
/// from on-chain pool rates, never an external USD price feed, per the
/// different-pairing hard rule.
pub fn three_leg_gap_bps(
    target: Address,
    leg_ax: &PoolSnapshot,
    connector_xy: &PoolSnapshot,
    leg_ay: &PoolSnapshot,
    decimals: &HashMap<Address, u8>,
) -> eyre::Result<f64> {
    if leg_ax.block_number != connector_xy.block_number || leg_ax.block_number != leg_ay.block_number {
        return Err(eyre::eyre!("cannot compare snapshots from different blocks"));
    }

    let x = leg_ax
        .other_token(target)
        .ok_or_else(|| eyre::eyre!("leg_ax does not hold the target token"))?;
    let y = leg_ay
        .other_token(target)
        .ok_or_else(|| eyre::eyre!("leg_ay does not hold the target token"))?;

    if TokenPairKey::new(connector_xy.token0, connector_xy.token1) != TokenPairKey::new(x, y) {
        return Err(eyre::eyre!("connector pool does not hold the expected token pair"));
    }

    let target_decimals = *decimals
        .get(&target)
        .ok_or_else(|| eyre::eyre!("missing decimals for target token"))?;
    let x_decimals = *decimals
        .get(&x)
        .ok_or_else(|| eyre::eyre!("missing decimals for {x}"))?;
    let y_decimals = *decimals
        .get(&y)
        .ok_or_else(|| eyre::eyre!("missing decimals for {y}"))?;

    let norm = |raw: u128, decimals: u8| raw as f64 / 10f64.powi(decimals as i32);

    let reserve_target_in_ax = norm(leg_ax.reserve_of(target).unwrap(), target_decimals);
    let reserve_x_in_ax = norm(leg_ax.reserve_of(x).unwrap(), x_decimals);
    let price_target_in_x = reserve_x_in_ax / reserve_target_in_ax;

    let reserve_x_in_xy = norm(connector_xy.reserve_of(x).unwrap(), x_decimals);
    let reserve_y_in_xy = norm(connector_xy.reserve_of(y).unwrap(), y_decimals);
    let price_x_in_y = reserve_y_in_xy / reserve_x_in_xy;

    let implied_price_target_in_y = price_target_in_x * price_x_in_y;

    let reserve_target_in_ay = norm(leg_ay.reserve_of(target).unwrap(), target_decimals);
    let reserve_y_in_ay = norm(leg_ay.reserve_of(y).unwrap(), y_decimals);
    let direct_price_target_in_y = reserve_y_in_ay / reserve_target_in_ay;

    let lower = implied_price_target_in_y.min(direct_price_target_in_y);
    if lower <= 0.0 {
        return Err(eyre::eyre!("non-positive implied price"));
    }

    Ok(((implied_price_target_in_y - direct_price_target_in_y).abs() / lower) * 10_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolMeta;
    use alloy::primitives::address;

    fn snapshot(
        pool: Address,
        token0: Address,
        token1: Address,
        reserve0: u128,
        reserve1: u128,
        block_number: u64,
    ) -> PoolSnapshot {
        PoolSnapshot {
            meta: PoolMeta {
                address: pool,
                dex: "test".to_string(),
                fee_bps: 30,
            },
            token0,
            token1,
            reserve0,
            reserve1,
            block_number,
        }
    }

    #[test]
    fn identical_reserves_same_block_gives_zero_gap() {
        let token_a = address!("0000000000000000000000000000000000000001");
        let token_b = address!("0000000000000000000000000000000000000002");
        let pool_1 = address!("0000000000000000000000000000000000000a01");
        let pool_2 = address!("0000000000000000000000000000000000000b02");

        let a = snapshot(pool_1, token_a, token_b, 1_000, 1_000, 100);
        let b = snapshot(pool_2, token_a, token_b, 1_000, 1_000, 100);

        let gap = two_leg_gap_bps(&a, &b).unwrap();
        assert!(gap.abs() < 1e-9);
    }

    #[test]
    fn mismatched_block_numbers_are_rejected() {
        let token_a = address!("0000000000000000000000000000000000000001");
        let token_b = address!("0000000000000000000000000000000000000002");
        let pool_1 = address!("0000000000000000000000000000000000000a01");
        let pool_2 = address!("0000000000000000000000000000000000000b02");

        let a = snapshot(pool_1, token_a, token_b, 1_000, 1_000, 100);
        let b = snapshot(pool_2, token_a, token_b, 1_000, 1_000, 101);

        assert!(two_leg_gap_bps(&a, &b).is_err());
    }
}
