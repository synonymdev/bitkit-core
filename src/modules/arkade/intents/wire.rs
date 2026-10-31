//! RFQ v1 messages, matching `@arkade-os/swap` 0.1.0-rc.34 byte for byte on the way out.
//!
//! Requests are exact: the solver's schema is `.strict()` and refuses any undeclared key (or a
//! `null`) as `unsupported_payload`. Replies are read leniently, as the TS client does: unknown
//! fields are kept, and amounts read as a JSON number or a canonical decimal string.

use super::error::Error;
use super::error::Result;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

pub const LIGHTNING_RECEIVE_PAIR: &str = "lightning:BTC->arkade:BTC";
pub const LIGHTNING_SEND_PAIR: &str = "arkade:BTC->lightning:BTC";

#[derive(Debug, Clone, Serialize)]
pub struct RfqRequest<P> {
    pub v: u8,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub rfq_id: String,
    pub pair: &'static str,
    pub amount_side: &'static str,
    /// Canonical decimal string of sats; absent on a Lightning send (the invoice fixes it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    pub profile: P,
}

/// `lightning:BTC->arkade:BTC`. No `claim_packet`: no covclaimd is deployed for this corridor
/// and the solver refuses an empty one.
#[derive(Debug, Clone, Serialize)]
pub struct ReceiveProfile {
    pub payment_hash: String,
    pub payout_address: String,
    pub payout_pubkey: String,
}

/// `arkade:BTC->lightning:BTC`.
#[derive(Debug, Clone, Serialize)]
pub struct SendProfile {
    pub invoice: String,
    pub refund_address: String,
    pub client_refund_pubkey: String,
}

pub fn receive_request(rfq_id: &str, amount_sat: u64, profile: ReceiveProfile) -> Value {
    serde_json::to_value(RfqRequest {
        v: 1,
        kind: "rfq_request",
        rfq_id: rfq_id.to_string(),
        pair: LIGHTNING_RECEIVE_PAIR,
        // `amountOn: "give"`: the payer pays exactly `amount`.
        amount_side: "from",
        amount: Some(amount_sat.to_string()),
        profile,
    })
    .expect("serializable")
}

pub fn send_request(rfq_id: &str, profile: SendProfile) -> Value {
    serde_json::to_value(RfqRequest {
        v: 1,
        kind: "rfq_request",
        rfq_id: rfq_id.to_string(),
        pair: LIGHTNING_SEND_PAIR,
        amount_side: "to",
        amount: None,
        profile,
    })
    .expect("serializable")
}

/// A sats amount on the wire: a JSON number or a canonical decimal string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireAmount {
    Number(u64),
    String(String),
}

impl WireAmount {
    pub fn sats(&self, field: &'static str) -> Result<u64> {
        match self {
            WireAmount::Number(n) => Ok(*n),
            WireAmount::String(s) => {
                let canonical = !s.is_empty()
                    && s.len() <= 30
                    && s.bytes().all(|b| b.is_ascii_digit())
                    && (s == "0" || !s.starts_with('0'));
                if !canonical {
                    return Err(Error::verification(field, "a canonical decimal amount", s));
                }
                s.parse()
                    .map_err(|_| Error::verification(field, "an amount in sats", s))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfqQuote {
    pub v: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub rfq_id: String,
    pub pair: String,
    pub from_amount: WireAmount,
    pub to_amount: WireAmount,
    pub solver_pubkey: String,
    pub valid_until: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refund_locktime: Option<u64>,
    #[serde(default)]
    pub profile: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl RfqQuote {
    pub fn give(&self) -> Result<u64> {
        self.from_amount.sats("from_amount")
    }

    pub fn take(&self) -> Result<u64> {
        self.to_amount.sats("to_amount")
    }

    pub fn profile_str(&self, field: &'static str) -> Result<&str> {
        self.profile
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| Error::verification("profile", format!("profile.{field}"), "missing"))
    }

    pub fn profile_u64(&self, field: &'static str) -> Result<u64> {
        self.profile
            .get(field)
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::verification("profile", format!("profile.{field}"), "missing"))
    }
}

/// `expectQuote`: a refusal becomes [`Error::Refused`]; anything not a quote for THIS rfq_id and
/// pair is an error.
pub fn expect_quote(payload: Value, rfq_id: &str, pair: &str) -> Result<RfqQuote> {
    let kind = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "rfq_refusal" {
        let reason = payload
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let error_code = payload
            .get("error_code")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut detail = Map::new();
        for field in ["field", "actual", "expected", "limit", "unit"] {
            if let Some(v) = payload.get(field) {
                detail.insert(field.into(), v.clone());
            }
        }
        return Err(Error::Refused {
            reason,
            error_code,
            detail: Value::Object(detail),
        });
    }
    if kind != "rfq_quote" || payload.get("rfq_id").and_then(Value::as_str) != Some(rfq_id) {
        return Err(Error::Transport(format!("unexpected reply: {kind}")));
    }
    if payload.get("pair").and_then(Value::as_str) != Some(pair) {
        return Err(Error::verification(
            "pair",
            pair,
            payload.get("pair").cloned().unwrap_or(Value::Null),
        ));
    }
    serde_json::from_value(payload).map_err(|e| Error::Transport(format!("malformed quote: {e}")))
}

pub fn new_rfq_id() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_match_the_ts_wire() {
        let fixture: Value = serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap();
        let ts = &fixture["receive"]["request"];
        let ours = receive_request(
            ts["rfq_id"].as_str().unwrap(),
            fixture["receive"]["amount"].as_u64().unwrap(),
            ReceiveProfile {
                payment_hash: ts["profile"]["payment_hash"].as_str().unwrap().into(),
                payout_address: ts["profile"]["payout_address"].as_str().unwrap().into(),
                payout_pubkey: ts["profile"]["payout_pubkey"].as_str().unwrap().into(),
            },
        );
        assert_eq!(
            serde_json::to_string(&ours).unwrap(),
            serde_json::to_string(ts).unwrap()
        );

        let ts = &fixture["send"]["request"];
        let ours = send_request(
            ts["rfq_id"].as_str().unwrap(),
            SendProfile {
                invoice: ts["profile"]["invoice"].as_str().unwrap().into(),
                refund_address: ts["profile"]["refund_address"].as_str().unwrap().into(),
                client_refund_pubkey: ts["profile"]["client_refund_pubkey"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
        );
        assert_eq!(
            serde_json::to_string(&ours).unwrap(),
            serde_json::to_string(ts).unwrap()
        );
    }

    #[test]
    fn reads_quotes_and_refusals() {
        let fixture: Value = serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap();
        let wire = fixture["receive"]["quote"].clone();
        let id = wire["rfq_id"].as_str().unwrap().to_string();
        let quote = expect_quote(wire.clone(), &id, LIGHTNING_RECEIVE_PAIR).unwrap();
        assert_eq!(quote.give().unwrap(), 1000);
        assert!(expect_quote(wire.clone(), "other", LIGHTNING_RECEIVE_PAIR).is_err());
        assert!(expect_quote(wire, &id, LIGHTNING_SEND_PAIR).is_err());

        let mut string_amounts = fixture["receive"]["quote"].clone();
        string_amounts["from_amount"] = "1000".into();
        assert_eq!(
            expect_quote(string_amounts, &id, LIGHTNING_RECEIVE_PAIR)
                .unwrap()
                .give()
                .unwrap(),
            1000
        );

        let refusal = serde_json::json!({"v":1,"type":"rfq_refusal","rfq_id":id,"reason":"amount_out_of_range","error_code":"amount_below_min","limit":500,"unit":"sats"});
        match expect_quote(refusal, &id, LIGHTNING_RECEIVE_PAIR) {
            Err(Error::Refused {
                reason, error_code, ..
            }) => {
                assert_eq!(reason, "amount_out_of_range");
                assert_eq!(error_code.as_deref(), Some("amount_below_min"));
            }
            other => panic!("{other:?}"),
        }
    }
}
