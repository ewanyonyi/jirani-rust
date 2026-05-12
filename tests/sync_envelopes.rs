use jirani_rust::auth::GatewayConfig;
use jirani_rust::models::{RelayBundle, RelayPublicHeader, SanitizedReportPayload, SyncEnvelope};
use jirani_rust::store::{EnvelopeStore, RelayBundleStore};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;

#[test]
fn post_then_get_sync_envelope() {
    let client = open_client();
    let envelope = community_envelope();

    let response = client
        .post("/sync/envelopes")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&envelope).expect("serialize envelope"))
        .dispatch();

    assert_eq!(response.status(), Status::Created);

    let response = client.get("/sync/envelopes").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().expect("response body");
    assert!(body.contains("livestock or grazing dispute"));
    assert!(body.contains(&envelope.content_hash));
}

#[test]
fn duplicate_envelope_returns_conflict_for_android_dedupe() {
    let client = open_client();
    let envelope = community_envelope();
    let body = serde_json::to_string(&envelope).expect("serialize envelope");

    assert_eq!(
        client
            .post("/sync/envelopes")
            .header(ContentType::JSON)
            .body(body.clone())
            .dispatch()
            .status(),
        Status::Created,
    );
    assert_eq!(
        client
            .post("/sync/envelopes")
            .header(ContentType::JSON)
            .body(body)
            .dispatch()
            .status(),
        Status::Conflict,
    );
}

#[test]
fn survivor_centered_envelope_is_rejected() {
    let client = open_client();
    let mut envelope = community_envelope();
    envelope.audience_tier = "SurvivorSupportOnly".to_string();
    envelope.payload.sensitivity = "SurvivorCentered".to_string();
    envelope.content_hash = envelope.payload.content_hash();

    let response = client
        .post("/sync/envelopes")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&envelope).expect("serialize envelope"))
        .dispatch();

    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn mismatched_content_hash_is_rejected() {
    let client = open_client();
    let mut envelope = community_envelope();
    envelope.content_hash = "tampered".to_string();

    let response = client
        .post("/sync/envelopes")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&envelope).expect("serialize envelope"))
        .dispatch();

    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn dashboard_shows_login_until_dashboard_user_signs_in() {
    let client = open_client();

    let response = client.get("/").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().expect("dashboard body");
    assert!(body.contains("Sign in to the Jirani dashboard"));
    assert!(body.contains("login-card"));

    let response = client.get("/analysis").dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

#[test]
fn token_auth_blocks_sync_when_enabled() {
    let client = Client::tracked(jirani_rust::rocket_with_config(GatewayConfig::with_token(
        "test-token",
    )))
    .expect("valid rocket instance");
    let envelope = community_envelope();

    assert_eq!(
        client
            .post("/sync/envelopes")
            .header(ContentType::JSON)
            .body(serde_json::to_string(&envelope).expect("serialize envelope"))
            .dispatch()
            .status(),
        Status::Unauthorized,
    );

    assert_eq!(
        client
            .post("/sync/envelopes")
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "Authorization",
                "Bearer test-token",
            ))
            .body(serde_json::to_string(&envelope).expect("serialize envelope"))
            .dispatch()
            .status(),
        Status::Created,
    );
}

#[test]
fn dashboard_username_password_auth_protects_report_pages() {
    let client = Client::tracked(jirani_rust::rocket_with_config(
        GatewayConfig::open()
            .with_dashboard_session_secret("test-session-secret")
            .with_dashboard_user("elder_osf", "correct horse battery"),
    ))
    .expect("valid rocket instance");

    let response = client.get("/").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("dashboard login body")
        .contains("Sign in to the Jirani dashboard"));
    assert_eq!(
        client.get("/reports").dispatch().status(),
        Status::Unauthorized
    );

    let response = client.get("/login").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("login body")
        .contains("Sign in to the Jirani dashboard"));

    let response = client
        .post("/login")
        .header(ContentType::Form)
        .body("username=elder_osf&password=wrong")
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("failed login body")
        .contains("not accepted"));

    let response = client
        .post("/login")
        .header(ContentType::Form)
        .body("username=elder_osf&password=correct%20horse%20battery")
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    let response = client.get("/reports").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("reports body")
        .contains("Accepted minimized reports"));

    assert_eq!(client.post("/logout").dispatch().status(), Status::SeeOther);
    assert_eq!(
        client.get("/analysis").dispatch().status(),
        Status::Unauthorized
    );
}

#[test]
fn file_backed_store_survives_restart_without_identity_metadata() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("envelopes.json");
    let envelope = community_envelope();

    let client = Client::tracked(jirani_rust::rocket_with_store(
        GatewayConfig::open(),
        EnvelopeStore::from_path(path.clone()),
    ))
    .expect("valid rocket instance");
    assert_eq!(
        client
            .post("/sync/envelopes")
            .header(ContentType::JSON)
            .body(serde_json::to_string(&envelope).expect("serialize envelope"))
            .dispatch()
            .status(),
        Status::Created,
    );

    let stored = std::fs::read_to_string(&path).expect("persisted envelopes");
    assert!(stored.contains(&envelope.envelope_id));
    assert!(!stored.contains("127.0.0.1"));
    assert!(!stored.contains("User-Agent"));

    let restarted = Client::tracked(jirani_rust::rocket_with_store(
        GatewayConfig::open(),
        EnvelopeStore::from_path(path),
    ))
    .expect("valid rocket instance");
    let response = restarted.get("/sync/envelopes").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("body")
        .contains(&envelope.content_hash));
}

#[test]
fn privacy_endpoint_states_network_identity_is_not_stored() {
    let client = open_client();
    let response = client.get("/privacy").dispatch();

    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().expect("privacy body");
    assert!(body.contains("\"storedNetworkIdentity\":false"));
    assert!(body.contains("direct HTTPS request exposes"));
}

#[test]
fn post_then_get_relay_bundle() {
    let client = open_client();
    let bundle = relay_bundle();

    let response = client
        .post("/relay/bundles")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&bundle).expect("serialize relay bundle"))
        .dispatch();

    assert_eq!(response.status(), Status::Created);

    let response = client.get("/relay/bundles").dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_string().expect("relay response body");
    assert!(body.contains("bundle-demo-community"));
    assert!(body.contains("Cattle movement reported"));
    assert!(body.contains(&bundle.bundle_hash));
}

#[test]
fn duplicate_relay_bundle_returns_conflict() {
    let client = open_client();
    let bundle = relay_bundle();
    let body = serde_json::to_string(&bundle).expect("serialize relay bundle");

    assert_eq!(
        client
            .post("/relay/bundles")
            .header(ContentType::JSON)
            .body(body.clone())
            .dispatch()
            .status(),
        Status::Created,
    );
    assert_eq!(
        client
            .post("/relay/bundles")
            .header(ContentType::JSON)
            .body(body)
            .dispatch()
            .status(),
        Status::Conflict,
    );
}

#[test]
fn relay_bundle_with_tampered_public_header_is_rejected() {
    let client = open_client();
    let mut bundle = relay_bundle();
    bundle.public_header.message = "Changed after hashing".to_string();

    let response = client
        .post("/relay/bundles")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&bundle).expect("serialize relay bundle"))
        .dispatch();

    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn relay_bundle_with_pii_in_public_header_is_rejected() {
    let client = open_client();
    let mut bundle = relay_bundle();
    bundle.public_header.message = "Call 0712345678 about this alert".to_string();
    bundle.bundle_hash = bundle.computed_bundle_hash();

    let response = client
        .post("/relay/bundles")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&bundle).expect("serialize relay bundle"))
        .dispatch();

    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn token_auth_blocks_relay_routes_when_enabled() {
    let client = Client::tracked(jirani_rust::rocket_with_config(GatewayConfig::with_token(
        "test-token",
    )))
    .expect("valid rocket instance");
    let bundle = relay_bundle();

    assert_eq!(
        client
            .post("/relay/bundles")
            .header(ContentType::JSON)
            .body(serde_json::to_string(&bundle).expect("serialize relay bundle"))
            .dispatch()
            .status(),
        Status::Unauthorized,
    );

    assert_eq!(
        client
            .post("/relay/bundles")
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "Authorization",
                "Bearer test-token",
            ))
            .body(serde_json::to_string(&bundle).expect("serialize relay bundle"))
            .dispatch()
            .status(),
        Status::Created,
    );
}

#[test]
fn relay_public_key_is_optional_and_authenticated() {
    let client = open_client();
    assert_eq!(
        client.get("/relay/public-key").dispatch().status(),
        Status::NotFound
    );

    let client = Client::tracked(jirani_rust::rocket_with_config(
        GatewayConfig::open().with_relay_public_key("demo-public-key"),
    ))
    .expect("valid rocket instance");
    let response = client.get("/relay/public-key").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("public key body")
        .contains("demo-public-key"));
}

#[test]
fn file_backed_relay_store_survives_restart_without_identity_metadata() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("relay-bundles.json");
    let bundle = relay_bundle();

    let client = Client::tracked(jirani_rust::rocket_with_stores(
        GatewayConfig::open(),
        EnvelopeStore::default(),
        RelayBundleStore::from_path(path.clone()),
    ))
    .expect("valid rocket instance");
    assert_eq!(
        client
            .post("/relay/bundles")
            .header(ContentType::JSON)
            .body(serde_json::to_string(&bundle).expect("serialize relay bundle"))
            .dispatch()
            .status(),
        Status::Created,
    );

    let stored = std::fs::read_to_string(&path).expect("persisted relay bundles");
    assert!(stored.contains(&bundle.bundle_id));
    assert!(stored.contains("encrypted-demo-payload"));
    assert!(!stored.contains("127.0.0.1"));
    assert!(!stored.contains("User-Agent"));

    let restarted = Client::tracked(jirani_rust::rocket_with_stores(
        GatewayConfig::open(),
        EnvelopeStore::default(),
        RelayBundleStore::from_path(path),
    ))
    .expect("valid rocket instance");
    let response = restarted.get("/relay/bundles").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert!(response
        .into_string()
        .expect("body")
        .contains(&bundle.bundle_hash));
}

fn open_client() -> Client {
    Client::tracked(jirani_rust::rocket_with_config(GatewayConfig::open()))
        .expect("valid rocket instance")
}

fn community_envelope() -> SyncEnvelope {
    let payload = SanitizedReportPayload {
        report_type: "livestock or grazing dispute".to_string(),
        general_area: "near river".to_string(),
        time_window: "morning".to_string(),
        submitted_at_epoch_seconds: 1_800_000_000,
        observed_risk: "Cattle crossed the grazing boundary this morning.".to_string(),
        verification_status: "PendingVerification".to_string(),
        sensitivity: "Community".to_string(),
    };

    SyncEnvelope {
        envelope_id: "env-demo-community".to_string(),
        record_type: "SafetyReportRecord".to_string(),
        record_id: "report-demo-community".to_string(),
        content_hash: payload.content_hash(),
        version: 1,
        last_modified_bucket: "day-20833".to_string(),
        audience_tier: "TrustedVerifier".to_string(),
        expires_at_epoch_seconds: 1_900_000_000,
        payload,
    }
}

fn relay_bundle() -> RelayBundle {
    let public_header = RelayPublicHeader {
        alert_type: "ResourceDispute".to_string(),
        general_area: "near river".to_string(),
        time_window: "morning".to_string(),
        risk_level: "Elevated".to_string(),
        message: "Cattle movement reported near shared grazing boundary.".to_string(),
        verification_status: "PendingVerification".to_string(),
        audience_tier: "TrustedVerifier".to_string(),
        sensitivity: "Community".to_string(),
    };
    let mut bundle = RelayBundle {
        bundle_id: "bundle-demo-community".to_string(),
        public_header,
        encrypted_payload: "encrypted-demo-payload".to_string(),
        payload_hash: String::new(),
        bundle_hash: String::new(),
        expires_at_epoch_seconds: 1_900_000_000,
    };
    bundle.payload_hash = bundle.computed_payload_hash();
    bundle.bundle_hash = bundle.computed_bundle_hash();
    bundle
}
