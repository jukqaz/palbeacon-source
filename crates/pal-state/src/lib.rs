//! Deterministic in-memory overlay state and replay position source.

mod position_source;
mod reducer;
mod replay;
mod server_agent;

pub use position_source::{
    ClockInvalidReason, PositionSource, PositionSourceError, PositionSourceEvent,
};
pub use reducer::{Event, ReducerDiagnostics, ReducerError, StateReducer};
pub use replay::{ReplayConfig, ReplayError, ReplayPositionSource};
pub use server_agent::{
    ServerAgentControlOutcome, ServerAgentDiagnostics, ServerAgentFrame, ServerAgentIngestOutcome,
    ServerAgentPositionSource, ServerAgentSender, ServerAgentSourceError, server_agent_channel,
};
