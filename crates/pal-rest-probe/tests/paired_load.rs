use std::collections::VecDeque;

use pal_rest_probe::{
    DistributionSummary, LoadProbePlan, LoadProbeSource, LoadWindowMeasurement, LoadWindowRequest,
    LoadWindowRole, PairOrder, ProbeRunError, ProbeRunner, SafetyAbortReason, WindowMetrics,
};

fn summary(value: f64, count: u64) -> DistributionSummary {
    DistributionSummary::from_constant(value, count).expect("finite distribution")
}

fn metrics() -> WindowMetrics {
    WindowMetrics {
        duration_ms: 100,
        request_attempts: 30,
        request_successes: 30,
        selected_player_samples: 30,
        response_latency_ms: summary(20.0, 30),
        decode_latency_ms: summary(2.0, 30),
        decoded_body_bytes: summary(64_000.0, 30),
        actor_count: summary(100.0, 30),
        server_fps: summary(60.0, 30),
        frame_time_ms: summary(16.0, 30),
        probe_cpu_percent: summary(0.4, 30),
        probe_private_bytes_peak: 20 * 1024 * 1024,
    }
}

#[derive(Default)]
struct RecordingSource {
    requests: Vec<LoadWindowRequest>,
    cooldowns: Vec<u64>,
    results: VecDeque<LoadWindowMeasurement>,
}

impl LoadProbeSource for RecordingSource {
    fn measure_window(
        &mut self,
        request: LoadWindowRequest,
    ) -> Result<LoadWindowMeasurement, ProbeRunError> {
        self.requests.push(request);
        Ok(self.results.pop_front().unwrap_or(LoadWindowMeasurement {
            metrics: metrics(),
            safety_abort: None,
        }))
    }

    fn cooldown(&mut self, duration_ms: u64) -> Result<(), ProbeRunError> {
        self.cooldowns.push(duration_ms);
        Ok(())
    }
}

#[test]
fn executes_five_equal_duration_pairs_in_interleaved_ab_ba_order() {
    let plan = LoadProbePlan {
        candidate_intervals_ms: vec![2_000, 1_000, 500],
        pair_count: 5,
        window_duration_ms: 100,
        cooldown_ms: 25,
    };
    let mut source = RecordingSource::default();

    let run = ProbeRunner::run_load(&plan, &mut source).expect("load run");

    assert_eq!(run.candidates.len(), 3);
    assert_eq!(source.requests.len(), 30);
    assert_eq!(source.cooldowns, vec![25; 15]);
    assert_eq!(source.requests[0].order, PairOrder::BaselineThenCandidate);
    assert_eq!(source.requests[0].role, LoadWindowRole::Baseline);
    assert_eq!(source.requests[1].role, LoadWindowRole::Candidate);
    assert_eq!(source.requests[2].order, PairOrder::CandidateThenBaseline);
    assert_eq!(source.requests[2].role, LoadWindowRole::Candidate);
    assert_eq!(source.requests[3].role, LoadWindowRole::Baseline);
    assert!(
        source
            .requests
            .iter()
            .all(|request| request.duration_ms == 100)
    );
    assert!(
        run.candidates
            .iter()
            .all(|candidate| candidate.pairs.len() == 5)
    );
}

#[test]
fn safety_abort_stops_the_current_pair_and_all_faster_candidates() {
    let plan = LoadProbePlan {
        candidate_intervals_ms: vec![2_000, 1_000, 500],
        pair_count: 5,
        window_duration_ms: 100,
        cooldown_ms: 25,
    };
    let mut source = RecordingSource {
        results: VecDeque::from(vec![
            LoadWindowMeasurement {
                metrics: metrics(),
                safety_abort: None,
            },
            LoadWindowMeasurement {
                metrics: metrics(),
                safety_abort: None,
            },
            LoadWindowMeasurement {
                metrics: metrics(),
                safety_abort: Some(SafetyAbortReason::ServerFpsFloor),
            },
        ]),
        ..RecordingSource::default()
    };

    let run = ProbeRunner::run_load(&plan, &mut source).expect("aborted run is evidence");

    assert_eq!(source.requests.len(), 3);
    assert_eq!(run.candidates.len(), 1);
    assert_eq!(run.candidates[0].pairs.len(), 1);
    assert_eq!(
        run.candidates[0].safety_abort,
        Some(SafetyAbortReason::ServerFpsFloor)
    );
}

#[test]
fn invalid_plan_is_rejected_before_the_source_is_touched() {
    let plan = LoadProbePlan {
        candidate_intervals_ms: vec![1_000],
        pair_count: 0,
        window_duration_ms: 100,
        cooldown_ms: 25,
    };
    let mut source = RecordingSource::default();

    assert_eq!(
        ProbeRunner::run_load(&plan, &mut source),
        Err(ProbeRunError::InvalidPlan)
    );
    assert!(source.requests.is_empty());
}
