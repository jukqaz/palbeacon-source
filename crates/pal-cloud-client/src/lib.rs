//! Native Cloudflare Access and private API client primitives.
//!
//! This crate is the only Windows-side owner of OAuth credentials. The management UI,
//! the overlay, local IPC payloads, SQLite and logs must never receive tokens,
//! authorization codes, PKCE verifiers or callback state.

#![deny(unsafe_op_in_unsafe_fn)]

mod access_auth;
mod cloud_error;
mod http_client;
mod loopback_callback;
mod oauth_discovery;
mod oauth_metadata;
mod oauth_registration;
mod oauth_tokens;
mod pkce;
mod system_browser;

pub use access_auth::{AccessAuthManager, CloudSessionState, LoginInteraction, ManagedOAuthPolicy};
pub use cloud_error::{AccessAuthError, CloudError, CloudErrorCode};
pub use http_client::{AccessSessionLease, AuthorizationCodeGrant, PalCloudClient};
pub use loopback_callback::LoopbackCallback;
pub use oauth_discovery::DiscoveryChallenge;
pub use oauth_metadata::{
    AuthorizationServerMetadata, ProtectedResourceMetadata, ValidatedAuthorizationServer,
};
pub use oauth_registration::{DynamicClientRegistration, RegisteredPublicClient};
pub use oauth_tokens::{OAuthTokenResponse, OAuthTokenSet, TokenClock};
pub use pkce::{PkceMaterial, RandomMaterial};
pub use system_browser::{BrowserLauncher, SystemBrowser};
