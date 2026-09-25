//! The device side of the gateway's v1 HTTP API.
//!
//! Every call is a pure `build_*` function that turns the arguments into an
//! [`HttpRequest`], a thin executor that sends it, and a pure `parse_*`
//! function that maps the [`HttpResponse`] to a result, so everything but
//! the network is tested without a gateway.

use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use super::crypto::parse_wake_id;
use super::errors::{json_error, WakeError};
use super::registration::{is_valid_device_secret, RegistrationFields};
use super::types::{
    WakeAckOutcome, WakeIdentityProof, WakeRegistration, WakeRegistrationRequest, WakeServerInfo,
    WakeTopic,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// A request to the gateway. `bearer` is a device secret, so this type has
/// no `Debug`.
pub(crate) struct HttpRequest {
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) bearer: Option<String>,
    /// JSON body.
    pub(crate) body: Option<String>,
}

/// A response from the gateway.
pub(crate) struct HttpResponse {
    pub(crate) status: u16,
    pub(crate) body: String,
}

impl HttpResponse {
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

// ----- Wire bodies -----

#[derive(Serialize)]
struct RegisterBody<'a> {
    app: &'a str,
    install_id: &'a str,
    platform: &'a str,
    environment: &'a str,
    push_token: &'a str,
    encryption_key: &'a str,
    secret_sha256: &'a str,
    identities: &'a [String],
    topics: &'a [String],
    ts: u64,
    proofs: Vec<ProofBody<'a>>,
}

#[derive(Serialize)]
struct ProofBody<'a> {
    identity: &'a str,
    sig: &'a str,
}

#[derive(Serialize)]
struct AckBody<'a> {
    id: &'a str,
    outcome: &'a str,
}

#[derive(Serialize)]
struct PresenceBody {
    ttl_secs: u32,
}

#[derive(Serialize)]
struct TopicsBody<'a> {
    topics: &'a [String],
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    message: String,
}

#[derive(Deserialize)]
struct InfoResponse {
    audience: String,
    server_time: u64,
    version: String,
}

#[derive(Deserialize)]
struct RegisterResponse {
    device_id: String,
    identities: Vec<String>,
    topics: Vec<String>,
    unknown_topics: Vec<String>,
    server_time: u64,
}

#[derive(Deserialize)]
struct PresenceResponse {
    expires_at: u64,
}

#[derive(Deserialize)]
struct SetTopicsResponse {
    topics: Vec<String>,
}

#[derive(Deserialize)]
struct TopicsResponse {
    topics: Vec<TopicWire>,
}

#[derive(Deserialize)]
struct TopicWire {
    name: String,
    description: String,
    urgency: String,
    deadline_secs: u32,
    peer: bool,
    alertable: bool,
    display: Option<DisplayWire>,
}

#[derive(Deserialize)]
struct DisplayWire {
    title: String,
    body: String,
}

// ----- Builders -----

/// `gateway_url` with the path ending in `/`, so relative routes append to
/// any path prefix the gateway is mounted under.
fn base_url(gateway_url: &str) -> Result<Url, WakeError> {
    let mut url = Url::parse(gateway_url)
        .map_err(|_| WakeError::invalid_input("gateway url is not a valid url"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(WakeError::invalid_input(
            "gateway url must be http or https",
        ));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn request(
    method: Method,
    gateway_url: &str,
    route: &str,
    bearer: Option<&str>,
    body: Option<String>,
) -> Result<HttpRequest, WakeError> {
    let url = base_url(gateway_url)?
        .join(route)
        .map_err(|_| WakeError::invalid_input("gateway url cannot be joined"))?;
    let bearer = match bearer {
        Some(token) if !is_valid_device_secret(token) => {
            return Err(WakeError::invalid_input("malformed device secret"))
        }
        other => other.map(str::to_string),
    };
    Ok(HttpRequest {
        method,
        url,
        bearer,
        body,
    })
}

fn to_json<T: Serialize>(value: &T) -> String {
    // Bodies hold only strings, integers and string lists.
    serde_json::to_string(value).expect("request body serialization cannot fail")
}

/// `GET /v1/info`
pub(crate) fn build_server_info(gateway_url: &str) -> Result<HttpRequest, WakeError> {
    request(Method::GET, gateway_url, "v1/info", None, None)
}

/// `GET /v1/topics`
pub(crate) fn build_list_topics(gateway_url: &str) -> Result<HttpRequest, WakeError> {
    request(Method::GET, gateway_url, "v1/topics", None, None)
}

/// `POST /v1/devices`. The fields must still be the ones `message` was
/// built from, or no proof could verify.
pub(crate) fn build_register(
    gateway_url: &str,
    registration: &WakeRegistrationRequest,
    proofs: &[WakeIdentityProof],
) -> Result<HttpRequest, WakeError> {
    let signed = RegistrationFields {
        audience: &registration.audience,
        app: &registration.app,
        install_id: &registration.install_id,
        platform: registration.platform,
        environment: registration.environment,
        push_token: &registration.push_token,
        encryption_key: &registration.encryption_public_key,
        secret_sha256: &registration.secret_sha256,
        identities: &registration.identities,
        topics: &registration.topics,
        ts: registration.timestamp,
    }
    .preimage()?;
    if signed != registration.message {
        return Err(WakeError::invalid_input(
            "the registration changed after its message was prepared",
        ));
    }
    let body = RegisterBody {
        app: &registration.app,
        install_id: &registration.install_id,
        platform: registration.platform.as_str(),
        environment: registration.environment.as_str(),
        push_token: &registration.push_token,
        encryption_key: &registration.encryption_public_key,
        secret_sha256: &registration.secret_sha256,
        identities: &registration.identities,
        topics: &registration.topics,
        ts: registration.timestamp,
        proofs: proofs
            .iter()
            .map(|proof| ProofBody {
                identity: &proof.identity,
                sig: &proof.signature,
            })
            .collect(),
    };
    request(
        Method::POST,
        gateway_url,
        "v1/devices",
        None,
        Some(to_json(&body)),
    )
}

/// `POST /v1/acks`
pub(crate) fn build_ack(
    gateway_url: &str,
    device_secret: &str,
    wake_id: &str,
    outcome: WakeAckOutcome,
) -> Result<HttpRequest, WakeError> {
    let id = parse_wake_id(wake_id)?;
    let body = AckBody {
        id: &id,
        outcome: outcome.as_str(),
    };
    request(
        Method::POST,
        gateway_url,
        "v1/acks",
        Some(device_secret),
        Some(to_json(&body)),
    )
}

/// `PUT /v1/presence`. The gateway clamps `ttl_secs` to 15..=300.
pub(crate) fn build_set_presence(
    gateway_url: &str,
    device_secret: &str,
    ttl_secs: u32,
) -> Result<HttpRequest, WakeError> {
    request(
        Method::PUT,
        gateway_url,
        "v1/presence",
        Some(device_secret),
        Some(to_json(&PresenceBody { ttl_secs })),
    )
}

/// `DELETE /v1/presence`
pub(crate) fn build_clear_presence(
    gateway_url: &str,
    device_secret: &str,
) -> Result<HttpRequest, WakeError> {
    request(
        Method::DELETE,
        gateway_url,
        "v1/presence",
        Some(device_secret),
        None,
    )
}

/// `PUT /v1/devices/self/topics`
pub(crate) fn build_set_topics(
    gateway_url: &str,
    device_secret: &str,
    topics: &[String],
) -> Result<HttpRequest, WakeError> {
    request(
        Method::PUT,
        gateway_url,
        "v1/devices/self/topics",
        Some(device_secret),
        Some(to_json(&TopicsBody { topics })),
    )
}

/// `DELETE /v1/devices/self`
pub(crate) fn build_unregister(
    gateway_url: &str,
    device_secret: &str,
) -> Result<HttpRequest, WakeError> {
    request(
        Method::DELETE,
        gateway_url,
        "v1/devices/self",
        Some(device_secret),
        None,
    )
}

// ----- Parsers -----

/// The error of a non-2xx response: the gateway's
/// `{"error":"<code>","message":"..."}`, or code `unknown` for any other
/// body (a proxy error page, say).
fn rejection(response: &HttpResponse) -> WakeError {
    match serde_json::from_str::<ErrorBody>(&response.body) {
        Ok(body) => WakeError::GatewayRejected {
            status: response.status,
            code: body.error,
            message: body.message,
        },
        Err(_) => WakeError::GatewayRejected {
            status: response.status,
            code: "unknown".to_string(),
            message: "the response carries no wake error body".to_string(),
        },
    }
}

fn success_json<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, WakeError> {
    if !response.is_success() {
        return Err(rejection(response));
    }
    serde_json::from_str(&response.body).map_err(|e| WakeError::invalid_response(json_error(&e)))
}

/// For routes that answer 204.
pub(crate) fn parse_no_content(response: &HttpResponse) -> Result<(), WakeError> {
    if response.is_success() {
        Ok(())
    } else {
        Err(rejection(response))
    }
}

pub(crate) fn parse_server_info(response: &HttpResponse) -> Result<WakeServerInfo, WakeError> {
    let info: InfoResponse = success_json(response)?;
    Ok(WakeServerInfo {
        audience: info.audience,
        server_time: info.server_time,
        version: info.version,
    })
}

pub(crate) fn parse_list_topics(response: &HttpResponse) -> Result<Vec<WakeTopic>, WakeError> {
    let topics: TopicsResponse = success_json(response)?;
    Ok(topics
        .topics
        .into_iter()
        .map(|topic| {
            let (title, body) = match topic.display {
                Some(display) => (Some(display.title), Some(display.body)),
                None => (None, None),
            };
            WakeTopic {
                name: topic.name,
                description: topic.description,
                urgency: topic.urgency,
                deadline_secs: topic.deadline_secs,
                alertable: topic.alertable,
                title,
                body,
                peer: topic.peer,
            }
        })
        .collect())
}

pub(crate) fn parse_register(response: &HttpResponse) -> Result<WakeRegistration, WakeError> {
    let registered: RegisterResponse = success_json(response)?;
    Ok(WakeRegistration {
        device_id: registered.device_id,
        identities: registered.identities,
        topics: registered.topics,
        unknown_topics: registered.unknown_topics,
        server_time: registered.server_time,
    })
}

/// The presence expiry, unix seconds.
pub(crate) fn parse_set_presence(response: &HttpResponse) -> Result<u64, WakeError> {
    let presence: PresenceResponse = success_json(response)?;
    Ok(presence.expires_at)
}

/// The stored topics; unknown exact names are dropped by the gateway.
pub(crate) fn parse_set_topics(response: &HttpResponse) -> Result<Vec<String>, WakeError> {
    let topics: SetTopicsResponse = success_json(response)?;
    Ok(topics.topics)
}

// ----- Executor -----

async fn send(request: HttpRequest) -> Result<HttpResponse, WakeError> {
    let failed = |e: reqwest::Error| WakeError::RequestFailed {
        reason: e.to_string(),
    };
    // A redirect would turn a POST into a GET; surface it as an error instead.
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(failed)?;
    let mut builder = client
        .request(request.method, request.url)
        .header(ACCEPT, "application/json");
    if let Some(token) = request.bearer {
        builder = builder.bearer_auth(token);
    }
    if let Some(body) = request.body {
        builder = builder.header(CONTENT_TYPE, "application/json").body(body);
    }
    let response = builder.send().await.map_err(failed)?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(failed)?;
    Ok(HttpResponse { status, body })
}

/// Reads the gateway's audience and clock.
pub async fn wake_server_info(gateway_url: String) -> Result<WakeServerInfo, WakeError> {
    parse_server_info(&send(build_server_info(&gateway_url)?).await?)
}

/// Registers the device, or updates its registration.
pub async fn wake_register(
    gateway_url: String,
    request: WakeRegistrationRequest,
    proofs: Vec<WakeIdentityProof>,
) -> Result<WakeRegistration, WakeError> {
    parse_register(&send(build_register(&gateway_url, &request, &proofs)?).await?)
}

/// Acknowledges a wake by the id from its envelope.
pub async fn wake_ack(
    gateway_url: String,
    device_secret: String,
    wake_id: String,
    outcome: WakeAckOutcome,
) -> Result<(), WakeError> {
    let request = build_ack(&gateway_url, &device_secret, &wake_id, outcome)?;
    parse_no_content(&send(request).await?)
}

/// Marks the app as in use for `ttl_secs` and returns when that expires.
pub async fn wake_set_presence(
    gateway_url: String,
    device_secret: String,
    ttl_secs: u32,
) -> Result<u64, WakeError> {
    let request = build_set_presence(&gateway_url, &device_secret, ttl_secs)?;
    parse_set_presence(&send(request).await?)
}

/// Ends presence early.
pub async fn wake_clear_presence(
    gateway_url: String,
    device_secret: String,
) -> Result<(), WakeError> {
    parse_no_content(&send(build_clear_presence(&gateway_url, &device_secret)?).await?)
}

/// Replaces the device's topics and returns the stored ones.
pub async fn wake_set_topics(
    gateway_url: String,
    device_secret: String,
    topics: Vec<String>,
) -> Result<Vec<String>, WakeError> {
    let request = build_set_topics(&gateway_url, &device_secret, &topics)?;
    parse_set_topics(&send(request).await?)
}

/// Unregisters the device and revokes its secret.
pub async fn wake_unregister(gateway_url: String, device_secret: String) -> Result<(), WakeError> {
    parse_no_content(&send(build_unregister(&gateway_url, &device_secret)?).await?)
}

/// Lists the topics devices can subscribe to.
pub async fn wake_list_topics(gateway_url: String) -> Result<Vec<WakeTopic>, WakeError> {
    parse_list_topics(&send(build_list_topics(&gateway_url)?).await?)
}
