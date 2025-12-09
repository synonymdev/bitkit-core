use paykit_lib::{
    EndpointData as ExternalEndpointData, MethodId as ExternalMethodId,
    PublicKey as ExternalPublicKey, SupportedPayments as ExternalSupportedPayments,
};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Identifier for a payment method specification (e.g., "lightning", "onchain", "bolt11").
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct MethodId {
    pub id: String,
}

impl MethodId {
    /// Common payment method constant for Lightning Network
    pub const LIGHTNING: &'static str = "lightning";
    /// Common payment method constant for on-chain Bitcoin
    pub const ONCHAIN: &'static str = "onchain";
    /// Common payment method constant for BOLT11 invoices
    pub const BOLT11: &'static str = "bolt11";
    /// Common payment method constant for BOLT12 offers
    pub const BOLT12: &'static str = "bolt12";
    /// Common payment method constant for LNURL
    pub const LNURL: &'static str = "lnurl";

    /// Creates a new MethodId from a string
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }

    /// Creates a Lightning method ID
    pub fn lightning() -> Self {
        Self::new(Self::LIGHTNING)
    }

    /// Creates an on-chain method ID
    pub fn onchain() -> Self {
        Self::new(Self::ONCHAIN)
    }
}

impl From<MethodId> for ExternalMethodId {
    fn from(value: MethodId) -> Self {
        ExternalMethodId(value.id)
    }
}

impl From<ExternalMethodId> for MethodId {
    fn from(value: ExternalMethodId) -> Self {
        MethodId { id: value.0 }
    }
}

/// Serialized payload served by a payment endpoint (UTF-8 text such as JSON, LNURL, etc.).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct EndpointData {
    pub data: String,
}

impl From<EndpointData> for ExternalEndpointData {
    fn from(value: EndpointData) -> Self {
        ExternalEndpointData(value.data)
    }
}

impl From<ExternalEndpointData> for EndpointData {
    fn from(value: ExternalEndpointData) -> Self {
        EndpointData { data: value.0 }
    }
}

/// Public key wrapper for Paykit operations.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct PublicKey {
    pub key: String,
}

impl PublicKey {
    /// Creates a new PublicKey from a string
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }

    /// Validates the public key format
    pub fn validate(&self) -> Result<(), String> {
        // Basic validation - ensure it's not empty and has reasonable length
        if self.key.is_empty() {
            return Err("Public key cannot be empty".to_string());
        }
        if self.key.len() < 32 || self.key.len() > 256 {
            return Err("Public key has invalid length".to_string());
        }
        // Additional validation can be added here for specific key formats
        Ok(())
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.key)
    }
}

impl FromStr for PublicKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = PublicKey::new(s);
        key.validate()?;
        Ok(key)
    }
}

impl TryFrom<PublicKey> for ExternalPublicKey {
    type Error = String;

    fn try_from(value: PublicKey) -> Result<Self, Self::Error> {
        // When using pubky feature, parse the string representation
        // The pubky crate's PublicKey type has a FromStr implementation
        value.key
            .parse::<ExternalPublicKey>()
            .map_err(|e| format!("Failed to parse public key: {}", e))
    }
}

impl From<ExternalPublicKey> for PublicKey {
    fn from(value: ExternalPublicKey) -> Self {
        PublicKey {
            key: value.to_string(),
        }
    }
}

/// Collection of supported payment entries keyed by method identifiers.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SupportedPayments {
    pub entries: HashMap<String, EndpointData>,
}

impl From<SupportedPayments> for ExternalSupportedPayments {
    fn from(value: SupportedPayments) -> Self {
        let entries = value
            .entries
            .into_iter()
            .map(|(k, v)| (ExternalMethodId(k), v.into()))
            .collect();
        ExternalSupportedPayments { entries }
    }
}

impl From<ExternalSupportedPayments> for SupportedPayments {
    fn from(value: ExternalSupportedPayments) -> Self {
        let entries = value
            .entries
            .into_iter()
            .map(|(k, v)| (k.0, v.into()))
            .collect();
        SupportedPayments { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_id_conversion() {
        let method = MethodId {
            id: "lightning".to_string(),
        };
        let external: ExternalMethodId = method.clone().into();
        assert_eq!(external.0, "lightning");

        let back: MethodId = external.into();
        assert_eq!(back, method);
    }

    #[test]
    fn test_method_id_helpers() {
        let lightning = MethodId::lightning();
        assert_eq!(lightning.id, "lightning");

        let onchain = MethodId::onchain();
        assert_eq!(onchain.id, "onchain");

        let custom = MethodId::new("custom_method");
        assert_eq!(custom.id, "custom_method");
    }

    #[test]
    fn test_endpoint_data_conversion() {
        let data = EndpointData {
            data: r#"{"bolt11":"lnbc..."}"#.to_string(),
        };
        let external: ExternalEndpointData = data.clone().into();
        assert_eq!(external.0, r#"{"bolt11":"lnbc..."}"#);

        let back: EndpointData = external.into();
        assert_eq!(back, data);
    }

    #[test]
    fn test_public_key_validation() {
        let valid_key = PublicKey::new("a".repeat(64));
        assert!(valid_key.validate().is_ok());

        let empty_key = PublicKey::new("");
        assert!(empty_key.validate().is_err());

        let short_key = PublicKey::new("abc");
        assert!(short_key.validate().is_err());

        let long_key = PublicKey::new("a".repeat(300));
        assert!(long_key.validate().is_err());
    }

    #[test]
    fn test_public_key_display() {
        let key = PublicKey::new("test_public_key_123");
        assert_eq!(format!("{}", key), "test_public_key_123");
    }

    #[test]
    fn test_public_key_from_str() {
        let key_str = "a".repeat(64);
        let key: Result<PublicKey, _> = key_str.parse();
        assert!(key.is_ok());
        assert_eq!(key.unwrap().key, key_str);

        let invalid_key: Result<PublicKey, _> = "".parse();
        assert!(invalid_key.is_err());
    }

    #[test]
    fn test_supported_payments_conversion() {
        let mut entries = HashMap::new();
        entries.insert(
            "lightning".to_string(),
            EndpointData {
                data: r#"{"bolt11":"lnbc..."}"#.to_string(),
            },
        );

        let payments = SupportedPayments { entries };
        let external: ExternalSupportedPayments = payments.clone().into();
        assert_eq!(external.entries.len(), 1);

        let back: SupportedPayments = external.into();
        assert_eq!(back, payments);
    }

    #[test]
    fn test_multiple_payment_methods() {
        let mut entries = HashMap::new();
        entries.insert(
            MethodId::LIGHTNING.to_string(),
            EndpointData {
                data: r#"{"bolt11":"lnbc..."}"#.to_string(),
            },
        );
        entries.insert(
            MethodId::ONCHAIN.to_string(),
            EndpointData {
                data: r#"{"address":"bc1..."}"#.to_string(),
            },
        );
        entries.insert(
            MethodId::LNURL.to_string(),
            EndpointData {
                data: r#"{"lnurl":"lnurl1..."}"#.to_string(),
            },
        );

        let payments = SupportedPayments { entries };
        assert_eq!(payments.entries.len(), 3);

        let external: ExternalSupportedPayments = payments.clone().into();
        assert_eq!(external.entries.len(), 3);

        let back: SupportedPayments = external.into();
        assert_eq!(back.entries.len(), 3);
    }
}
