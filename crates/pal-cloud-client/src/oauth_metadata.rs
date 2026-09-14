use reqwest::Url;
use serde::Deserialize;

use crate::{AccessAuthError, ManagedOAuthPolicy};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: String,
    pub grant_types_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedAuthorizationServer {
    pub issuer: Url,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub registration_endpoint: Url,
}

impl ProtectedResourceMetadata {
    pub fn validate(&self, policy: &ManagedOAuthPolicy) -> Result<Url, AccessAuthError> {
        policy.validate()?;
        let resource = Url::parse(&self.resource).map_err(|_| AccessAuthError::InvalidMetadata)?;
        if resource != policy.resource || self.authorization_servers.len() != 1 {
            return Err(AccessAuthError::InvalidMetadata);
        }
        let issuer = Url::parse(&self.authorization_servers[0])
            .map_err(|_| AccessAuthError::InvalidMetadata)?;
        if !policy.permits_authorization_server(&issuer)? {
            return Err(AccessAuthError::InvalidMetadata);
        }
        Ok(issuer)
    }
}

impl AuthorizationServerMetadata {
    pub fn validate(
        &self,
        expected_issuer: &Url,
        policy: &ManagedOAuthPolicy,
    ) -> Result<ValidatedAuthorizationServer, AccessAuthError> {
        let issuer = Url::parse(&self.issuer).map_err(|_| AccessAuthError::InvalidMetadata)?;
        if &issuer != expected_issuer || !policy.permits_authorization_server(&issuer)? {
            return Err(AccessAuthError::InvalidMetadata);
        }
        require_value(&self.grant_types_supported, "authorization_code")?;
        require_value(&self.grant_types_supported, "refresh_token")?;
        require_value(&self.response_types_supported, "code")?;
        require_value(&self.code_challenge_methods_supported, "S256")?;
        require_value(&self.token_endpoint_auth_methods_supported, "none")?;

        Ok(ValidatedAuthorizationServer {
            authorization_endpoint: validate_endpoint(&self.authorization_endpoint, &issuer)?,
            token_endpoint: validate_endpoint(&self.token_endpoint, &issuer)?,
            registration_endpoint: validate_endpoint(&self.registration_endpoint, &issuer)?,
            issuer,
        })
    }
}

fn require_value(values: &[String], expected: &str) -> Result<(), AccessAuthError> {
    if values.iter().any(|value| value == expected) {
        Ok(())
    } else {
        Err(AccessAuthError::InvalidMetadata)
    }
}

fn validate_endpoint(value: &str, issuer: &Url) -> Result<Url, AccessAuthError> {
    let endpoint = Url::parse(value).map_err(|_| AccessAuthError::InvalidMetadata)?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.origin() != issuer.origin()
    {
        return Err(AccessAuthError::InvalidMetadata);
    }
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn policy() -> ManagedOAuthPolicy {
        ManagedOAuthPolicy::new(
            Url::parse("https://pal.example.com/").unwrap(),
            BTreeSet::from(["https://team.cloudflareaccess.com/".to_owned()]),
        )
        .unwrap()
    }

    fn server_metadata() -> AuthorizationServerMetadata {
        AuthorizationServerMetadata {
            issuer: "https://team.cloudflareaccess.com/".to_owned(),
            authorization_endpoint: "https://team.cloudflareaccess.com/cdn-cgi/access/authorize"
                .to_owned(),
            token_endpoint: "https://team.cloudflareaccess.com/oauth2/token".to_owned(),
            registration_endpoint: "https://team.cloudflareaccess.com/oauth2/register".to_owned(),
            grant_types_supported: vec![
                "authorization_code".to_owned(),
                "refresh_token".to_owned(),
            ],
            response_types_supported: vec!["code".to_owned()],
            code_challenge_methods_supported: vec!["S256".to_owned()],
            token_endpoint_auth_methods_supported: vec!["none".to_owned()],
        }
    }

    #[test]
    fn validates_exact_resource_and_one_allowlisted_issuer() {
        let policy = policy();
        let metadata = ProtectedResourceMetadata {
            resource: policy.resource.to_string(),
            authorization_servers: vec!["https://team.cloudflareaccess.com/".to_owned()],
        };
        assert_eq!(
            metadata.validate(&policy).unwrap().as_str(),
            "https://team.cloudflareaccess.com/"
        );
    }

    #[test]
    fn rejects_alternate_resource_or_multiple_issuers() {
        let policy = policy();
        let mut metadata = ProtectedResourceMetadata {
            resource: "https://evil.example/".to_owned(),
            authorization_servers: vec!["https://team.cloudflareaccess.com/".to_owned()],
        };
        assert_eq!(
            metadata.validate(&policy),
            Err(AccessAuthError::InvalidMetadata)
        );
        metadata.resource = policy.resource.to_string();
        metadata
            .authorization_servers
            .push("https://other.cloudflareaccess.com/".to_owned());
        assert_eq!(
            metadata.validate(&policy),
            Err(AccessAuthError::InvalidMetadata)
        );
    }

    #[test]
    fn requires_public_client_refresh_and_pkce_capabilities() {
        let policy = policy();
        let issuer = Url::parse("https://team.cloudflareaccess.com/").unwrap();
        let validated = server_metadata().validate(&issuer, &policy).unwrap();
        assert_eq!(
            validated.authorization_endpoint.path(),
            "/cdn-cgi/access/authorize"
        );

        let mut missing = server_metadata();
        missing.code_challenge_methods_supported.clear();
        assert_eq!(
            missing.validate(&issuer, &policy),
            Err(AccessAuthError::InvalidMetadata)
        );
    }

    #[test]
    fn rejects_cross_origin_or_fragmented_endpoints() {
        let policy = policy();
        let issuer = Url::parse("https://team.cloudflareaccess.com/").unwrap();
        let mut metadata = server_metadata();
        metadata.token_endpoint = "https://evil.example/token".to_owned();
        assert_eq!(
            metadata.validate(&issuer, &policy),
            Err(AccessAuthError::InvalidMetadata)
        );
        metadata = server_metadata();
        metadata.registration_endpoint =
            "https://team.cloudflareaccess.com/register#secret".to_owned();
        assert_eq!(
            metadata.validate(&issuer, &policy),
            Err(AccessAuthError::InvalidMetadata)
        );
    }
}
