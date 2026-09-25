use super::UsdtError;
use alloy_primitives::{address, Address, Bytes, U256};
use alloy_sol_types::{sol, SolCall};

pub(super) const ENTRY_POINT: Address = address!("4337084D9E255Ff0702461CF8895CE9E3b5Ff108");
pub(super) const DELEGATE: Address = address!("e6Cae83BdE06E4c305530e199D7217f42808555B");

sol! {
    struct Call { address target; uint256 value; bytes data; }
    interface SimpleAccount {
        function execute(address target, uint256 value, bytes data);
        function executeBatch(Call[] calls);
    }
}

pub(super) fn validate_delegation(code: &[u8]) -> Result<(), UsdtError> {
    if code.is_empty()
        || (code.len() == 23 && code[..3] == [0xef, 0x01, 0x00] && code[3..] == DELEGATE[..])
    {
        return Ok(());
    }
    Err(UsdtError::UnsupportedDelegation)
}

pub(super) fn batch(calls: &[(Address, Bytes)]) -> Bytes {
    SimpleAccount::executeBatchCall {
        calls: calls
            .iter()
            .map(|(target, data)| Call {
                target: *target,
                value: U256::ZERO,
                data: data.clone(),
            })
            .collect(),
    }
    .abi_encode()
    .into()
}
