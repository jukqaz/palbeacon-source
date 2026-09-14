use pal_rest::{RestError, SafePlayerObservation, ServerMetrics, TimedResponse};

use crate::{
    EvidenceError, LoadErrorCounts, WindowMetrics,
    stats::{DistributionSummary, StatsError},
};

/// Aggregate-only output of one REST load window.
#[derive(Clone, Debug, PartialEq)]
pub struct RestWindowAggregate {
    pub metrics: WindowMetrics,
    pub errors: LoadErrorCounts,
}

/// Consumes sanitized `pal-rest` responses while retaining no subject, coordinate, body, or time
/// sample. Only numeric measurements needed by Gate A survive `finish`.
#[derive(Debug)]
pub struct RestWindowBuilder {
    duration_ms: u64,
    request_attempts: u64,
    request_successes: u64,
    selected_player_samples: u64,
    response_latency_ms: Vec<f64>,
    decode_latency_ms: Vec<f64>,
    decoded_body_bytes: Vec<f64>,
    actor_count: Vec<f64>,
    server_fps: Vec<f64>,
    frame_time_ms: Vec<f64>,
    probe_cpu_percent: Vec<f64>,
    probe_private_bytes_peak: u64,
    errors: LoadErrorCounts,
}

impl RestWindowBuilder {
    pub fn new(duration_ms: u64) -> Result<Self, EvidenceError> {
        if duration_ms == 0 {
            return Err(EvidenceError::NegativeDuration);
        }
        Ok(Self {
            duration_ms,
            request_attempts: 0,
            request_successes: 0,
            selected_player_samples: 0,
            response_latency_ms: Vec::new(),
            decode_latency_ms: Vec::new(),
            decoded_body_bytes: Vec::new(),
            actor_count: Vec::new(),
            server_fps: Vec::new(),
            frame_time_ms: Vec::new(),
            probe_cpu_percent: Vec::new(),
            probe_private_bytes_peak: 0,
            errors: LoadErrorCounts::default(),
        })
    }

    pub fn record_success(
        &mut self,
        game_data: &TimedResponse<SafePlayerObservation>,
        metrics: &TimedResponse<ServerMetrics>,
        probe_cpu_percent: f64,
        probe_private_bytes: u64,
    ) -> Result<(), EvidenceError> {
        let response_latency_ms = game_data.response_latency.as_secs_f64() * 1_000.0;
        let decode_latency_ms = game_data.decode_latency.as_secs_f64() * 1_000.0;
        let body_bytes = game_data.body_bytes as f64;
        let actor_count = game_data.actor_count as f64;
        let server_fps = metrics.value.server_fps as f64;
        let frame_time_ms = metrics.value.server_frame_time_ms;
        if [
            response_latency_ms,
            decode_latency_ms,
            body_bytes,
            actor_count,
            server_fps,
            frame_time_ms,
            probe_cpu_percent,
        ]
        .into_iter()
        .any(|value| !value.is_finite() || value < 0.0)
        {
            return Err(EvidenceError::NonFinite);
        }

        self.request_attempts = self
            .request_attempts
            .checked_add(1)
            .ok_or(EvidenceError::CountOverflow)?;
        self.request_successes = self
            .request_successes
            .checked_add(1)
            .ok_or(EvidenceError::CountOverflow)?;
        self.selected_player_samples = self
            .selected_player_samples
            .checked_add(1)
            .ok_or(EvidenceError::CountOverflow)?;
        self.response_latency_ms.push(response_latency_ms);
        self.decode_latency_ms.push(decode_latency_ms);
        self.decoded_body_bytes.push(body_bytes);
        self.actor_count.push(actor_count);
        self.server_fps.push(server_fps);
        self.frame_time_ms.push(frame_time_ms);
        self.probe_cpu_percent.push(probe_cpu_percent);
        self.probe_private_bytes_peak = self.probe_private_bytes_peak.max(probe_private_bytes);
        Ok(())
    }

    pub fn record_error(&mut self, error: &RestError) {
        self.request_attempts = self.request_attempts.saturating_add(1);
        match error {
            RestError::Timeout => self.errors.timeouts = self.errors.timeouts.saturating_add(1),
            RestError::Unauthorized => self.errors.auth = self.errors.auth.saturating_add(1),
            RestError::SelectedPlayerMissing => {
                self.errors.selected_player_missing =
                    self.errors.selected_player_missing.saturating_add(1);
            }
            RestError::SelectedPlayerAmbiguous => {
                self.errors.selected_player_ambiguous =
                    self.errors.selected_player_ambiguous.saturating_add(1);
            }
            RestError::SelectedPlayerInactive => {
                self.errors.selected_player_inactive =
                    self.errors.selected_player_inactive.saturating_add(1);
            }
            RestError::Decode { .. }
            | RestError::ContentTypeRejected
            | RestError::CompressionNotAllowed
            | RestError::ResponseTooLarge => {
                self.errors.parse = self.errors.parse.saturating_add(1);
            }
            RestError::InvalidSelector
            | RestError::InvalidWorldAlias
            | RestError::EndpointNotAllowed => {
                self.errors.privacy = self.errors.privacy.saturating_add(1);
            }
            RestError::InvalidEndpoint
            | RestError::InvalidClientConfig
            | RestError::ConnectionUntrusted
            | RestError::RedirectRejected
            | RestError::Transport
            | RestError::UnexpectedStatus(_) => {
                self.errors.transport = self.errors.transport.saturating_add(1);
            }
        }
    }

    pub const fn error_counts(&self) -> LoadErrorCounts {
        self.errors
    }

    pub fn finish(self) -> Result<RestWindowAggregate, EvidenceError> {
        Ok(RestWindowAggregate {
            metrics: WindowMetrics {
                duration_ms: self.duration_ms,
                request_attempts: self.request_attempts,
                request_successes: self.request_successes,
                selected_player_samples: self.selected_player_samples,
                response_latency_ms: aggregate(self.response_latency_ms)?,
                decode_latency_ms: aggregate(self.decode_latency_ms)?,
                decoded_body_bytes: aggregate(self.decoded_body_bytes)?,
                actor_count: aggregate(self.actor_count)?,
                server_fps: aggregate(self.server_fps)?,
                frame_time_ms: aggregate(self.frame_time_ms)?,
                probe_cpu_percent: aggregate(self.probe_cpu_percent)?,
                probe_private_bytes_peak: self.probe_private_bytes_peak,
            },
            errors: self.errors,
        })
    }
}

fn aggregate(samples: Vec<f64>) -> Result<DistributionSummary, EvidenceError> {
    DistributionSummary::from_samples(samples).map_err(map_stats_error)
}

const fn map_stats_error(_: StatsError) -> EvidenceError {
    EvidenceError::Aggregate
}
