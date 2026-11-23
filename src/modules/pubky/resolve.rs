use once_cell::sync::OnceCell;
use pubky::Pubky;

use super::errors::PubkyError;

static PUBKY_SDK: OnceCell<Pubky> = OnceCell::new();

pub(super) fn get_pubky() -> Result<&'static Pubky, PubkyError> {
    PUBKY_SDK.get_or_try_init(|| {
        Pubky::new().map_err(|e| PubkyError::ResolutionFailed {
            reason: e.to_string(),
        })
    })
}

/// Convert a `pubky://` URI to its `https://_pubky.<z32>/…` transport URL.
pub fn resolve_pubky_url(uri: String) -> Result<String, PubkyError> {
    let url = pubky::resolve_pubky(&uri).map_err(|e| PubkyError::ResolutionFailed {
        reason: e.to_string(),
    })?;
    Ok(url.to_string())
}

/// Fetch a public resource from a `pubky://` URI and return as a UTF-8 string.
pub async fn fetch_pubky_file_string(uri: String) -> Result<String, PubkyError> {
    let bytes = fetch_pubky_file(uri).await?;
    String::from_utf8(bytes).map_err(|e| PubkyError::FetchFailed {
        reason: e.to_string(),
    })
}

/// Fetch a public resource from a `pubky://` URI and return its raw bytes.
pub async fn fetch_pubky_file(uri: String) -> Result<Vec<u8>, PubkyError> {
    let pubky = get_pubky()?;

    let response = pubky
        .public_storage()
        .get(&uri)
        .await
        .map_err(|e| PubkyError::FetchFailed {
            reason: e.to_string(),
        })?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| PubkyError::FetchFailed {
            reason: e.to_string(),
        })?;

    Ok(bytes.to_vec())
}
