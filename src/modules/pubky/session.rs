use pubky::{PubkySession, PublicKey};

use super::errors::PubkyError;
use super::keys::keypair_from_hex;
use super::resolve::get_pubky;

/// Sign up on a homeserver and return the session secret (`"<pubkey_z32>:<cookie>"`).
pub async fn pubky_sign_up(
    secret_key_hex: String,
    homeserver_public_key_z32: String,
    signup_code: Option<String>,
) -> Result<String, PubkyError> {
    let kp = keypair_from_hex(&secret_key_hex)?;

    let homeserver_pk =
        PublicKey::try_from_z32(&homeserver_public_key_z32).map_err(|e| PubkyError::KeyError {
            reason: e.to_string(),
        })?;

    let pubky = get_pubky()?;
    let signer = pubky.signer(kp);

    let session = signer
        .signup(&homeserver_pk, signup_code.as_deref())
        .await
        .map_err(|e| PubkyError::AuthFailed {
            reason: e.to_string(),
        })?;

    Ok(session.export_secret())
}

/// Re-authenticate with an existing secret key and return a new session secret.
pub async fn pubky_sign_in(secret_key_hex: String) -> Result<String, PubkyError> {
    let kp = keypair_from_hex(&secret_key_hex)?;

    let pubky = get_pubky()?;
    let signer = pubky.signer(kp);

    let session = signer.signin().await.map_err(|e| PubkyError::AuthFailed {
        reason: e.to_string(),
    })?;

    Ok(session.export_secret())
}

/// Write content to a path on the user's homeserver using their session.
pub async fn pubky_session_put(
    session_secret: String,
    path: String,
    content: Vec<u8>,
) -> Result<(), PubkyError> {
    let session = import_session(&session_secret).await?;

    session
        .storage()
        .put(path.as_str(), content)
        .await
        .map_err(|e| PubkyError::WriteFailed {
            reason: e.to_string(),
        })?;

    Ok(())
}

/// Delete a resource at path on the user's homeserver.
pub async fn pubky_session_delete(session_secret: String, path: String) -> Result<(), PubkyError> {
    let session = import_session(&session_secret).await?;

    session
        .storage()
        .delete(path.as_str())
        .await
        .map_err(|e| PubkyError::WriteFailed {
            reason: format!("delete failed: {e}"),
        })?;

    Ok(())
}

/// Sign in with a secret key and write content in one shot (e.g., redirect.json on old identity).
pub async fn pubky_put_with_secret_key(
    secret_key_hex: String,
    path: String,
    content: Vec<u8>,
) -> Result<(), PubkyError> {
    let kp = keypair_from_hex(&secret_key_hex)?;

    let pubky = get_pubky()?;
    let signer = pubky.signer(kp);

    let session = signer.signin().await.map_err(|e| PubkyError::AuthFailed {
        reason: e.to_string(),
    })?;

    session
        .storage()
        .put(path.as_str(), content)
        .await
        .map_err(|e| PubkyError::WriteFailed {
            reason: e.to_string(),
        })?;

    Ok(())
}

/// List resources in a directory on the user's homeserver. `dir_path` must end with `/`.
pub async fn pubky_session_list(
    session_secret: String,
    dir_path: String,
) -> Result<Vec<String>, PubkyError> {
    let session = import_session(&session_secret).await?;

    let entries = session
        .storage()
        .list(dir_path.as_str())
        .map_err(|e| PubkyError::FetchFailed {
            reason: e.to_string(),
        })?
        .send()
        .await
        .map_err(|e| PubkyError::FetchFailed {
            reason: e.to_string(),
        })?;

    Ok(entries.iter().map(|e| e.to_pubky_url()).collect())
}

/// Import a session from its secret token, validating it against the homeserver.
async fn import_session(session_secret: &str) -> Result<PubkySession, PubkyError> {
    PubkySession::import_secret(session_secret, None)
        .await
        .map_err(|e| PubkyError::AuthFailed {
            reason: e.to_string(),
        })
}
