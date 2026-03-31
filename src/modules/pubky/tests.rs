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
        PubkyError::FetchFailed { .. } => {}
        other => panic!("expected FetchFailed, got: {other:?}"),
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
    assert_eq!(
        profile.image.as_deref(),
        Some("https://example.com/avatar.png")
    );
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
    assert_eq!(
        profile.image.as_deref(),
        Some("https://example.com/img.png")
    );
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

// ============================================================================
// Key derivation tests
// ============================================================================

#[test]
fn derive_secret_key_returns_valid_hex() {
    let seed = vec![0xABu8; 64];
    let hex_str = derive_pubky_secret_key(seed).unwrap();
    assert_eq!(hex_str.len(), 64); // 32 bytes = 64 hex chars
    assert!(hex::decode(&hex_str).is_ok());
}

#[test]
fn derive_secret_key_is_deterministic() {
    let seed = vec![0x42u8; 64];
    let key1 = derive_pubky_secret_key(seed.clone()).unwrap();
    let key2 = derive_pubky_secret_key(seed).unwrap();
    assert_eq!(key1, key2);
}

#[test]
fn derive_secret_key_different_seeds_produce_different_keys() {
    let key1 = derive_pubky_secret_key(vec![0x01u8; 64]).unwrap();
    let key2 = derive_pubky_secret_key(vec![0x02u8; 64]).unwrap();
    assert_ne!(key1, key2);
}

#[test]
fn derive_secret_key_rejects_short_seed() {
    let result = derive_pubky_secret_key(vec![0u8; 32]);
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

#[test]
fn derive_secret_key_rejects_empty_seed() {
    let result = derive_pubky_secret_key(vec![]);
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

#[test]
fn public_key_from_derived_secret_roundtrips() {
    let seed = vec![0xCDu8; 64];
    let secret = derive_pubky_secret_key(seed).unwrap();
    let pk1 = pubky_public_key_from_secret(secret.clone()).unwrap();
    let pk2 = pubky_public_key_from_secret(secret).unwrap();
    assert_eq!(pk1, pk2);
    assert!(!pk1.is_empty());
    // z32-encoded public keys are 52 characters
    assert_eq!(pk1.len(), 52);
}

#[test]
fn public_key_from_invalid_hex_returns_error() {
    let result = pubky_public_key_from_secret("not-valid-hex".into());
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

#[test]
fn public_key_from_empty_hex_returns_error() {
    let result = pubky_public_key_from_secret(String::new());
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

#[test]
fn public_key_from_wrong_length_hex_returns_error() {
    let result = pubky_public_key_from_secret("aabb".into()); // only 2 bytes, need 32
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

// ============================================================================
// Sign up / sign in error tests
// ============================================================================

#[tokio::test]
async fn sign_up_invalid_secret_key_returns_error() {
    let result = pubky_sign_up("bad-hex".into(), "some-homeserver".into(), None).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

#[tokio::test]
async fn sign_in_invalid_secret_key_returns_error() {
    let result = pubky_sign_in("bad-hex".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

// ============================================================================
// Put with secret key error tests
// ============================================================================

#[tokio::test]
async fn put_with_secret_key_invalid_key_returns_error() {
    let result = pubky_put_with_secret_key("bad-hex".into(), "/pub/test".into(), vec![]).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

// ============================================================================
// Session operation error tests
// ============================================================================

#[tokio::test]
async fn session_put_invalid_session_returns_error() {
    let result = pubky_session_put("bad-session".into(), "/pub/test".into(), vec![]).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

#[tokio::test]
async fn session_delete_invalid_session_returns_error() {
    let result = pubky_session_delete("bad-session".into(), "/pub/test".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

#[tokio::test]
async fn session_list_invalid_session_returns_error() {
    let result = pubky_session_list("bad-session".into(), "/pub/test/".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

// ============================================================================
// Approver auth tests
// ============================================================================

#[test]
fn parse_signin_auth_url() {
    let url = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link";
    let details = parse_pubky_auth_url(url.into()).unwrap();
    assert_eq!(details.kind, PubkyAuthKind::Signin);
    assert_eq!(details.capabilities, "/pub/pubky.app/:rw");
    assert_eq!(details.relay, "https://httprelay.pubky.app/link");
    assert!(details.homeserver.is_none());
    assert!(details.signup_token.is_none());
}

#[test]
fn parse_signup_auth_url_with_token() {
    let url = "pubkyauth://signup?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link&hs=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo&st=1234567890";
    let details = parse_pubky_auth_url(url.into()).unwrap();
    assert_eq!(details.kind, PubkyAuthKind::Signup);
    assert_eq!(details.capabilities, "/pub/pubky.app/:rw");
    assert_eq!(details.relay, "https://httprelay.pubky.app/link");
    assert_eq!(
        details.homeserver.as_deref(),
        Some("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo")
    );
    assert_eq!(details.signup_token.as_deref(), Some("1234567890"));
}

#[test]
fn parse_signup_auth_url_without_token() {
    let url = "pubkyauth://signup?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link&hs=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo";
    let details = parse_pubky_auth_url(url.into()).unwrap();
    assert_eq!(details.kind, PubkyAuthKind::Signup);
    assert_eq!(
        details.homeserver.as_deref(),
        Some("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo")
    );
    assert!(details.signup_token.is_none());
}

#[test]
fn parse_old_format_auth_url() {
    let url = "pubkyauth:///?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link";
    let details = parse_pubky_auth_url(url.into()).unwrap();
    assert_eq!(details.kind, PubkyAuthKind::Signin);
}

#[test]
fn parse_invalid_auth_url_returns_error() {
    let result = parse_pubky_auth_url("not-a-valid-url".into());
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

#[test]
fn parse_auth_url_missing_params_returns_error() {
    // Missing secret and relay
    let result = parse_pubky_auth_url("pubkyauth://signin?caps=/pub/pubky.app/:rw".into());
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

#[test]
fn parse_seed_export_url_returns_error() {
    let url = "pubkyauth://secret_export?secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8";
    let result = parse_pubky_auth_url(url.into());
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

#[tokio::test]
async fn approve_auth_invalid_key_returns_error() {
    let url = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link";
    let result = approve_pubky_auth(url.into(), "bad-hex".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::KeyError { .. } => {}
        other => panic!("expected KeyError, got: {other:?}"),
    }
}

#[tokio::test]
async fn approve_auth_invalid_url_returns_error() {
    let seed = vec![0xABu8; 64];
    let secret = derive_pubky_secret_key(seed).unwrap();
    let result = approve_pubky_auth("not-a-url".into(), secret).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}

// ============================================================================
// String fetch tests
// ============================================================================

#[tokio::test]
async fn fetch_file_string_malformed_uri_returns_error() {
    let result = fetch_pubky_file_string("not-a-uri".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::FetchFailed { .. } => {}
        other => panic!("expected FetchFailed, got: {other:?}"),
    }
}
