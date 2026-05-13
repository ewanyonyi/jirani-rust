# Jirani Communication & Anonymous Relay Specification

This document describes a possible offline-first communication layer for Jirani.
It separates what belongs in the Android app from what belongs in this optional
Rust/Rocket gateway.

The goal is to support community safety alerts and delayed synchronization while
preserving Jirani's core privacy posture: the gateway must not require or store
reporter names, phone numbers, device IDs, exact GPS coordinates, exact homes, or
raw survivor-centered reports.

## 1. Architecture: Aware Relay DTN

Jirani can use an "Aware Relay" delay-tolerant networking model. Android devices
may carry relay bundles between neighbors when internet access is unavailable.
Each relay bundle has two parts:

1. **Public header:** A minimized, cleartext safety summary that lets the carrier
   understand immediate local risk. This can include coarse area, alert type,
   risk level, time bucket, and verification status.
2. **Private encrypted payload:** An opaque encrypted blob that relay devices and
   the gateway do not inspect by default. This should not be treated as a license
   to collect PII. Any future decryption flow needs a separate threat model,
   retention policy, and community-controlled authorization.

The public header and encrypted payload are hashed together. If either part is
modified, the bundle hash changes and the receiving app or gateway rejects it.

## 2. Android Responsibilities

The Android app owns the offline mesh, carrier experience, and local relay
decision-making.

| Responsibility | Android implementation |
| :--- | :--- |
| Mesh transport | Google Nearby Connections API, using a peer-to-peer cluster strategy. |
| Local state | Kotlin Coroutines and Flow for reactive relay state. |
| Data persistence | Room database for bundles waiting for local relay or remote sync. |
| UI rendering | Jetpack Compose, observing Room/Flow state. |
| Background sync | Foreground service when the user opts into active relay mode. |

Android should implement:

- generation of minimized public headers from local reports;
- encryption of any private payload before storage or relay;
- local validation of bundle hashes before import;
- Nearby handoff with `ConnectionsClient.sendPayload()`;
- local deduplication by `bundleId` or `bundleHash`;
- multi-peer verification, where a "High Alert" UI state requires the same
  bundle hash from at least two different neighbor sessions;
- rate limiting for gossip events to reduce battery drain and spam;
- a user-visible relay toggle, enabled by default only when appropriate for the
  community demo and platform permissions.

### Android UI Concepts

The Android app may expose a "Relay Shield" area in the Compose dashboard:

- current mesh status, such as "Mesh active";
- active public-header alerts, such as "Neighbor alert near shared grazing area";
- count of anonymous packets carried;
- a share/relay toggle;
- a subtle mesh range or sync activity indicator.

The UI should avoid implying that unverified reports are confirmed incidents.
Alert copy should use cautious language such as "reported", "pending review",
or "needs local verification".

## 3. Rust/Rocket Gateway Responsibilities

This repository owns only the optional internet gateway. It should stay small,
auditable, and compatible with the existing minimized sync API.

The current gateway already supports:

- `POST /sync/envelopes` for minimized report sync envelopes;
- `GET /sync/envelopes` for trusted Android downloads;
- `GET /analytics/anonymous-summary` for aggregate non-PII counts;
- token auth for hosted demos;
- content-hash validation, deduplication, expiry checks, and basic PII rejection.

Storage is selected by environment:

- default local runs use in-memory storage;
- `JIRANI_STORE_PATH` and `JIRANI_RELAY_STORE_PATH` enable JSON-file demo
  persistence;
- `JIRANI_DATABASE_URL` enables PostgreSQL storage for accepted sync envelopes
  and relay bundles.

Relay support can be integrated as a separate API surface instead of changing
the existing sync envelope contract:

- `POST /relay/bundles`: accept privacy-safe relay bundles from trusted Android
  clients.
- `GET /relay/bundles`: return accepted relay bundles for trusted Android
  clients.
- `GET /relay/public-key`: optional endpoint for Android to fetch the gateway's
  configured encryption public key.

The Rust gateway should validate and store only:

- a bundle ID;
- a minimized public header;
- an encrypted payload as an opaque string or byte encoding;
- payload and bundle hashes;
- coarse timestamps or buckets;
- expiry metadata.

The gateway should not decrypt private payloads in the default demo flow. It
should reject relay bundles when:

- the bundle is expired;
- the public header contains obvious PII or exact-home hints;
- the public header is survivor-centered or marked survivor-support-only;
- the encrypted payload is missing when required;
- the payload hash or full bundle hash does not match;
- the bundle was already stored.

## 4. Proposed Relay Bundle Shape

This shape is intentionally separate from `SyncEnvelope` so the existing Android
gateway contract can remain stable.

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

Hashing must use a deterministic representation shared by Android and Rust. If
relay bundles are implemented, update this document, `ANDROID_INTEGRATION.md`,
the Android repo, and Rust integration tests together.

The Rust implementation computes `payloadHash` as the SHA-256 hex digest of the
`encryptedPayload` string bytes. It computes `bundleHash` as the SHA-256 hex
digest of the public-header fields joined in this order, followed by
`payloadHash`:

```text
alertType|generalArea|timeWindow|riskLevel|message|verificationStatus|audienceTier|sensitivity|payloadHash
```

## 5. Security And Trust Guardrails

- A relay carrier may know the coarse public safety warning, but never who
  reported it.
- The public header must remain minimized and should not include names, phone
  numbers, exact homes, exact GPS coordinates, or household identifiers.
- The encrypted payload is opaque to this gateway by default.
- Direct HTTPS upload still exposes source IP at the network layer. Hosted
  deployments should use HTTPS, token auth, durable storage, and anonymized
  reverse-proxy logs.
- Multi-peer verification is an Android-side UI trust signal, not proof that an
  incident occurred.
- Survivor-centered GBV/domestic reports must remain outside broad relay,
  analytics, and default gateway sync.

Every neighbor can be a relay, but every relay must remain privacy-preserving.
