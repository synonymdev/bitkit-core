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
    #[derive(Debug)]
    struct SendParam {
        uint32 dstEid;
        bytes32 to;
        uint256 amountLD;
        uint256 minAmountLD;
        bytes extraOptions;
        bytes composeMsg;
        bytes oftCmd;
    }
    #[derive(Debug)]
    struct MessagingFee {
        uint256 nativeFee;
        uint256 lzTokenFee;
    }
    struct OFTLimit {
        uint256 minAmountLD;
        uint256 maxAmountLD;
    }
    struct OFTFeeDetail {
        int256 feeAmountLD;
        string description;
    }
    struct OFTReceipt {
        uint256 amountSentLD;
        uint256 amountReceivedLD;
    }
    interface Oft {
        function token() external view returns (address);
        function peers(uint32 eid) external view returns (bytes32);
        function quoteOFT(SendParam param) external view returns (OFTLimit limit, OFTFeeDetail[] fees, OFTReceipt receipt);
        function quoteSend(SendParam param, bool payInLzToken) external view returns (MessagingFee fee);
        event OFTSent(bytes32 indexed guid, uint32 dstEid, address indexed fromAddress, uint256 amountSentLD, uint256 amountReceivedLD);
    }
    interface BridgeHelper {
        function token() external view returns (address);
        function maxGas() external view returns (uint256);
        function quoteSend(SendParam param, MessagingFee fee) external view returns (uint256 totalAmount);
        function send(address oft, SendParam param, MessagingFee fee) external payable;
        event LogSend(address indexed sender, address indexed oft, uint256 amountLD, uint256 nativeFee, uint256 feeInToken, uint256 totalAmount);
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

pub(super) fn entry_point_event(
    log: &serde_json::Value,
) -> Result<Option<EntryPoint::UserOperationEvent>, UsdtError> {
    use alloy_sol_types::SolEvent;
    let address: alloy_primitives::Address = serde_json::from_value(log["address"].clone())?;
    if address != super::account::ENTRY_POINT {
        return Ok(None);
    }
    let data = event_data(log)?;
    if data.topics().first() != Some(&EntryPoint::UserOperationEvent::SIGNATURE_HASH) {
        return Ok(None);
    }
    EntryPoint::UserOperationEvent::decode_log_data(&data)
        .map(Some)
        .map_err(|_| UsdtError::InvalidResponse)
}

pub(super) fn operation_logs(
    receipt: &serde_json::Value,
    hash: B256,
) -> Result<&[serde_json::Value], UsdtError> {
    let logs = receipt["logs"]
        .as_array()
        .ok_or(UsdtError::InvalidResponse)?;
    let mut start = 0;
    for (index, log) in logs.iter().enumerate() {
        if let Some(event) = entry_point_event(log)? {
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
