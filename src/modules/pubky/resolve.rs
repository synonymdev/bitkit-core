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
