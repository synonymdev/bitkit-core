# Wake Module

Device side of the wake push gateway: generating the device's keys and
credentials, preparing and sending a signed registration, decrypting the
wakes that arrive in pushes, and the device routes of the gateway's v1 API
(acks, presence, topics, unregister).

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

Grants and peer sends are not exposed yet.

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
`6037343ad39603ce436e2a551cac9db877ede9c6`. The tests assert them exactly:
the v0 server and client envelopes with their shared points and keys, the v1
envelope, the registration preimage with both proofs, the legacy signature
(used to check the test signer), and every push shape in `pushes.json`.

To update them, copy the files from a newer wake commit, change the hash
above and run `cargo test wake`.

## Tests

```bash
cargo test wake
WAKE_GATEWAY_URL=http://127.0.0.1:9010 cargo test wake -- --ignored   # against a running gateway
```

The ignored test registers with an `ln:` proof (signed by a local stand-in for
ldk-node `signMessage`) and a `pk:` proof, then sets and clears presence,
lists and sets topics, and unregisters.
