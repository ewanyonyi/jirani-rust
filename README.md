# jirani-rust

Optional Rust/Rocket gateway for Jirani minimized report sync.

The Android app stays offline-first. This server is only a trusted gateway for communities or OSF-style partners that want anonymized report aggregation, backup of minimized envelopes, and download by other Jirani Android apps.

Companion Android repo:

```text
/home/ewanyonyi/dev/jirani
```

## Development Setup

Install:

- Rust 2021 toolchain.
- Cargo.
- Docker with Compose support, if you want local PostgreSQL.

Check the project:

```bash
cargo fmt -- --check
cargo test
```

### Local In-Memory Mode

This is the fastest development mode. Data is lost when the process exits.

```bash
cargo run
```

By default Rocket listens on `0.0.0.0:8080`, which matches the Android emulator URL:

```text
http://10.0.2.2:8080
```

Local development is open by default. To test the same token-protected behavior
used by staging or production, run the gateway with `JIRANI_GATEWAY_TOKEN`:

```bash
JIRANI_GATEWAY_TOKEN=change-this-demo-token \
  cargo run
```

Then build Android with the matching client-side token name:

```bash
cd /home/ewanyonyi/dev/jirani
./gradlew assembleDebug \
  -PJIRANI_REMOTE_GATEWAY_URL=http://10.0.2.2:8080 \
  -PJIRANI_REMOTE_GATEWAY_TOKEN=change-this-demo-token
```

The names are intentionally different: Rust reads `JIRANI_GATEWAY_TOKEN`;
Android is built with `JIRANI_REMOTE_GATEWAY_TOKEN`. The values must match.

### Local JSON-File Mode

Use JSON files when you want durable demo storage without running PostgreSQL.

```bash
JIRANI_STORE_PATH=./data/envelopes.json cargo run
```

Relay bundle storage can be enabled separately:

```bash
JIRANI_STORE_PATH=./data/envelopes.json \
JIRANI_RELAY_STORE_PATH=./data/relay-bundles.json \
cargo run
```

### Local PostgreSQL Mode

Start PostgreSQL 16:

```bash
docker compose up -d postgres
```

Then run the gateway with:

```bash
JIRANI_DATABASE_URL=postgres://jirani:jirani_dev_password@localhost:5432/jirani_gateway \
cargo run
```

When `JIRANI_DATABASE_URL` is set, the gateway stores sync envelopes and relay
bundles in PostgreSQL. It creates the demo tables automatically at startup.

## Production Setup On Ubuntu 24.04

This gateway is still a prototype, but a hosted test deployment should run with
PostgreSQL, HTTPS, token auth, and a reverse proxy. Do not expose an in-memory or
JSON-file demo process as community infrastructure.

The examples below assume:

- Ubuntu 24.04 LTS.
- Domain: `gateway.example.org`.
- App user: `jirani`.
- App directory: `/opt/jirani-rust`.
- Rocket listens only on local port `8080`.

Replace the domain, repository URL, database password, and token before running
these commands.

### 1. Install System Packages

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  ca-certificates \
  certbot \
  curl \
  git \
  libpq-dev \
  nginx \
  pkg-config \
  postgresql \
  postgresql-contrib \
  python3-certbot-nginx \
  ufw
```

Install Rust with `rustup` for the deploy user or build user:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup default stable
rustup show
```

### 2. Create The App User And Clone The Repo

```bash
sudo useradd --system --create-home --shell /usr/sbin/nologin jirani
sudo mkdir -p /opt/jirani-rust
sudo chown "$USER":"$USER" /opt/jirani-rust

git clone https://github.com/YOUR-ORG/jirani-rust.git /opt/jirani-rust
cd /opt/jirani-rust
cargo test
cargo build --release

sudo chown -R jirani:jirani /opt/jirani-rust
```

If you deploy from a private repo, clone with your normal SSH or deploy-key
workflow, then keep the final files owned by `jirani`.

### 3. Create PostgreSQL Storage

```bash
sudo -u postgres psql
```

Inside `psql`:

```sql
CREATE DATABASE jirani_gateway;
CREATE USER jirani_gateway WITH ENCRYPTED PASSWORD 'replace-with-a-long-db-password';
GRANT ALL PRIVILEGES ON DATABASE jirani_gateway TO jirani_gateway;
\c jirani_gateway
GRANT ALL ON SCHEMA public TO jirani_gateway;
\q
```

The gateway creates its demo tables automatically at startup when
`JIRANI_DATABASE_URL` is set.

### 4. Configure Environment Variables

Create an environment file readable by the service user only:

```bash
sudo install -o jirani -g jirani -m 0750 -d /etc/jirani-rust
sudo nano /etc/jirani-rust/gateway.env
```

Example `/etc/jirani-rust/gateway.env`:

```bash
JIRANI_DATABASE_URL=postgres://jirani_gateway:replace-with-a-long-db-password@localhost:5432/jirani_gateway
JIRANI_GATEWAY_TOKEN=replace-with-a-long-random-token
JIRANI_RELAY_PUBLIC_KEY=base64-der-rsa-public-key
ROCKET_ADDRESS=127.0.0.1
ROCKET_PORT=8080
```

`JIRANI_GATEWAY_TOKEN` is a runtime server secret. It is not baked in by
`cargo build --release`; systemd loads it from this environment file each time
the service starts. Generate a strong token with:

```bash
openssl rand -base64 48
```

`JIRANI_RELAY_PUBLIC_KEY` must be a valid RSA public key, either PEM text or
base64 DER. Do not set it to a random shared secret. Generate a relay key pair
with:

```bash
sudo openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out /etc/jirani-rust/relay-private.pem

sudo openssl rsa -in /etc/jirani-rust/relay-private.pem \
  -pubout -outform DER | base64 -w0
```

Paste the printed base64 public key into `JIRANI_RELAY_PUBLIC_KEY`. Keep
`relay-private.pem` private and readable only by trusted server operators.
Android uses `/relay/public-key` to encrypt relay payloads for the configured
gateway key when this value is valid.

Lock down the environment file:

```bash
sudo chown jirani:jirani /etc/jirani-rust/gateway.env
sudo chmod 0640 /etc/jirani-rust/gateway.env
```

### 5. Create A systemd Service

```bash
sudo nano /etc/systemd/system/jirani-rust.service
```

Service file:

```ini
[Unit]
Description=Jirani Rust Gateway
After=network-online.target postgresql.service
Wants=network-online.target

[Service]
User=jirani
Group=jirani
WorkingDirectory=/opt/jirani-rust
EnvironmentFile=/etc/jirani-rust/gateway.env
ExecStart=/opt/jirani-rust/target/release/jirani-rust
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
```

Start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now jirani-rust
sudo systemctl status jirani-rust
curl http://127.0.0.1:8080/health
```

Useful logs:

```bash
sudo journalctl -u jirani-rust -f
```

### 6. Configure Nginx Reverse Proxy

Create a site:

```bash
sudo nano /etc/nginx/sites-available/jirani-rust
```

Nginx config before TLS:

```nginx
server {
    listen 80;
    listen [::]:80;
    server_name gateway.example.org;

    access_log off;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For "";
        proxy_set_header X-Real-IP "";
        proxy_set_header User-Agent "";
        proxy_read_timeout 30s;
    }
}
```

Enable it:

```bash
sudo ln -s /etc/nginx/sites-available/jirani-rust /etc/nginx/sites-enabled/jirani-rust
sudo nginx -t
sudo systemctl reload nginx
```

If the default site is still enabled and conflicts with the domain, remove that
symlink:

```bash
sudo rm /etc/nginx/sites-enabled/default
sudo nginx -t
sudo systemctl reload nginx
```

### 7. Enable Firewall And HTTPS With Let's Encrypt

Allow SSH and web traffic:

```bash
sudo ufw allow OpenSSH
sudo ufw allow 'Nginx Full'
sudo ufw enable
sudo ufw status
```

Issue and install a Let's Encrypt certificate:

```bash
sudo certbot --nginx -d gateway.example.org
```

Certbot should update the Nginx site with TLS settings and automatic HTTP to
HTTPS redirect. Confirm renewal:

```bash
sudo certbot renew --dry-run
```

### 8. Smoke Test The Hosted Gateway

Public health check:

```bash
curl https://gateway.example.org/health
```

Protected privacy posture check:

```bash
curl -H "Authorization: Bearer replace-with-a-long-random-token" \
  https://gateway.example.org/privacy
```

Relay public-key check:

```bash
curl -H "Authorization: Bearer replace-with-a-long-random-token" \
  https://gateway.example.org/relay/public-key
```

Dashboard pages can be opened for browser testing with:

```text
https://gateway.example.org/?token=replace-with-a-long-random-token
https://gateway.example.org/reports?token=replace-with-a-long-random-token
https://gateway.example.org/analysis?token=replace-with-a-long-random-token
```

### 9. Build Android For The Hosted Gateway

After the Rust service is running with `JIRANI_GATEWAY_TOKEN`, build Android
with the same token under Android's Gradle property name:

```bash
cd /home/ewanyonyi/dev/jirani
./gradlew assembleDebug \
  -PJIRANI_REMOTE_GATEWAY_URL=https://gateway.example.org \
  -PJIRANI_REMOTE_GATEWAY_TOKEN=replace-with-a-long-random-token
```

For the staging host used by this project, replace the URL with:

```text
https://snf-6731.vlab.ac.ke
```

If you rotate `JIRANI_GATEWAY_TOKEN` on the server, rebuild and redeploy Android
with the new matching `JIRANI_REMOTE_GATEWAY_TOKEN`.

### Production Safety Checklist

Before exposing the gateway:

- point Android builds at `https://gateway.example.org`, not plain HTTP;
- set `JIRANI_GATEWAY_TOKEN` and keep it out of source control;
- build Android with the same value as `JIRANI_REMOTE_GATEWAY_TOKEN`;
- set `JIRANI_RELAY_PUBLIC_KEY` to a valid RSA public key, not a random secret;
- use a managed, backed-up, or monitored PostgreSQL database;
- disable or anonymize reverse-proxy access logs;
- rotate demo/test tokens after presentations;
- keep `/health` public, but protect sync, relay, analytics, and dashboard routes;
- avoid storing raw safety reports, reporter identity, device IDs, exact GPS, or
  exact-home details;
- confirm reverse-proxy logs do not preserve source IPs if gateway-operator IP
  anonymity matters;
- prefer a trusted relay/proxy if source-IP anonymity from the gateway operator
  is required.

For real community deployment, add community-controlled authentication,
retention/deletion jobs, encrypted storage review, structured audit logs without
reporter identity, and a local safety expert review of PII detection before
relying on this as production infrastructure.

## Endpoints

- `GET /health`
- `GET /privacy`
- `GET /privacy-page`
- `GET /`
- `GET /reports`
- `GET /analysis`
- `POST /sync/envelopes`
- `GET /sync/envelopes`
- `POST /relay/bundles`
- `GET /relay/bundles`
- `GET /relay/public-key`
- `GET /analytics/anonymous-summary`

## Dashboard And Auth

For local demos, the gateway is open by default. For a hosted test server, set a shared token at runtime:

```bash
JIRANI_GATEWAY_TOKEN=change-this-demo-token \
JIRANI_DATABASE_URL=postgres://USER:PASSWORD@HOST:5432/jirani_gateway \
cargo run
```

For release builds, `cargo build --release` only compiles the binary. Set
`JIRANI_GATEWAY_TOKEN` in the shell, process manager, container secret, or
systemd `EnvironmentFile` that starts `./target/release/jirani-rust`.

When `JIRANI_GATEWAY_TOKEN` is set:

- API clients must send `Authorization: Bearer change-this-demo-token`.
- Browser dashboard pages can be opened with `?token=change-this-demo-token`.
- `GET /health` remains public for simple uptime checks.

Dashboard pages:

- `/`: overview cards and recent reports.
- `/reports`: accepted minimized envelope list.
- `/analysis`: anonymous aggregate counts by sensitivity, verification status, and coarse area.
- `/privacy-page`: plain-language privacy posture.

## Anonymity And Reliability

A direct HTTPS request always exposes the connecting IP address at the network layer. This gateway cannot cryptographically hide that from the network path by itself. For stronger IP anonymity from the gateway operator, place a trusted relay/proxy in front of the server or route traffic through infrastructure that strips/anonymizes source logs.

What this gateway does by default:

- does not store IP addresses, User-Agent values, device IDs, precise locations, or reporter identities in application storage;
- persists only minimized accepted envelopes when `JIRANI_STORE_PATH` is set;
- persists accepted relay bundles when `JIRANI_RELAY_STORE_PATH` is set;
- uses PostgreSQL for accepted envelopes and relay bundles when `JIRANI_DATABASE_URL` is set;
- verifies `contentHash` before storage;
- deduplicates without overwriting an existing envelope;
- rejects survivor-centered, expired, PII-looking, or hash-mismatched uploads.

For hosted testing:

- use HTTPS;
- set `JIRANI_GATEWAY_TOKEN`;
- set `JIRANI_DATABASE_URL`;
- disable or anonymize reverse-proxy access logs;
- rotate the test token after demos.

## Android Communication

Android defaults to the emulator URL `http://10.0.2.2:8080`. For a hosted test server, build the Android app with:

```bash
cd /home/ewanyonyi/dev/jirani
./gradlew assembleDebug \
  -PJIRANI_REMOTE_GATEWAY_URL=https://your-test-gateway.example \
  -PJIRANI_REMOTE_GATEWAY_TOKEN=change-this-demo-token
```

See `docs/ANDROID_INTEGRATION.md` for the full API contract shared by both repos.

## Privacy Rules

- Accepts minimized sync envelopes only.
- Verifies `contentHash` against the sanitized payload before storing.
- Rejects survivor-centered GBV/domestic reports in the default gateway flow.
- Rejects obvious phone-number-like values and exact-home hints.
- Deduplicates by `envelopeId`; duplicate uploads return `409 Conflict`, which Android treats as already uploaded.

## Relay Bundles

The relay API is separate from minimized sync envelopes. It is intended for
Android's offline mesh relay flow:

- `POST /relay/bundles`: accept a privacy-safe relay bundle.
- `GET /relay/bundles`: return accepted relay bundles.
- `GET /relay/public-key`: return `JIRANI_RELAY_PUBLIC_KEY` when configured, or
  `404 Not Found` when no relay public key is configured.

Relay bundles contain a minimized public header plus an opaque encrypted payload.
The default gateway validates hashes, expiry, survivor-safety rules, and obvious
PII in the public header. It does not decrypt the private payload.

This scaffold uses in-memory storage unless `JIRANI_DATABASE_URL` is set for
PostgreSQL, or `JIRANI_STORE_PATH` and/or `JIRANI_RELAY_STORE_PATH` are set for
JSON-file demo storage.
