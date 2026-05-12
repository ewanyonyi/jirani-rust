use rocket::serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct SyncEnvelope {
    pub envelope_id: String,
    pub record_type: String,
    pub record_id: String,
    pub content_hash: String,
    pub version: u32,
    pub last_modified_bucket: String,
    pub audience_tier: String,
    pub expires_at_epoch_seconds: i64,
    pub payload: SanitizedReportPayload,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct SanitizedReportPayload {
    pub report_type: String,
    pub general_area: String,
    pub time_window: String,
    pub submitted_at_epoch_seconds: i64,
    pub observed_risk: String,
    pub verification_status: String,
    pub sensitivity: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct EnvelopeList {
    pub envelopes: Vec<SyncEnvelope>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct RelayBundle {
    pub bundle_id: String,
    pub public_header: RelayPublicHeader,
    pub encrypted_payload: String,
    pub payload_hash: String,
    pub bundle_hash: String,
    pub expires_at_epoch_seconds: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct RelayPublicHeader {
    pub alert_type: String,
    pub general_area: String,
    pub time_window: String,
    pub risk_level: String,
    pub message: String,
    pub verification_status: String,
    pub audience_tier: String,
    pub sensitivity: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct RelayBundleList {
    pub bundles: Vec<RelayBundle>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct RelayPublicKeyResponse {
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct ApiMessage {
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub stores_network_identity: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct PrivacyResponse {
    pub direct_ip_visibility: &'static str,
    pub stored_network_identity: bool,
    pub stored_device_identity: bool,
    pub stored_precise_location: bool,
    pub payload_policy: &'static str,
    pub hosted_recommendation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct AnonymousSummary {
    pub total_envelopes: usize,
    pub by_sensitivity: Vec<SummaryCount>,
    pub by_verification_status: Vec<SummaryCount>,
    pub top_areas: Vec<AreaSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct SummaryCount {
    pub key: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(crate = "rocket::serde", rename_all = "camelCase")]
pub struct AreaSummary {
    pub general_area: String,
    pub count: usize,
}

impl SyncEnvelope {
    pub fn validate_for_gateway(&self, now_epoch_seconds: i64) -> Result<(), String> {
        if self.envelope_id.trim().is_empty() || self.record_id.trim().is_empty() {
            return Err("Envelope and record IDs are required.".to_string());
        }
        if self.expires_at_epoch_seconds <= now_epoch_seconds {
            return Err("Expired envelopes are not accepted.".to_string());
        }
        if self
            .payload
            .sensitivity
            .eq_ignore_ascii_case("SurvivorCentered")
            || self
                .audience_tier
                .eq_ignore_ascii_case("SurvivorSupportOnly")
        {
            return Err(
                "Survivor-centered reports are not accepted by the default gateway.".to_string(),
            );
        }
        if self.contains_obvious_pii() {
            return Err(
                "Envelope appears to contain personal identifying information.".to_string(),
            );
        }
        if self.content_hash != self.payload.content_hash() {
            return Err("Content hash does not match sanitized payload.".to_string());
        }
        Ok(())
    }

    fn contains_obvious_pii(&self) -> bool {
        let combined = format!(
            "{} {} {}",
            self.payload.general_area, self.payload.observed_risk, self.payload.report_type
        );
        contains_phone_like_value(&combined) || contains_exact_home_hint(&combined)
    }
}

impl SanitizedReportPayload {
    pub fn content_hash(&self) -> String {
        let content = [
            self.report_type.as_str(),
            self.general_area.as_str(),
            self.time_window.as_str(),
            &self.submitted_at_epoch_seconds.to_string(),
            self.observed_risk.as_str(),
            self.verification_status.as_str(),
            self.sensitivity.as_str(),
        ]
        .join("|");
        let digest = Sha256::digest(content.as_bytes());
        hex::encode(digest)
    }
}

impl RelayBundle {
    pub fn validate_for_gateway(&self, now_epoch_seconds: i64) -> Result<(), String> {
        if self.bundle_id.trim().is_empty() {
            return Err("Bundle ID is required.".to_string());
        }
        if self.expires_at_epoch_seconds <= now_epoch_seconds {
            return Err("Expired relay bundles are not accepted.".to_string());
        }
        if self.encrypted_payload.trim().is_empty() {
            return Err("Encrypted payload is required.".to_string());
        }
        if self.public_header.is_survivor_centered() {
            return Err(
                "Survivor-centered reports are not accepted by the default relay.".to_string(),
            );
        }
        if self.public_header.contains_obvious_pii() {
            return Err(
                "Relay public header appears to contain personal identifying information."
                    .to_string(),
            );
        }
        if self.payload_hash != self.computed_payload_hash() {
            return Err("Payload hash does not match encrypted payload.".to_string());
        }
        if self.bundle_hash != self.computed_bundle_hash() {
            return Err("Bundle hash does not match public header and payload hash.".to_string());
        }
        Ok(())
    }

    pub fn computed_payload_hash(&self) -> String {
        sha256_hex(self.encrypted_payload.as_bytes())
    }

    pub fn computed_bundle_hash(&self) -> String {
        let content = [
            self.public_header.canonical_content(),
            self.payload_hash.clone(),
        ]
        .join("|");
        sha256_hex(content.as_bytes())
    }
}

impl RelayPublicHeader {
    fn canonical_content(&self) -> String {
        [
            self.alert_type.as_str(),
            self.general_area.as_str(),
            self.time_window.as_str(),
            self.risk_level.as_str(),
            self.message.as_str(),
            self.verification_status.as_str(),
            self.audience_tier.as_str(),
            self.sensitivity.as_str(),
        ]
        .join("|")
    }

    fn is_survivor_centered(&self) -> bool {
        self.sensitivity.eq_ignore_ascii_case("SurvivorCentered")
            || self
                .audience_tier
                .eq_ignore_ascii_case("SurvivorSupportOnly")
            || self.alert_type.eq_ignore_ascii_case("SurvivorCentered")
    }

    fn contains_obvious_pii(&self) -> bool {
        let combined = format!(
            "{} {} {} {}",
            self.alert_type, self.general_area, self.message, self.risk_level
        );
        contains_phone_like_value(&combined) || contains_exact_home_hint(&combined)
    }
}

fn contains_phone_like_value(value: &str) -> bool {
    let digits = value.chars().filter(|ch| ch.is_ascii_digit()).count();
    digits >= 7
}

fn contains_exact_home_hint(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ["house ", "home ", "plot ", "room "]
        .iter()
        .any(|term| lower.contains(term))
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    hex::encode(digest)
}
