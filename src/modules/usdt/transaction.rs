use super::{user_operation::UserOperation, UsdtError};
use alloy_primitives::{Bytes, B256};
use alloy_sol_types::sol;
use bitcoin::secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

sol! {
    struct PackedOperation {
        address sender;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        bytes32 accountGasLimits;
        uint256 preVerificationGas;
        bytes32 gasFees;
        bytes paymasterAndData;
        bytes signature;
    }
    interface EntryPoint {
        function getNonce(address sender, uint192 key) view returns (uint256);
        function handleOps(PackedOperation[] ops, address beneficiary);
        event UserOperationEvent(bytes32 indexed userOpHash, address indexed sender, address indexed paymaster, uint256 nonce, bool success, uint256 actualGasCost, uint256 actualGasUsed);
    }
    interface Paymaster {
        event UserOperationSponsored(bytes32 indexed userOpHash, address indexed user, uint8 paymasterMode, address token, uint256 tokenAmountPaid, uint256 exchangeRate);
    }
    interface Erc20 {
        function approve(address spender, uint256 amount) returns (bool);
        function transfer(address recipient, uint256 amount) returns (bool);
        function balanceOf(address owner) view returns (uint256);
        event Transfer(address indexed from, address indexed to, uint256 value);
    }
}

pub(super) fn operation_logs(
    receipt: &serde_json::Value,
    hash: B256,
) -> Result<&[serde_json::Value], UsdtError> {
    use alloy_sol_types::SolEvent;
    let logs = receipt["logs"]
        .as_array()
        .ok_or(UsdtError::InvalidResponse)?;
    let mut start = 0;
    for (index, log) in logs.iter().enumerate() {
        let address: alloy_primitives::Address = serde_json::from_value(log["address"].clone())?;
        if address != super::account::ENTRY_POINT {
            continue;
        }
        if let Ok(event) = EntryPoint::UserOperationEvent::decode_log_data(&event_data(log)?) {
            if event.userOpHash == hash {
                return Ok(&logs[start..=index]);
            }
            start = index + 1;
        }
    }
    Err(UsdtError::InvalidResponse)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Plan {
    pub operation: UserOperation,
    pub created_block: u64,
    pub expires_at: u64,
}

impl Plan {
    pub fn sign(&mut self, key: &SecretKey) -> Result<(B256, String), UsdtError> {
        let hash = self.operation.sign(key, super::types::CHAIN_ID)?;
        Ok((hash, serde_json::to_string(self)?))
    }
}

pub(super) fn event_data(
    value: &serde_json::Value,
) -> Result<alloy_primitives::LogData, UsdtError> {
    let topics: Vec<B256> = serde_json::from_value(value["topics"].clone())?;
    let data: Bytes = serde_json::from_value(value["data"].clone())?;
    alloy_primitives::LogData::new(topics, data).ok_or(UsdtError::InvalidResponse)
}
