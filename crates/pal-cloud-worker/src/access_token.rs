use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use pal_companion_service::VerifiedAccessIdentity;
use serde::Deserialize;
use thiserror::Error;

const MAX_ASSERTION_BYTES: usize = 16 * 1024;
const CLOCK_SKEW_SECONDS: i64 = 30;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AccessHeader {
    pub alg: String,
    pub kid: String,
    #[serde(default)]
    pub typ: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AccessClaims {
    pub iss: String,
    #[serde(deserialize_with = "deserialize_audience")]
    pub aud: Vec<String>,
    pub sub: String,
    pub email: String,
    pub exp: i64,
    #[serde(default)]
    pub nbf: Option<i64>,
    #[serde(default)]
    pub iat: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedAccessAssertion {
    pub header: AccessHeader,
    pub claims: AccessClaims,
    pub signing_input: Vec<u8>,
    pub signature: Vec<u8>,
}

impl ParsedAccessAssertion {
    /// Parses the untrusted envelope. This never makes an identity trusted;
    /// callers must verify `signature` before calling [`validate_verified`].
    pub fn parse(assertion: &str) -> Result<Self, AccessTokenError> {
        if assertion.is_empty() || assertion.len() > MAX_ASSERTION_BYTES {
            return Err(AccessTokenError::InvalidLength);
        }
        let mut parts = assertion.split('.');
        let header_part = parts.next().ok_or(AccessTokenError::Malformed)?;
        let claims_part = parts.next().ok_or(AccessTokenError::Malformed)?;
        let signature_part = parts.next().ok_or(AccessTokenError::Malformed)?;
        if parts.next().is_some()
            || header_part.is_empty()
            || claims_part.is_empty()
            || signature_part.is_empty()
        {
            return Err(AccessTokenError::Malformed);
        }

        let header: AccessHeader = decode_json(header_part)?;
        if header.alg != "RS256" || header.kid.trim().is_empty() {
            return Err(AccessTokenError::UnsupportedAlgorithm);
        }
        if header
            .typ
            .as_deref()
            .is_some_and(|value| !value.eq_ignore_ascii_case("JWT"))
        {
            return Err(AccessTokenError::UnexpectedType);
        }

        let claims = decode_json(claims_part)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature_part)
            .map_err(|_| AccessTokenError::InvalidBase64)?;

        Ok(Self {
            header,
            claims,
            signing_input: format!("{header_part}.{claims_part}").into_bytes(),
            signature,
        })
    }

    /// Validates claims only after a caller has verified the RS256 signature.
    pub fn validate_verified(
        &self,
        expected_issuer: &str,
        expected_audience: &str,
        now_unix_seconds: i64,
    ) -> Result<VerifiedAccessIdentity, AccessTokenError> {
        if self.claims.iss != expected_issuer {
            return Err(AccessTokenError::IssuerMismatch);
        }
        if !self
            .claims
            .aud
            .iter()
            .any(|audience| audience == expected_audience)
        {
            return Err(AccessTokenError::AudienceMismatch);
        }
        if self.claims.exp.saturating_add(CLOCK_SKEW_SECONDS) < now_unix_seconds {
            return Err(AccessTokenError::Expired);
        }
        if self.claims.nbf.is_some_and(|not_before| {
            not_before > now_unix_seconds.saturating_add(CLOCK_SKEW_SECONDS)
        }) {
            return Err(AccessTokenError::NotYetValid);
        }
        if self.claims.sub.trim().is_empty()
            || self.claims.sub.len() > 512
            || !valid_email(&self.claims.email)
        {
            return Err(AccessTokenError::InvalidIdentity);
        }

        Ok(VerifiedAccessIdentity {
            subject: self.claims.sub.clone(),
            email: self.claims.email.trim().to_ascii_lowercase(),
        })
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AccessTokenError {
    #[error("Access assertion length is invalid")]
    InvalidLength,
    #[error("Access assertion is malformed")]
    Malformed,
    #[error("Access assertion contains invalid base64url")]
    InvalidBase64,
    #[error("Access assertion JSON is invalid")]
    InvalidJson,
    #[error("only RS256 Access assertions are accepted")]
    UnsupportedAlgorithm,
    #[error("unexpected JWT type")]
    UnexpectedType,
    #[error("Access issuer does not match")]
    IssuerMismatch,
    #[error("Access audience does not match")]
    AudienceMismatch,
    #[error("Access assertion expired")]
    Expired,
    #[error("Access assertion is not valid yet")]
    NotYetValid,
    #[error("Access identity is incomplete")]
    InvalidIdentity,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AudienceValue {
    One(String),
    Many(Vec<String>),
}

fn deserialize_audience<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match AudienceValue::deserialize(deserializer)? {
        AudienceValue::One(value) => vec![value],
        AudienceValue::Many(values) => values,
    })
}

fn decode_json<T: for<'de> Deserialize<'de>>(part: &str) -> Result<T, AccessTokenError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| AccessTokenError::InvalidBase64)?;
    serde_json::from_slice(&bytes).map_err(|_| AccessTokenError::InvalidJson)
}

fn valid_email(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 320
        && value.contains('@')
        && !value.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::json;

    fn assertion(header: serde_json::Value, claims: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let signature = URL_SAFE_NO_PAD.encode([1, 2, 3, 4]);
        format!("{header}.{claims}.{signature}")
    }

    fn valid_claims() -> serde_json::Value {
        json!({
            "iss": "https://team.cloudflareaccess.com",
            "aud": ["policy-aud"],
            "sub": "access-subject",
            "email": "PLAYER@example.com",
            "exp": 2_000_000_000_i64,
            "nbf": 1_700_000_000_i64
        })
    }

    #[test]
    fn accepts_only_strict_rs256_envelopes() {
        let token = assertion(
            json!({"alg": "RS256", "kid": "key-1", "typ": "JWT"}),
            valid_claims(),
        );
        let parsed = ParsedAccessAssertion::parse(&token).unwrap();
        assert_eq!(parsed.header.kid, "key-1");
        assert_eq!(parsed.signature, [1, 2, 3, 4]);

        let none = assertion(json!({"alg": "none", "kid": "key-1"}), valid_claims());
        assert_eq!(
            ParsedAccessAssertion::parse(&none),
            Err(AccessTokenError::UnsupportedAlgorithm)
        );
    }

    #[test]
    fn verified_claims_require_exact_issuer_and_audience() {
        let token = assertion(json!({"alg": "RS256", "kid": "key-1"}), valid_claims());
        let parsed = ParsedAccessAssertion::parse(&token).unwrap();
        let identity = parsed
            .validate_verified(
                "https://team.cloudflareaccess.com",
                "policy-aud",
                1_800_000_000,
            )
            .unwrap();
        assert_eq!(identity.email, "player@example.com");

        assert_eq!(
            parsed.validate_verified(
                "https://other.cloudflareaccess.com",
                "policy-aud",
                1_800_000_000
            ),
            Err(AccessTokenError::IssuerMismatch)
        );
        assert_eq!(
            parsed.validate_verified(
                "https://team.cloudflareaccess.com",
                "other-aud",
                1_800_000_000
            ),
            Err(AccessTokenError::AudienceMismatch)
        );
    }

    #[test]
    fn expired_or_future_assertions_are_rejected() {
        let token = assertion(
            json!({"alg": "RS256", "kid": "key-1"}),
            json!({
                "iss": "https://team.cloudflareaccess.com",
                "aud": "policy-aud",
                "sub": "subject",
                "email": "user@example.com",
                "exp": 100,
                "nbf": 10
            }),
        );
        let parsed = ParsedAccessAssertion::parse(&token).unwrap();
        assert_eq!(
            parsed.validate_verified("https://team.cloudflareaccess.com", "policy-aud", 1_000),
            Err(AccessTokenError::Expired)
        );

        let future = assertion(
            json!({"alg": "RS256", "kid": "key-1"}),
            json!({
                "iss": "https://team.cloudflareaccess.com",
                "aud": "policy-aud",
                "sub": "subject",
                "email": "user@example.com",
                "exp": 2_000,
                "nbf": 1_500
            }),
        );
        assert_eq!(
            ParsedAccessAssertion::parse(&future)
                .unwrap()
                .validate_verified("https://team.cloudflareaccess.com", "policy-aud", 1_000),
            Err(AccessTokenError::NotYetValid)
        );
    }

    #[test]
    fn input_is_bounded() {
        assert_eq!(
            ParsedAccessAssertion::parse(&"a".repeat(MAX_ASSERTION_BYTES + 1)),
            Err(AccessTokenError::InvalidLength)
        );
    }
}
