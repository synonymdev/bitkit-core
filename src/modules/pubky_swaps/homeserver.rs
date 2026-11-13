//! Public reads from a provider's Pubky homeserver.

use once_cell::sync::OnceCell;
use pubky::Pubky;

use super::errors::PubkySwapError;

static PUBKY: OnceCell<Pubky> = OnceCell::new();

fn pubky() -> Result<&'static Pubky, PubkySwapError> {
    PUBKY.get_or_try_init(|| {
        Pubky::new().map_err(|e| PubkySwapError::transport(format!("Resolution failed: {e}")))
    })
}

fn is_not_found(err: &impl std::fmt::Display) -> bool {
    let msg = err.to_string();
    msg.contains("404") || msg.to_lowercase().contains("not found")
}

fn fetch_failed(err: impl std::fmt::Display) -> PubkySwapError {
    PubkySwapError::transport(format!("Fetch failed: {err}"))
}

/// Read a public file at `addr` (`<z32>/pub/…`), or `None` when it is not published.
pub(super) async fn fetch_public_file_if_any(
    addr: &str,
) -> Result<Option<Vec<u8>>, PubkySwapError> {
    let response = match pubky()?.public_storage().get(addr).await {
        Ok(response) => response,
        Err(e) if is_not_found(&e) => return Ok(None),
        Err(e) => return Err(fetch_failed(e)),
    };
    let bytes = response.bytes().await.map_err(fetch_failed)?;
    Ok(Some(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_matches_404_and_not_found() {
        assert!(is_not_found(&"HTTP status client error (404 Not Found)"));
        assert!(is_not_found(&"Resource Not Found"));
        assert!(!is_not_found(&"connection refused"));
    }

    #[tokio::test]
    async fn malformed_address_is_a_transport_error() {
        match fetch_public_file_if_any("totally-not-a-pubky-uri").await {
            Err(PubkySwapError::Transport { error_details }) => {
                assert!(
                    error_details.starts_with("Fetch failed: "),
                    "{error_details}"
                )
            }
            other => panic!("expected Transport, got: {other:?}"),
        }
    }
}
