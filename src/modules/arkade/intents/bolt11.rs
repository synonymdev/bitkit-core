//! The BOLT11 facts the corridor needs, read like `decodeBolt11` in `client/corridors/bolt11.ts`:
//! signature-blind, amount floored to whole sats, amountless as 0.

use super::error::Error;
use super::error::Result;
use lightning_invoice::Bolt11Invoice;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceFacts {
    pub raw: String,
    /// `sha256(P)`, lowercase hex.
    pub payment_hash: String,
    /// 0 for an amountless invoice.
    pub amount_sats: u64,
    /// Absolute expiry, unix seconds.
    pub expires_at: u64,
}

pub fn decode(invoice: &str) -> Result<InvoiceFacts> {
    let parsed = Bolt11Invoice::from_str(invoice.trim())
        .map_err(|e| Error::Invoice(format!("undecodable BOLT11: {e}")))?;
    let timestamp = parsed.duration_since_epoch().as_secs();
    Ok(InvoiceFacts {
        raw: invoice.trim().to_string(),
        payment_hash: hex::encode(parsed.payment_hash().as_ref() as &[u8]),
        amount_sats: parsed.amount_milli_satoshis().unwrap_or(0) / 1000,
        expires_at: timestamp + parsed.expiry_time().as_secs(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_the_ts_decoder() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap();
        let ts = &fixture["receive"]["invoice"];
        let facts = super::decode(ts["raw"].as_str().unwrap()).unwrap();
        assert_eq!(facts.payment_hash, ts["paymentHash"].as_str().unwrap());
        assert_eq!(facts.amount_sats, ts["amountSats"].as_u64().unwrap());
        assert_eq!(facts.expires_at, ts["expiresAt"].as_u64().unwrap());
    }
}
