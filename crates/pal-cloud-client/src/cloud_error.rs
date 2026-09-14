use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AccessAuthError {
    #[error("Cloud endpoint policy is invalid")]
    InvalidPolicy,
    #[error("OAuth metadata is invalid")]
    InvalidMetadata,
    #[error("OAuth client registration is invalid")]
    InvalidRegistration,
    #[error("OAuth token response is invalid")]
    InvalidTokenResponse,
    #[error("secure random generation failed")]
    RandomUnavailable,
    #[error("OAuth callback was rejected")]
    CallbackRejected,
    #[error("the system browser could not be opened")]
    BrowserLaunchFailed,
    #[error("Cloud login is required")]
    LoginRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudErrorCode {
    AccessSessionInvalid,
    LoginRequired,
    AuthenticatedForbidden,
    ProfileUnbound,
    OwnerScopeDenied,
    SourceConflict,
    QuotaExceeded,
    DeletionInProgress,
    ProtocolRejected,
    NetworkUnavailable,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("private Cloud request failed: {code:?}")]
pub struct CloudError {
    pub code: CloudErrorCode,
}

impl CloudError {
    pub const fn new(code: CloudErrorCode) -> Self {
        Self { code }
    }
}
