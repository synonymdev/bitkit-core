use super::{BoltzDB, BoltzError, BoltzNetwork};
use once_cell::sync::Lazy;
use pubky_swap_boltz::{
    chain::{Chain, ElectrumChain},
    provider::{canonical_pubky, identity_from_secret, Provider, PubkyProvider},
    service::{Bridge, BridgeSettings},
    store::Store,
};
use std::{future::Future, path::Path, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

/// Configuration for the embedded Pubky swap bridge. Secrets remain in memory.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PubkySwapConfig {
    pub network: BoltzNetwork,
    /// Provider public key, in plain z32 or with a `pubky` prefix.
    pub provider: String,
    pub electrum_url: String,
    /// Absolute directory in the application's private wallet storage.
    pub data_dir: String,
    /// Maximum total swap fee, in basis points.
    pub max_fee_bps: u16,
    /// Maximum total amount admitted for one swap, in satoshis.
    pub max_amount_sat: u64,
}

struct ConfiguredBridge {
    config: PubkySwapConfig,
    binding: String,
    bridge: Arc<Bridge>,
    chain: Arc<dyn Chain>,
    provider: Option<Arc<PubkyProvider>>,
    delivery_task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for ConfiguredBridge {
    fn drop(&mut self) {
        if let Some(task) = &self.delivery_task {
            task.abort();
        }
    }
}

static CONFIGURED: Lazy<Mutex<Option<ConfiguredBridge>>> = Lazy::new(|| Mutex::new(None));
pub(crate) static RECOVERY_WAKEUP: Lazy<tokio::sync::Notify> = Lazy::new(tokio::sync::Notify::new);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct SessionCredentials {
    token: Zeroizing<String>,
    owner: String,
    scope: String,
    wallet_secret: Zeroizing<[u8; 32]>,
}

/// Connect an existing registered Pubky identity to its configured swap provider.
/// Repeated calls with the same configuration reuse the bridge and its database.
/// Disconnect before changing identities, providers, networks, or storage paths.
pub async fn configure_pubky(
    db: &BoltzDB,
    config: PubkySwapConfig,
    secret_key_hex: String,
) -> Result<(), BoltzError> {
    let config = normalize_config(config)?;
    let secret = decode_secret(secret_key_hex)?;
    let identity = identity_from_secret(&secret);
    let provider_config = config.clone();
    configure_provider(db, config, identity, None, async move {
        PubkyProvider::from_secret_key_with_storage(
            *secret,
            provider_config.provider,
            REQUEST_TIMEOUT,
            provider_config.data_dir.into(),
            provider_config.network.as_bitcoin_network(),
        )
        .await
        .map_err(bridge_error)
    })
    .await
}

/// Use an existing scoped session, including an account imported through Pubky Ring.
/// The wallet secret derives only a separate communication key, never the account key.
/// Matching configurations renew the grant in place while keeping recovery attached.
pub async fn configure_pubky_session(
    db: &BoltzDB,
    config: PubkySwapConfig,
    session_secret: String,
    public_key: String,
    application_scope: String,
    wallet_secret_hex: String,
) -> Result<(), BoltzError> {
    let session_secret = Zeroizing::new(session_secret);
    let config = normalize_config(config)?;
    let identity = canonical_pubky(&public_key).map_err(bridge_error)?;
    let wallet_secret = decode_secret(wallet_secret_hex)?;
    let transport_identity = pubky_session_identity(
        hex::encode(*wallet_secret),
        identity.clone(),
        config.provider.clone(),
        application_scope.clone(),
    )?;
    let credentials = SessionCredentials {
        token: session_secret,
        owner: identity,
        scope: application_scope,
        wallet_secret,
    };
    let renewal = credentials.clone();
    let provider_config = config.clone();
    configure_provider(db, config, transport_identity, Some(renewal), async move {
        PubkyProvider::from_session_with_storage(
            credentials.token.to_string(),
            credentials.owner,
            credentials.scope,
            *credentials.wallet_secret,
            provider_config.provider,
            REQUEST_TIMEOUT,
            provider_config.data_dir.into(),
            provider_config.network.as_bitcoin_network(),
        )
        .await
        .map_err(bridge_error)
    })
    .await
}

/// Read the account hint in a saved grant for selecting local recovery state.
/// This does not validate authorization; network requests revalidate the grant.
pub fn pubky_session_account(session_secret: String) -> Result<String, BoltzError> {
    let session_secret = Zeroizing::new(session_secret);
    pubky_swap_boltz::pubky_transport::session_rpc::session_account_hint(&session_secret).map_err(
        |_| BoltzError::InvalidInput {
            error_details: "Invalid Pubky grant credential".into(),
        },
    )
}

/// Return the deterministic communication identity for a Ring-authorized account.
pub fn pubky_session_identity(
    wallet_secret_hex: String,
    public_key: String,
    provider: String,
    application_scope: String,
) -> Result<String, BoltzError> {
    let secret = decode_secret(wallet_secret_hex)?;
    let derived = Zeroizing::new(
        pubky_swap_boltz::pubky_transport::session_rpc::derive_transport_secret(
            &secret,
            &public_key,
            &provider,
            &application_scope,
        )
        .map_err(|_| BoltzError::InvalidInput {
            error_details: "Invalid Pubky session identity configuration".into(),
        })?,
    );
    Ok(identity_from_secret(&derived))
}

fn normalize_config(mut config: PubkySwapConfig) -> Result<PubkySwapConfig, BoltzError> {
    config.provider = canonical_pubky(&config.provider).map_err(bridge_error)?;
    config.electrum_url = normalize_electrum_url(&config.electrum_url)?;
    validate_config(&config)?;
    Ok(config)
}

async fn configure_provider<F>(
    db: &BoltzDB,
    config: PubkySwapConfig,
    identity: String,
    renewal: Option<SessionCredentials>,
    create_provider: F,
) -> Result<(), BoltzError>
where
    F: Future<Output = Result<PubkyProvider, BoltzError>>,
{
    let binding = format!(
        "{}|{}|{}",
        identity,
        config.provider,
        config.network.as_str()
    );
    let mut guard = CONFIGURED.lock().await;
    // Credential renewal changes neither recovery state nor its binding. The inbox
    // serializes replacement between requests, so active recovery may keep its read guard.
    if let (Some(active), Some(credentials)) = (guard.as_ref(), renewal) {
        if active.config == config && active.binding == binding {
            let provider = active
                .provider
                .as_ref()
                .ok_or_else(|| BoltzError::ConnectionError {
                    error_details: "The configured Pubky provider is unavailable".into(),
                })?;
            provider
                .renew_session(
                    credentials.token.to_string(),
                    credentials.owner,
                    credentials.scope,
                    *credentials.wallet_secret,
                )
                .await
                .map_err(bridge_error)?;
            RECOVERY_WAKEUP.notify_one();
            return Ok(());
        }
    }
    let _operation = try_configuration_guard(db)?;
    db.ensure_pubky_binding_allowed(Some(&binding)).await?;
    if let Some(active) = guard.as_ref() {
        if active.config == config && active.binding == binding {
            return Ok(());
        }
        return Err(BoltzError::InvalidInput {
            error_details: "Disconnect the Pubky swap bridge before changing its configuration"
                .into(),
        });
    }

    let store = Store::open(Path::new(&config.data_dir), &binding).map_err(bridge_error)?;
    let provider = Arc::new(create_provider.await?);
    let chain = ElectrumChain::connect(
        config.electrum_url.clone(),
        config.network.as_bitcoin_network(),
    )
    .await
    .map_err(bridge_error)?;
    let chain: Arc<dyn Chain> = Arc::new(chain);
    let bridge = Bridge::new(
        provider.clone(),
        chain.clone(),
        store,
        BridgeSettings {
            network: config.network.as_bitcoin_network(),
            max_fee_bps: config.max_fee_bps,
            max_amount_sat: config.max_amount_sat,
        },
    );
    let delivery_task = provider.subscribe_delivery().map(|mut notifications| {
        tokio::spawn(async move {
            while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                notifications.recv().await
            {
                RECOVERY_WAKEUP.notify_one();
            }
        })
    });
    *guard = Some(ConfiguredBridge {
        config,
        binding,
        bridge,
        chain,
        provider: Some(provider),
        delivery_task,
    });
    RECOVERY_WAKEUP.notify_one();
    Ok(())
}

fn try_configuration_guard(
    db: &BoltzDB,
) -> Result<tokio::sync::RwLockWriteGuard<'_, ()>, BoltzError> {
    // Recovery can take nested read guards. Never queue a writer ahead of them.
    db.recovery_gate
        .try_write()
        .map_err(|_| BoltzError::InvalidInput {
            error_details: "A swap operation is active. Try again after it finishes".into(),
        })
}

/// Detach only when no pending swap or interrupted creation needs this binding.
/// Reject an active operation without queuing behind its nested recovery reads.
pub async fn prepare_pubky_switch(db: &BoltzDB) -> Result<(), BoltzError> {
    let _operation = try_configuration_guard(db)?;
    db.ensure_pubky_binding_allowed(None).await?;
    disconnect_pubky().await;
    Ok(())
}

/// Release the configured Pubky identity while keeping legacy swap recovery active.
pub async fn disconnect_pubky() {
    CONFIGURED.lock().await.take();
}

/// Sanitized delivery state for one resource in the configured private store.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PubkyDeliveryStatus {
    pub id: String,
    pub scope: String,
    pub published: bool,
    pub publication_paused: bool,
    pub cleanup_paused: bool,
    pub publication_failure: Option<String>,
    pub cleanup_failure: Option<String>,
}

/// The independent delivery operation explicitly retried after correction.
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum PubkyDeliveryOperation {
    Publication,
    Cleanup,
}

pub async fn pubky_delivery_status() -> Result<Vec<PubkyDeliveryStatus>, BoltzError> {
    let guard = CONFIGURED.lock().await;
    let provider = guard
        .as_ref()
        .and_then(|active| active.provider.as_ref())
        .ok_or_else(|| BoltzError::ConnectionError {
            error_details: "Configure the original Pubky identity and provider first".into(),
        })?;
    Ok(provider
        .delivery_status()
        .map_err(bridge_error)?
        .into_iter()
        .map(|status| PubkyDeliveryStatus {
            id: status.id,
            scope: status.scope,
            published: status.published,
            publication_paused: status.publication_paused,
            cleanup_paused: status.cleanup_paused,
            publication_failure: status
                .publication_failure
                .map(|failure| failure.to_string()),
            cleanup_failure: status.cleanup_failure.map(|failure| failure.to_string()),
        })
        .collect())
}

pub async fn retry_pubky_delivery(
    id: String,
    operation: PubkyDeliveryOperation,
) -> Result<(), BoltzError> {
    use pubky_swap_boltz::pubky_transport::DeliveryOperation;
    let guard = CONFIGURED.lock().await;
    let provider = guard
        .as_ref()
        .and_then(|active| active.provider.as_ref())
        .ok_or_else(|| BoltzError::ConnectionError {
            error_details: "Configure the original Pubky identity and provider first".into(),
        })?;
    let operation = match operation {
        PubkyDeliveryOperation::Publication => DeliveryOperation::Publication,
        PubkyDeliveryOperation::Cleanup => DeliveryOperation::Cleanup,
    };
    provider
        .retry_delivery(&id, operation)
        .map_err(bridge_error)?;
    RECOVERY_WAKEUP.notify_one();
    Ok(())
}

pub(crate) async fn backup_context() -> Option<(String, Arc<Bridge>)> {
    CONFIGURED
        .lock()
        .await
        .as_ref()
        .map(|active| (active.config.data_dir.clone(), active.bridge.clone()))
}

pub(crate) struct RestoreGuard {
    _guard: tokio::sync::MutexGuard<'static, Option<ConfiguredBridge>>,
}

pub(crate) async fn restore_guard() -> Result<RestoreGuard, BoltzError> {
    let guard = CONFIGURED.lock().await;
    if guard.is_some() {
        return Err(BoltzError::InvalidInput {
            error_details: "Disconnect the Pubky swap bridge before restoring a backup".into(),
        });
    }
    Ok(RestoreGuard { _guard: guard })
}

pub(crate) async fn bridge(network: BoltzNetwork) -> Result<Arc<Bridge>, BoltzError> {
    let guard = CONFIGURED.lock().await;
    let active = configured_for(&guard, network)?;
    Ok(active.bridge.clone())
}

pub(crate) async fn binding(network: BoltzNetwork) -> Result<String, BoltzError> {
    let guard = CONFIGURED.lock().await;
    Ok(configured_for(&guard, network)?.binding.clone())
}

pub(crate) async fn bridge_for_binding(
    network: BoltzNetwork,
    expected_binding: &str,
) -> Result<Arc<Bridge>, BoltzError> {
    let guard = CONFIGURED.lock().await;
    let active = configured_for(&guard, network)?;
    verify_binding(active, expected_binding)?;
    Ok(active.bridge.clone())
}

pub(crate) async fn chain_for_binding(
    network: BoltzNetwork,
    expected_binding: &str,
) -> Result<Arc<dyn Chain>, BoltzError> {
    let guard = CONFIGURED.lock().await;
    let active = configured_for(&guard, network)?;
    verify_binding(active, expected_binding)?;
    Ok(active.chain.clone())
}

pub(crate) async fn electrum_for_binding(
    network: BoltzNetwork,
    expected_binding: &str,
    requested_url: &str,
) -> Result<String, BoltzError> {
    let guard = CONFIGURED.lock().await;
    let active = configured_for(&guard, network)?;
    verify_binding(active, expected_binding)?;
    if normalize_electrum_url(requested_url)? != active.config.electrum_url {
        return Err(BoltzError::InvalidInput {
            error_details: "Swap Electrum URL must match the configured Pubky bridge".into(),
        });
    }
    Ok(active.config.electrum_url.clone())
}

fn verify_binding(active: &ConfiguredBridge, expected: &str) -> Result<(), BoltzError> {
    if active.binding != expected {
        return Err(BoltzError::ConnectionError {
            error_details:
                "Reconnect the original Pubky identity and provider to recover this swap".into(),
        });
    }
    Ok(())
}

fn configured_for(
    configured: &Option<ConfiguredBridge>,
    network: BoltzNetwork,
) -> Result<&ConfiguredBridge, BoltzError> {
    configured.as_ref().filter(|c| c.config.network == network).ok_or_else(|| {
        BoltzError::ConnectionError {
            error_details: "Configure a registered Pubky identity and a provider for this network before swapping".into(),
        }
    })
}

fn decode_secret(secret_key_hex: String) -> Result<Zeroizing<[u8; 32]>, BoltzError> {
    let secret_key_hex = Zeroizing::new(secret_key_hex);
    let mut secret = Zeroizing::new([0; 32]);
    hex::decode_to_slice(secret_key_hex.as_bytes(), secret.as_mut()).map_err(|_| {
        BoltzError::InvalidInput {
            error_details: "Pubky secret must be 32 bytes encoded as hexadecimal".into(),
        }
    })?;
    Ok(secret)
}

fn validate_config(config: &PubkySwapConfig) -> Result<(), BoltzError> {
    if !Path::new(&config.data_dir).is_absolute()
        || config.max_amount_sat == 0
        || config.max_fee_bps == 0
        || config.max_fee_bps > 10_000
    {
        return Err(BoltzError::InvalidInput {
            error_details: "Pubky swaps require an absolute private data directory and valid amount and fee limits".into(),
        });
    }
    Ok(())
}

fn normalize_electrum_url(value: &str) -> Result<String, BoltzError> {
    let value = value.trim();
    if value.is_empty() || value.starts_with("http://") || value.starts_with("https://") {
        return Err(BoltzError::InvalidInput {
            error_details: "Pubky swaps require an Electrum TCP or TLS endpoint".into(),
        });
    }
    if let Some(host) = value.strip_prefix("tls://") {
        Ok(format!("ssl://{host}"))
    } else if value.starts_with("ssl://") || value.starts_with("tcp://") {
        Ok(value.into())
    } else if value.contains("://") {
        Err(BoltzError::InvalidInput {
            error_details: "Unsupported Electrum URL scheme".into(),
        })
    } else {
        Ok(format!("ssl://{value}"))
    }
}

pub(crate) fn bridge_error(error: pubky_swap_boltz::Error) -> BoltzError {
    BoltzError::SwapError {
        error_details: format!("Pubky swap bridge: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::boltz::pubky_fixture as fixture;

    #[tokio::test]
    async fn busy_configuration_does_not_queue_a_writer() {
        let db = BoltzDB::new(":memory:").await.unwrap();
        let recovery = db.recovery_gate.read().await;
        assert!(try_configuration_guard(&db).is_err());
        // Claim and refund recovery must still acquire their nested read guard.
        let nested = db.recovery_gate.try_read().unwrap();
        drop(nested);
        drop(recovery);
        assert!(try_configuration_guard(&db).is_ok());
    }

    #[tokio::test]
    #[serial_test::serial(pubky_configuration)]
    async fn rejected_switch_preserves_the_configured_bridge() {
        let db = BoltzDB::new(":memory:").await.unwrap();
        let configured = fixture::fixture_bridge().unwrap();
        let binding = fixture::fixture_binding();
        *CONFIGURED.lock().await = Some(ConfiguredBridge {
            config: PubkySwapConfig {
                network: BoltzNetwork::Regtest,
                provider: fixture::fixture_provider(),
                electrum_url: "tcp://127.0.0.1:50001".into(),
                data_dir: "/unused-test-data".into(),
                max_fee_bps: 500,
                max_amount_sat: 1_000_000,
            },
            binding,
            bridge: configured.clone(),
            chain: Arc::new(fixture::FixtureChain),
            provider: None,
            delivery_task: None,
        });
        // An unreadable intent must fail closed, too, without dropping recovery access.
        db.conn
            .lock()
            .await
            .execute(
                "INSERT INTO creation_intents VALUES ('key','id','invalid-json')",
                [],
            )
            .unwrap();
        assert!(prepare_pubky_switch(&db).await.is_err());
        assert!(Arc::ptr_eq(
            &bridge(BoltzNetwork::Regtest).await.unwrap(),
            &configured
        ));
        db.conn
            .lock()
            .await
            .execute("DELETE FROM creation_intents", [])
            .unwrap();
        prepare_pubky_switch(&db).await.unwrap();
        assert!(CONFIGURED.lock().await.is_none());
    }

    #[tokio::test]
    #[serial_test::serial(pubky_configuration)]
    async fn repeated_configuration_does_not_reopen_exclusive_journals() {
        let db = BoltzDB::new(":memory:").await.unwrap();
        let config = PubkySwapConfig {
            network: BoltzNetwork::Regtest,
            provider: fixture::fixture_provider(),
            electrum_url: "tcp://127.0.0.1:50001".into(),
            data_dir: "/unused-test-data".into(),
            max_fee_bps: 500,
            max_amount_sat: 1_000_000,
        };
        *CONFIGURED.lock().await = Some(ConfiguredBridge {
            config: config.clone(),
            binding: fixture::fixture_binding(),
            bridge: fixture::fixture_bridge().unwrap(),
            chain: Arc::new(fixture::FixtureChain),
            provider: None,
            delivery_task: None,
        });
        configure_provider(&db, config, fixture::fixture_identity(), None, async {
            panic!("reused configuration must not initialize another journal owner")
        })
        .await
        .unwrap();
        disconnect_pubky().await;
    }

    fn grant_fixture(owner: &str, token: &str) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let claims = serde_json::json!({
            "iss": owner, "client_id": "bitkit.to", "caps": [], "cnf": owner,
            "jti": token, "iat": 0, "exp": 1,
        });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("pubky-grant-credential-v1:{owner}:fixture:e30.{payload}.fixture")
    }

    async fn configured_session_fixture() -> (PubkySwapConfig, Arc<Bridge>) {
        let owner = identity_from_secret(&[11; 32]);
        let provider = Arc::new(
            PubkyProvider::from_session(
                grant_fixture(&owner, "original"),
                owner.clone(),
                "/pub/bitkit.to/bitkit/wallet/".into(),
                [12; 32],
                owner.clone(),
                REQUEST_TIMEOUT,
            )
            .await
            .unwrap(),
        );
        let config = PubkySwapConfig {
            network: BoltzNetwork::Regtest,
            provider: owner,
            electrum_url: "tcp://127.0.0.1:50001".into(),
            data_dir: "/unused-test-data".into(),
            max_fee_bps: 500,
            max_amount_sat: 1_000_000,
        };
        let bridge = fixture::fixture_bridge().unwrap();
        *CONFIGURED.lock().await = Some(ConfiguredBridge {
            binding: format!("{}|{}|regtest", provider.identity(), config.provider),
            config: config.clone(),
            bridge: bridge.clone(),
            chain: Arc::new(fixture::FixtureChain),
            provider: Some(provider),
            delivery_task: None,
        });
        (config, bridge)
    }

    async fn renew_fixture_session(
        db: &BoltzDB,
        config: &PubkySwapConfig,
        token: String,
    ) -> Result<(), BoltzError> {
        configure_pubky_session(
            db,
            config.clone(),
            token,
            config.provider.clone(),
            "/pub/bitkit.to/bitkit/wallet/".into(),
            hex::encode([12; 32]),
        )
        .await
    }

    #[tokio::test]
    #[serial_test::serial(pubky_configuration)]
    async fn session_renewal_remains_available_during_recovery_and_rejects_invalid_credentials() {
        let db = BoltzDB::new(":memory:").await.unwrap();
        let (config, configured) = configured_session_fixture().await;
        let recovery = db.recovery_gate.read().await;
        renew_fixture_session(&db, &config, grant_fixture(&config.provider, "renewed"))
            .await
            .unwrap();
        let mut changed_config = config.clone();
        changed_config.max_amount_sat += 1;
        assert!(renew_fixture_session(
            &db,
            &changed_config,
            grant_fixture(&config.provider, "renewed"),
        )
        .await
        .is_err());
        assert!(db.recovery_gate.try_read().is_ok());
        assert!(Arc::ptr_eq(
            &bridge(BoltzNetwork::Regtest).await.unwrap(),
            &configured
        ));
        drop(recovery);

        // Reusing the binding must validate the new credential rather than return cached success.
        assert!(renew_fixture_session(&db, &config, "invalid grant".into())
            .await
            .is_err());
        assert!(Arc::ptr_eq(
            &bridge(BoltzNetwork::Regtest).await.unwrap(),
            &configured
        ));
        renew_fixture_session(&db, &config, grant_fixture(&config.provider, "renewed"))
            .await
            .unwrap();
        assert!(Arc::ptr_eq(
            &bridge(BoltzNetwork::Regtest).await.unwrap(),
            &configured
        ));
        disconnect_pubky().await;
    }

    #[tokio::test]
    #[serial_test::serial(pubky_configuration)]
    async fn cancelled_session_renewal_preserves_the_bridge_without_blocking_recovery() {
        let db = BoltzDB::new(":memory:").await.unwrap();
        let (config, configured) = configured_session_fixture().await;
        let guard = CONFIGURED.lock().await;
        let mut renewal = Box::pin(renew_fixture_session(
            &db,
            &config,
            grant_fixture(&config.provider, "renewed"),
        ));
        tokio::select! {
            biased;
            result = &mut renewal => panic!("renewal must wait for configuration lock: {result:?}"),
            _ = tokio::task::yield_now() => {}
        }
        assert!(db.recovery_gate.try_read().is_ok());
        drop(renewal);
        assert!(db.recovery_gate.try_read().is_ok());
        drop(guard);
        assert!(Arc::ptr_eq(
            &bridge(BoltzNetwork::Regtest).await.unwrap(),
            &configured
        ));
        renew_fixture_session(&db, &config, grant_fixture(&config.provider, "renewed"))
            .await
            .unwrap();
        disconnect_pubky().await;
    }

    #[tokio::test]
    async fn ring_storage_identity_matches_the_provider_and_survives_session_renewal() {
        let owner = identity_from_secret(&[1; 32]);
        let provider = identity_from_secret(&[2; 32]);
        let scope = "/pub/bitkit.to/bitkit/wallet/";
        let identity = pubky_session_identity(
            hex::encode([3; 32]),
            owner.clone(),
            provider.clone(),
            scope.into(),
        )
        .unwrap();
        for token in ["first-session", "renewed-session"] {
            let handle = PubkyProvider::from_session(
                grant_fixture(&owner, token),
                owner.clone(),
                scope.into(),
                [3; 32],
                provider.clone(),
                REQUEST_TIMEOUT,
            )
            .await
            .unwrap();
            assert_eq!(handle.identity(), identity);
        }
        assert_ne!(identity, owner);
        assert_ne!(
            identity,
            pubky_session_identity(
                hex::encode([3; 32]),
                owner,
                provider,
                "/pub/another.app/another/wallet/".into()
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn same_account_session_uses_a_stable_distinct_transport_identity() {
        let owner = identity_from_secret(&[11; 32]);
        let scope = "/pub/bitkit.to/bitkit/wallet/";
        let transport = pubky_session_identity(
            hex::encode([12; 32]),
            owner.clone(),
            owner.clone(),
            scope.into(),
        )
        .unwrap();
        assert_ne!(transport, owner);
        for credential in ["original-grant", "renewed-grant"] {
            let handle = PubkyProvider::from_session(
                grant_fixture(&owner, credential),
                owner.clone(),
                scope.into(),
                [12; 32],
                owner.clone(),
                REQUEST_TIMEOUT,
            )
            .await
            .unwrap();
            assert_eq!(handle.identity(), transport);
            assert_eq!(handle.provider_key(), owner);
        }
    }

    #[tokio::test]
    async fn different_account_root_identity_remains_the_authenticated_transport() {
        let owner = identity_from_secret(&[13; 32]);
        let provider = identity_from_secret(&[14; 32]);
        let handle = PubkyProvider::from_secret_key([13; 32], provider.clone(), REQUEST_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(handle.identity(), owner);
        assert_eq!(handle.provider_key(), provider);
    }

    #[test]
    fn provider_prefixes_identify_the_same_key() {
        let plain = "q9x5sfjbpajdebk45b9jashgb86iem7rnwpmu16px3ens63xzwro";
        assert_eq!(
            canonical_pubky(plain).unwrap(),
            canonical_pubky(&format!("pubky{plain}")).unwrap()
        );
    }

    #[test]
    fn secret_errors_do_not_include_the_input() {
        let supplied = "private-material";
        let error = decode_secret(supplied.into()).unwrap_err().to_string();
        assert!(!error.contains(supplied));
        assert!(decode_secret("12".repeat(32)).is_ok());
        assert!(decode_secret("12".repeat(33)).is_err());
    }

    #[tokio::test]
    async fn cancellation_aborts_detached_configuration_work() {
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker = finished.clone();
        let gate = Arc::new(tokio::sync::Notify::new());
        let waiter = gate.clone();
        let task = tokio::spawn(async move {
            waiter.notified().await;
            marker.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let cancellation = crate::AbortSwapConfiguration(task.abort_handle());
        drop(cancellation);
        gate.notify_one();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!finished.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn electrum_urls_keep_tls_as_default_and_reject_http() {
        assert_eq!(
            normalize_electrum_url("example.com:50002").unwrap(),
            "ssl://example.com:50002"
        );
        assert_eq!(
            normalize_electrum_url("tls://example.com:50002").unwrap(),
            "ssl://example.com:50002"
        );
        assert_eq!(
            normalize_electrum_url("tcp://127.0.0.1:50001").unwrap(),
            "tcp://127.0.0.1:50001"
        );
        assert!(normalize_electrum_url("https://example.com/api").is_err());
        assert!(normalize_electrum_url("").is_err());
    }
}
