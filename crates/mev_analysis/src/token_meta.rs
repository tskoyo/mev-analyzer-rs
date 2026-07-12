use alloy::primitives::Address;
use alloy::sol_types::SolCall;
use mev_core::ERC20;
use revm::context::TxEnv;
use revm::context::result::{ExecutionResult, Output};
use revm::primitives::TxKind;

/// `call` runs a `TxEnv` against whatever forked EVM the caller is holding
/// and returns its `ExecutionResult` (or `None` on a transact error). Taking
/// a closure instead of the EVM itself keeps this function decoupled from
/// revm's `Evm<...>` generic parameters, which are awkward to name across a
/// crate boundary.
pub fn fetch_token_meta(
    mut call: impl FnMut(TxEnv) -> Option<ExecutionResult>,
    token: Address,
) -> (String, u8) {
    let dec_tx = TxEnv {
        caller: Address::ZERO,
        kind: TxKind::Call(token),
        data: ERC20::decimalsCall {}.abi_encode().into(),
        gas_limit: 1_000_000,
        gas_price: 1_000_000_000_000u128,
        ..Default::default()
    };
    let decimals = match call(dec_tx) {
        Some(ExecutionResult::Success {
            output: Output::Call(bytes),
            ..
        }) => ERC20::decimalsCall::abi_decode_returns(&bytes).unwrap_or(18),
        _ => 18,
    };

    let sym_tx = TxEnv {
        caller: Address::ZERO,
        kind: TxKind::Call(token),
        data: ERC20::symbolCall {}.abi_encode().into(),
        gas_limit: 1_000_000,
        gas_price: 1_000_000_000_000u128,
        ..Default::default()
    };
    let symbol = match call(sym_tx) {
        Some(ExecutionResult::Success {
            output: Output::Call(bytes),
            ..
        }) => ERC20::symbolCall::abi_decode_returns(&bytes)
            .unwrap_or_else(|_| "failed to parse".into()),
        _ => "???".into(),
    };

    (symbol, decimals)
}
