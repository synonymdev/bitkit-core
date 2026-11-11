use pubky::deep_links::DeepLink;

use super::errors::PubkyError;
use super::keys::keypair_from_hex;
use super::resolve::get_pubky;

/// The type of a `pubkyauth://` deep-link flow.
#[derive(uniffi::Enum, Debug, Clone, PartialEq)]
pub enum PubkyAuthKind {
    Signin,
    Signup,
}

/// Details extracted from a `pubkyauth://` deep-link URL.
#[derive(uniffi::Record, Debug, Clone)]
pub struct PubkyAuthDetails {
    /// Whether this is a signin or signup flow.
    pub kind: PubkyAuthKind,
    /// Requested capabilities (e.g. `"/pub/pubky.app/:rw"`).
    pub capabilities: String,
    /// Relay URL used for the auth exchange.
    pub relay: String,
    /// Homeserver public key (z32-encoded). Present only for signup flows.
    pub homeserver: Option<String>,
    /// Signup token. Present only for signup flows.
    pub signup_token: Option<String>,
}

/// Parse a `pubkyauth://` URL and return its details for UI display.
pub fn parse_pubky_auth_url(auth_url: String) -> Result<PubkyAuthDetails, PubkyError> {
    let deep_link: DeepLink =
        auth_url
            .parse()
            .map_err(
                |e: pubky::deep_links::DeepLinkParseError| PubkyError::AuthFailed {
                    reason: e.to_string(),
                },
            )?;

    match deep_link {
        DeepLink::Signin(signin) => Ok(PubkyAuthDetails {
            kind: PubkyAuthKind::Signin,
            capabilities: signin.capabilities().to_string(),
            relay: signin.relay().to_string(),
            homeserver: None,
            signup_token: None,
        }),
        DeepLink::Signup(signup) => Ok(PubkyAuthDetails {
            kind: PubkyAuthKind::Signup,
            capabilities: signup.capabilities().to_string(),
            relay: signup.relay().to_string(),
            homeserver: Some(signup.homeserver().z32()),
            signup_token: signup.signup_token(),
        }),
        DeepLink::SeedExport(_) => Err(PubkyError::AuthFailed {
            reason: "seed_export deep links are not auth URLs".to_string(),
        }),
    }
}

/// Approve a Pubky auth request by signing an AuthToken and posting it to the relay.
pub async fn approve_pubky_auth(
    auth_url: String,
    secret_key_hex: String,
) -> Result<(), PubkyError> {
    let kp = keypair_from_hex(&secret_key_hex)?;
    let pubky = get_pubky()?;
    let signer = pubky.signer(kp);

    signer
        .approve_auth(&auth_url)
        .await
        .map_err(|e| PubkyError::AuthFailed {
            reason: e.to_string(),
        })
}
