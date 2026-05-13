use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use sha2::{Digest, Sha256};
use std::env;

const DASHBOARD_COOKIE: &str = "jirani_dashboard_session";
const DEFAULT_HASH_ITERATIONS: u32 = 120_000;
const SESSION_TTL_SECONDS: i64 = 8 * 60 * 60;

#[derive(Debug, Clone, Default)]
pub struct GatewayConfig {
    access_token: Option<String>,
    relay_public_key: Option<String>,
    dashboard_users: Vec<DashboardUser>,
    dashboard_session_secret: String,
}

#[derive(Debug, Clone)]
struct DashboardUser {
    username: String,
    password_hash: PasswordHash,
}

#[derive(Debug, Clone)]
struct PasswordHash {
    iterations: u32,
    salt_hex: String,
    digest_hex: String,
}

impl GatewayConfig {
    pub fn from_env() -> Self {
        let access_token = env::var("JIRANI_GATEWAY_TOKEN")
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        let dashboard_session_secret = env::var("JIRANI_SESSION_SECRET")
            .ok()
            .map(|secret| secret.trim().to_string())
            .filter(|secret| !secret.is_empty())
            .or_else(|| access_token.clone())
            .unwrap_or_else(|| {
                format!(
                    "local-dashboard-session-secret:{}:{}",
                    now_epoch_seconds(),
                    std::process::id()
                )
            });

        Self {
            access_token,
            relay_public_key: env::var("JIRANI_RELAY_PUBLIC_KEY")
                .ok()
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty()),
            dashboard_users: env::var("JIRANI_DASHBOARD_USERS")
                .ok()
                .map(|users| parse_dashboard_users(&users))
                .unwrap_or_default(),
            dashboard_session_secret,
        }
    }

    #[allow(dead_code)]
    pub fn open() -> Self {
        Self {
            access_token: None,
            relay_public_key: None,
            dashboard_users: Vec::new(),
            dashboard_session_secret: "local-dashboard-session-secret".to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            access_token: Some(token.into()),
            relay_public_key: None,
            dashboard_users: Vec::new(),
            dashboard_session_secret: "local-dashboard-session-secret".to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn with_relay_public_key(mut self, public_key: impl Into<String>) -> Self {
        self.relay_public_key = Some(public_key.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_dashboard_user(mut self, username: impl Into<String>, password: &str) -> Self {
        let username = username.into();
        let salt_hex = format!(
            "{:x}",
            Sha256::digest(format!("{username}:jirani-dashboard-test-salt").as_bytes())
        );
        self.dashboard_users.push(DashboardUser {
            username,
            password_hash: hash_dashboard_password(password, &salt_hex, DEFAULT_HASH_ITERATIONS),
        });
        self
    }

    #[allow(dead_code)]
    pub fn with_dashboard_session_secret(mut self, secret: impl Into<String>) -> Self {
        self.dashboard_session_secret = secret.into();
        self
    }

    pub fn auth_enabled(&self) -> bool {
        self.access_token.is_some()
    }

    pub fn dashboard_auth_enabled(&self) -> bool {
        !self.dashboard_users.is_empty()
    }

    #[allow(dead_code)]
    pub fn accepts_token(&self, token: Option<&str>) -> bool {
        let Some(expected) = self.access_token.as_deref() else {
            return true;
        };
        token.is_some_and(|token| token == expected)
    }

    pub fn relay_public_key(&self) -> Option<&str> {
        self.relay_public_key.as_deref()
    }

    fn accepts(&self, request: &Request<'_>) -> bool {
        let Some(expected) = self.access_token.as_deref() else {
            return true;
        };

        request
            .headers()
            .get_one("Authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|token| token == expected)
            || request
                .query_value::<&str>("token")
                .and_then(Result::ok)
                .is_some_and(|token| token == expected)
    }

    pub fn authenticate_dashboard_user(&self, username: &str, password: &str) -> bool {
        self.dashboard_users
            .iter()
            .find(|user| user.username == username)
            .is_some_and(|user| user.password_hash.verify(password))
    }

    pub fn dashboard_session_cookie_name(&self) -> &'static str {
        DASHBOARD_COOKIE
    }

    pub fn issue_dashboard_session(&self, username: &str, now_epoch_seconds: i64) -> String {
        let expires_at = now_epoch_seconds + SESSION_TTL_SECONDS;
        let signature = self.sign_dashboard_session(username, expires_at);
        format!("v1.{username}.{expires_at}.{signature}")
    }

    pub fn accepts_dashboard_session_cookie(
        &self,
        session: Option<&str>,
        now_epoch_seconds: i64,
    ) -> bool {
        self.accepts_dashboard_session(session, now_epoch_seconds)
    }

    fn accepts_dashboard_session(&self, session: Option<&str>, now_epoch_seconds: i64) -> bool {
        let Some(session) = session else {
            return false;
        };
        let mut parts = session.split('.');
        let Some("v1") = parts.next() else {
            return false;
        };
        let Some(username) = parts.next() else {
            return false;
        };
        let Some(expires_at) = parts.next().and_then(|value| value.parse::<i64>().ok()) else {
            return false;
        };
        let Some(signature) = parts.next() else {
            return false;
        };
        if parts.next().is_some() || expires_at <= now_epoch_seconds {
            return false;
        }
        if !self
            .dashboard_users
            .iter()
            .any(|user| user.username == username)
        {
            return false;
        }

        constant_time_eq(
            signature.as_bytes(),
            self.sign_dashboard_session(username, expires_at).as_bytes(),
        )
    }

    fn sign_dashboard_session(&self, username: &str, expires_at: i64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.dashboard_session_secret.as_bytes());
        hasher.update(b":jirani-dashboard-session:");
        hasher.update(username.as_bytes());
        hasher.update(b":");
        hasher.update(expires_at.to_string().as_bytes());
        hex::encode(hasher.finalize())
    }
}

pub struct GatewayAuth;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GatewayAuth {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(config) = request.rocket().state::<GatewayConfig>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };

        if config.accepts(request) {
            Outcome::Success(GatewayAuth)
        } else {
            Outcome::Error((Status::Unauthorized, ()))
        }
    }
}

pub struct DashboardAuth;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for DashboardAuth {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(config) = request.rocket().state::<GatewayConfig>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };

        let session = request
            .cookies()
            .get(DASHBOARD_COOKIE)
            .map(|cookie| cookie.value());
        if config.accepts_dashboard_session(session, now_epoch_seconds()) {
            Outcome::Success(DashboardAuth)
        } else {
            Outcome::Error((Status::Unauthorized, ()))
        }
    }
}

pub fn dashboard_password_hash_for_config(password: &str, salt_seed: &str) -> String {
    let salt_hex = hex::encode(Sha256::digest(salt_seed.as_bytes()));
    hash_dashboard_password(password, &salt_hex, DEFAULT_HASH_ITERATIONS).to_string()
}

fn hash_dashboard_password(password: &str, salt_hex: &str, iterations: u32) -> PasswordHash {
    let iterations = iterations.max(1);
    let mut digest = Vec::new();
    digest.extend_from_slice(salt_hex.as_bytes());
    digest.extend_from_slice(password.as_bytes());
    let mut digest = Sha256::digest(&digest).to_vec();
    for _ in 1..iterations {
        digest = Sha256::digest(&digest).to_vec();
    }

    PasswordHash {
        iterations,
        salt_hex: salt_hex.to_string(),
        digest_hex: hex::encode(digest),
    }
}

fn parse_dashboard_users(value: &str) -> Vec<DashboardUser> {
    value
        .split(',')
        .filter_map(|entry| {
            let (username, password_hash) = entry.trim().split_once(':')?;
            let username = username.trim();
            if !is_valid_dashboard_username(username) {
                return None;
            }
            PasswordHash::parse(password_hash.trim()).map(|password_hash| DashboardUser {
                username: username.to_string(),
                password_hash,
            })
        })
        .collect()
}

fn is_valid_dashboard_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 64
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '@'))
}

fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

impl PasswordHash {
    fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('$');
        let Some("sha256") = parts.next() else {
            return None;
        };
        let iterations = parts.next()?.parse::<u32>().ok()?;
        let salt_hex = parts.next()?.to_string();
        let digest_hex = parts.next()?.to_string();
        if parts.next().is_some() || salt_hex.is_empty() || digest_hex.is_empty() {
            return None;
        }
        Some(Self {
            iterations,
            salt_hex,
            digest_hex,
        })
    }

    fn verify(&self, password: &str) -> bool {
        let computed = hash_dashboard_password(password, &self.salt_hex, self.iterations);
        constant_time_eq(computed.digest_hex.as_bytes(), self.digest_hex.as_bytes())
    }
}

impl std::fmt::Display for PasswordHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "sha256${}${}${}",
            self.iterations, self.salt_hex, self.digest_hex
        )
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right.iter())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}
