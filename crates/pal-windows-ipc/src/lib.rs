//! Current-user-only Windows IPC primitives for the local companion control plane.

pub mod client;
pub mod framed_stream;
pub mod handshake;
pub mod security;
pub mod server;
pub mod session;

pub const CORE_PIPE_NAME: &str = r"\\.\pipe\PalBeacon.Core.v2";
pub const CORE_MODE_SAFE_EVENT: &str = r"Local\PalBeacon.Core.Mode.Safe.v2";
pub const CORE_MODE_APPROVED_MAP_PACK_EVENT: &str = r"Local\PalBeacon.Core.Mode.ApprovedMapPack.v2";
pub const CORE_SHUTDOWN_EVENT: &str = r"Local\PalBeacon.Core.Shutdown.v2";
pub const OVERLAY_RUNNING_EVENT: &str = r"Local\PalBeacon.Overlay.Running.v2";
pub const OVERLAY_POSITION_LIVE_EVENT: &str = r"Local\PalBeacon.Overlay.PositionLive.v2";
