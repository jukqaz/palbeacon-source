use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::AccessAuthError;

pub const MAX_CLIENT_ID_BYTES: usize = 512;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DynamicClientRegistration {
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub client_name: String,
    pub resource: String,
}

impl DynamicClientRegistration {
    pub fn loopback(redirect_uri: &Url, resource: &Url) -> Result<Self, AccessAuthError> {
        if redirect_uri.scheme() != "http"
            || redirect_uri.host_str() != Some("127.0.0.1")
            || redirect_uri.port().is_none()
            || !redirect_uri.username().is_empty()
            || redirect_uri.password().is_some()
            || redirect_uri.query().is_some()
            || redirect_uri.fragment().is_some()
            || !redirect_uri.path().starts_with("/oauth/callback/")
            || resource.scheme() != "https"
            || resource.host_str().is_none()
            || !resource.username().is_empty()
            || resource.password().is_some()
            || resource.query().is_some()
            || resource.fragment().is_some()
            || resource.path() != "/"
        {
            return Err(AccessAuthError::InvalidRegistration);
        }
        Ok(Self {
            redirect_uris: vec![redirect_uri.to_string()],
            token_endpoint_auth_method: "none".to_owned(),
            grant_types: vec!["authorization_code".to_owned(), "refresh_token".to_owned()],
            response_types: vec!["code".to_owned()],
            client_name: "Pal Companion Windows".to_owned(),
            resource: resource.to_string(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RegisteredPublicClient {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

impl RegisteredPublicClient {
    pub fn validate(self) -> Result<Self, AccessAuthError> {
        if self.client_id.is_empty()
            || self.client_id.len() > MAX_CLIENT_ID_BYTES
            || self
                .client_secret
                .as_deref()
                .is_some_and(|secret| !secret.is_empty())
        {
            return Err(AccessAuthError::InvalidRegistration);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_is_an_uncredentialed_loopback_public_client() {
        let redirect = Url::parse("http://127.0.0.1:49152/oauth/callback/fixed-nonce").unwrap();
        let resource = Url::parse("https://pal.example.com/").unwrap();
        let registration = DynamicClientRegistration::loopback(&redirect, &resource).unwrap();
        assert_eq!(registration.redirect_uris, [redirect.to_string()]);
        assert_eq!(registration.token_endpoint_auth_method, "none");
        assert_eq!(
            registration.grant_types,
            ["authorization_code", "refresh_token"]
        );
        assert_eq!(registration.response_types, ["code"]);
        assert_eq!(registration.client_name, "Pal Companion Windows");
        assert_eq!(registration.resource, resource.to_string());
    }

    #[test]
    fn registration_rejects_non_loopback_and_secret_clients() {
        let remote = Url::parse("https://pal.example.com/oauth/callback/x").unwrap();
        let resource = Url::parse("https://pal.example.com/").unwrap();
        assert_eq!(
            DynamicClientRegistration::loopback(&remote, &resource),
            Err(AccessAuthError::InvalidRegistration)
        );
        let wrong_resource = Url::parse("https://pal.example.com/not-an-origin").unwrap();
        assert_eq!(
            DynamicClientRegistration::loopback(
                &Url::parse("http://127.0.0.1:49152/oauth/callback/fixed-nonce").unwrap(),
                &wrong_resource,
            ),
            Err(AccessAuthError::InvalidRegistration)
        );
        assert_eq!(
            RegisteredPublicClient {
                client_id: "client".to_owned(),
                client_secret: Some("must-not-exist".to_owned()),
            }
            .validate(),
            Err(AccessAuthError::InvalidRegistration)
        );
    }
}
