//! The device side of the gateway's v1 HTTP API.
//!
//! Every call is a pure `build_*` function that turns the arguments into an
//! [`HttpRequest`], a thin executor that sends it, and a pure `parse_*`
//! function that maps the [`HttpResponse`] to a result, so everything but
//! the network is tested without a gateway.

use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use super::crypto::parse_wake_id;
use super::errors::{json_error, WakeError};
use super::registration::{
    grant_token_id, identity_scheme, is_valid_device_secret, is_valid_topic_name,
    RegistrationFields,
};
use super::types::{
    WakeAckOutcome, WakeDevice, WakeEnvironment, WakeGrant, WakeGrantSummary, WakeIdentityProof,
    WakePlatform, WakeRegistration, WakeRegistrationRequest, WakeServerInfo, WakeSubscription,
    WakeTopic,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// The most topics one grant can list.
const MAX_GRANT_TOPICS: usize = 16;

/// The credential a route takes.
enum Bearer<'a> {
    /// `wkd_...`: the device routes, grants among them.
    Device(&'a str),
    /// `wkg_...`: `POST /v1/peer/wakes`.
    Grant(&'a str),
}

/// A request to the gateway. `bearer` is a device secret or a grant token,
/// so this type has no `Debug`.
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
    /// `Retry-After` in whole seconds; the HTTP-date form is ignored.
    pub(crate) retry_after_secs: Option<u64>,
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

#[derive(Serialize)]
struct CreateGrantBody<'a> {
    recipient: &'a str,
    topics: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    per_hour: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    per_day: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in_days: Option<u32>,
}

#[derive(Serialize)]
struct PeerWakeBody<'a> {
    topic: &'a str,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    message: String,
    #[serde(default)]
    server_time: Option<u64>,
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
    unknown_topics: Vec<String>,
}

#[derive(Deserialize)]
struct DeviceResponse {
    device_id: String,
    app: String,
    platform: String,
    environment: String,
    identities: Vec<String>,
    topics: Vec<String>,
    presence_expires_at: Option<u64>,
}

#[derive(Deserialize)]
struct TestResponse {
    id: String,
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

#[derive(Deserialize)]
struct CreateGrantResponse {
    id: String,
    token: String,
    recipient: String,
    topics: Vec<String>,
    per_hour: u32,
    per_day: u32,
    created_at: u64,
    expires_at: u64,
}

#[derive(Deserialize)]
struct ListGrantsResponse {
    grants: Vec<GrantSummaryWire>,
}

#[derive(Deserialize)]
struct GrantSummaryWire {
    id: String,
    recipient: String,
    topics: Vec<String>,
    per_hour: u32,
    per_day: u32,
    created_at: u64,
    expires_at: u64,
    revoked_at: Option<u64>,
    last_used_at: Option<u64>,
    sent_1h: u32,
    sent_24h: u32,
}

#[derive(Deserialize)]
struct PeerWakeResponse {
    status: String,
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
    bearer: Option<Bearer<'_>>,
    body: Option<String>,
) -> Result<HttpRequest, WakeError> {
    let url = base_url(gateway_url)?
        .join(route)
        .map_err(|_| WakeError::invalid_input("gateway url cannot be joined"))?;
    let bearer = match bearer {
        Some(Bearer::Device(token)) if !is_valid_device_secret(token) => {
            return Err(WakeError::invalid_input("malformed device secret"))
        }
        Some(Bearer::Grant(token)) if grant_token_id(token).is_none() => {
            return Err(WakeError::invalid_input("malformed grant token"))
        }
        Some(Bearer::Device(token) | Bearer::Grant(token)) => Some(token.to_string()),
        None => None,
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
        Some(Bearer::Device(device_secret)),
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
        Some(Bearer::Device(device_secret)),
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
        Some(Bearer::Device(device_secret)),
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
        Some(Bearer::Device(device_secret)),
        Some(to_json(&TopicsBody { topics })),
    )
}

/// `GET /v1/devices/self`
pub(crate) fn build_get_device(
    gateway_url: &str,
    device_secret: &str,
) -> Result<HttpRequest, WakeError> {
    request(
        Method::GET,
        gateway_url,
        "v1/devices/self",
        Some(Bearer::Device(device_secret)),
        None,
    )
}

/// `POST /v1/devices/self/test`, with the empty object it takes.
pub(crate) fn build_send_test(
    gateway_url: &str,
    device_secret: &str,
) -> Result<HttpRequest, WakeError> {
    request(
        Method::POST,
        gateway_url,
        "v1/devices/self/test",
        Some(Bearer::Device(device_secret)),
        Some("{}".to_string()),
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
        Some(Bearer::Device(device_secret)),
        None,
    )
}

/// `POST /v1/grants`. Checks what the gateway checks without its config: a
/// canonical recipient, 1 to 16 topic names (patterns are refused), and no
/// zero quota or lifetime. Whether each topic is a peer topic, whether the
/// recipient is bound to this device, and `per_day` being at least
/// `per_hour` are left to the gateway: it applies its defaults and caps
/// first, so only it can tell.
pub(crate) fn build_create_grant(
    gateway_url: &str,
    device_secret: &str,
    recipient: &str,
    topics: &[String],
    per_hour: Option<u32>,
    per_day: Option<u32>,
    expires_in_days: Option<u32>,
) -> Result<HttpRequest, WakeError> {
    identity_scheme(recipient).map_err(|_| {
        WakeError::invalid_input("recipient is not a canonical ln: or pk: identity")
    })?;
    if !(1..=MAX_GRANT_TOPICS).contains(&topics.len()) {
        return Err(WakeError::invalid_input("a grant lists 1 to 16 topics"));
    }
    if !topics.iter().all(|topic| is_valid_topic_name(topic)) {
        return Err(WakeError::invalid_input(
            "every grant topic must be a topic name, not a pattern",
        ));
    }
    for (value, reason) in [
        (per_hour, "per_hour must be at least 1"),
        (per_day, "per_day must be at least 1"),
        (expires_in_days, "expires_in_days must be at least 1"),
    ] {
        if value == Some(0) {
            return Err(WakeError::invalid_input(reason));
        }
    }
    let body = CreateGrantBody {
        recipient,
        topics,
        per_hour,
        per_day,
        expires_in_days,
    };
    request(
        Method::POST,
        gateway_url,
        "v1/grants",
        Some(Bearer::Device(device_secret)),
        Some(to_json(&body)),
    )
}

/// `GET /v1/grants`
pub(crate) fn build_list_grants(
    gateway_url: &str,
    device_secret: &str,
) -> Result<HttpRequest, WakeError> {
    request(
        Method::GET,
        gateway_url,
        "v1/grants",
        Some(Bearer::Device(device_secret)),
        None,
    )
}

/// `DELETE /v1/grants/{id}`, with the id in any uuid spelling.
pub(crate) fn build_revoke_grant(
    gateway_url: &str,
    device_secret: &str,
    grant_id: &str,
) -> Result<HttpRequest, WakeError> {
    let id = uuid::Uuid::parse_str(grant_id)
        .map_err(|_| WakeError::invalid_input("grant id is not a uuid"))?;
    request(
        Method::DELETE,
        gateway_url,
        &format!("v1/grants/{id}"),
        Some(Bearer::Device(device_secret)),
        None,
    )
}

/// `POST /v1/peer/wakes`, authenticated by the grant token a peer handed us.
pub(crate) fn build_send_peer_wake(
    gateway_url: &str,
    grant_token: &str,
    topic: &str,
) -> Result<HttpRequest, WakeError> {
    if !is_valid_topic_name(topic) {
        return Err(WakeError::invalid_input(
            "the topic must be a topic name, not a pattern",
        ));
    }
    request(
        Method::POST,
        gateway_url,
        "v1/peer/wakes",
        Some(Bearer::Grant(grant_token)),
        Some(to_json(&PeerWakeBody { topic })),
    )
}

// ----- Parsers -----

/// The error of a non-2xx response: the gateway's
/// `{"error":"<code>","message":"..."}`, or code `unknown` for any other
/// body (a proxy error page, say).
fn rejection(response: &HttpResponse) -> WakeError {
    let retry_after_secs = response.retry_after_secs;
    match serde_json::from_str::<ErrorBody>(&response.body) {
        Ok(body) => WakeError::GatewayRejected {
            status: response.status,
            code: body.error,
            detail: body.message,
            server_time: body.server_time,
            retry_after_secs,
        },
        Err(_) => WakeError::GatewayRejected {
            status: response.status,
            code: "unknown".to_string(),
            detail: "the response carries no wake error body".to_string(),
            server_time: None,
            retry_after_secs,
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

/// The stored topics, and the exact names the gateway dropped as unknown.
pub(crate) fn parse_set_topics(response: &HttpResponse) -> Result<WakeSubscription, WakeError> {
    let topics: SetTopicsResponse = success_json(response)?;
    Ok(WakeSubscription {
        topics: topics.topics,
        unknown_topics: topics.unknown_topics,
    })
}

pub(crate) fn parse_get_device(response: &HttpResponse) -> Result<WakeDevice, WakeError> {
    let device: DeviceResponse = success_json(response)?;
    let platform = match device.platform.as_str() {
        "apns" => WakePlatform::Apns,
        "fcm" => WakePlatform::Fcm,
        _ => return Err(WakeError::invalid_response("unknown platform")),
    };
    let environment = match device.environment.as_str() {
        "production" => WakeEnvironment::Production,
        "sandbox" => WakeEnvironment::Sandbox,
        _ => return Err(WakeError::invalid_response("unknown environment")),
    };
    Ok(WakeDevice {
        device_id: device.device_id,
        app: device.app,
        platform,
        environment,
        identities: device.identities,
        topics: device.topics,
        presence_expires_at: device.presence_expires_at,
    })
}

/// The id of the queued test wake, to match against the push and ack.
pub(crate) fn parse_send_test(response: &HttpResponse) -> Result<String, WakeError> {
    let queued: TestResponse = success_json(response)?;
    parse_wake_id(&queued.id).map_err(|_| WakeError::invalid_response("the wake id is not a uuid"))
}

/// The new grant. Its token must be well formed and name the grant's own
/// id, since it is about to be handed to a peer.
pub(crate) fn parse_create_grant(response: &HttpResponse) -> Result<WakeGrant, WakeError> {
    let grant: CreateGrantResponse = success_json(response)?;
    let id = uuid::Uuid::parse_str(&grant.id)
        .map_err(|_| WakeError::invalid_response("the grant id is not a uuid"))?
        .to_string();
    if grant_token_id(&grant.token).as_deref() != Some(id.as_str()) {
        return Err(WakeError::invalid_response(
            "the grant token is malformed or names another grant",
        ));
    }
    Ok(WakeGrant {
        id,
        token: grant.token,
        recipient: grant.recipient,
        topics: grant.topics,
        per_hour: grant.per_hour,
        per_day: grant.per_day,
        created_at: grant.created_at,
        expires_at: grant.expires_at,
    })
}

pub(crate) fn parse_list_grants(
    response: &HttpResponse,
) -> Result<Vec<WakeGrantSummary>, WakeError> {
    let listed: ListGrantsResponse = success_json(response)?;
    Ok(listed
        .grants
        .into_iter()
        .map(|grant| WakeGrantSummary {
            id: grant.id,
            recipient: grant.recipient,
            topics: grant.topics,
            per_hour: grant.per_hour,
            per_day: grant.per_day,
            created_at: grant.created_at,
            expires_at: grant.expires_at,
            revoked_at: grant.revoked_at,
            last_used_at: grant.last_used_at,
            sent_1h: grant.sent_1h,
            sent_24h: grant.sent_24h,
        })
        .collect())
}

/// `{"status":"accepted"}`, the one success answer of a peer wake.
pub(crate) fn parse_send_peer_wake(response: &HttpResponse) -> Result<(), WakeError> {
    let answer: PeerWakeResponse = success_json(response)?;
    if answer.status == "accepted" {
        Ok(())
    } else {
        Err(WakeError::invalid_response(
            "the peer wake answer is not accepted",
        ))
    }
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
    let retry_after_secs = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok());
    let body = response.text().await.map_err(failed)?;
    Ok(HttpResponse {
        status,
        body,
        retry_after_secs,
    })
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

/// Replaces the device's topics. Returns the stored ones and the exact names
/// the gateway does not know.
pub async fn wake_set_topics(
    gateway_url: String,
    device_secret: String,
    topics: Vec<String>,
) -> Result<WakeSubscription, WakeError> {
    let request = build_set_topics(&gateway_url, &device_secret, &topics)?;
    parse_set_topics(&send(request).await?)
}

/// Reads the device as the gateway stores it.
pub async fn wake_get_device(
    gateway_url: String,
    device_secret: String,
) -> Result<WakeDevice, WakeError> {
    parse_get_device(&send(build_get_device(&gateway_url, &device_secret)?).await?)
}

/// Sends the gateway's built-in `wake.test` to this device alone and returns
/// the queued wake's id. The gateway limits how often a device may do this.
pub async fn wake_send_test(
    gateway_url: String,
    device_secret: String,
) -> Result<String, WakeError> {
    parse_send_test(&send(build_send_test(&gateway_url, &device_secret)?).await?)
}

/// Unregisters the device and revokes its secret.
pub async fn wake_unregister(gateway_url: String, device_secret: String) -> Result<(), WakeError> {
    parse_no_content(&send(build_unregister(&gateway_url, &device_secret)?).await?)
}

/// Lists the topics devices can subscribe to.
pub async fn wake_list_topics(gateway_url: String) -> Result<Vec<WakeTopic>, WakeError> {
    parse_list_topics(&send(build_list_topics(&gateway_url)?).await?)
}

/// Lets peers wake `recipient`, an identity of this device, on `topics`.
/// Returns the grant with its token, which is shown only once.
pub async fn wake_create_grant(
    gateway_url: String,
    device_secret: String,
    recipient: String,
    topics: Vec<String>,
    per_hour: Option<u32>,
    per_day: Option<u32>,
    expires_in_days: Option<u32>,
) -> Result<WakeGrant, WakeError> {
    let request = build_create_grant(
        &gateway_url,
        &device_secret,
        &recipient,
        &topics,
        per_hour,
        per_day,
        expires_in_days,
    )?;
    parse_create_grant(&send(request).await?)
}

/// Lists the grants of this device's identities, without their tokens.
pub async fn wake_list_grants(
    gateway_url: String,
    device_secret: String,
) -> Result<Vec<WakeGrantSummary>, WakeError> {
    parse_list_grants(&send(build_list_grants(&gateway_url, &device_secret)?).await?)
}

/// Revokes a grant. Idempotent.
pub async fn wake_revoke_grant(
    gateway_url: String,
    device_secret: String,
    grant_id: String,
) -> Result<(), WakeError> {
    let request = build_revoke_grant(&gateway_url, &device_secret, &grant_id)?;
    parse_no_content(&send(request).await?)
}

/// Wakes the recipient of a grant a peer handed us, on a topic it covers.
pub async fn wake_send_peer_wake(
    gateway_url: String,
    grant_token: String,
    topic: String,
) -> Result<(), WakeError> {
    let request = build_send_peer_wake(&gateway_url, &grant_token, &topic)?;
    parse_send_peer_wake(&send(request).await?)
}
