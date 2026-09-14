//! Privacy-preserving access to the official Palworld dedicated-server REST API.
//!
//! Raw server and player identities are deliberately opaque. Callers can only
//! serialize values after they have crossed the keyed pseudonymization boundary.

mod client;
mod error;
mod live;
mod metrics;
mod privacy;
mod settings;
mod wire;

pub use client::{
    BasicAuthSecret, ClientConfig, ConnectedStreamVerifier, EndpointPolicy, PalRestClient,
    TimedResponse,
};
pub use error::{EndpointKind, RestError};
pub use live::{
    LiveSnapshotRequest, SERVER_LIVE_SNAPSHOT_SCHEMA_V1, ServerLiveGameDataV1, ServerLiveInfoV1,
    ServerLiveMetricsV1, ServerLivePlayerV1, ServerLiveSectionStatusV1, ServerLiveSectionV1,
    ServerLiveSettingsKnownV1, ServerLiveSettingsV1, ServerLiveSnapshotOptionsV1,
    ServerLiveSnapshotV1, ServerLiveStateV1,
};
pub use metrics::ServerMetrics;
pub use privacy::{
    PlayerSelector, Pseudonymizer, SafePlayerObservation, SanitizedServerInfo,
    SelectedPlayerObservation, sanitize_player, sanitize_server_info,
};
pub use settings::ServerSettings;
pub use wire::{
    decode_info, decode_live_game_data, decode_live_info, decode_live_metrics, decode_live_players,
    decode_live_settings, decode_metrics, decode_selected_player, decode_settings,
};
