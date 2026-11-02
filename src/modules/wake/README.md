# Wake Module

Device side of the wake push gateway: generating the device's keys and
credentials, preparing and sending a signed registration, decrypting the
wakes that arrive in pushes, the device routes of the gateway's v1 API
(acks, presence, topics, unregister), grants that let a peer wake this
device, and the peer wake a holder of someone else's grant sends.

> The v1 wire format is experimental until the Bitkit apps adopt it. The
> legacy (v0) envelope is the one deployed apps already decrypt, and is
> stable.

The module is a port of the gateway's `wake-proto` crate, which bitkit-core
cannot depend on. It carries no database state: the host keeps the key pair,
the device secret and the install id.

## Available Methods

| Method | What it does |
|---|---|
| `wakeGenerateKeypair` | secp256k1 key pair wakes are encrypted to |
| `wakeGenerateDeviceSecret` | `wkd_` bearer secret and its SHA-256 |
| `wakeGenerateInstallId` | 22-character install id |
| `wakePrepareRegistration` | validates a registration and builds the message every identity signs |
| `wakeSignPubkyProof` | signs that message with the pubky key (only registration messages) |
| `wakeRegister` | `POST /v1/devices` with one proof per identity |
| `wakeDecryptPush` | finds and decrypts the wake in an APNs payload or FCM data map |
| `wakeDecryptV0`, `wakeDecryptV1` | decrypt one envelope directly |
| `wakeAck` | `POST /v1/acks` |
| `wakeSetPresence`, `wakeClearPresence` | `PUT` / `DELETE /v1/presence` |
| `wakeSetTopics` | `PUT /v1/devices/self/topics` |
| `wakeUnregister` | `DELETE /v1/devices/self` |
| `wakeServerInfo` | `GET /v1/info` (the audience to register with) |
| `wakeListTopics` | `GET /v1/topics` |
| `wakeCreateGrant` | `POST /v1/grants`: lets peers wake one of this device's identities |
| `wakeListGrants` | `GET /v1/grants`, never with tokens |
| `wakeRevokeGrant` | `DELETE /v1/grants/{id}` |
| `wakeSendPeerWake` | `POST /v1/peer/wakes` with a grant token a peer handed us |

## Registering

Keep the encryption secret key and the device secret where the notification
extension can read them (the shared keychain on iOS). Register again whenever
the push token, the key or the topics change; a newer registration from the
same install replaces the older one.

```swift
import BitkitCore
import LDKNode

func registerForWakes(node: Node, installId: String, pubkySecretHex: String, pubkyZ32: String,
                      pushToken: String) async throws {
    let gateway = "https://wake.example"
    let info = try await wakeServerInfo(gatewayUrl: gateway)
    let keys = try wakeGenerateKeypair()          // store both, reuse across launches
    let secret = try wakeGenerateDeviceSecret()   // store the token
    let nodeId = node.nodeId()

    let request = try wakePrepareRegistration(
        audience: info.audience,
        app: "bitkit",
        installId: installId,                     // wakeGenerateInstallId() once per install
        platform: .apns,
        environment: .production,
        pushToken: pushToken,                     // lowercase hex on APNs
        encryptionPublicKey: keys.publicKeyHex,
        secretSha256: secret.sha256Hex,
        identities: ["ln:\(nodeId)", "pk:\(pubkyZ32)"],
        topics: ["blocktank.*"],
        timestamp: nil
    )
    let proofs = [
        WakeIdentityProof(identity: "ln:\(nodeId)",
                          signature: node.signMessage(msg: Array(request.message.utf8))),
        WakeIdentityProof(identity: "pk:\(pubkyZ32)",
                          signature: try wakeSignPubkyProof(secretKeyHex: pubkySecretHex, message: request.message)),
    ]
    let registration = try await wakeRegister(gatewayUrl: gateway, request: request, proofs: proofs)
    print("registered \(registration.deviceId), unknown topics: \(registration.unknownTopics)")
}
```

The request must reach the gateway within 600 seconds of `timestamp`.

## Handling a push

```swift
let envelope = try wakeDecryptPush(secretKeyHex: keys.secretKeyHex, pushJson: payloadJson)
if envelope.fallback {
    // A visible fallback alert: nothing to decrypt, the wake was not handled in time.
} else if let id = envelope.id {
    // v1: handle envelope.topic / envelope.payloadJson, then acknowledge.
    try await wakeAck(gatewayUrl: gateway, deviceSecret: secret.token, wakeId: id, outcome: .handled)
} else {
    // v0 (legacy): no id and nothing to acknowledge.
}
```

`pushJson` is the APNs `userInfo` dictionary or the FCM data map, serialized
as JSON. Four shapes are recognised: the legacy APNs `aps.alert.payload`, the
legacy flat FCM data (`cipher`, `iv`, `tag`, `publicKey`), a v1 `wake` object
(APNs) and a v1 `wake` JSON string (FCM). A `wake_fallback` marker returns
`fallback = true` without reading the key.

Acknowledging `.handled` cancels the fallback alert. `.noHandler` and
`.failed` are recorded, and the fallback still fires.

## Grants and peer wakes

A peer can wake this device only with a grant the device issued. Grants cover
peer topics only (`paykit.sync` today; `wakeListTopics` marks them `peer`),
for an identity the device registered. The gateway's anti-spam document
explains the quotas, the per-recipient cap and why a peer wake carries no
content.

```swift
// Recipient: let a linked peer wake this device.
let grant = try await wakeCreateGrant(
    gatewayUrl: gateway,
    deviceSecret: secret.token,
    recipient: "pk:\(pubkyZ32)",
    topics: ["paykit.sync"],
    perHour: nil, perDay: nil, expiresInDays: nil   // the gateway's defaults and caps
)
// Send grant.token to that peer over an authenticated channel; the gateway
// never shows it again. Keep grant.id with the peer, and when the peer is
// blocked or unlinked:
try await wakeRevokeGrant(gatewayUrl: gateway, deviceSecret: secret.token, grantId: grant.id)

// Sender: after writing something for a peer that handed us its token.
do {
    try await wakeSendPeerWake(gatewayUrl: peerGateway, grantToken: peerToken, topic: "paykit.sync")
} catch WakeError.GatewayRejected(_, let code, _) where code == "grant_invalid" {
    // Revoked or expired: drop the token and wait for a new one.
} catch WakeError.GatewayRejected(_, let code, _) where code == "rate_limited" {
    // The grant's quota is used up for now: skip this wake.
}
```

A peer wake reaches the recipient's devices as a background push whose
decrypted `payloadJson` is `nil`. Every request within the grant's quota gets
the same answer, whether a push went out, was coalesced into an open wake, or
was suppressed because the recipient had the app open, so the sender learns
nothing about the recipient. Make the wake best effort: what the sender wrote
must never depend on it.

## Crypto and wire compatibility

- `S33 = compressed(d * P)` from `bitcoin::secp256k1::ecdh::shared_secret_point`,
  i.e. `(0x02 | y & 1) || x`.
- `key = SHA256(SHA256(S33 || label))`, two plain SHA-256 passes (not the
  byte-reversed `sha256d` display form).
- AES-256-GCM, no AAD, 16-byte tag. The IV must be 12 or 16 bytes.
- v0: label `bitkit-notifications`, 16-byte IV, plaintext
  `{"source","type","payload","createdAt"}`; the envelope's topic is
  `source.type`.
- v1: label `wake-v1`, 12-byte IV, container `{"v":1,...}`, plaintext
  `{"v":1,"id","topic","urgency","deadline","payload"}`. Any other `v` is
  `UnsupportedEnvelope`.
- The registration message is the gateway's 12-line preimage: identities and
  topics are deduplicated and sorted bytewise, and a newline in any field, or
  an empty list entry or a comma in one, is rejected.
- `payloadJson` is the producer's payload exactly as encrypted.

Error messages never contain keys, secrets or decrypted plaintext.

## Test vectors

`test_vectors/*.json` are copied byte for byte from `crates/wake-proto/vectors`
of the wake repository at commit
`782f66510c505b0331e6299e96c1233d3c263484`. The tests assert them exactly:
the v0 server and client envelopes with their shared points and keys, the v1
envelope, the registration preimage with both proofs, the legacy signature
(used to check the test signer), and every push shape in `pushes.json`.

To update them, copy the files from a newer wake commit, change the hash
above and run `cargo test wake`.

## Tests

```bash
cargo test wake
# against a running gateway, such as the wake repository's docker stack
WAKE_GATEWAY_URL=http://127.0.0.1:9010 WAKE_FAKE_PUSH_URL=http://127.0.0.1:9100 \
    cargo test wake -- --ignored
```

CI runs `cargo test --lib modules::wake` (`.github/workflows/wake-tests.yml`).
The two ignored tests need a gateway:

- `live_gateway_round_trip` registers with an `ln:` proof (signed by a local
  stand-in for ldk-node `signMessage`) and a `pk:` proof, then sets and clears
  presence, lists and sets topics, and unregisters.
- `live_gateway_grant_round_trip` registers a `pk:` device and creates a grant.
  It checks that a non-peer topic and a foreign identity are refused, sends a
  peer wake with the grant, and checks the grant's counters. It then revokes
  the grant twice and checks that the token stops working. With
  `WAKE_FAKE_PUSH_URL` set, it also decrypts the push the fake APNs recorded
  and acks it.
