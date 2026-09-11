use super::{BoltzError, BoltzNetwork};
use once_cell::sync::Lazy;
use pubky_swap_boltz::{
    chain::{Chain, ElectrumChain},
    provider::{canonical_pubky, identity_from_secret, Provider, PubkyProvider},
    service::{Bridge, BridgeSettings},
    store::Store,
};
use std::{path::Path, sync::Arc, time::Duration};
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
}

static CONFIGURED: Lazy<Mutex<Option<ConfiguredBridge>>> = Lazy::new(|| Mutex::new(None));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Connect an existing registered Pubky identity to its configured swap provider.
/// Repeated calls with the same configuration reuse the bridge and its database.
/// Disconnect before changing identities, providers, networks, or storage paths.
pub async fn configure_pubky(
    config: PubkySwapConfig,
    secret_key_hex: String,
) -> Result<(), BoltzError> {
    let config = normalize_config(config)?;
    let secret = decode_secret(secret_key_hex)?;
    let identity = identity_from_secret(&secret);
    let provider =
        PubkyProvider::from_secret_key(*secret, config.provider.clone(), REQUEST_TIMEOUT)
            .await
            .map_err(bridge_error)?;
    configure_provider(config, identity, provider).await
}

/// Use an existing scoped session, including an account imported through Pubky Ring.
/// The wallet secret derives only a separate communication key, never the account key.
pub async fn configure_pubky_session(
    config: PubkySwapConfig,
    session_secret: String,
    public_key: String,
    application_scope: String,
    wallet_secret_hex: String,
) -> Result<(), BoltzError> {
    let config = normalize_config(config)?;
    let identity = canonical_pubky(&public_key).map_err(bridge_error)?;
    let wallet_secret = decode_secret(wallet_secret_hex)?;
    let provider = PubkyProvider::from_session(
        session_secret,
        identity.clone(),
        application_scope,
        *wallet_secret,
        config.provider.clone(),
        REQUEST_TIMEOUT,
    )
    .await
    .map_err(bridge_error)?;
    let transport_identity = provider.identity();
    configure_provider(config, transport_identity, provider).await
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

async fn configure_provider(
    config: PubkySwapConfig,
    identity: String,
    provider: PubkyProvider,
) -> Result<(), BoltzError> {
    let binding = format!(
        "{}|{}|{}",
        identity,
        config.provider,
        config.network.as_str()
    );
    let mut guard = CONFIGURED.lock().await;
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
    let chain = ElectrumChain::connect(
        config.electrum_url.clone(),
        config.network.as_bitcoin_network(),
    )
    .await
    .map_err(bridge_error)?;
    let chain: Arc<dyn Chain> = Arc::new(chain);
    let bridge = Bridge::new(
        Arc::new(provider),
        chain.clone(),
        store,
        BridgeSettings {
            network: config.network.as_bitcoin_network(),
            max_fee_bps: config.max_fee_bps,
            max_amount_sat: config.max_amount_sat,
        },
    );
    *guard = Some(ConfiguredBridge {
        config,
        binding,
        bridge,
        chain,
    });
    Ok(())
}

/// Release the configured Pubky identity while keeping legacy swap recovery active.
pub async fn disconnect_pubky() {
    CONFIGURED.lock().await.take();
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

    fn grant_fixture(owner: &str, token: &str) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let claims = serde_json::json!({
            "iss": owner, "client_id": "bitkit.to", "caps": [], "cnf": owner,
            "jti": token, "iat": 0, "exp": 1,
        });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("pubky-grant-credential-v1:{owner}:fixture:e30.{payload}.fixture")
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
