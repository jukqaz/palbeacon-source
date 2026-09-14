//! Fail-closed dedicated-server position agent.

mod config;
mod gate_profile;
mod health;
mod live_process;
mod metrics;
mod pipeline;
mod poller;
mod single_instance;

pub use config::{
    AgentCommand, AgentConfig, ConfigError, RestTransport, SecretBundle, SelectorKind,
    load_protected_config, parse_command, parse_config, parse_verification_key,
};
pub use gate_profile::{
    ApprovedGate, AttestedGateProfile, GateProfileError, GateProfilePayload, PreverifiedGate,
    StartupGateConfig, bind_preverified_gate, build_agent_descriptor, canonical_profile_sha256,
    enforce_startup_gate, finalize_remote_startup_gate, finalize_startup_gate, load_gate_profile,
    parse_gate_profile, preverify_startup_gate,
};
pub use health::{HealthErrorKind, HealthMonitor, HealthSnapshot};
pub use live_process::{LiveProcessError, LiveServerIdentity, inspect_live_server};
pub use metrics::{AgentMetrics, AgentMetricsSnapshot};
pub use pal_protected_file::{
    ProtectedFileError, read_protected_file, read_regular_file_no_follow,
};
pub use pipeline::{AgentPipeline, PipelineError};
pub use poller::{
    GameDataSource, Poller, PollerBuildError, PollerExit, RestGameDataSource, SourceTrustError,
};
pub use single_instance::{SingleInstanceError, SingleInstanceGuard};
