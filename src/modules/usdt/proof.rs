use super::{
    amount::token_amount,
    keys::derive_owner_key,
    transaction::{entry_point_event, event_data, operation_logs, Erc20},
    types::{CHAIN_ID, TOKEN},
    user_operation::sign_hash,
    UsdtDestination, UsdtError, UsdtTransferStatus, UsdtWallet,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_sol_types::{eip712_domain, sol, SolEvent, SolStruct};
use bitcoin::secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, Secp256k1,
};
use serde_json::{json, Value};

sol! {
    struct RequestBinding {
        string payer;
        string payee;
        string paymentAppId;
        string paymentRequestId;
        string paymentReference;
        string paymentEndpointIdentifier;
        string periodStartsAt;
        string periodEndsAt;
        string conversionQuoteId;
    }
    struct Erc20Payment {
        bytes32 transactionHash;
        uint256 receiptLogIndex;
        RequestBinding request;
    }
}

/// Immutable Paykit request fields, using the canonical strings from the accepted request/proof.
/// Pubky keys are bare z32; absent period and conversion quote fields are empty strings.
#[derive(Clone, Debug, uniffi::Record)]
pub struct UsdtPaymentProofBinding {
    pub payer: String,
    pub payee: String,
    pub payment_app_id: String,
    pub payment_request_id: String,
    pub payment_reference: String,
    pub payment_endpoint_identifier: String,
    pub period_starts_at: String,
    pub period_ends_at: String,
    pub conversion_quote_id: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct UsdtPaymentProof {
    pub chain_id: String,
    pub transaction_hash: String,
    /// Decimal position in the complete receipt logs array, not the block-wide RPC logIndex.
    pub receipt_log_index: String,
    pub signature: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct UsdtVerifiedPayment {
    /// Verified chain, transaction and receipt position; claim at most once across requests/periods.
    pub payment_id: String,
    /// Existing incoming activity identity (transaction and block-wide log index).
    pub transfer_id: String,
    pub sender: String,
    pub recipient: String,
    pub amount: u64,
    pub timestamp: u64,
}

#[uniffi::export(async_runtime = "tokio")]
impl UsdtWallet {
    /// Signs the executed direct payment using the Paykit ERC-20 EIP-712 profile.
    /// Persist the binding and payment ID before send. Retry this after execution; it never sends.
    pub async fn create_payment_proof(
        &self,
        transfer_id: String,
        binding: UsdtPaymentProofBinding,
        mnemonic: String,
        passphrase: Option<String>,
    ) -> Result<Option<UsdtPaymentProof>, UsdtError> {
        let mnemonic = zeroize::Zeroizing::new(mnemonic);
        let passphrase = passphrase.map(zeroize::Zeroizing::new);
        validate_binding(&binding)?;
        let _guard = self.operation.lock().await;
        let transfer = self
            .store
            .transfer(&transfer_id)?
            .ok_or(UsdtError::InvalidPaymentProof)?;
        if transfer.destination != UsdtDestination::Arbitrum
            || transfer.is_incoming
            || matches!(
                transfer.status,
                UsdtTransferStatus::Failed | UsdtTransferStatus::Replaced
            )
        {
            return Err(UsdtError::InvalidPaymentProof);
        }
        let Some(hash) = transfer.tx_hash else {
            return Ok(None);
        };
        let hash = canonical_hash(&hash)?;
        let Some((receipt, _)) = self.payment_receipt(hash).await? else {
            return Ok(None);
        };
        let operation = transfer
            .user_operation_hash
            .ok_or(UsdtError::InvalidPaymentProof)?
            .parse::<B256>()
            .map_err(|_| UsdtError::InvalidPaymentProof)?;
        let logs = operation_logs(&receipt, operation)?;
        let event = logs
            .last()
            .and_then(|log| entry_point_event(log).ok().flatten())
            .ok_or(UsdtError::InvalidPaymentProof)?;
        if !event.success || event.sender != self.address {
            return Err(UsdtError::InvalidPaymentProof);
        }
        let recipient = transfer
            .recipient
            .parse::<Address>()
            .map_err(|_| UsdtError::InvalidPaymentProof)?;
        let mut selected = None;
        for log in logs {
            let Some(event) = token_transfer(log)? else {
                continue;
            };
            if event.from == self.address
                && event.to == recipient
                && event.value == U256::from(transfer.amount)
            {
                if selected.is_some() {
                    return Err(UsdtError::InvalidPaymentProof);
                }
                selected = Some(receipt_index(log)?);
            }
        }
        let index = selected.ok_or(UsdtError::InvalidPaymentProof)?;
        let digest = proof_digest(CHAIN_ID, hash, index, &binding);
        let key = derive_owner_key(mnemonic, passphrase, self.address)?;
        let signature = sign_hash(digest, &key)?;
        Ok(Some(UsdtPaymentProof {
            chain_id: CHAIN_ID.to_string(),
            transaction_hash: format!("{hash:#x}"),
            receipt_log_index: index.to_string(),
            signature: format!("{signature:#x}"),
        }))
    }

    /// Verifies a successful canonical ERC-20 transfer to this wallet and its request signature.
    /// Ordinary EOA transfers are supported. None means evidence is not yet available.
    /// The caller verifies accepted terms, payment-time deadlines and payment_id deduplication.
    pub async fn verify_payment_proof(
        &self,
        binding: UsdtPaymentProofBinding,
        proof: UsdtPaymentProof,
    ) -> Result<Option<UsdtVerifiedPayment>, UsdtError> {
        validate_binding(&binding)?;
        if proof.chain_id != CHAIN_ID.to_string() {
            return Err(UsdtError::InvalidPaymentProof);
        }
        let hash = canonical_hash(&proof.transaction_hash)?;
        let index = decimal_index(&proof.receipt_log_index)?;
        let sender = proof_sender(
            proof_digest(CHAIN_ID, hash, index, &binding),
            &proof.signature,
        )?;
        let Some((receipt, timestamp)) = self.payment_receipt(hash).await? else {
            return Ok(None);
        };
        let log = receipt["logs"]
            .as_array()
            .ok_or(UsdtError::InvalidResponse)?
            .iter()
            .find(|log| receipt_index(log).ok() == Some(index))
            .ok_or(UsdtError::InvalidPaymentProof)?;
        let transfer = token_transfer(log)?.ok_or(UsdtError::InvalidPaymentProof)?;
        if transfer.from != sender || transfer.to != self.address || transfer.value.is_zero() {
            return Err(UsdtError::InvalidPaymentProof);
        }
        let log_index: U256 = serde_json::from_value(log["logIndex"].clone())?;
        Ok(Some(UsdtVerifiedPayment {
            payment_id: format!("{CHAIN_ID}:{hash:#x}:{index}"),
            transfer_id: format!("{hash:#x}:{log_index}"),
            sender: sender.to_checksum(None),
            recipient: self.receive_address(),
            amount: token_amount(transfer.value)?,
            timestamp,
        }))
    }
}

impl UsdtWallet {
    async fn payment_receipt(&self, hash: B256) -> Result<Option<(Value, u64)>, UsdtError> {
        self.rpc.verify_chain().await?;
        let receipt: Value = self
            .rpc
            .call("eth_getTransactionReceipt", json!([hash]))
            .await?;
        if receipt.is_null() {
            return Ok(None);
        }
        if serde_json::from_value::<B256>(receipt["transactionHash"].clone())? != hash {
            return Err(UsdtError::InvalidResponse);
        }
        if serde_json::from_value::<U256>(receipt["status"].clone())? != U256::from(1) {
            return Err(UsdtError::InvalidPaymentProof);
        }
        let number: U256 = serde_json::from_value(receipt["blockNumber"].clone())?;
        let block = self
            .rpc
            .block(number.try_into().map_err(|_| UsdtError::InvalidResponse)?)
            .await?;
        if serde_json::from_value::<B256>(receipt["blockHash"].clone())? != block.hash {
            return Err(UsdtError::NetworkUnavailable);
        }
        let mut positions = std::collections::HashSet::new();
        for log in receipt["logs"]
            .as_array()
            .ok_or(UsdtError::InvalidResponse)?
        {
            if !positions.insert(receipt_index(log)?)
                || serde_json::from_value::<B256>(log["transactionHash"].clone())? != hash
                || serde_json::from_value::<B256>(log["blockHash"].clone())? != block.hash
                || serde_json::from_value::<U256>(log["blockNumber"].clone())? != number
                || log["removed"].as_bool() == Some(true)
            {
                return Err(UsdtError::InvalidResponse);
            }
        }
        Ok(Some((receipt, block.timestamp()?)))
    }
}

fn validate_binding(binding: &UsdtPaymentProofBinding) -> Result<(), UsdtError> {
    if binding.payment_endpoint_identifier != "usdt-arbitrum-address"
        || [
            &binding.payer,
            &binding.payee,
            &binding.payment_app_id,
            &binding.payment_request_id,
            &binding.payment_reference,
        ]
        .into_iter()
        .any(|s| s.is_empty())
        || [
            &binding.payer,
            &binding.payee,
            &binding.payment_app_id,
            &binding.payment_request_id,
            &binding.payment_reference,
            &binding.period_starts_at,
            &binding.period_ends_at,
            &binding.conversion_quote_id,
        ]
        .into_iter()
        .any(|s| s.len() > 1024 || s.contains('\0'))
    {
        return Err(UsdtError::InvalidPaymentProof);
    }
    Ok(())
}

fn proof_digest(chain: u64, hash: B256, index: U256, binding: &UsdtPaymentProofBinding) -> B256 {
    Erc20Payment {
        transactionHash: hash,
        receiptLogIndex: index,
        request: RequestBinding {
            payer: binding.payer.clone(),
            payee: binding.payee.clone(),
            paymentAppId: binding.payment_app_id.clone(),
            paymentRequestId: binding.payment_request_id.clone(),
            paymentReference: binding.payment_reference.clone(),
            paymentEndpointIdentifier: binding.payment_endpoint_identifier.clone(),
            periodStartsAt: binding.period_starts_at.clone(),
            periodEndsAt: binding.period_ends_at.clone(),
            conversionQuoteId: binding.conversion_quote_id.clone(),
        },
    }
    .eip712_signing_hash(
        &eip712_domain! { name: "Paykit ERC20 Payment", version: "1", chain_id: chain, },
    )
}

fn canonical_hex(value: &str, bytes: usize) -> bool {
    value.len() == 2 + bytes * 2
        && value.starts_with("0x")
        && value.as_bytes()[2..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}
fn canonical_hash(value: &str) -> Result<B256, UsdtError> {
    if !canonical_hex(value, 32) {
        return Err(UsdtError::InvalidPaymentProof);
    }
    value.parse().map_err(|_| UsdtError::InvalidPaymentProof)
}
fn decimal_index(value: &str) -> Result<U256, UsdtError> {
    if value.is_empty()
        || value.len() > 78
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(UsdtError::InvalidPaymentProof);
    }
    U256::from_str_radix(value, 10).map_err(|_| UsdtError::InvalidPaymentProof)
}
fn receipt_index(log: &Value) -> Result<U256, UsdtError> {
    // The proxy supplies the original position before projecting the receipt to protocol events.
    decimal_index(
        log["receiptLogIndex"]
            .as_str()
            .ok_or(UsdtError::InvalidResponse)?,
    )
    .map_err(|_| UsdtError::InvalidResponse)
}
fn token_transfer(log: &Value) -> Result<Option<Erc20::Transfer>, UsdtError> {
    if serde_json::from_value::<Address>(log["address"].clone())? != TOKEN {
        return Ok(None);
    }
    let data = event_data(log)?;
    if data.topics().first() != Some(&Erc20::Transfer::SIGNATURE_HASH) {
        return Ok(None);
    }
    Erc20::Transfer::decode_log_data_validate(&data)
        .map(Some)
        .map_err(|_| UsdtError::InvalidResponse)
}
fn proof_sender(digest: B256, signature: &str) -> Result<Address, UsdtError> {
    if !canonical_hex(signature, 65) {
        return Err(UsdtError::InvalidPaymentProof);
    }
    let signature = signature
        .parse::<Bytes>()
        .map_err(|_| UsdtError::InvalidPaymentProof)?;
    if !matches!(signature[64], 27 | 28) {
        return Err(UsdtError::InvalidPaymentProof);
    }
    let recovery = RecoveryId::from_i32(i32::from(signature[64] - 27))
        .map_err(|_| UsdtError::InvalidPaymentProof)?;
    let signature = RecoverableSignature::from_compact(&signature[..64], recovery)
        .map_err(|_| UsdtError::InvalidPaymentProof)?;
    let standard = signature.to_standard();
    let mut normalized = standard;
    normalized.normalize_s();
    if standard != normalized {
        return Err(UsdtError::InvalidPaymentProof);
    }
    let public = Secp256k1::new()
        .recover_ecdsa(&Message::from_digest(digest.0), &signature)
        .map_err(|_| UsdtError::InvalidPaymentProof)?;
    Ok(Address::from_raw_public_key(
        &public.serialize_uncompressed()[1..],
    ))
}

#[cfg(test)]
mod tests;
