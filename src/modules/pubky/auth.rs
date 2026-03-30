use once_cell::sync::OnceCell;
use tokio::sync::Mutex as TokioMutex;
use pubky::{PubkyAuthFlow, Capabilities, AuthFlowKind};
use pubky::deep_links::DeepLink;

use super::errors::PubkyError;
use super::keys::keypair_from_hex;
use super::resolve::get_pubky;

static AUTH_FLOW: OnceCell<TokioMutex<Option<PubkyAuthFlow>>> = OnceCell::new();

fn auth_flow_slot() -> &'static TokioMutex<Option<PubkyAuthFlow>> {
    AUTH_FLOW.get_or_init(|| TokioMutex::new(None))
}

/// Start a Pubky auth flow and return the `pubkyauth://` deep-link URL.
pub async fn start_pubky_auth(caps: String) -> Result<String, PubkyError> {
    let capabilities = Capabilities::try_from(caps.as_str())
        .map_err(|e| PubkyError::InvalidCapabilities { reason: e.to_string() })?;

    let mut guard = auth_flow_slot().lock().await;

    if guard.is_some() {
        return Err(PubkyError::AuthFailed {
            reason: "An auth flow is already in progress".into(),
        });
    }

    let flow = PubkyAuthFlow::start(&capabilities, AuthFlowKind::signin())
        .map_err(|e| PubkyError::AuthFailed { reason: e.to_string() })?;

    let url = flow.authorization_url().to_string();
    *guard = Some(flow);

    Ok(url)
}

/// Cancel an in-progress auth flow, freeing the slot for a new one.
pub async fn cancel_pubky_auth() -> Result<(), PubkyError> {
    let mut guard = auth_flow_slot().lock().await;
    guard.take().ok_or(PubkyError::NoActiveFlow)?;
    Ok(())
}

/// Await Ring approval and return the session secret (`"<pubkey_z32>:<cookie>"`).
pub async fn complete_pubky_auth() -> Result<String, PubkyError> {
    let flow = {
        let mut guard = auth_flow_slot().lock().await;
        guard.take().ok_or(PubkyError::NoActiveFlow)?
    };

    let session = flow
        .await_approval()
        .await
        .map_err(|e| PubkyError::AuthFailed { reason: e.to_string() })?;

    Ok(session.export_secret())
}

/// Details extracted from a `pubkyauth://` deep-link URL.
#[derive(uniffi::Record, Debug, Clone)]
pub struct PubkyAuthDetails {
    /// `"signin"` or `"signup"`.
    pub kind: String,
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
    let deep_link: DeepLink = auth_url
        .parse()
        .map_err(|e: pubky::deep_links::DeepLinkParseError| PubkyError::AuthFailed {
            reason: e.to_string(),
        })?;

    match deep_link {
        DeepLink::Signin(signin) => Ok(PubkyAuthDetails {
            kind: "signin".to_string(),
            capabilities: signin.capabilities().to_string(),
            relay: signin.relay().to_string(),
            homeserver: None,
            signup_token: None,
        }),
        DeepLink::Signup(signup) => Ok(PubkyAuthDetails {
            kind: "signup".to_string(),
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
        .map_err(|e| PubkyError::AuthFailed { reason: e.to_string() })
}
