use crate::auth::{DashboardAuth, GatewayAuth, GatewayConfig};
use crate::models::{
    AnonymousSummary, ApiMessage, EnvelopeList, HealthResponse, PrivacyResponse, RelayBundle,
    RelayBundleList, RelayPublicKeyResponse, SyncEnvelope,
};
use crate::store::{GatewayStore, StoreWrite};
use rocket::form::{Form, FromForm};
use rocket::http::{Cookie, CookieJar, SameSite, Status};
use rocket::response::content::RawHtml;
use rocket::response::Redirect;
use rocket::serde::json::Json;
use rocket::{get, post, routes, Route, State};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(FromForm)]
struct LoginForm {
    username: String,
    password: String,
}

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
async fn upload_envelope(
    _auth: GatewayAuth,
    store: &State<GatewayStore>,
    envelope: Json<SyncEnvelope>,
) -> Result<Status, (Status, Json<ApiMessage>)> {
    let envelope = envelope.into_inner();
    envelope
        .validate_for_gateway(now_epoch_seconds())
        .map_err(|message| (Status::BadRequest, Json(ApiMessage { message })))?;

    match store.upsert_envelope(envelope).await {
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
async fn list_envelopes(
    _auth: GatewayAuth,
    store: &State<GatewayStore>,
) -> Result<Json<EnvelopeList>, (Status, Json<ApiMessage>)> {
    store
        .list_envelopes()
        .await
        .map(|envelopes| Json(EnvelopeList { envelopes }))
        .map_err(storage_read_error)
}

#[get("/analytics/anonymous-summary")]
async fn anonymous_summary(
    _auth: GatewayAuth,
    store: &State<GatewayStore>,
) -> Result<Json<AnonymousSummary>, (Status, Json<ApiMessage>)> {
    store.summary().await.map(Json).map_err(storage_read_error)
}

#[post("/relay/bundles", format = "json", data = "<bundle>")]
async fn upload_relay_bundle(
    _auth: GatewayAuth,
    store: &State<GatewayStore>,
    bundle: Json<RelayBundle>,
) -> Result<Status, (Status, Json<ApiMessage>)> {
    let bundle = bundle.into_inner();
    bundle
        .validate_for_gateway(now_epoch_seconds())
        .map_err(|message| (Status::BadRequest, Json(ApiMessage { message })))?;

    match store.upsert_relay_bundle(bundle).await {
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
async fn list_relay_bundles(
    _auth: GatewayAuth,
    store: &State<GatewayStore>,
) -> Result<Json<RelayBundleList>, (Status, Json<ApiMessage>)> {
    store
        .list_relay_bundles()
        .await
        .map(|bundles| Json(RelayBundleList { bundles }))
        .map_err(storage_read_error)
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

#[get("/")]
async fn dashboard(
    cookies: &CookieJar<'_>,
    store: &State<GatewayStore>,
    config: &State<GatewayConfig>,
) -> RawHtml<String> {
    let session = cookies
        .get(config.dashboard_session_cookie_name())
        .map(|cookie| cookie.value());
    if !config.accepts_dashboard_session_cookie(session, now_epoch_seconds()) {
        return RawHtml(login_page(None));
    }

    let Ok(summary) = store.summary().await else {
        return RawHtml(storage_error_page());
    };
    let Ok(envelopes) = store.list_envelopes().await else {
        return RawHtml(storage_error_page());
    };
    let recent_rows = envelopes
        .iter()
        .take(6)
        .map(compact_report_row)
        .collect::<Vec<_>>()
        .join("");

    RawHtml(page(
        "Jirani",
        &format!(
            r#"
            <section class="dashboard-head">
              <div>
                <h1>Dashboard</h1>
                <p>Review minimized report signals, anonymous trends, and trusted Android sync activity while keeping reporter identity out of the gateway.</p>
              </div>
              <div class="actions">
                <a class="button primary" href="/reports">View Reports</a>
                <a class="button secondary" href="/analysis">Open Analysis</a>
              </div>
            </section>
            <section class="metrics-grid">
              <article class="metric-card metric-card-primary">
                <div class="metric-card-head"><span>Total Reports</span><a href="/reports">View</a></div>
                <strong>{}</strong>
                <p>{}</p>
              </article>
              {}
            </section>
            <section class="dashboard-grid">
              <article class="panel panel-wide">
                <div class="panel-head">
                  <h2>Report Activity</h2>
                  <span class="muted">Last 7 review windows</span>
                </div>
                <div class="bar-chart" aria-hidden="true">
                  <span style="--h: 52%"></span><span style="--h: 72%"></span><span style="--h: 44%"></span><span style="--h: 84%"></span><span style="--h: 62%"></span><span style="--h: 48%"></span><span style="--h: 70%"></span>
                </div>
                <div class="chart-labels"><span>S</span><span>M</span><span>T</span><span>W</span><span>T</span><span>F</span><span>S</span></div>
              </article>
              <article class="panel">
                <div class="panel-head">
                  <h2>Review Focus</h2>
                  <a href="/privacy-page">Privacy</a>
                </div>
                <p class="focus-title">Anonymous coordination only</p>
                <p class="note">Unverified reports remain signals for local review, not confirmed incidents.</p>
              </article>
              <article class="panel panel-wide">
                <div class="panel-head">
                  <h2>Recent Reports</h2>
                  <div class="links"><a href="/analysis">Analysis</a><a href="/reports">View all</a>{}</div>
                </div>
                <table>
                  <thead><tr><th>Type</th><th>Area</th><th>Status</th><th>Sensitivity</th></tr></thead>
                  <tbody>{}</tbody>
                </table>
              </article>
            </section>
            "#,
            summary.total_envelopes,
            if config.auth_enabled() {
                "Token auth enabled"
            } else {
                "Open local/demo mode"
            },
            summary_cards(&summary),
            logout_link(config),
            if recent_rows.is_empty() {
                empty_row(4, "No envelopes stored yet.")
            } else {
                recent_rows
            },
        ),
    ))
}

#[get("/login")]
fn login() -> RawHtml<String> {
    RawHtml(login_page(None))
}

#[post("/login", data = "<form>")]
fn login_submit(
    form: Form<LoginForm>,
    cookies: &CookieJar<'_>,
    config: &State<GatewayConfig>,
) -> Result<Redirect, RawHtml<String>> {
    if !config.dashboard_auth_enabled() {
        return Err(RawHtml(login_page(Some(
            "Dashboard users are not configured yet. Set JIRANI_DASHBOARD_USERS before signing in.",
        ))));
    }

    let form = form.into_inner();
    if !config.authenticate_dashboard_user(form.username.trim(), &form.password) {
        return Err(RawHtml(login_page(Some(
            "The username or password was not accepted.",
        ))));
    }

    let session = config.issue_dashboard_session(form.username.trim(), now_epoch_seconds());
    let mut cookie = Cookie::new(config.dashboard_session_cookie_name(), session);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Strict);
    cookie.set_path("/");
    cookies.add(cookie);

    Ok(Redirect::to("/"))
}

#[post("/logout")]
fn logout(cookies: &CookieJar<'_>, config: &State<GatewayConfig>) -> Redirect {
    cookies.remove(Cookie::from(config.dashboard_session_cookie_name()));
    Redirect::to("/login")
}

fn login_page(error: Option<&str>) -> String {
    let error = error
        .map(|message| format!(r#"<p class="error">{}</p>"#, escape_html(message)))
        .unwrap_or_default();

    login_shell(
        "Jirani Login",
        &format!(
            r#"
        <section class="login-card" aria-labelledby="login-title">
          <div class="login-copy">
            <a class="brand login-brand" href="/" aria-label="Jirani home">
              <span class="brand-mark">J</span>
              <strong>Jirani</strong>
            </a>
            <div>
              <p class="login-kicker">Protected review workspace</p>
              <h1 id="login-title">Sign in to the Jirani dashboard</h1>
              <p>Access is limited to authorized OSF staff and community elders reviewing minimized, anonymous report signals.</p>
            </div>
            <div class="login-note">
              <strong>Privacy boundary</strong>
              <p>The dashboard does not ask for reporter names, phone numbers, device identity, or precise locations.</p>
            </div>
          </div>
          <form class="login-form" method="post" action="/login">
            <div>
              <h2>Dashboard access</h2>
              <p>Use the credentials issued for your review role.</p>
            </div>
            {}
            <label for="username">Username</label>
            <input id="username" name="username" type="text" autocomplete="username" placeholder="elder_osf">
            <label for="password">Password</label>
            <input id="password" name="password" type="password" autocomplete="current-password" placeholder="Enter your password">
            <button type="submit">Open Dashboard</button>
            <p class="form-footnote">For hosted deployments, use HTTPS and rotate credentials after demos.</p>
          </form>
        </section>
        "#,
            error,
        ),
    )
}

#[get("/privacy-page")]
fn privacy_page(_auth: DashboardAuth) -> RawHtml<String> {
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
async fn reports(
    _auth: DashboardAuth,
    store: &State<GatewayStore>,
    config: &State<GatewayConfig>,
) -> RawHtml<String> {
    let Ok(envelopes) = store.list_envelopes().await else {
        return RawHtml(storage_error_page());
    };
    let rows = envelopes
        .iter()
        .map(report_row)
        .collect::<Vec<_>>()
        .join("");
    RawHtml(page(
        "Jirani Reports",
        &format!(
            r#"
            <section class="panel">
              <div class="panel-head">
                <h1>Accepted minimized reports</h1>
                <div class="links"><a href="/">Dashboard</a>{}</div>
              </div>
              <table>
                <thead>
                  <tr><th>Type</th><th>Area</th><th>Time</th><th>Status</th><th>Sensitivity</th><th>Hash</th></tr>
                </thead>
                <tbody>{}</tbody>
              </table>
            </section>
            "#,
            logout_link(config),
            if rows.is_empty() {
                empty_row(6, "No envelopes stored yet.")
            } else {
                rows
            },
        ),
    ))
}

#[get("/analysis")]
async fn analysis(
    _auth: DashboardAuth,
    store: &State<GatewayStore>,
    config: &State<GatewayConfig>,
) -> RawHtml<String> {
    let Ok(summary) = store.summary().await else {
        return RawHtml(storage_error_page());
    };
    RawHtml(page(
        "Jirani Analysis",
        &format!(
            r#"
            <section class="panel">
              <div class="panel-head">
                <h1>Anonymous analysis</h1>
                <div class="links"><a href="/">Dashboard</a>{}</div>
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
            logout_link(config),
            summary_cards(&summary),
            area_rows(&summary),
        ),
    ))
}

pub fn routes() -> Vec<Route> {
    routes![
        health,
        privacy,
        login,
        login_submit,
        logout,
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

fn storage_read_error(message: String) -> (Status, Json<ApiMessage>) {
    (
        Status::InternalServerError,
        Json(ApiMessage {
            message: format!("Storage read failed: {message}"),
        }),
    )
}

fn storage_error_page() -> String {
    page(
        "Jirani Storage Error",
        r#"
        <section class="panel">
          <h1>Storage unavailable</h1>
          <p class="note">The gateway could not read its configured store. Check PostgreSQL connectivity or local storage configuration.</p>
        </section>
        "#,
    )
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
        <article class="metric-card"><div class="metric-card-head"><span>Sensitivity</span><a href="/analysis">View</a></div><strong>{}</strong><p>Current mix</p></article>
        <article class="metric-card"><div class="metric-card-head"><span>Verification</span><a href="/analysis">View</a></div><strong>{}</strong><p>Review status</p></article>
        <article class="metric-card"><div class="metric-card-head"><span>Top areas</span><a href="/analysis">View</a></div><strong>{}</strong><p>Coarse locations</p></article>
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

fn logout_link(config: &GatewayConfig) -> String {
    if config.dashboard_auth_enabled() {
        r#"<form class="inline-form" method="post" action="/logout"><button type="submit">Log out</button></form>"#
            .to_string()
    } else {
        String::new()
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
          <div class="app-frame">
            <aside class="sidebar">
              <a class="brand" href="/" aria-label="Jirani home">
                <span class="brand-mark">J</span>
                <strong>Jirani</strong>
              </a>
              <div class="nav-group">
                <span>Review</span>
                <a href="/">Dashboard</a>
                <a href="/reports">Reports</a>
                <a href="/analysis">Analysis</a>
              </div>
              <div class="nav-group">
                <span>Trust</span>
                <a href="/privacy-page">Privacy</a>
              </div>
              <div class="sidebar-card">
                <strong>Safe review</strong>
                <p>Minimized signals only. No reporter identity storage.</p>
              </div>
            </aside>
            <section class="workspace">
              <header class="topbar">
                <div class="search"><span></span><p>Search reports</p><kbd>F</kbd></div>
                <div class="profile">
                  <span class="profile-dot"></span>
                  <div><strong>Review Team</strong><p>OSF staff and elders</p></div>
                </div>
              </header>
              <main>{}</main>
            </section>
          </div>
        </body>
        </html>"#,
        escape_html(title),
        css(),
        body
    )
}

fn login_shell(title: &str, body: &str) -> String {
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
        <body class="login-body">
          <main class="login-main">{}</main>
        </body>
        </html>"#,
        escape_html(title),
        css(),
        body
    )
}

fn css() -> &'static str {
    r#"
    :root {
      color-scheme: light;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #e9ece9;
      color: #111a15;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      padding: 32px;
      background: radial-gradient(circle at 20% 0%, #ffffff 0, #e8ece8 34%, #dfe4e1 100%);
    }
    .app-frame {
      display: grid;
      grid-template-columns: 220px minmax(0, 1fr);
      width: min(100%, 1320px);
      min-height: calc(100vh - 64px);
      margin: 0 auto;
      overflow: hidden;
      border: 1px solid rgba(255, 255, 255, .8);
      border-radius: 24px;
      background: #f8faf8;
      box-shadow: 0 28px 70px rgba(29, 38, 33, .16);
    }
    .sidebar {
      display: flex;
      flex-direction: column;
      gap: 28px;
      padding: 24px 18px;
      background: rgba(255, 255, 255, .78);
      border-right: 1px solid #e7ece7;
    }
    .brand { display: inline-flex; align-items: center; gap: 10px; color: #101814; text-decoration: none; padding: 2px 8px; }
    .brand-mark {
      display: grid;
      width: 32px;
      height: 32px;
      place-items: center;
      border-radius: 50%;
      background: #087a52;
      color: #ffffff;
      font-weight: 800;
      box-shadow: inset 0 -2px 0 rgba(0, 0, 0, .16);
    }
    .nav-group { display: grid; gap: 6px; }
    .nav-group span {
      padding: 0 12px 8px;
      color: #8a958f;
      font-size: 11px;
      font-weight: 800;
      text-transform: uppercase;
    }
    .nav-group a {
      display: flex;
      align-items: center;
      min-height: 38px;
      padding: 0 12px;
      border-radius: 12px;
      color: #69756f;
      font-size: 14px;
      font-weight: 650;
      text-decoration: none;
    }
    .nav-group a:hover, .nav-group a:first-of-type {
      background: #eaf5ef;
      color: #087a52;
    }
    .sidebar-card {
      margin-top: auto;
      padding: 18px;
      border-radius: 18px;
      background: #06150e;
      color: #ffffff;
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, .08);
    }
    .sidebar-card p { margin: 8px 0 0; color: #b8cbc1; font-size: 12px; line-height: 1.45; }
    .workspace { min-width: 0; background: #f8faf8; }
    .topbar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 18px;
      padding: 22px 28px 10px;
    }
    .search {
      display: flex;
      align-items: center;
      gap: 10px;
      width: min(100%, 340px);
      min-height: 48px;
      padding: 0 14px;
      border-radius: 20px;
      background: #ffffff;
      box-shadow: 0 10px 24px rgba(26, 40, 32, .06);
      color: #9aa49e;
    }
    .search span {
      width: 12px;
      height: 12px;
      border: 2px solid #111a15;
      border-radius: 50%;
      box-shadow: 7px 7px 0 -5px #111a15;
    }
    .search p { margin: 0; font-size: 13px; }
    kbd {
      margin-left: auto;
      padding: 3px 7px;
      border-radius: 8px;
      background: #f0f3f0;
      color: #59655f;
      font: inherit;
      font-size: 12px;
      font-weight: 700;
    }
    .profile { display: flex; align-items: center; gap: 10px; }
    .profile-dot {
      width: 42px;
      height: 42px;
      border-radius: 50%;
      background: linear-gradient(145deg, #f0c8a2, #8b5b3f);
      border: 3px solid #f5ddd0;
    }
    .profile strong { display: block; font-size: 14px; }
    .profile p { margin: 2px 0 0; color: #7a857f; font-size: 12px; }
    .login-body {
      display: grid;
      min-height: 100vh;
      padding: 32px;
      place-items: center;
      background:
        radial-gradient(circle at 18% 12%, rgba(255, 255, 255, .95) 0, rgba(255, 255, 255, 0) 30%),
        linear-gradient(135deg, #f3f5f2 0%, #dfe6e1 52%, #cfdad3 100%);
    }
    .login-main {
      width: min(100%, 980px);
      padding: 0;
    }
    .login-card {
      display: grid;
      grid-template-columns: minmax(0, 1fr) 420px;
      min-height: 620px;
      overflow: hidden;
      border: 1px solid rgba(255, 255, 255, .82);
      border-radius: 24px;
      background: #ffffff;
      box-shadow: 0 30px 80px rgba(21, 35, 28, .18);
    }
    .login-copy {
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      gap: 32px;
      padding: 44px;
      background:
        linear-gradient(160deg, rgba(8, 122, 82, .95), rgba(5, 48, 35, .98)),
        #073a2a;
      color: #ffffff;
    }
    .login-brand { color: #ffffff; padding: 0; }
    .login-brand .brand-mark { background: #ffffff; color: #087a52; }
    .login-kicker {
      margin: 0 0 12px;
      color: #bfe1d3;
      font-size: 12px;
      font-weight: 850;
      text-transform: uppercase;
    }
    .login-copy h1 {
      margin: 0;
      max-width: 520px;
      font-size: 42px;
      line-height: 1.05;
      letter-spacing: 0;
    }
    .login-copy p {
      max-width: 540px;
      color: #d7ebe3;
      line-height: 1.65;
    }
    .login-note {
      max-width: 460px;
      padding: 18px;
      border: 1px solid rgba(255, 255, 255, .2);
      border-radius: 16px;
      background: rgba(255, 255, 255, .08);
    }
    .login-note p { margin: 8px 0 0; font-size: 13px; }
    .login-form {
      display: flex;
      flex-direction: column;
      justify-content: center;
      padding: 44px;
      background: #ffffff;
    }
    .login-form h2 {
      margin: 0 0 6px;
      font-size: 24px;
    }
    .login-form p {
      margin: 0 0 24px;
      color: #6e7a73;
      font-size: 14px;
      line-height: 1.5;
    }
    .login-form label {
      margin: 14px 0 8px;
      color: #2d3832;
      font-size: 13px;
      font-weight: 800;
    }
    .login-form input {
      width: 100%;
      min-height: 48px;
      border: 1px solid #c7d0ca;
      border-radius: 10px;
      padding: 0 13px;
      background: #fbfcfb;
      color: #111a15;
      font: inherit;
    }
    .login-form input:focus {
      outline: 3px solid rgba(8, 122, 82, .18);
      border-color: #087a52;
      background: #ffffff;
    }
    .login-form button {
      min-height: 50px;
      margin-top: 22px;
      border: 0;
      border-radius: 10px;
      background: #087a52;
      color: #ffffff;
      font: inherit;
      font-weight: 850;
      cursor: pointer;
      box-shadow: 0 14px 26px rgba(8, 122, 82, .22);
    }
    .login-form button:hover { background: #066342; }
    .login-form .error {
      margin: 0 0 10px;
      padding: 12px 14px;
      border-radius: 10px;
      background: #fff1f1;
      color: #8b1e1e;
      font-size: 13px;
    }
    .form-footnote { margin: 16px 0 0 !important; font-size: 12px !important; }
    main {
      width: 100%;
      padding: 12px 28px 28px;
    }
    .dashboard-head, .hero {
      display: flex;
      align-items: flex-end;
      justify-content: space-between;
      gap: 20px;
      margin-bottom: 18px;
    }
    .dashboard-head h1, .hero h1 {
      margin: 0;
      max-width: 760px;
      font-size: 34px;
      line-height: 1.05;
      font-weight: 800;
      letter-spacing: 0;
    }
    .dashboard-head p, .hero p {
      max-width: 760px;
      margin: 8px 0 0;
      color: #8a958f;
      font-size: 14px;
      line-height: 1.55;
    }
    .actions, .links { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
    .button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-height: 44px;
      padding: 0 18px;
      border-radius: 22px;
      font-size: 14px;
      font-weight: 800;
      text-decoration: none;
    }
    .button.primary { background: #087a52; color: #ffffff; box-shadow: 0 12px 24px rgba(8, 122, 82, .24); }
    .button.secondary { color: #0d3d2d; border: 1px solid #b9c9c1; background: #ffffff; }
    .metrics-grid {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: 14px;
      margin-bottom: 14px;
    }
    .metric-card, .status-card, .card, .panel {
      background: #ffffff;
      border: 1px solid #eef1ef;
      border-radius: 16px;
      box-shadow: 0 12px 28px rgba(24, 39, 31, .06);
    }
    .metric-card {
      min-height: 150px;
      padding: 18px;
      color: #101814;
    }
    .metric-card-primary {
      background: linear-gradient(145deg, #087a52, #0c5f43);
      color: #ffffff;
      box-shadow: 0 18px 34px rgba(8, 122, 82, .24);
    }
    .metric-card-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      margin-bottom: 14px;
      font-size: 14px;
      font-weight: 750;
    }
    .metric-card-head a {
      display: inline-grid;
      place-items: center;
      min-width: 34px;
      height: 34px;
      padding: 0 10px;
      border-radius: 17px;
      color: inherit;
      border: 1px solid currentColor;
      font-size: 12px;
      text-decoration: none;
      opacity: .86;
    }
    .metric-card strong {
      display: block;
      min-height: 42px;
      font-size: 34px;
      line-height: 1.05;
      font-weight: 850;
      overflow-wrap: anywhere;
    }
    .metric-card p { margin: 8px 0 0; color: #718078; font-size: 12px; }
    .metric-card-primary p, .metric-card-primary .metric-card-head a { color: #dceee7; }
    .dashboard-grid {
      display: grid;
      grid-template-columns: minmax(0, 2fr) minmax(260px, 1fr);
      gap: 14px;
      align-items: start;
    }
    .panel-wide { grid-column: span 1; }
    .panel-wide:last-child { grid-column: 1 / -1; }
    .bar-chart {
      display: grid;
      grid-template-columns: repeat(7, 1fr);
      align-items: end;
      gap: 14px;
      height: 120px;
      padding: 10px 2px 0;
    }
    .bar-chart span {
      display: block;
      height: var(--h);
      min-height: 34px;
      border-radius: 24px;
      background: #0b7f57;
    }
    .bar-chart span:nth-child(odd) {
      background: repeating-linear-gradient(135deg, #dbe4de 0 4px, #ffffff 4px 8px);
      border: 1px solid #d8e1db;
    }
    .chart-labels {
      display: grid;
      grid-template-columns: repeat(7, 1fr);
      gap: 14px;
      color: #8a958f;
      font-size: 12px;
      text-align: center;
    }
    .focus-title {
      margin: 18px 0 8px;
      color: #0b5d43;
      font-size: 21px;
      line-height: 1.2;
      font-weight: 800;
    }
    .status-card {
      display: flex;
      flex-direction: column;
      justify-content: center;
      min-height: 148px;
      padding: 24px;
      border-top: 4px solid #c68b2c;
    }
    .status-card label { font-weight: 700; margin-bottom: 8px; color: #2b3830; }
    .status-card input {
      width: 100%;
      min-height: 46px;
      border: 1px solid #bbc7bd;
      border-radius: 8px;
      padding: 0 12px;
      margin-bottom: 12px;
      font: inherit;
      background: #fbfcfa;
    }
    .status-card input:focus {
      outline: 3px solid rgba(21, 95, 70, .18);
      border-color: #155f46;
      background: #ffffff;
    }
    .status-card button {
      min-height: 46px;
      border: 0;
      border-radius: 8px;
      background: #155f46;
      color: #ffffff;
      font-weight: 800;
      cursor: pointer;
    }
    .status-card button:hover { background: #0f4b39; }
    .inline-form { display: inline; margin: 0; }
    .inline-form button {
      border: 0;
      background: transparent;
      color: #155f46;
      font: inherit;
      font-weight: 700;
      cursor: pointer;
      padding: 0;
    }
    .error { color: #8b1e1e; font-weight: 700; margin: 0 0 12px; }
    .metric {
      margin-top: 4px;
      font-size: 52px;
      line-height: 1;
      font-weight: 850;
      color: #123d31;
    }
    .label {
      color: #5f6b64;
      font-size: 13px;
      font-weight: 800;
      text-transform: uppercase;
    }
    .auth-state, .note { color: #617069; }
    .auth-state { margin-top: 12px; line-height: 1.4; }
    .grid {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 14px;
      margin-bottom: 20px;
    }
    .card h2, .panel h2 {
      margin: 0 0 10px;
      font-size: 15px;
      color: #1e2a23;
    }
    .card p { margin: 0; color: #526058; line-height: 1.55; }
    .panel {
      padding: 18px;
      overflow-x: auto;
    }
    .panel-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 14px;
      margin-bottom: 14px;
    }
    .panel-head h1, .panel-head h2 { margin: 0; }
    a { color: #155f46; font-weight: 750; text-decoration: none; }
    a:hover, .inline-form button:hover { text-decoration: underline; }
    table {
      width: 100%;
      min-width: 680px;
      border-collapse: separate;
      border-spacing: 0;
      font-size: 14px;
    }
    th, td {
      padding: 13px 12px;
      border-bottom: 1px solid #e7ece6;
      text-align: left;
      vertical-align: top;
    }
    th {
      color: #46534b;
      background: #f4f7f3;
      font-size: 12px;
      font-weight: 800;
      text-transform: uppercase;
    }
    th:first-child { border-top-left-radius: 8px; }
    th:last-child { border-top-right-radius: 8px; }
    code { font-size: 12px; color: #334338; }
    .empty { color: #617069; text-align: center; padding: 34px 12px; }
    .muted { color: #8a958f; font-size: 12px; }
    @media (max-width: 1060px) {
      body { padding: 18px; }
      .app-frame { grid-template-columns: 1fr; }
      .sidebar {
        flex-direction: row;
        align-items: center;
        overflow-x: auto;
        border-right: 0;
        border-bottom: 1px solid #e7ece7;
      }
      .nav-group { display: flex; align-items: center; gap: 4px; }
      .nav-group span, .sidebar-card { display: none; }
      .metrics-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
      .dashboard-grid { grid-template-columns: 1fr; }
    }
    @media (max-width: 720px) {
      body { padding: 0; background: #f8faf8; }
      .login-body { padding: 16px; background: #e8eee9; }
      .login-card { grid-template-columns: 1fr; min-height: 0; }
      .login-copy { padding: 28px; }
      .login-copy h1 { font-size: 32px; }
      .login-form { padding: 28px; }
      .app-frame { min-height: 100vh; border: 0; border-radius: 0; box-shadow: none; }
      .topbar { align-items: stretch; flex-direction: column; padding: 16px 18px 8px; }
      .search { width: 100%; }
      main { padding: 10px 18px 22px; }
      .dashboard-head, .hero { align-items: flex-start; flex-direction: column; }
      .metrics-grid { grid-template-columns: 1fr; }
      .hero h1, .dashboard-head h1 { font-size: 30px; }
      .status-card { min-height: 0; }
      .panel-head { align-items: flex-start; flex-direction: column; }
    }
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
