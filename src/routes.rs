use crate::auth::{token_link_suffix, GatewayAuth, GatewayConfig};
use crate::models::{
    AnonymousSummary, ApiMessage, EnvelopeList, HealthResponse, PrivacyResponse, RelayBundle,
    RelayBundleList, RelayPublicKeyResponse, SyncEnvelope,
};
use crate::store::{EnvelopeStore, RelayBundleStore, StoreWrite};
use rocket::http::Status;
use rocket::response::content::RawHtml;
use rocket::serde::json::Json;
use rocket::{get, post, routes, Route, State};
use std::time::{SystemTime, UNIX_EPOCH};

#[get("/health")]
fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "jirani-rust-gateway",
        stores_network_identity: false,
    })
}

#[get("/privacy")]
fn privacy() -> Json<PrivacyResponse> {
    Json(privacy_response())
}

#[post("/sync/envelopes", format = "json", data = "<envelope>")]
fn upload_envelope(
    _auth: GatewayAuth,
    store: &State<EnvelopeStore>,
    envelope: Json<SyncEnvelope>,
) -> Result<Status, (Status, Json<ApiMessage>)> {
    let envelope = envelope.into_inner();
    envelope
        .validate_for_gateway(now_epoch_seconds())
        .map_err(|message| (Status::BadRequest, Json(ApiMessage { message })))?;

    match store.upsert(envelope) {
        StoreWrite::Created => Ok(Status::Created),
        StoreWrite::AlreadyStored => Ok(Status::Conflict),
        StoreWrite::PersistFailed(message) => Err((
            Status::InternalServerError,
            Json(ApiMessage {
                message: format!("Envelope accepted but could not be persisted: {message}"),
            }),
        )),
    }
}

#[get("/sync/envelopes")]
fn list_envelopes(_auth: GatewayAuth, store: &State<EnvelopeStore>) -> Json<EnvelopeList> {
    Json(EnvelopeList {
        envelopes: store.list(),
    })
}

#[get("/analytics/anonymous-summary")]
fn anonymous_summary(_auth: GatewayAuth, store: &State<EnvelopeStore>) -> Json<AnonymousSummary> {
    Json(store.summary())
}

#[post("/relay/bundles", format = "json", data = "<bundle>")]
fn upload_relay_bundle(
    _auth: GatewayAuth,
    store: &State<RelayBundleStore>,
    bundle: Json<RelayBundle>,
) -> Result<Status, (Status, Json<ApiMessage>)> {
    let bundle = bundle.into_inner();
    bundle
        .validate_for_gateway(now_epoch_seconds())
        .map_err(|message| (Status::BadRequest, Json(ApiMessage { message })))?;

    match store.upsert(bundle) {
        StoreWrite::Created => Ok(Status::Created),
        StoreWrite::AlreadyStored => Ok(Status::Conflict),
        StoreWrite::PersistFailed(message) => Err((
            Status::InternalServerError,
            Json(ApiMessage {
                message: format!("Relay bundle accepted but could not be persisted: {message}"),
            }),
        )),
    }
}

#[get("/relay/bundles")]
fn list_relay_bundles(
    _auth: GatewayAuth,
    store: &State<RelayBundleStore>,
) -> Json<RelayBundleList> {
    Json(RelayBundleList {
        bundles: store.list(),
    })
}

#[get("/relay/public-key")]
fn relay_public_key(
    _auth: GatewayAuth,
    config: &State<GatewayConfig>,
) -> Result<Json<RelayPublicKeyResponse>, Status> {
    config
        .relay_public_key()
        .map(|public_key| {
            Json(RelayPublicKeyResponse {
                public_key: public_key.to_string(),
            })
        })
        .ok_or(Status::NotFound)
}

#[get("/?<token>")]
fn dashboard(
    token: Option<&str>,
    store: &State<EnvelopeStore>,
    config: &State<GatewayConfig>,
) -> RawHtml<String> {
    if !config.accepts_token(token) {
        return RawHtml(access_page());
    }

    let summary = store.summary();
    let envelopes = store.list();
    let recent_rows = envelopes
        .iter()
        .take(6)
        .map(compact_report_row)
        .collect::<Vec<_>>()
        .join("");
    let link_suffix = token_link_suffix(token);

    RawHtml(page(
        "Jirani Gateway",
        &format!(
            r#"
            <section class="hero">
              <div>
                <p class="eyebrow">Optional Rust gateway</p>
                <h1>Jirani report sync</h1>
                <p>Minimized envelopes for anonymous analysis and Android downloads. Local and Nearby sync remain first.</p>
              </div>
              <div class="status-card">
                <span class="metric">{}</span>
                <span class="label">stored envelopes</span>
                <span class="auth-state">{}</span>
              </div>
            </section>
            <section class="grid">
              {}
            </section>
            <section class="panel">
              <div class="panel-head">
                <h2>Recent reports</h2>
                <div class="links"><a href="/analysis{}">Analysis</a><a href="/reports{}">View all</a></div>
              </div>
              <table>
                <thead><tr><th>Type</th><th>Area</th><th>Status</th><th>Sensitivity</th></tr></thead>
                <tbody>{}</tbody>
              </table>
            </section>
            "#,
            summary.total_envelopes,
            if config.auth_enabled() {
                "Token auth enabled"
            } else {
                "Open local/demo mode"
            },
            summary_cards(&summary),
            escape_html(&link_suffix),
            escape_html(&link_suffix),
            if recent_rows.is_empty() {
                empty_row(4, "No envelopes stored yet.")
            } else {
                recent_rows
            },
        ),
    ))
}

fn access_page() -> String {
    page(
        "Jirani Gateway Access",
        r#"
        <section class="hero">
          <div>
            <p class="eyebrow">Protected gateway</p>
            <h1>Jirani gateway access</h1>
            <p>This hosted test gateway uses a shared demo token. The token protects minimized sync data without asking for names, accounts, phone numbers, or device identity.</p>
          </div>
          <form class="status-card" method="get" action="/">
            <label for="token">Access token</label>
            <input id="token" name="token" type="password" autocomplete="off" placeholder="Demo token">
            <button type="submit">Open Dashboard</button>
          </form>
        </section>
        <section class="panel">
          <h2>Privacy note</h2>
          <p class="note">Jirani stores minimized envelopes only. Direct HTTPS still exposes source IP at the network layer, so hosted deployments should disable or anonymize proxy access logs.</p>
        </section>
        "#,
    )
}

#[get("/privacy-page")]
fn privacy_page(_auth: GatewayAuth) -> RawHtml<String> {
    let privacy = privacy_response();
    RawHtml(page(
        "Jirani Privacy",
        &format!(
            r#"
            <section class="panel">
              <h1>Gateway privacy posture</h1>
              <p class="note">{}</p>
              <table>
                <tbody>
                  <tr><th>Stores network identity</th><td>{}</td></tr>
                  <tr><th>Stores device identity</th><td>{}</td></tr>
                  <tr><th>Stores precise location</th><td>{}</td></tr>
                  <tr><th>Payload policy</th><td>{}</td></tr>
                  <tr><th>Hosted recommendation</th><td>{}</td></tr>
                </tbody>
              </table>
            </section>
            "#,
            escape_html(privacy.direct_ip_visibility),
            privacy.stored_network_identity,
            privacy.stored_device_identity,
            privacy.stored_precise_location,
            escape_html(privacy.payload_policy),
            escape_html(privacy.hosted_recommendation),
        ),
    ))
}

#[get("/reports")]
fn reports(auth: GatewayAuth, store: &State<EnvelopeStore>) -> RawHtml<String> {
    let rows = store
        .list()
        .iter()
        .map(report_row)
        .collect::<Vec<_>>()
        .join("");
    let link_suffix = auth.link_suffix();
    RawHtml(page(
        "Jirani Reports",
        &format!(
            r#"
            <section class="panel">
              <div class="panel-head">
                <h1>Accepted minimized reports</h1>
                <a href="/{}">Dashboard</a>
              </div>
              <table>
                <thead>
                  <tr><th>Type</th><th>Area</th><th>Time</th><th>Status</th><th>Sensitivity</th><th>Hash</th></tr>
                </thead>
                <tbody>{}</tbody>
              </table>
            </section>
            "#,
            escape_html(&link_suffix),
            if rows.is_empty() {
                empty_row(6, "No envelopes stored yet.")
            } else {
                rows
            },
        ),
    ))
}

#[get("/analysis")]
fn analysis(auth: GatewayAuth, store: &State<EnvelopeStore>) -> RawHtml<String> {
    let summary = store.summary();
    let link_suffix = auth.link_suffix();
    RawHtml(page(
        "Jirani Analysis",
        &format!(
            r#"
            <section class="panel">
              <div class="panel-head">
                <h1>Anonymous analysis</h1>
                <a href="/{}">Dashboard</a>
              </div>
              <div class="grid">
                {}
              </div>
              <h2>Top areas</h2>
              <table>
                <thead><tr><th>General area</th><th>Count</th></tr></thead>
                <tbody>{}</tbody>
              </table>
              <p class="note">Counts are for coordination and verification only. They do not confirm incidents.</p>
            </section>
            "#,
            escape_html(&link_suffix),
            summary_cards(&summary),
            area_rows(&summary),
        ),
    ))
}

pub fn routes() -> Vec<Route> {
    routes![
        health,
        privacy,
        privacy_page,
        dashboard,
        reports,
        analysis,
        upload_envelope,
        list_envelopes,
        anonymous_summary,
        upload_relay_bundle,
        list_relay_bundles,
        relay_public_key
    ]
}

fn privacy_response() -> PrivacyResponse {
    PrivacyResponse {
        direct_ip_visibility: "A direct HTTPS request exposes the connecting IP at the network layer. This gateway does not store IP, device ID, GPS, or User-Agent values in application data.",
        stored_network_identity: false,
        stored_device_identity: false,
        stored_precise_location: false,
        payload_policy: "Only minimized sync envelopes are accepted. Survivor-centered reports, obvious PII, expired envelopes, and hash mismatches are rejected.",
        hosted_recommendation: "Use HTTPS, disable reverse-proxy access logs or anonymize them, rotate the shared test token, and use a trusted relay/proxy if IP anonymity from the gateway operator is required.",
    }
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn summary_cards(summary: &AnonymousSummary) -> String {
    let sensitivity = summary_count_list(&summary.by_sensitivity);
    let verification = summary_count_list(&summary.by_verification_status);
    let areas = summary
        .top_areas
        .iter()
        .map(|item| format!("{} ({})", escape_html(&item.general_area), item.count))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"
        <article class="card"><h2>Sensitivity</h2><p>{}</p></article>
        <article class="card"><h2>Verification</h2><p>{}</p></article>
        <article class="card"><h2>Top areas</h2><p>{}</p></article>
        "#,
        fallback_text(&sensitivity),
        fallback_text(&verification),
        fallback_text(&areas),
    )
}

fn summary_count_list(counts: &[crate::models::SummaryCount]) -> String {
    counts
        .iter()
        .map(|item| format!("{} ({})", escape_html(&item.key), item.count))
        .collect::<Vec<_>>()
        .join(", ")
}

fn report_row(envelope: &SyncEnvelope) -> String {
    format!(
        "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td></tr>",
        escape_html(&envelope.payload.report_type),
        escape_html(&envelope.payload.general_area),
        escape_html(&envelope.payload.time_window),
        escape_html(&envelope.payload.verification_status),
        escape_html(&envelope.payload.sensitivity),
        escape_html(&envelope.content_hash.chars().take(12).collect::<String>()),
    )
}

fn compact_report_row(envelope: &SyncEnvelope) -> String {
    format!(
        "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
        escape_html(&envelope.payload.report_type),
        escape_html(&envelope.payload.general_area),
        escape_html(&envelope.payload.verification_status),
        escape_html(&envelope.payload.sensitivity),
    )
}

fn area_rows(summary: &AnonymousSummary) -> String {
    if summary.top_areas.is_empty() {
        return empty_row(2, "No area counts yet.");
    }
    summary
        .top_areas
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                escape_html(&item.general_area),
                item.count
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn empty_row(columns: usize, message: &str) -> String {
    format!(
        r#"<tr><td colspan="{}" class="empty">{}</td></tr>"#,
        columns,
        escape_html(message)
    )
}

fn fallback_text(value: &str) -> String {
    if value.is_empty() {
        "No data yet".to_string()
    } else {
        value.to_string()
    }
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
        <html lang="en">
        <head>
          <meta charset="utf-8">
          <meta name="viewport" content="width=device-width, initial-scale=1">
          <meta http-equiv="Content-Security-Policy" content="default-src 'self'; style-src 'unsafe-inline'">
          <title>{}</title>
          <style>{}</style>
        </head>
        <body>
          <nav><strong>Jirani Gateway</strong><span>Minimized sync, anonymous analysis, no identity storage</span></nav>
          <main>{}</main>
        </body>
        </html>"#,
        escape_html(title),
        css(),
        body
    )
}

fn css() -> &'static str {
    r#"
    :root { color-scheme: light; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #f6f7f2; color: #18201b; }
    body { margin: 0; }
    nav { display: flex; justify-content: space-between; gap: 16px; padding: 18px 28px; border-bottom: 1px solid #d9dfd5; background: #ffffff; }
    nav span { color: #5e6a61; }
    main { max-width: 1120px; margin: 0 auto; padding: 28px; }
    .hero { display: grid; grid-template-columns: minmax(0, 1fr) 220px; gap: 24px; align-items: stretch; margin-bottom: 22px; }
    .hero h1 { margin: 0; font-size: 40px; line-height: 1.05; }
    .hero p { max-width: 720px; color: #526056; font-size: 16px; line-height: 1.6; }
    .eyebrow { margin: 0 0 8px; text-transform: uppercase; letter-spacing: .08em; font-size: 12px; font-weight: 700; color: #286447; }
    .status-card, .card, .panel { background: #ffffff; border: 1px solid #dce3d8; border-radius: 8px; box-shadow: 0 10px 28px rgba(18, 33, 22, .06); }
    .status-card { display: flex; flex-direction: column; justify-content: center; padding: 22px; }
    .status-card label { font-weight: 700; margin-bottom: 8px; }
    .status-card input { min-height: 42px; border: 1px solid #b9c4b8; border-radius: 6px; padding: 0 10px; margin-bottom: 10px; font: inherit; }
    .status-card button { min-height: 42px; border: 0; border-radius: 6px; background: #1c6b49; color: #fff; font-weight: 800; cursor: pointer; }
    .metric { font-size: 44px; font-weight: 800; color: #143d2b; }
    .label, .auth-state, .note { color: #617066; }
    .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px; margin-bottom: 20px; }
    .card { padding: 18px; }
    .card h2, .panel h2 { margin: 0 0 8px; font-size: 16px; }
    .card p { margin: 0; color: #526056; line-height: 1.5; }
    .panel { padding: 18px; overflow-x: auto; }
    .panel-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
    .links { display: flex; gap: 14px; flex-wrap: wrap; }
    .panel-head h1, .panel-head h2 { margin: 0; }
    a { color: #1c6b49; font-weight: 700; text-decoration: none; }
    table { width: 100%; border-collapse: collapse; font-size: 14px; }
    th, td { padding: 11px 10px; border-bottom: 1px solid #e6ebe2; text-align: left; vertical-align: top; }
    th { color: #46534a; background: #f8faf6; }
    code { font-size: 12px; color: #334338; }
    .empty { color: #617066; text-align: center; padding: 28px; }
    @media (max-width: 760px) { nav, .hero, .grid { display: block; } nav span { display: block; margin-top: 4px; } .status-card, .card { margin-top: 12px; } main { padding: 18px; } .hero h1 { font-size: 32px; } }
    "#
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
