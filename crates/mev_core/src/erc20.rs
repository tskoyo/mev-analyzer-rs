// use alloy::sol;
// use alloy::sol_types::SolCall;

use alloy::sol;

sol! {
    #[sol(rpc)]
    abstract contract ERC20 {
        function decimals() public view virtual returns (uint8);
        function symbol() public view virtual returns (string memory);
    }
}
