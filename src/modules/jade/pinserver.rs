//! The blind pinserver exchange that unlocks a PIN protected Jade.
//!
//! `auth_user` either returns `true`, meaning the device is already usable, or
//! it returns an `http_request` describing a call the host must make on the
//! device's behalf. The host performs it and feeds the response back through the
//! method named in `on-reply`, which is `pin`. The exchange is end to end
//! encrypted between device and pinserver, so the host never sees the PIN; its
//! role is purely to carry bytes.
//!
//! Two details here are load bearing and easy to get wrong:
//!
//! - The response body must be JSON decoded into a CBOR **map**. Firmware
//!   requires `params` to be a map with a text `data` member and rejects
//!   anything else, so forwarding raw HTTP bytes fails every unlock.
//! - An HTTP failure must still send a `pin` message, with no `params`. The
//!   device is blocked indefinitely waiting for one; abandoning the loop leaves
//!   it consuming the next unrelated request as the awaited reply, which puts
//!   every subsequent call one message out of step.

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::errors::JadeError;
use super::transport::JadeConnection;
use super::types::JadeNetwork;

/// How long the whole unlock may take, including user PIN entry on the device.
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(300);

/// How long a single pinserver call may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound on a pinserver response body.
const MAX_BODY_BYTES: u64 = 64 * 1024;

/// Round trips the device may ask for before the host gives up.
const MAX_ROUND_TRIPS: usize = 4;

/// The pinserver Blockstream operates, and the only host expected in practice.
const DEFAULT_PINSERVER_HOST: &str = "jadepin.blockstream.com";

/// The one method the device is allowed to name in `on-reply`.
const EXPECTED_ON_REPLY: &str = "pin";

/// Performs the pinserver call.
///
/// A trait so tests can drive the whole unlock with no network access. It is
/// deliberately internal: there is no FFI seam for swapping the implementation.
#[async_trait]
pub(crate) trait PinServerHttp: Send + Sync {
    /// POST or GET `body` to the chosen URL and return the response bytes.
    async fn request(
        &self,
        url: &str,
        method: &str,
        body: Option<String>,
    ) -> Result<Vec<u8>, JadeError>;
}

/// The real implementation, over `reqwest`.
pub(crate) struct ReqwestPinServer;

#[async_trait]
impl PinServerHttp for ReqwestPinServer {
    async fn request(
        &self,
        url: &str,
        method: &str,
        body: Option<String>,
    ) -> Result<Vec<u8>, JadeError> {
        let parsed = validate_url(url)?;
        let address = resolve_and_validate(&parsed).await?;

        let host = parsed.host_str().unwrap_or_default().to_string();
        let client = reqwest::Client::builder()
            // The URL list comes from the device. Following a redirect would let
            // a tampered unit bounce the host somewhere the checks above already
            // rejected.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(HTTP_TIMEOUT)
            .timeout(HTTP_TIMEOUT)
            // Pin the socket to the address that was validated, so a second DNS
            // lookup cannot return a different one.
            .resolve_to_addrs(&host, &[address])
            .build()
            .map_err(|error| JadeError::PinServerError {
                error_details: format!("could not build the http client: {error}"),
            })?;

        let request = match method {
            "POST" => {
                let builder = client.post(parsed.clone());
                match body {
                    Some(body) => builder
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(body),
                    None => builder,
                }
            }
            "GET" => client.get(parsed.clone()),
            other => {
                return Err(JadeError::PinServerError {
                    error_details: format!("unsupported http method {other}"),
                })
            }
        };

        // reqwest embeds the full URL in its Display output, so it is stripped
        // before the error reaches a log or the application.
        let response = request.send().await.map_err(|error| {
            let error = error.without_url();
            JadeError::PinServerError {
                error_details: format!("pin server request failed: {error}"),
            }
        })?;

        if !response.status().is_success() {
            return Err(JadeError::PinServerError {
                error_details: format!("pin server returned status {}", response.status()),
            });
        }

        if let Some(length) = response.content_length() {
            if length > MAX_BODY_BYTES {
                return Err(JadeError::PinServerError {
                    error_details: format!(
                        "pin server response of {length} bytes exceeds the {MAX_BODY_BYTES} byte limit"
                    ),
                });
            }
        }

        let bytes = response.bytes().await.map_err(|error| {
            let error = error.without_url();
            JadeError::PinServerError {
                error_details: format!("could not read the pin server response: {error}"),
            }
        })?;

        // Re-check after reading, because a response without Content-Length
        // slips past the check above.
        if bytes.len() as u64 > MAX_BODY_BYTES {
            return Err(JadeError::PinServerError {
                error_details: format!(
                    "pin server response exceeds the {MAX_BODY_BYTES} byte limit"
                ),
            });
        }

        Ok(bytes.to_vec())
    }
}

/// Reject any URL this host should not be making a request to.
fn validate_url(url: &str) -> Result<url::Url, JadeError> {
    let reject = |reason: &str| JadeError::PinServerError {
        error_details: format!("refusing pin server url: {reason}"),
    };

    let parsed = url::Url::parse(url).map_err(|error| reject(&format!("unparsable ({error})")))?;

    if parsed.scheme() != "https" {
        return Err(reject("only https is supported"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(reject("credentials are not allowed"));
    }
    if let Some(port) = parsed.port() {
        if port != 443 {
            return Err(reject("only port 443 is allowed"));
        }
    }
    let Some(host) = parsed.host_str() else {
        return Err(reject("no host"));
    };
    if host.ends_with(".onion") {
        return Err(reject("onion services are not supported"));
    }
    if !host.eq_ignore_ascii_case(DEFAULT_PINSERVER_HOST) {
        // A second-hand or tampered unit can carry a pinserver its previous
        // owner configured, so this is worth surfacing even though a custom
        // pinserver is a legitimate configuration.
        log::warn!("[jade] using a non-default pin server host");
    }
    Ok(parsed)
}

/// Resolve the host and reject addresses that should never be reachable here.
async fn resolve_and_validate(url: &url::Url) -> Result<std::net::SocketAddr, JadeError> {
    let host = url.host_str().unwrap_or_default().to_string();
    let port = url.port().unwrap_or(443);
    let target = format!("{host}:{port}");

    let addresses = tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        target
            .to_socket_addrs()
            .map(|iter| iter.collect::<Vec<_>>())
    })
    .await
    .map_err(|error| JadeError::PinServerError {
        error_details: format!("dns task failed: {error}"),
    })?
    .map_err(|error| JadeError::PinServerError {
        error_details: format!("could not resolve the pin server host: {error}"),
    })?;

    addresses
        .into_iter()
        .find(|address| is_public(address.ip()))
        .ok_or_else(|| JadeError::PinServerError {
            error_details: "pin server host resolved to no usable public address".to_string(),
        })
}

/// Whether an address is one this host should send a device-directed request to.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 100.64.0.0/10, carrier grade NAT. There is no stable std helper.
            let is_cgnat = octets[0] == 100 && (64..128).contains(&octets[1]);
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || is_cgnat)
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            // fc00::/7 unique local, fe80::/10 link local.
            let is_unique_local = (segments[0] & 0xfe00) == 0xfc00;
            let is_link_local = (segments[0] & 0xffc0) == 0xfe80;
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.to_ipv4_mapped().is_some()
                || is_unique_local
                || is_link_local)
        }
    }
}

// ============================================================================
// Wire shapes
// ============================================================================

#[derive(Debug, Deserialize)]
struct HttpRequestEnvelope {
    http_request: HttpRequest,
}

#[derive(Debug, Deserialize)]
struct HttpRequest {
    params: HttpRequestParams,
    #[serde(rename = "on-reply")]
    on_reply: String,
}

#[derive(Debug, Deserialize)]
struct HttpRequestParams {
    urls: Vec<String>,
    method: String,
    #[serde(default)]
    accept: Option<String>,
    #[serde(default)]
    data: Option<ciborium::Value>,
}

#[derive(serde::Serialize)]
struct AuthUserParams<'a> {
    network: &'a str,
    epoch: u64,
}

/// Run `auth_user` and, if the device asks, the pinserver exchange.
pub(crate) async fn run_unlock(
    connection: &mut JadeConnection,
    network: JadeNetwork,
    http: &dyn PinServerHttp,
    epoch: u64,
) -> Result<(), JadeError> {
    let params = AuthUserParams {
        network: network.wire_name(),
        epoch,
    };
    let reply = connection
        .exchange("auth_user", Some(params), UNLOCK_TIMEOUT)
        .await?;
    let mut result = reply.into_result(super::types::MIN_JADE_FIRMWARE)?;

    for _ in 0..MAX_ROUND_TRIPS {
        // A boolean result ends the exchange either way.
        if let Some(unlocked) = result.as_bool() {
            return if unlocked {
                Ok(())
            } else {
                Err(JadeError::InvalidPin)
            };
        }

        let envelope: HttpRequestEnvelope = result
            .deserialized()
            .map_err(|error| JadeError::protocol(format!("unexpected auth_user reply: {error}")))?;
        let request = envelope.http_request;

        // The method name is supplied by the device. Dispatching on it blindly
        // would let a device make the host invoke any RPC with chosen params.
        if request.on_reply != EXPECTED_ON_REPLY {
            return Err(JadeError::protocol(format!(
                "device asked the host to call '{}', expected '{EXPECTED_ON_REPLY}'",
                request.on_reply
            )));
        }

        let body = perform(http, &request.params).await;
        result = send_pin(connection, body).await?;
    }

    Err(JadeError::PinServerError {
        error_details: format!("unlock did not finish within {MAX_ROUND_TRIPS} round trips"),
    })
}

/// Make the call the device asked for, returning the params for the follow-up.
///
/// A failure yields `None`, which becomes a `pin` message with no params. That
/// is what the device expects, and it is what keeps the two sides in step.
async fn perform(http: &dyn PinServerHttp, params: &HttpRequestParams) -> Option<ciborium::Value> {
    let use_json = matches!(
        params.accept.as_deref(),
        Some("json") | Some("application/json")
    );

    let url = params
        .urls
        .iter()
        .find(|candidate| !is_onion(candidate))
        .or_else(|| params.urls.first())?;

    // Firmware wraps the payload in an extra layer when it wants JSON, so
    // `data` is a CBOR map that has to be rendered as a JSON document.
    let body = match (&params.data, use_json) {
        (Some(data), true) => match cbor_to_json(data) {
            Ok(json) => Some(json.to_string()),
            Err(error) => {
                log::warn!("[jade] could not render pin server payload: {error}");
                return None;
            }
        },
        (Some(ciborium::Value::Text(text)), false) => Some(text.clone()),
        _ => None,
    };

    let response = match http.request(url, &params.method, body).await {
        Ok(response) => response,
        Err(error) => {
            log::warn!("[jade] pin server call failed: {error}");
            return None;
        }
    };

    if !use_json {
        return Some(ciborium::Value::Bytes(response));
    }

    match serde_json::from_slice::<serde_json::Value>(&response) {
        Ok(json) if json.is_object() => json_to_cbor(&json).ok(),
        Ok(_) => {
            log::warn!("[jade] pin server returned a non-object json body");
            None
        }
        Err(error) => {
            log::warn!("[jade] pin server returned invalid json: {error}");
            None
        }
    }
}

/// Send the follow-up `pin` message.
async fn send_pin(
    connection: &mut JadeConnection,
    params: Option<ciborium::Value>,
) -> Result<ciborium::Value, JadeError> {
    let reply = connection.exchange("pin", params, UNLOCK_TIMEOUT).await?;
    reply.into_result(super::types::MIN_JADE_FIRMWARE)
}

/// Whether a URL's host is an onion service.
///
/// A suffix test on the whole URL does not work: firmware sends
/// `http://<...>.onion/get_pin`, so the string ends with the document name.
fn is_onion(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.ends_with(".onion")))
        .unwrap_or(false)
}

/// Render a CBOR value as JSON for the pinserver request body.
pub(crate) fn cbor_to_json(value: &ciborium::Value) -> Result<serde_json::Value, JadeError> {
    let unsupported = |what: &str| JadeError::protocol(format!("cannot render {what} as json"));

    Ok(match value {
        ciborium::Value::Null => serde_json::Value::Null,
        ciborium::Value::Bool(inner) => serde_json::Value::Bool(*inner),
        ciborium::Value::Text(inner) => serde_json::Value::String(inner.clone()),
        ciborium::Value::Integer(inner) => {
            let as_i128: i128 = (*inner).into();
            let number = i64::try_from(as_i128).map_err(|_| unsupported("an oversized integer"))?;
            serde_json::Value::Number(number.into())
        }
        ciborium::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(cbor_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        ciborium::Value::Map(entries) => {
            let mut map = serde_json::Map::with_capacity(entries.len());
            for (key, value) in entries {
                let key = key
                    .as_text()
                    .ok_or_else(|| unsupported("a map with a non-text key"))?;
                map.insert(key.to_string(), cbor_to_json(value)?);
            }
            serde_json::Value::Object(map)
        }
        // The pinserver protocol carries binary as hex or base64 text, so a raw
        // byte string here means the device sent something unexpected.
        ciborium::Value::Bytes(_) => return Err(unsupported("a byte string")),
        ciborium::Value::Float(_) => return Err(unsupported("a float")),
        _ => return Err(unsupported("an unrecognised cbor value")),
    })
}

/// Convert the pinserver's JSON reply into the CBOR map the device expects.
pub(crate) fn json_to_cbor(value: &serde_json::Value) -> Result<ciborium::Value, JadeError> {
    Ok(match value {
        serde_json::Value::Null => ciborium::Value::Null,
        serde_json::Value::Bool(inner) => ciborium::Value::Bool(*inner),
        serde_json::Value::String(inner) => ciborium::Value::Text(inner.clone()),
        serde_json::Value::Number(number) => {
            if let Some(inner) = number.as_i64() {
                ciborium::Value::Integer(inner.into())
            } else {
                return Err(JadeError::protocol("cannot represent a json float in cbor"));
            }
        }
        serde_json::Value::Array(items) => ciborium::Value::Array(
            items
                .iter()
                .map(json_to_cbor)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        serde_json::Value::Object(entries) => ciborium::Value::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    json_to_cbor(value).map(|value| (ciborium::Value::Text(key.clone()), value))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}
