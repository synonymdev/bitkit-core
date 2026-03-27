use pubky_app_specs::{PubkyAppUser, PubkyAppUserLink};

use super::errors::PubkyError;
use super::resolve::get_pubky;

const PROFILE_PATH: &str = "/pub/pubky.app/profile.json";
const FOLLOWS_PATH: &str = "/pub/pubky.app/follows/";

#[derive(uniffi::Record, Debug, Clone)]
pub struct PubkyProfileLink {
    pub title: String,
    pub url: String,
}

#[derive(uniffi::Record, Debug, Clone)]
pub struct PubkyProfile {
    pub name: String,
    pub bio: Option<String>,
    pub image: Option<String>,
    pub links: Option<Vec<PubkyProfileLink>>,
    pub status: Option<String>,
}

impl From<PubkyAppUserLink> for PubkyProfileLink {
    fn from(link: PubkyAppUserLink) -> Self {
        PubkyProfileLink {
            title: link.title,
            url: link.url,
        }
    }
}

impl From<PubkyAppUser> for PubkyProfile {
    fn from(user: PubkyAppUser) -> Self {
        PubkyProfile {
            name: user.name,
            bio: user.bio,
            image: user.image,
            links: user
                .links
                .map(|links| links.into_iter().map(PubkyProfileLink::from).collect()),
            status: user.status,
        }
    }
}

fn is_not_found(err: &impl std::fmt::Display) -> bool {
    let msg = err.to_string();
    msg.contains("404") || msg.to_lowercase().contains("not found")
}

/// Fetch and parse a user's `profile.json` by their public key (z32-encoded).
pub async fn fetch_pubky_profile(public_key: String) -> Result<PubkyProfile, PubkyError> {
    let pubky = get_pubky()?;
    let addr = format!("{public_key}{PROFILE_PATH}");

    let response = pubky.public_storage().get(&addr).await.map_err(|e| {
        if is_not_found(&e) {
            PubkyError::ProfileNotFound
        } else {
            PubkyError::FetchFailed {
                reason: e.to_string(),
            }
        }
    })?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| PubkyError::FetchFailed {
            reason: e.to_string(),
        })?;

    let user: PubkyAppUser =
        serde_json::from_slice(&bytes).map_err(|e| PubkyError::ProfileParseFailed {
            reason: e.to_string(),
        })?;

    Ok(PubkyProfile::from(user))
}

/// Fetch the followed public keys for a user. Returns an empty list if the user has no follows.
pub async fn fetch_pubky_contacts(public_key: String) -> Result<Vec<String>, PubkyError> {
    let pubky = get_pubky()?;
    let addr = format!("{public_key}{FOLLOWS_PATH}");
    let storage = pubky.public_storage();

    let list_builder = storage.list(&addr).map_err(|e| PubkyError::FetchFailed {
        reason: e.to_string(),
    })?;

    let entries = match list_builder.send().await {
        Ok(entries) => entries,
        Err(e) if is_not_found(&e) => return Ok(Vec::new()),
        Err(e) => {
            return Err(PubkyError::FetchFailed {
                reason: e.to_string(),
            })
        }
    };

    let mut contacts = Vec::new();
    for entry in entries {
        let url_str = entry.to_pubky_url();
        if url_str.ends_with('/') {
            continue;
        }
        if let Some(pk_str) = url_str.rsplit('/').next().filter(|s| !s.is_empty()) {
            contacts.push(pk_str.to_string());
        }
    }

    Ok(contacts)
}
