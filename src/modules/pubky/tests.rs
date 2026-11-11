use super::*;

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
    let secret = hex::encode([0xABu8; 32]);
    let result = approve_pubky_auth("not-a-url".into(), secret).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        PubkyError::AuthFailed { .. } => {}
        other => panic!("expected AuthFailed, got: {other:?}"),
    }
}
