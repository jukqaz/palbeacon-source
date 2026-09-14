use std::fmt;

use thiserror::Error;

/// Fixed official REST endpoints. Keeping this closed prevents arbitrary paths
/// from being appended to an authenticated server URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointKind {
    GameData,
    Info,
    Metrics,
    Players,
    Settings,
}

impl EndpointKind {
    pub(crate) const fn path(self) -> &'static str {
        match self {
            Self::GameData => "game-data",
            Self::Info => "info",
            Self::Metrics => "metrics",
            Self::Players => "players",
            Self::Settings => "settings",
        }
    }
}

impl fmt::Display for EndpointKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.path())
    }
}

/// Errors intentionally contain neither response bodies nor credentials.
#[derive(Debug, Error)]
pub enum RestError {
    #[error("player selector is invalid")]
    InvalidSelector,
    #[error("local world alias is invalid")]
    InvalidWorldAlias,
    #[error("selected player was not present in the snapshot")]
    SelectedPlayerMissing,
    #[error("selected player appeared more than once in the snapshot")]
    SelectedPlayerAmbiguous,
    #[error("selected player was not confirmed active in the snapshot")]
    SelectedPlayerInactive,
    #[error("official REST {endpoint} response did not match the expected schema")]
    Decode { endpoint: EndpointKind },
    #[error("REST endpoint URL is invalid")]
    InvalidEndpoint,
    #[error("REST endpoint is outside the explicit LAN policy")]
    EndpointNotAllowed,
    #[error("REST client configuration is invalid")]
    InvalidClientConfig,
    #[error("the established REST connection could not be bound to the trusted server process")]
    ConnectionUntrusted,
    #[error("REST authentication was rejected")]
    Unauthorized,
    #[error("REST redirect was rejected")]
    RedirectRejected,
    #[error("REST response used an unsupported content encoding")]
    CompressionNotAllowed,
    #[error("REST response did not use the required JSON content type")]
    ContentTypeRejected,
    #[error("REST response exceeded the configured body limit")]
    ResponseTooLarge,
    #[error("REST request timed out")]
    Timeout,
    #[error("REST request failed")]
    Transport,
    #[error("REST endpoint returned HTTP status {0}")]
    UnexpectedStatus(u16),
}
