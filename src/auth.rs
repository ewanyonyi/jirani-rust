use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use std::env;

#[derive(Debug, Clone, Default)]
pub struct GatewayConfig {
    access_token: Option<String>,
    relay_public_key: Option<String>,
}

impl GatewayConfig {
    pub fn from_env() -> Self {
        Self {
            access_token: env::var("JIRANI_GATEWAY_TOKEN")
                .ok()
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty()),
            relay_public_key: env::var("JIRANI_RELAY_PUBLIC_KEY")
                .ok()
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty()),
        }
    }

    #[allow(dead_code)]
    pub fn open() -> Self {
        Self {
            access_token: None,
            relay_public_key: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            access_token: Some(token.into()),
            relay_public_key: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_relay_public_key(mut self, public_key: impl Into<String>) -> Self {
        self.relay_public_key = Some(public_key.into());
        self
    }

    pub fn auth_enabled(&self) -> bool {
        self.access_token.is_some()
    }

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
}

pub struct GatewayAuth {
    query_token: Option<String>,
}

impl GatewayAuth {
    pub fn link_suffix(&self) -> String {
        token_link_suffix(self.query_token.as_deref())
    }
}

pub fn token_link_suffix(token: Option<&str>) -> String {
    token
        .map(|token| format!("?token={}", token))
        .unwrap_or_default()
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GatewayAuth {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(config) = request.rocket().state::<GatewayConfig>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };

        if config.accepts(request) {
            Outcome::Success(GatewayAuth {
                query_token: request
                    .query_value::<&str>("token")
                    .and_then(Result::ok)
                    .map(str::to_string),
            })
        } else {
            Outcome::Error((Status::Unauthorized, ()))
        }
    }
}
