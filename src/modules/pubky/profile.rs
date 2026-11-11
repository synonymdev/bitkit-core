use super::errors::PubkyError;
use super::resolve::get_pubky;

fn is_not_found(err: &impl std::fmt::Display) -> bool {
    let msg = err.to_string();
    msg.contains("404") || msg.to_lowercase().contains("not found")
}

/// Read a public file at `addr` (`<z32>/pub/…`), or `None` when it is not published.
pub(crate) async fn fetch_public_file_if_any(addr: &str) -> Result<Option<Vec<u8>>, PubkyError> {
    let pubky = get_pubky()?;
    let response = match pubky.public_storage().get(addr).await {
        Ok(response) => response,
        Err(e) if is_not_found(&e) => return Ok(None),
        Err(e) => {
            return Err(PubkyError::FetchFailed {
                reason: e.to_string(),
            })
        }
    };
    let bytes = response
        .bytes()
        .await
        .map_err(|e| PubkyError::FetchFailed {
            reason: e.to_string(),
        })?;
    Ok(Some(bytes.to_vec()))
}
