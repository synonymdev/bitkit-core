use super::*;

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

#[tokio::test]
async fn fetch_malformed_uri_returns_error() {
    let result = fetch_pubky_file("totally-not-a-pubky-uri".into()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::ResolutionFailed { .. } => {}
        other => panic!("expected ResolutionFailed, got: {other:?}"),
    }
}

