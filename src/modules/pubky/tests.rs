use super::*;

// ============================================================================
// Resolution tests
// ============================================================================

#[test]
fn resolve_pubky_uri() {
    let pk = "operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";
    let uri = format!("pubky://{pk}/pub/pubky.app/files/0034RC8872KNG");
    let result = resolve_pubky_url(uri).unwrap();
    assert_eq!(
        result,
        format!("https://_pubky.{pk}/pub/pubky.app/files/0034RC8872KNG")
    );
}

#[test]
fn resolve_pubky_prefixed_form() {
    let pk = "operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";
    let uri = format!("pubky{pk}/pub/pubky.app/profile.json");
    let result = resolve_pubky_url(uri).unwrap();
    assert_eq!(
        result,
        format!("https://_pubky.{pk}/pub/pubky.app/profile.json")
    );
}

#[test]
fn resolve_invalid_uri() {
    let result = resolve_pubky_url("not-a-pubky-uri".into());
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::ResolutionFailed { .. } => {}
        other => panic!("expected ResolutionFailed, got: {other:?}"),
    }
}

// ============================================================================
// Auth flow tests
// ============================================================================

#[tokio::test]
async fn complete_without_active_flow_returns_no_active_flow() {
    let result = complete_pubky_auth().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::NoActiveFlow => {}
        other => panic!("expected NoActiveFlow, got: {other:?}"),
    }
}

#[tokio::test]
async fn cancel_without_active_flow_returns_no_active_flow() {
    let result = cancel_pubky_auth().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::NoActiveFlow => {}
        other => panic!("expected NoActiveFlow, got: {other:?}"),
    }
}

// ============================================================================
// File fetch tests
// ============================================================================

#[tokio::test]
async fn fetch_malformed_uri_returns_error() {
    let result = fetch_pubky_file("totally-not-a-pubky-uri".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::ResolutionFailed { .. } => {}
        other => panic!("expected ResolutionFailed, got: {other:?}"),
    }
}

// ============================================================================
// Profile conversion tests
// ============================================================================

#[test]
fn profile_from_full_app_user() {
    use pubky_app_specs::{PubkyAppUser, PubkyAppUserLink};

    let user = PubkyAppUser::new(
        "Alice".to_string(),
        Some("Hello world".to_string()),
        Some("https://example.com/avatar.png".to_string()),
        Some(vec![PubkyAppUserLink::new(
            "Website".to_string(),
            "https://alice.example.com".to_string(),
        )]),
        Some("Online".to_string()),
    );

    let profile = PubkyProfile::from(user);
    assert_eq!(profile.name, "Alice");
    assert_eq!(profile.bio.as_deref(), Some("Hello world"));
    assert_eq!(profile.image.as_deref(), Some("https://example.com/avatar.png"));
    assert_eq!(profile.status.as_deref(), Some("Online"));

    let links = profile.links.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].title, "Website");
    assert!(links[0].url.starts_with("https://alice.example.com"));
}

#[test]
fn profile_from_minimal_app_user() {
    use pubky_app_specs::PubkyAppUser;

    let user = PubkyAppUser::new("Bob".to_string(), None, None, None, None);

    let profile = PubkyProfile::from(user);
    assert_eq!(profile.name, "Bob");
    assert!(profile.bio.is_none());
    assert!(profile.image.is_none());
    assert!(profile.links.is_none());
    assert!(profile.status.is_none());
}

#[test]
fn profile_from_user_with_empty_links() {
    use pubky_app_specs::PubkyAppUser;

    let user = PubkyAppUser::new(
        "Carol".to_string(),
        Some("Bio".to_string()),
        None,
        Some(vec![]),
        None,
    );

    let profile = PubkyProfile::from(user);
    assert_eq!(profile.name, "Carol");
    let links = profile.links.unwrap();
    assert!(links.is_empty());
}

// ============================================================================
// Profile JSON deserialization tests
// ============================================================================

#[test]
fn profile_deserialized_from_full_json() {
    let json = r#"{
        "name": "Dave",
        "bio": "Hello",
        "image": "https://example.com/img.png",
        "links": [
            {"title": "Blog", "url": "https://blog.example.com"},
            {"title": "GitHub", "url": "https://github.com/dave"}
        ],
        "status": "Away"
    }"#;

    let user: pubky_app_specs::PubkyAppUser = serde_json::from_str(json).unwrap();
    let profile = PubkyProfile::from(user);

    assert_eq!(profile.name, "Dave");
    assert_eq!(profile.bio.as_deref(), Some("Hello"));
    assert_eq!(profile.image.as_deref(), Some("https://example.com/img.png"));
    assert_eq!(profile.status.as_deref(), Some("Away"));
    let links = profile.links.unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].title, "Blog");
    assert_eq!(links[1].title, "GitHub");
}

#[test]
fn profile_deserialized_from_name_only_json() {
    let json = r#"{"name": "Eve"}"#;
    let user: pubky_app_specs::PubkyAppUser = serde_json::from_str(json).unwrap();
    let profile = PubkyProfile::from(user);

    assert_eq!(profile.name, "Eve");
    assert!(profile.bio.is_none());
    assert!(profile.image.is_none());
    assert!(profile.links.is_none());
    assert!(profile.status.is_none());
}

#[test]
fn profile_missing_name_json_fails() {
    let json = r#"{"bio": "no name field"}"#;
    let result: Result<pubky_app_specs::PubkyAppUser, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn profile_invalid_json_fails() {
    let json = r#"not valid json"#;
    let result: Result<pubky_app_specs::PubkyAppUser, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ============================================================================
// Profile fetch error tests
// ============================================================================

#[tokio::test]
async fn fetch_profile_invalid_key_returns_fetch_failed() {
    let result = fetch_pubky_profile("not-a-valid-public-key".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::FetchFailed { .. } | PubkyError::ProfileNotFound => {}
        other => panic!("expected FetchFailed or ProfileNotFound, got: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_profile_empty_key_returns_fetch_failed() {
    let result = fetch_pubky_profile(String::new()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::FetchFailed { .. } | PubkyError::ProfileNotFound => {}
        other => panic!("expected FetchFailed or ProfileNotFound, got: {other:?}"),
    }
}

// ============================================================================
// Contacts fetch error tests
// ============================================================================

#[tokio::test]
async fn fetch_contacts_invalid_key_returns_fetch_failed() {
    let result = fetch_pubky_contacts("not-a-valid-public-key".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::FetchFailed { .. } => {}
        other => panic!("expected FetchFailed, got: {other:?}"),
    }
}

#[tokio::test]
async fn fetch_contacts_empty_key_returns_fetch_failed() {
    let result = fetch_pubky_contacts(String::new()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::FetchFailed { .. } => {}
        other => panic!("expected FetchFailed, got: {other:?}"),
    }
}

