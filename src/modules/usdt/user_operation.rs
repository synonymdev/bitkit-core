use super::{
    account::{DELEGATE, ENTRY_POINT},
    keys::key_address,
    types::CHAIN_ID,
    UsdtError,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_rlp::Encodable;
use alloy_sol_types::{eip712_domain, sol, SolStruct};
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};

sol! {
    struct PackedUserOperation {
        address sender;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        bytes32 accountGasLimits;
        uint256 preVerificationGas;
        bytes32 gasFees;
        bytes paymasterAndData;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Authorization {
    pub chain_id: U256,
    pub address: Address,
    pub nonce: U256,
    pub y_parity: U256,
    pub r: B256,
    pub s: B256,
}

impl Authorization {
    pub fn dummy(nonce: u64) -> Self {
        Self {
            chain_id: U256::from(CHAIN_ID),
            address: DELEGATE,
            nonce: U256::from(nonce),
            y_parity: U256::ZERO,
            r: B256::repeat_byte(0x11),
            s: B256::repeat_byte(0x22),
        }
    }

    pub fn hash(&self) -> Result<B256, UsdtError> {
        let nonce: u64 = self
            .nonce
            .try_into()
            .map_err(|_| UsdtError::InvalidResponse)?;
        if self.chain_id != U256::from(CHAIN_ID) || self.address != DELEGATE || nonce == u64::MAX {
            return Err(UsdtError::InvalidResponse);
        }
        let mut payload = Vec::new();
        CHAIN_ID.encode(&mut payload);
        self.address.as_slice().encode(&mut payload);
        nonce.encode(&mut payload);
        let mut encoded = vec![0x05];
        alloy_rlp::Header {
            list: true,
            payload_length: payload.len(),
        }
        .encode(&mut encoded);
        encoded.extend(payload);
        Ok(alloy_primitives::keccak256(encoded))
    }

    fn sign(&mut self, key: &SecretKey) -> Result<(), UsdtError> {
        let signature = sign_hash(self.hash()?, key)?;
        self.r = B256::from_slice(&signature[..32]);
        self.s = B256::from_slice(&signature[32..64]);
        self.y_parity = U256::from(signature[64] - 27);
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UserOperation {
    pub sender: Address,
    pub nonce: U256,
    pub factory: Bytes,
    pub factory_data: Bytes,
    pub call_data: Bytes,
    pub call_gas_limit: U256,
    pub verification_gas_limit: U256,
    pub pre_verification_gas: U256,
    pub max_fee_per_gas: U256,
    pub max_priority_fee_per_gas: U256,
    pub paymaster: Address,
    pub paymaster_verification_gas_limit: U256,
    pub paymaster_post_op_gas_limit: U256,
    pub paymaster_data: Bytes,
    pub signature: Bytes,
    pub eip7702_auth: Authorization,
}

impl UserOperation {
    pub fn paymaster_and_data(&self) -> Result<Bytes, UsdtError> {
        Ok([
            self.paymaster.as_slice(),
            &narrow(self.paymaster_verification_gas_limit)?.to_be_bytes(),
            &narrow(self.paymaster_post_op_gas_limit)?.to_be_bytes(),
            &self.paymaster_data,
        ]
        .concat()
        .into())
    }

    pub fn hash(&self, chain_id: u64) -> Result<B256, UsdtError> {
        if self.factory.as_ref() != [0x77, 0x02] || !self.factory_data.is_empty() {
            return Err(UsdtError::InvalidResponse);
        }
        let packed = PackedUserOperation {
            sender: self.sender,
            nonce: self.nonce,
            // EntryPoint replaces the 7702 marker with the active delegate before hashing.
            initCode: Bytes::copy_from_slice(DELEGATE.as_slice()),
            callData: self.call_data.clone(),
            accountGasLimits: pack(self.verification_gas_limit, self.call_gas_limit)?,
            preVerificationGas: self.pre_verification_gas,
            gasFees: pack(self.max_priority_fee_per_gas, self.max_fee_per_gas)?,
            paymasterAndData: self.paymaster_and_data()?,
        };
        Ok(packed.eip712_signing_hash(&eip712_domain! {
            name: "ERC4337", version: "1", chain_id: chain_id, verifying_contract: ENTRY_POINT,
        }))
    }

    pub fn sign(&mut self, key: &SecretKey, chain_id: u64) -> Result<B256, UsdtError> {
        if key_address(key) != self.sender || chain_id != CHAIN_ID {
            return Err(UsdtError::InvalidCredentials);
        }
        self.eip7702_auth.sign(key)?;
        let hash = self.hash(chain_id)?;
        self.signature = sign_hash(hash, key)?;
        Ok(hash)
    }

    pub fn maximum_native_cost(&self) -> Result<U256, UsdtError> {
        let gas = [
            self.call_gas_limit,
            self.verification_gas_limit,
            self.pre_verification_gas,
            self.paymaster_verification_gas_limit,
            self.paymaster_post_op_gas_limit,
        ]
        .into_iter()
        .try_fold(U256::ZERO, |total, value| total.checked_add(value))
        .ok_or(UsdtError::InvalidResponse)?;
        gas.checked_mul(self.max_fee_per_gas)
            .ok_or(UsdtError::InvalidResponse)
    }
}

fn sign_hash(hash: B256, key: &SecretKey) -> Result<Bytes, UsdtError> {
    let (recovery, signature) = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest(hash.0), key)
        .serialize_compact();
    if recovery.to_i32() > 1 {
        return Err(UsdtError::InvalidCredentials);
    }
    let mut encoded = signature.to_vec();
    encoded.push(27 + recovery.to_i32() as u8);
    Ok(encoded.into())
}

fn pack(high: U256, low: U256) -> Result<B256, UsdtError> {
    Ok(B256::from_slice(
        &[narrow(high)?.to_be_bytes(), narrow(low)?.to_be_bytes()].concat(),
    ))
}

fn narrow(value: U256) -> Result<u128, UsdtError> {
    value.try_into().map_err(|_| UsdtError::InvalidResponse)
}
