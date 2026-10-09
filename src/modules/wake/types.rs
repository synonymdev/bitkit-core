use std::fmt;

/// A secp256k1 key pair for push encryption. The public key is registered
/// with the gateway; the secret key decrypts wakes and never leaves the device.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct WakeKeyPair {
    /// 64 lowercase hex characters.
    pub secret_key_hex: String,
    /// Compressed public key, 66 lowercase hex characters.
    pub public_key_hex: String,
}

impl fmt::Debug for WakeKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WakeKeyPair")
            .field("secret_key_hex", &"<redacted>")
            .field("public_key_hex", &self.public_key_hex)
            .finish()
    }
}

/// The bearer credential of the device routes, generated on the device.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct WakeDeviceSecret {
    /// `wkd_` followed by 43 base64url characters. Keep it in secure storage
    /// the notification extension can read.
    pub token: String,
    /// Lowercase hex SHA-256 of `token`, sent as `secret_sha256` when
    /// registering. The gateway stores only this hash.
    pub sha256_hex: String,
}

impl fmt::Debug for WakeDeviceSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WakeDeviceSecret")
            .field("token", &"wkd_<redacted>")
            .field("sha256_hex", &self.sha256_hex)
            .finish()
    }
}

/// Push transport of a device.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakePlatform {
    Apns,
    Fcm,
}

impl WakePlatform {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            WakePlatform::Apns => "apns",
            WakePlatform::Fcm => "fcm",
        }
    }
}

/// APNs environment. FCM devices are always `Production`.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeEnvironment {
    Production,
    Sandbox,
}

impl WakeEnvironment {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            WakeEnvironment::Production => "production",
            WakeEnvironment::Sandbox => "sandbox",
        }
    }
}

/// What the device did with a wake.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeAckOutcome {
    /// The wake was handled; the gateway cancels its fallback alert.
    Handled,
    /// The app has no handler for the topic; the fallback alert still fires.
    NoHandler,
    /// Handling failed; the fallback alert still fires.
    Failed,
}

impl WakeAckOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            WakeAckOutcome::Handled => "handled",
            WakeAckOutcome::NoHandler => "no_handler",
            WakeAckOutcome::Failed => "failed",
        }
    }
}

/// A registration ready to be signed. Every identity signs `message`: `ln:`
/// through the Lightning node's message signing (ldk-node `signMessage`),
/// `pk:` through `wake_sign_pubky_proof`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WakeRegistrationRequest {
    /// The gateway's audience, from `wake_server_info`.
    pub audience: String,
    pub app: String,
    pub install_id: String,
    pub platform: WakePlatform,
    pub environment: WakeEnvironment,
    pub push_token: String,
    /// Compressed secp256k1 public key, 66 lowercase hex characters.
    pub encryption_public_key: String,
    /// `WakeDeviceSecret.sha256_hex`.
    pub secret_sha256: String,
    /// One `ln:` and/or one `pk:` identity.
    pub identities: Vec<String>,
    /// Topic names and `<namespace>.*` patterns.
    pub topics: Vec<String>,
    /// Unix seconds.
    pub timestamp: u64,
    /// The exact UTF-8 text every identity signs.
    pub message: String,
}

/// One identity's signature over `WakeRegistrationRequest.message`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WakeIdentityProof {
    pub identity: String,
    /// `ln:`: 104 zbase32 characters. `pk:`: 128 lowercase hex characters.
    pub signature: String,
}

/// The gateway's view of a registered device.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WakeRegistration {
    pub device_id: String,
    pub identities: Vec<String>,
    /// Stored topics: the known names and every pattern.
    pub topics: Vec<String>,
    /// Exact names the gateway does not know. They are not stored.
    pub unknown_topics: Vec<String>,
    pub server_time: u64,
}

/// `GET /v1/info`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WakeServerInfo {
    /// The value registrations must be prepared with.
    pub audience: String,
    pub server_time: u64,
    pub version: String,
}

/// A wake found in a push.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WakeEnvelope {
    /// 0 for the legacy envelope, 1 for wake v1.
    pub version: u8,
    /// The wake id to acknowledge (v1 and fallback only).
    pub id: Option<String>,
    /// The topic; `source.type` for legacy envelopes.
    pub topic: String,
    /// `background`, `alert` or `time_sensitive` (v1 only).
    pub urgency: Option<String>,
    /// Unix seconds after which the wake is stale (v1 only).
    pub deadline: Option<u64>,
    /// The producer payload as raw JSON, when there is one.
    pub payload_json: Option<String>,
    /// `createdAt` of a legacy envelope.
    pub created_at: Option<String>,
    /// True for a fallback alert, which carries nothing to decrypt.
    pub fallback: bool,
}

/// A topic devices can subscribe to.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WakeTopic {
    pub name: String,
    pub description: String,
    /// `background`, `alert` or `time_sensitive`.
    pub urgency: String,
    pub deadline_secs: u32,
    /// Whether a visible fallback alert may be shown for this topic.
    pub alertable: bool,
    /// Alert title, for topics with display text.
    pub title: Option<String>,
    /// Alert body, for topics with display text.
    pub body: Option<String>,
    /// Peers (not services) send this topic, through grants.
    pub peer: bool,
}
