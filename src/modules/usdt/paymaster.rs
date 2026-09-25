use super::{
    account::ENTRY_POINT,
    amount::with_margin,
    rpc::Rpc,
    transaction::Erc20,
    user_operation::{Authorization, UserOperation},
    UsdtError,
};
use alloy_primitives::{address, Address, Bytes, U256};
use alloy_sol_types::SolCall;
use serde::Deserialize;
use serde_json::json;

use super::types::{CHAIN_ID as ARBITRUM_CHAIN_ID, TOKEN as USDT0};
pub(super) const PAYMASTER: Address = address!("888888888888Ec68A58AB8094Cc1AD20Ba3D2402");
const MIN_PAYMASTER_VALIDITY_SECONDS: u64 = 15;
const MAX_PAYMASTER_VALIDITY_SECONDS: u64 = 900;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GasPrice {
    max_fee_per_gas: U256,
    max_priority_fee_per_gas: U256,
}

#[derive(Deserialize)]
struct GasPrices {
    slow: GasPrice,
    fast: GasPrice,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenQuote {
    paymaster: Address,
    token: Address,
    post_op_gas: U256,
    exchange_rate: U256,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymasterData {
    paymaster: Address,
    paymaster_data: Bytes,
    paymaster_verification_gas_limit: Option<U256>,
    paymaster_post_op_gas_limit: Option<U256>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GasEstimate {
    call_gas_limit: U256,
    verification_gas_limit: U256,
    pre_verification_gas: U256,
    paymaster_verification_gas_limit: Option<U256>,
    paymaster_post_op_gas_limit: Option<U256>,
}

impl GasEstimate {
    fn apply(&self, op: &mut UserOperation) -> Result<bool, UsdtError> {
        let mut changed = false;
        // Pre-verification estimates also vary with Arbitrum's L1 data cost.
        for (limit, estimate) in [
            (&mut op.call_gas_limit, self.call_gas_limit),
            (&mut op.pre_verification_gas, self.pre_verification_gas),
        ] {
            if estimate > *limit {
                *limit = with_margin(estimate, 10)?;
                changed = true;
            }
        }
        for (limit, estimate) in [
            (&mut op.verification_gas_limit, self.verification_gas_limit),
            (
                &mut op.paymaster_verification_gas_limit,
                self.paymaster_verification_gas_limit.unwrap_or_default(),
            ),
            (
                &mut op.paymaster_post_op_gas_limit,
                self.paymaster_post_op_gas_limit.unwrap_or_default(),
            ),
        ] {
            if estimate > *limit {
                *limit = estimate;
                changed = true;
            }
        }
        Ok(changed)
    }
}

pub(super) struct Pimlico {
    pub rpc: Rpc,
}

impl Pimlico {
    pub async fn prepare(
        &self,
        address: Address,
        nonce: U256,
        authorization: Authorization,
        calls: &[(Address, Bytes)],
        timestamp: u64,
    ) -> Result<(UserOperation, u64, u64), UsdtError> {
        self.rpc.verify_chain().await?;
        #[derive(Deserialize)]
        struct Quotes {
            quotes: Vec<TokenQuote>,
        }
        let quotes: Quotes = self
            .rpc
            .call(
                "pimlico_getTokenQuotes",
                json!([{ "tokens": [USDT0] }, ENTRY_POINT, U256::from(ARBITRUM_CHAIN_ID)]),
            )
            .await?;
        let quote = quotes
            .quotes
            .into_iter()
            .find(|quote| {
                quote.token == USDT0
                    && quote.paymaster == PAYMASTER
                    && !quote.exchange_rate.is_zero()
            })
            .ok_or(UsdtError::UnsupportedRoute)?;
        let price = self.gas_prices().await?.fast;
        let dummy_signature = Bytes::from_static(&alloy_primitives::hex!("fffffffffffffffffffffffffffffff0000000000000000000000000000000007aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c"));
        let mut op = UserOperation {
            sender: address,
            nonce,
            factory: Bytes::from_static(&[0x77, 0x02]),
            factory_data: Bytes::new(),
            call_data: with_approval(calls, U256::MAX),
            call_gas_limit: U256::ZERO,
            verification_gas_limit: U256::ZERO,
            pre_verification_gas: U256::ZERO,
            max_fee_per_gas: price.max_fee_per_gas,
            max_priority_fee_per_gas: price.max_priority_fee_per_gas,
            paymaster: PAYMASTER,
            paymaster_verification_gas_limit: U256::ZERO,
            paymaster_post_op_gas_limit: U256::ZERO,
            paymaster_data: Bytes::new(),
            signature: dummy_signature,
            eip7702_auth: authorization,
        };
        self.apply_paymaster(&mut op, "pm_getPaymasterStubData")
            .await?;
        let estimate: GasEstimate = self
            .rpc
            .call("eth_estimateUserOperationGas", json!([op, ENTRY_POINT]))
            .await?;
        estimate.apply(&mut op)?;
        let estimate_terms = Terms {
            exchange_rate: quote.exchange_rate,
            post_op_gas: quote.post_op_gas,
            constant_fee: U256::ZERO,
            valid_until: 0,
            valid_after: 0,
        };
        let mut allowance = with_margin(estimate_terms.maximum_token_cost(&op)?, 5)?;
        // Provider data can change gas limits and token charges. Refine both before signing.
        for _ in 0..3 {
            op.call_data = with_approval(calls, allowance);
            self.apply_paymaster(&mut op, "pm_getPaymasterData").await?;
            let terms = Terms::decode(&op.paymaster_data)?;
            let estimate: GasEstimate = self
                .rpc
                .call("eth_estimateUserOperationGas", json!([op, ENTRY_POINT]))
                .await?;
            let gas_changed = estimate.apply(&mut op)?;
            let required = terms.maximum_token_cost(&op)?;
            if gas_changed || required > allowance {
                allowance = allowance.max(with_margin(required, 5)?);
                continue;
            }
            if terms.valid_until == 0
                || terms.valid_until > timestamp.saturating_add(MAX_PAYMASTER_VALIDITY_SECONDS)
            {
                return Err(UsdtError::InvalidResponse);
            }
            if terms.valid_after > timestamp
                || terms.valid_until <= timestamp.saturating_add(MIN_PAYMASTER_VALIDITY_SECONDS)
            {
                return Err(UsdtError::QuoteExpired);
            }
            return Ok((
                op,
                required
                    .try_into()
                    .map_err(|_| UsdtError::InvalidResponse)?,
                terms.valid_until,
            ));
        }
        Err(UsdtError::QuoteExpired)
    }

    async fn apply_paymaster(&self, op: &mut UserOperation, method: &str) -> Result<(), UsdtError> {
        let data: PaymasterData = self
            .rpc
            .call(
                method,
                json!([op, ENTRY_POINT, U256::from(ARBITRUM_CHAIN_ID), {"token": USDT0}]),
            )
            .await?;
        if data.paymaster != PAYMASTER
            || (method == "pm_getPaymasterStubData" && data.paymaster_post_op_gas_limit.is_none())
        {
            return Err(UsdtError::InvalidResponse);
        }
        op.paymaster_data = data.paymaster_data;
        // Final data signs the estimated limits; only stub data supplies gas estimates.
        if method == "pm_getPaymasterStubData" {
            if let Some(gas) = data.paymaster_verification_gas_limit {
                op.paymaster_verification_gas_limit = gas;
            }
            if let Some(gas) = data.paymaster_post_op_gas_limit {
                op.paymaster_post_op_gas_limit = gas;
            }
        }
        Ok(())
    }

    pub async fn validate_gas(&self, op: &UserOperation) -> Result<(), UsdtError> {
        let price = self.gas_prices().await?.slow;
        if price.max_fee_per_gas > op.max_fee_per_gas
            || price.max_priority_fee_per_gas > op.max_priority_fee_per_gas
        {
            return Err(UsdtError::QuoteExpired);
        }
        let estimate: GasEstimate = self
            .rpc
            .call("eth_estimateUserOperationGas", json!([op, ENTRY_POINT]))
            .await?;
        if estimate.call_gas_limit > op.call_gas_limit
            || estimate.verification_gas_limit > op.verification_gas_limit
            || estimate.pre_verification_gas > op.pre_verification_gas
            || estimate
                .paymaster_verification_gas_limit
                .is_some_and(|gas| gas > op.paymaster_verification_gas_limit)
            || estimate
                .paymaster_post_op_gas_limit
                .is_some_and(|gas| gas > op.paymaster_post_op_gas_limit)
        {
            return Err(UsdtError::QuoteExpired);
        }
        Ok(())
    }

    async fn gas_prices(&self) -> Result<GasPrices, UsdtError> {
        let prices: GasPrices = self
            .rpc
            .call("pimlico_getUserOperationGasPrice", json!([]))
            .await?;
        for price in [&prices.slow, &prices.fast] {
            if price.max_fee_per_gas.is_zero()
                || price.max_priority_fee_per_gas > price.max_fee_per_gas
            {
                return Err(UsdtError::InvalidResponse);
            }
        }
        if prices.slow.max_fee_per_gas > prices.fast.max_fee_per_gas
            || prices.slow.max_priority_fee_per_gas > prices.fast.max_priority_fee_per_gas
        {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(prices)
    }
}

fn with_approval(calls: &[(Address, Bytes)], amount: U256) -> Bytes {
    let mut batch = vec![(
        USDT0,
        Erc20::approveCall {
            spender: PAYMASTER,
            amount,
        }
        .abi_encode()
        .into(),
    )];
    batch.extend_from_slice(calls);
    super::account::batch(&batch)
}

struct Terms {
    exchange_rate: U256,
    post_op_gas: U256,
    constant_fee: U256,
    valid_until: u64,
    valid_after: u64,
}

impl Terms {
    // SingletonPaymasterV8 inherits the V7 token-fee encoding.
    fn decode(data: &[u8]) -> Result<Self, UsdtError> {
        if data.len() < 182
            || data[0] >> 1 != 1
            || data[1] & !1 != 0
            || Address::from_slice(&data[14..34]) != USDT0
        {
            return Err(UsdtError::InvalidResponse);
        }
        // Pre-funding needs a separate approval; recipient mode can collect unused prefund. Neither is used.
        let constant_present = data[1] & 1 != 0;
        let signature_start = 118 + if constant_present { 16 } else { 0 };
        if !matches!(data.len().checked_sub(signature_start), Some(64 | 65)) {
            return Err(UsdtError::InvalidResponse);
        }
        let terms = Self {
            valid_until: U256::from_be_slice(&data[2..8])
                .try_into()
                .map_err(|_| UsdtError::InvalidResponse)?,
            valid_after: U256::from_be_slice(&data[8..14])
                .try_into()
                .map_err(|_| UsdtError::InvalidResponse)?,
            post_op_gas: U256::from_be_slice(&data[34..50]),
            exchange_rate: U256::from_be_slice(&data[50..82]),
            constant_fee: if constant_present {
                U256::from_be_slice(&data[118..134])
            } else {
                U256::ZERO
            },
        };
        if terms.exchange_rate.is_zero() {
            return Err(UsdtError::InvalidResponse);
        }
        Ok(terms)
    }

    fn maximum_token_cost(&self, op: &UserOperation) -> Result<U256, UsdtError> {
        let overhead = self
            .post_op_gas
            .checked_mul(op.max_fee_per_gas)
            .ok_or(UsdtError::InvalidResponse)?;
        let native = op
            .maximum_native_cost()?
            .checked_add(overhead)
            .ok_or(UsdtError::InvalidResponse)?;
        let scaled = native
            .checked_mul(self.exchange_rate)
            .ok_or(UsdtError::InvalidResponse)?;
        let divisor = U256::from(1_000_000_000_000_000_000u64);
        let rounded = scaled / divisor + U256::from(u8::from(scaled % divisor != U256::ZERO));
        rounded
            .checked_add(self.constant_fee)
            .ok_or(UsdtError::InvalidResponse)
    }
}

// Packed EntryPoint paymaster data starts after the address and two 16-byte gas limits.
pub(super) fn supported_payment(data: &[u8]) -> bool {
    data.get(..20)
        .is_some_and(|address| address == PAYMASTER.as_slice())
        && data
            .get(52..)
            .is_some_and(|terms| Terms::decode(terms).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gas_margins_reject_overflow() {
        let vector: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/eip7702-vectors.json")).unwrap();
        for (call, pre_verification) in [(U256::MAX, U256::ZERO), (U256::ZERO, U256::MAX)] {
            let mut op: UserOperation =
                serde_json::from_value(vector["operation"].clone()).unwrap();
            let estimate = GasEstimate {
                call_gas_limit: call,
                verification_gas_limit: U256::ZERO,
                pre_verification_gas: pre_verification,
                paymaster_verification_gas_limit: None,
                paymaster_post_op_gas_limit: None,
            };
            assert!(matches!(
                estimate.apply(&mut op),
                Err(UsdtError::InvalidResponse)
            ));
        }
    }

    #[test]
    fn token_fee_includes_fixed_charge_and_rejects_other_collection_modes() {
        let mut data = vec![0; 118];
        data[0] = 3;
        data[1] = 1;
        data[14..34].copy_from_slice(USDT0.as_slice());
        data[34..50].copy_from_slice(&50_000u128.to_be_bytes());
        data[50..82].copy_from_slice(&U256::from(3_000_000_000u64).to_be_bytes::<32>());
        data.extend_from_slice(&123u128.to_be_bytes());
        data.extend_from_slice(&[1; 65]);
        let vector: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/eip7702-vectors.json")).unwrap();
        let op: UserOperation = serde_json::from_value(vector["operation"].clone()).unwrap();
        let terms = Terms::decode(&data).unwrap();
        assert_eq!(terms.maximum_token_cost(&op).unwrap(), U256::from(102_123));
        for flags in [2, 4, 8] {
            data[1] = flags;
            assert!(Terms::decode(&data).is_err());
        }
        data[1] = 1;
        data[14] ^= 1;
        assert!(Terms::decode(&data).is_err());
        for length in 0..182 {
            assert!(Terms::decode(&data[..length]).is_err());
        }
    }
}
