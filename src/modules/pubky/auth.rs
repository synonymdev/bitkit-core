use once_cell::sync::OnceCell;
use pubky::{AuthFlowKind, Capabilities, PubkyAuthFlow};
use tokio::sync::Mutex as TokioMutex;

use super::errors::PubkyError;

static AUTH_FLOW: OnceCell<TokioMutex<Option<PubkyAuthFlow>>> = OnceCell::new();

fn auth_flow_slot() -> &'static TokioMutex<Option<PubkyAuthFlow>> {
    AUTH_FLOW.get_or_init(|| TokioMutex::new(None))
}

/// Start a Pubky auth flow and return the `pubkyauth://` deep-link URL.
pub async fn start_pubky_auth(caps: String) -> Result<String, PubkyError> {
    let capabilities =
        Capabilities::try_from(caps.as_str()).map_err(|e| PubkyError::InvalidCapabilities {
            reason: e.to_string(),
        })?;

    let mut guard = auth_flow_slot().lock().await;

    if guard.is_some() {
        return Err(PubkyError::AuthFailed {
            reason: "An auth flow is already in progress".into(),
        });
    }

    let flow = PubkyAuthFlow::start(&capabilities, AuthFlowKind::signin()).map_err(|e| {
        PubkyError::AuthFailed {
            reason: e.to_string(),
        }
    })?;

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
        .map_err(|e| PubkyError::AuthFailed {
            reason: e.to_string(),
        })?;

    Ok(session.export_secret())
}
