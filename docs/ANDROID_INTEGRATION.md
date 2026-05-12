# Android Integration

## Companion Repository

The Android app lives in:

```text
/home/ewanyonyi/dev/jirani
```

The Rust gateway is optional. Jirani Android must continue working offline and through Nearby Connections even when this server is unavailable.

## Local Development

Start the Rust gateway:

```bash
cd /home/ewanyonyi/dev/jirani-rust
cargo run
```

The Android emulator reaches the host machine at:

```text
http://10.0.2.2:8080
```

That is the Android repo's default `JIRANI_REMOTE_GATEWAY_URL`.

## Hosted Test Server

For easier phone or judge testing, host this Rocket service on a test server and build Android with the hosted base URL:

```bash
cd /home/ewanyonyi/dev/jirani
./gradlew assembleDebug \
  -PJIRANI_REMOTE_GATEWAY_URL=https://your-test-gateway.example \
  -PJIRANI_REMOTE_GATEWAY_TOKEN=change-this-demo-token
```

An environment variable also works:

```bash
JIRANI_REMOTE_GATEWAY_URL=https://your-test-gateway.example \
JIRANI_REMOTE_GATEWAY_TOKEN=change-this-demo-token \
./gradlew assembleDebug
```

Use HTTPS for hosted testing. Android rejects non-HTTPS remote gateway URLs by default. Plain HTTP is allowed only for local development hosts such as `10.0.2.2`, `localhost`, and `127.0.0.1`.

## Authentication

Local sync/API demos can run without bearer-token auth. Dashboard pages require
username/password login after `JIRANI_DASHBOARD_USERS` is configured. Hosted
test servers should set:

```bash
JIRANI_GATEWAY_TOKEN=change-this-demo-token \
JIRANI_DASHBOARD_USERS='elder_osf:sha256$120000$...$...' \
JIRANI_SESSION_SECRET=replace-with-a-long-random-session-secret \
JIRANI_DATABASE_URL=postgres://jirani:jirani_dev_password@localhost:5432/jirani_gateway \
cargo run
```

For lightweight demos without PostgreSQL, `JIRANI_STORE_PATH` and
`JIRANI_RELAY_STORE_PATH` can still be used for JSON-file persistence.

Android sends `Authorization: Bearer <token>` when built with `JIRANI_REMOTE_GATEWAY_TOKEN`.

Dashboard pages use username/password login at `/login` when `JIRANI_DASHBOARD_USERS`
is configured. Keep dashboard credentials private, rotate them after demos, and
do not treat this as production-grade identity or authorization.

Create one dashboard password hash per authorized OSF staff member or community
elder:

```bash
cargo run -- hash-dashboard-password 'elder-development-password'
cargo run -- hash-dashboard-password 'osf-development-password'
```

For development, start Rocket with the generated hashes:

```bash
JIRANI_GATEWAY_TOKEN=change-this-demo-token \
JIRANI_DASHBOARD_USERS='elder_osf:sha256$120000$...$...,osf_staff:sha256$120000$...$...' \
JIRANI_SESSION_SECRET=local-dev-session-secret \
cargo run
```

For production, generate long unique passwords, store only the generated hashes
and `JIRANI_SESSION_SECRET` in the server's secret manager or process
environment, and give each reviewer only their own username and temporary
password. Rotate the affected user's hash when access changes.

## Anonymity Limits

Direct Android-to-server HTTPS hides report content from the network path but still exposes the connecting IP address to the server/network layer. Jirani handles this by not storing IP, device, User-Agent, exact location, or reporter identity in gateway application data.

For stronger IP anonymity, deploy a trusted relay/proxy in front of Rocket and disable or anonymize proxy access logs. The relay should forward only the request body and required auth header, not source-identifying headers.

## API Expected By Android

### `POST /sync/envelopes`

Android uploads a minimized envelope shaped like:

```json
{
  "envelopeId": "env-...",
  "recordType": "SafetyReportRecord",
  "recordId": "report-...",
  "contentHash": "sha256...",
  "version": 1,
  "lastModifiedBucket": "day-...",
  "audienceTier": "TrustedVerifier",
  "expiresAtEpochSeconds": 1900000000,
  "payload": {
    "reportType": "livestock or grazing dispute",
    "generalArea": "near river",
    "timeWindow": "morning",
    "submittedAtEpochSeconds": 1800000000,
    "observedRisk": "Cattle crossed the grazing boundary this morning.",
    "verificationStatus": "PendingVerification",
    "sensitivity": "Community"
  }
}
```

Expected responses:

- `201 Created`: new envelope stored.
- `409 Conflict`: duplicate envelope already stored. Android treats this as uploaded.
- `400 Bad Request`: rejected for privacy, expiry, or content-hash mismatch.

### `GET /sync/envelopes`

Android accepts either:

```json
{ "envelopes": [] }
```

or a raw JSON array. The current Rocket route returns `{ "envelopes": [...] }`.

Android verifies each downloaded envelope's `contentHash` before importing it into the receiving-device inbox.

### `GET /analytics/anonymous-summary`

Returns aggregate, non-PII counts for trusted demo dashboards and analysis.

### `GET /privacy`

Returns a machine-readable statement of the gateway privacy posture, including that network identity is not stored by the application.

### `POST /relay/bundles`

Android may upload an offline relay bundle for optional internet backup and
cross-device download:

```json
{
  "bundleId": "bundle-demo-001",
  "publicHeader": {
    "alertType": "ResourceDispute",
    "generalArea": "near river",
    "timeWindow": "morning",
    "riskLevel": "Elevated",
    "message": "Cattle movement reported near shared grazing boundary.",
    "verificationStatus": "PendingVerification",
    "audienceTier": "TrustedVerifier",
    "sensitivity": "Community"
  },
  "encryptedPayload": "base64-encoded-ciphertext",
  "payloadHash": "hex-sha256-of-encrypted-payload",
  "bundleHash": "hex-sha256-of-public-header-and-payload-hash",
  "expiresAtEpochSeconds": 1900000000
}
```

Expected responses:

- `201 Created`: new relay bundle stored.
- `409 Conflict`: duplicate relay bundle already stored.
- `400 Bad Request`: rejected for privacy, expiry, or hash mismatch.

The gateway treats `encryptedPayload` as opaque and does not decrypt it in the
default demo flow.

### `GET /relay/bundles`

Returns:

```json
{ "bundles": [] }
```

### `GET /relay/public-key`

Returns `{ "publicKey": "..." }` when the gateway is configured with
`JIRANI_RELAY_PUBLIC_KEY`. Returns `404 Not Found` when no relay public key is
configured.

## Dashboard Pages

- `GET /`: overview of stored minimized envelopes.
- `GET /reports`: list of accepted envelopes.
- `GET /analysis`: anonymous aggregate counts.
- `GET /privacy-page`: browser-readable privacy posture.

## Shared Safety Rules

The server and Android app must agree on these rules:

- only minimized payloads cross the gateway;
- relay private payloads stay opaque to the gateway by default;
- survivor-centered GBV/domestic reports are not accepted by default gateway sync;
- phone numbers, names, device IDs, exact homes, and GPS coordinates must not be required or stored;
- unverified reports are treated as verification signals, not confirmed incidents;
- `contentHash` is the integrity and deduplication anchor shared across repos.
