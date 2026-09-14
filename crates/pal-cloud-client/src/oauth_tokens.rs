use std::time::{Duration, SystemTime};

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::AccessAuthError;

pub const MAX_TOKEN_BYTES: usize = 8_192;
pub const MAX_ACCESS_TOKEN_LIFETIME_SECS: u64 = 900;
pub const REFRESH_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct OAuthTokenResponse {
    pub token_type: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

pub trait TokenClock {
    fn now(&self) -> SystemTime;
}

pub struct OAuthTokenSet {
    access_token: SecretString,
    refresh_token: SecretString,
    expires_at: SystemTime,
}

impl OAuthTokenSet {
    pub fn from_response(
        response: OAuthTokenResponse,
        received_at: SystemTime,
    ) -> Result<Self, AccessAuthError> {
        if !response.token_type.eq_ignore_ascii_case("bearer")
            || !valid_token(&response.access_token)
            || !valid_token(&response.refresh_token)
            || !(1..=MAX_ACCESS_TOKEN_LIFETIME_SECS).contains(&response.expires_in)
        {
            return Err(AccessAuthError::InvalidTokenResponse);
        }
        let expires_at = received_at
            .checked_add(Duration::from_secs(response.expires_in))
            .ok_or(AccessAuthError::InvalidTokenResponse)?;
        Ok(Self {
            access_token: SecretString::from(response.access_token),
            refresh_token: SecretString::from(response.refresh_token),
            expires_at,
        })
    }

    pub fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    pub fn needs_refresh(&self, now: SystemTime) -> bool {
        self.expires_at
            .duration_since(now)
            .map_or(true, |remaining| remaining <= REFRESH_WINDOW)
    }

    pub(crate) fn access_token(&self) -> &str {
        self.access_token.expose_secret()
    }

    pub(crate) fn refresh_token(&self) -> &str {
        self.refresh_token.expose_secret()
    }
}

impl std::fmt::Debug for OAuthTokenSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OAuthTokenSet")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> OAuthTokenResponse {
        OAuthTokenResponse {
            token_type: "Bearer".to_owned(),
            access_token: "access.secret.marker".to_owned(),
            refresh_token: "refresh.secret.marker".to_owned(),
            expires_in: 900,
        }
    }

    #[test]
    fn opaque_tokens_are_bounded_and_expiry_uses_only_expires_in() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let tokens = OAuthTokenSet::from_response(response(), now).unwrap();
        assert_eq!(
            tokens.expires_at(),
            now + Duration::from_secs(MAX_ACCESS_TOKEN_LIFETIME_SECS)
        );
        assert!(!tokens.needs_refresh(now + Duration::from_secs(839)));
        assert!(tokens.needs_refresh(now + Duration::from_secs(840)));
        assert_eq!(tokens.access_token(), "access.secret.marker");
    }

    #[test]
    fn debug_never_contains_access_or_refresh_tokens() {
        let tokens = OAuthTokenSet::from_response(response(), SystemTime::UNIX_EPOCH).unwrap();
        let rendered = format!("{tokens:?}");
        assert!(!rendered.contains("access.secret.marker"));
        assert!(!rendered.contains("refresh.secret.marker"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn rejects_unbounded_lifetime_missing_refresh_or_control_bytes() {
        let mut invalid = response();
        invalid.expires_in = 901;
        assert!(matches!(
            OAuthTokenSet::from_response(invalid, SystemTime::UNIX_EPOCH),
            Err(AccessAuthError::InvalidTokenResponse)
        ));
        let mut invalid = response();
        invalid.refresh_token.clear();
        assert!(matches!(
            OAuthTokenSet::from_response(invalid, SystemTime::UNIX_EPOCH),
            Err(AccessAuthError::InvalidTokenResponse)
        ));
        let mut invalid = response();
        invalid.access_token.push('\n');
        assert!(matches!(
            OAuthTokenSet::from_response(invalid, SystemTime::UNIX_EPOCH),
            Err(AccessAuthError::InvalidTokenResponse)
        ));
    }
}
