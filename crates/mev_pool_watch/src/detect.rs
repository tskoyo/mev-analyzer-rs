use crate::candidate::{ArbCandidate, LoopKind, PoolLeg};
use crate::connectors::ConnectorPools;
use crate::economics::{self, AmmLeg};
use crate::gap;
use crate::grouping::{self, TokenPairKey};
use crate::pool::PoolSnapshot;
use alloy::primitives::Address;
use std::collections::HashMap;

/// Detects every 2-leg and 3-leg candidate for `target` given the pools
/// known to hold it (`all_snapshots` should include every pool from the
/// same poll cycle, not just `target`'s pools, so 3-leg connector lookups
/// can find their own reserves).
///
/// Purely computational -- no RPC calls here. `decimals` and the snapshots
/// themselves must already be fetched (see `reserves.rs`/`token_decimals.rs`).
pub fn detect_candidates(
    target: Address,
    all_snapshots: &[PoolSnapshot],
    decimals: &HashMap<Address, u8>,
    connectors: &ConnectorPools,
    gas_cost_in_start_token: f64,
) -> Vec<ArbCandidate> {
    let target_snapshots: Vec<PoolSnapshot> = all_snapshots
        .iter()
        .filter(|s| s.holds(target))
        .cloned()
        .collect();

    let groups = grouping::group_by_pair(&target_snapshots);
    let mut candidates = Vec::new();

    // 2-leg: every pairwise combination within a same-pair bucket. token0/
    // token1 equality is already guaranteed by the grouping key.
    for pools in groups.values() {
        if pools.len() < 2 {
            continue;
        }
        for i in 0..pools.len() {
            for j in (i + 1)..pools.len() {
                if let Some(c) =
                    try_two_leg(target, &pools[i], &pools[j], decimals, gas_cost_in_start_token)
                {
                    candidates.push(c);
                }
            }
        }
    }

    // 3-leg: pools in different pair-groups + a known connector between
    // their quote tokens. No connector known -> skip, never fabricate a
    // gap via an external price feed.
    let keys: Vec<TokenPairKey> = groups.keys().copied().collect();
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            candidates.extend(try_three_leg(
                target,
                &groups[&keys[i]],
                &groups[&keys[j]],
                all_snapshots,
                decimals,
                connectors,
                gas_cost_in_start_token,
            ));
        }
    }

    candidates
}

fn norm(raw: u128, decimals: u8) -> f64 {
    raw as f64 / 10f64.powi(decimals as i32)
}

fn try_two_leg(
    target: Address,
    a: &PoolSnapshot,
    b: &PoolSnapshot,
    decimals: &HashMap<Address, u8>,
    gas_cost_in_start_token: f64,
) -> Option<ArbCandidate> {
    let gap_bps = gap::two_leg_gap_bps(a, b).ok()?;
    let quote = a.other_token(target)?;

    let target_decimals = *decimals.get(&target)?;
    let quote_decimals = *decimals.get(&quote)?;

    let a_target = norm(a.reserve_of(target)?, target_decimals);
    let a_quote = norm(a.reserve_of(quote)?, quote_decimals);
    let b_target = norm(b.reserve_of(target)?, target_decimals);
    let b_quote = norm(b.reserve_of(quote)?, quote_decimals);

    if a_target <= 0.0 || b_target <= 0.0 {
        return None;
    }

    // price of target in quote-terms; buy where it's cheap, sell where it's
    // expensive, ending back in `target`.
    let price_a = a_quote / a_target;
    let price_b = b_quote / b_target;

    let (cheap, cheap_target, cheap_quote, expensive, expensive_target, expensive_quote) =
        if price_a < price_b {
            (a, a_target, a_quote, b, b_target, b_quote)
        } else {
            (b, b_target, b_quote, a, a_target, a_quote)
        };

    let legs = [
        AmmLeg {
            reserve_in: cheap_target,
            reserve_out: cheap_quote,
            fee_bps: cheap.meta.fee_bps,
        },
        AmmLeg {
            reserve_in: expensive_quote,
            reserve_out: expensive_target,
            fee_bps: expensive.meta.fee_bps,
        },
    ];

    let optimal_input = economics::optimal_input(&legs, 0.0, cheap_target.min(expensive_target));
    let gross = economics::gross_profit(optimal_input, &legs);
    let net = economics::net_profit(optimal_input, &legs, gas_cost_in_start_token);

    Some(ArbCandidate {
        legs: vec![
            PoolLeg {
                pool: cheap.meta.address,
                token_in: target,
                token_out: quote,
                reserve_in: cheap.reserve_of(target)?,
                reserve_out: cheap.reserve_of(quote)?,
            },
            PoolLeg {
                pool: expensive.meta.address,
                token_in: quote,
                token_out: target,
                reserve_in: expensive.reserve_of(quote)?,
                reserve_out: expensive.reserve_of(target)?,
            },
        ],
        loop_kind: LoopKind::TwoLeg,
        block_number: a.block_number,
        gap_bps,
        estimated_optimal_input: optimal_input,
        estimated_gross_profit: gross,
        estimated_net_profit: net,
    })
}

/// Picks the deepest pool (by `target`'s reserve) as the representative for
/// a pair-group -- Stage 2 is an approximate signal, not the final trusted
/// number, so we don't need to try every pool in a group here.
fn deepest(pools: &[PoolSnapshot], target: Address) -> Option<&PoolSnapshot> {
    pools
        .iter()
        .max_by(|a, b| {
            let ra = a.reserve_of(target).unwrap_or(0);
            let rb = b.reserve_of(target).unwrap_or(0);
            ra.cmp(&rb)
        })
}

#[allow(clippy::too_many_arguments)]
fn try_three_leg(
    target: Address,
    group_x: &[PoolSnapshot],
    group_y: &[PoolSnapshot],
    all_snapshots: &[PoolSnapshot],
    decimals: &HashMap<Address, u8>,
    connectors: &ConnectorPools,
    gas_cost_in_start_token: f64,
) -> Vec<ArbCandidate> {
    let mut out = Vec::new();

    let Some(leg_ax) = deepest(group_x, target) else {
        return out;
    };
    let Some(leg_ay) = deepest(group_y, target) else {
        return out;
    };

    let Some(x) = leg_ax.other_token(target) else {
        return out;
    };
    let Some(y) = leg_ay.other_token(target) else {
        return out;
    };

    let key = TokenPairKey::new(x, y);
    let Some(connector_addr) = connectors.get(&key) else {
        return out; // no known connector -- never fabricate a gap
    };
    let Some(connector) = all_snapshots.iter().find(|s| s.meta.address == *connector_addr) else {
        return out; // connector configured but not in this poll cycle's snapshots
    };

    // Build both directions -- the real swap economics (not a heuristic)
    // decides which, if either, is actually profitable.
    if let Some(c) = build_three_leg(
        target,
        leg_ax,
        connector,
        leg_ay,
        x,
        y,
        decimals,
        gas_cost_in_start_token,
    ) {
        out.push(c);
    }
    if let Some(c) = build_three_leg(
        target,
        leg_ay,
        connector,
        leg_ax,
        y,
        x,
        decimals,
        gas_cost_in_start_token,
    ) {
        out.push(c);
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn build_three_leg(
    target: Address,
    leg_first: &PoolSnapshot,
    connector: &PoolSnapshot,
    leg_last: &PoolSnapshot,
    first: Address,
    second: Address,
    decimals: &HashMap<Address, u8>,
    gas_cost_in_start_token: f64,
) -> Option<ArbCandidate> {
    let target_decimals = *decimals.get(&target)?;
    let first_decimals = *decimals.get(&first)?;
    let second_decimals = *decimals.get(&second)?;

    let leg1 = AmmLeg {
        reserve_in: norm(leg_first.reserve_of(target)?, target_decimals),
        reserve_out: norm(leg_first.reserve_of(first)?, first_decimals),
        fee_bps: leg_first.meta.fee_bps,
    };
    let leg2 = AmmLeg {
        reserve_in: norm(connector.reserve_of(first)?, first_decimals),
        reserve_out: norm(connector.reserve_of(second)?, second_decimals),
        fee_bps: connector.meta.fee_bps,
    };
    let leg3 = AmmLeg {
        reserve_in: norm(leg_last.reserve_of(second)?, second_decimals),
        reserve_out: norm(leg_last.reserve_of(target)?, target_decimals),
        fee_bps: leg_last.meta.fee_bps,
    };
    let legs = [leg1, leg2, leg3];

    if leg1.reserve_in <= 0.0 {
        return None;
    }

    let optimal_input = economics::optimal_input(&legs, 0.0, leg1.reserve_in);
    let gross = economics::gross_profit(optimal_input, &legs);
    let net = economics::net_profit(optimal_input, &legs, gas_cost_in_start_token);

    let gap_bps = gap::three_leg_gap_bps(target, leg_first, connector, leg_last, decimals).ok()?;

    Some(ArbCandidate {
        legs: vec![
            PoolLeg {
                pool: leg_first.meta.address,
                token_in: target,
                token_out: first,
                reserve_in: leg_first.reserve_of(target)?,
                reserve_out: leg_first.reserve_of(first)?,
            },
            PoolLeg {
                pool: connector.meta.address,
                token_in: first,
                token_out: second,
                reserve_in: connector.reserve_of(first)?,
                reserve_out: connector.reserve_of(second)?,
            },
            PoolLeg {
                pool: leg_last.meta.address,
                token_in: second,
                token_out: target,
                reserve_in: leg_last.reserve_of(second)?,
                reserve_out: leg_last.reserve_of(target)?,
            },
        ],
        loop_kind: LoopKind::ThreeLeg {
            connector: connector.meta.address,
        },
        block_number: leg_first.block_number,
        gap_bps,
        estimated_optimal_input: optimal_input,
        estimated_gross_profit: gross,
        estimated_net_profit: net,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolMeta;
    use alloy::primitives::address;

    fn decimals_map(pairs: &[(Address, u8)]) -> HashMap<Address, u8> {
        pairs.iter().copied().collect()
    }

    fn snapshot(
        pool: Address,
        token0: Address,
        token1: Address,
        reserve0: u128,
        reserve1: u128,
        fee_bps: u32,
    ) -> PoolSnapshot {
        PoolSnapshot {
            meta: PoolMeta {
                address: pool,
                dex: "test".to_string(),
                fee_bps,
            },
            token0,
            token1,
            reserve0,
            reserve1,
            block_number: 100,
        }
    }

    #[test]
    fn three_leg_connector_present_yields_a_candidate() {
        let target = address!("0000000000000000000000000000000000000001");
        let x = address!("0000000000000000000000000000000000000002");
        let y = address!("0000000000000000000000000000000000000003");

        let pool_ax = snapshot(
            address!("0000000000000000000000000000000000000a01"),
            target,
            x,
            1_000_000,
            1_000_000_000_000_000_000,
            30,
        );
        let pool_ay = snapshot(
            address!("0000000000000000000000000000000000000a02"),
            target,
            y,
            1_000_000,
            1_050_000_000,
            30,
        );
        let connector = snapshot(
            address!("0000000000000000000000000000000000000a03"),
            x,
            y,
            1_000_000_000_000_000_000,
            1_000_000_000,
            30,
        );

        let decimals = decimals_map(&[(target, 6), (x, 18), (y, 6)]);
        let mut connectors = ConnectorPools::new();
        connectors.insert(TokenPairKey::new(x, y), connector.meta.address);

        let all = vec![pool_ax.clone(), pool_ay.clone(), connector.clone()];
        let candidates = detect_candidates(target, &all, &decimals, &connectors, 0.0);

        assert!(
            candidates.iter().any(|c| matches!(c.loop_kind, LoopKind::ThreeLeg { .. })),
            "expected at least one 3-leg candidate when a connector is present"
        );
    }

    #[test]
    fn three_leg_without_connector_yields_nothing() {
        let target = address!("0000000000000000000000000000000000000001");
        let x = address!("0000000000000000000000000000000000000002");
        let y = address!("0000000000000000000000000000000000000003");

        let pool_ax = snapshot(
            address!("0000000000000000000000000000000000000a01"),
            target,
            x,
            1_000_000,
            1_000_000_000_000_000_000,
            30,
        );
        let pool_ay = snapshot(
            address!("0000000000000000000000000000000000000a02"),
            target,
            y,
            1_000_000,
            1_050_000_000,
            30,
        );

        let decimals = decimals_map(&[(target, 6), (x, 18), (y, 6)]);
        let connectors = ConnectorPools::new(); // empty -- no connector known

        let all = vec![pool_ax, pool_ay];
        let candidates = detect_candidates(target, &all, &decimals, &connectors, 0.0);

        assert!(
            candidates.is_empty(),
            "must never fabricate a 3-leg candidate without a known connector"
        );
    }
}
