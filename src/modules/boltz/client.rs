use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::types::BoltzNetwork;
use boltz_client::network::electrum::ElectrumBitcoinClient;
use boltz_client::swaps::boltz::BoltzApiClientV2;
use boltz_client::swaps::ChainClient;

/// Electrum socket timeout, in seconds, used when building swap chain clients.
const ELECTRUM_TIMEOUT_SECS: u8 = 30;

/// Build a Boltz REST client for the given network.
pub fn build_boltz_client(network: BoltzNetwork) -> BoltzApiClientV2 {
    BoltzApiClientV2::default(network.as_client_network())
}

/// Build an Electrum-backed [`ChainClient`] for broadcasting and fetching swap
/// UTXOs. `electrum_url` accepts the same `ssl://`/`tcp://` scheme conventions
/// used elsewhere in the crate; a bare `host:port` is treated as TLS.
pub fn build_chain_client(
    network: BoltzNetwork,
    electrum_url: &str,
) -> Result<ChainClient, BoltzError> {
    let (host_port, tls) = parse_electrum_url(electrum_url);
    let client = ElectrumBitcoinClient::new(
        network.as_bitcoin_chain(),
        &host_port,
        tls,
        tls,
        ELECTRUM_TIMEOUT_SECS,
    )
    .map_err(|e| BoltzError::ConnectionError {
        error_details: format!("Failed to connect to Electrum server: {}", e),
    })?;
    Ok(ChainClient::new().with_bitcoin(client))
}

/// Split an Electrum URL into a `host:port` and a TLS flag. `ssl://` and
/// `tls://` mean TLS; `tcp://` means plaintext; a scheme-less URL defaults to
/// TLS (the safe default for public servers).
fn parse_electrum_url(url: &str) -> (String, bool) {
    if let Some(rest) = url.strip_prefix("ssl://") {
        (rest.to_string(), true)
    } else if let Some(rest) = url.strip_prefix("tls://") {
        (rest.to_string(), true)
    } else if let Some(rest) = url.strip_prefix("tcp://") {
        (rest.to_string(), false)
    } else {
        (url.to_string(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_electrum_url;

    #[test]
    fn parses_schemes() {
        assert_eq!(
            parse_electrum_url("ssl://electrum.example.com:50002"),
            ("electrum.example.com:50002".to_string(), true)
        );
        assert_eq!(
            parse_electrum_url("tcp://10.0.0.1:50001"),
            ("10.0.0.1:50001".to_string(), false)
        );
        assert_eq!(
            parse_electrum_url("electrum.example.com:50002"),
            ("electrum.example.com:50002".to_string(), true)
        );
    }
}
