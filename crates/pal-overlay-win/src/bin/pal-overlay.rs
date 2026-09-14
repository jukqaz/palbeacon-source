use std::error::Error;
use std::ffi::OsString;
#[cfg(any(
    feature = "development-live-performance-diagnostic",
    feature = "development-local-alignment-diagnostic"
))]
use std::io;
#[cfg(feature = "development-local-alignment-diagnostic")]
use std::io::Write as _;
#[cfg(any(
    feature = "test-harness",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
#[cfg(feature = "development-live-performance-diagnostic")]
use std::{cell::Cell, rc::Rc};

#[cfg(feature = "approved-map-pack-runtime")]
use pal_build_contract::CURRENT_BUILD_HAS_LIVE_POSITION_PROFILE;
#[cfg(any(feature = "approved-map-pack-runtime", feature = "test-harness"))]
use pal_fullscreen_bridge::{FrameSearchEntry, FrameSearchKind};
#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
use pal_overlay_win::actual_map_preview::MapPoi;
#[cfg(feature = "approved-map-pack-runtime")]
use pal_overlay_win::actual_map_preview::MapPoiKind;
#[cfg(any(
    feature = "test-harness",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
use pal_overlay_win::actual_map_preview::MapView;
#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
use pal_overlay_win::actual_map_preview::WorldToImageTransform;
#[cfg(any(
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
use pal_overlay_win::actual_map_preview::authoritative_main_map_world_to_image;
#[cfg(any(
    feature = "test-harness",
    feature = "development-live-agent",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
use pal_overlay_win::actual_map_preview::{ActualMapSurface, MapRaster};
#[cfg(feature = "test-harness")]
use pal_overlay_win::actual_map_runtime::PendingActualMapView;
#[cfg(any(
    feature = "development-live-agent",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
use pal_overlay_win::actual_map_runtime::PreviewVisibilityGate;
#[cfg(any(
    feature = "test-harness",
    feature = "development-live-agent",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
use pal_overlay_win::actual_map_runtime::{ActualMapPreviewRuntime, PreviewTick};
#[cfg(feature = "approved-map-pack-runtime")]
use pal_overlay_win::approved_map_surface::{
    MapSearchEntry, MapSearchKind, MapSearchTarget, PreparedApprovedMapPack,
    PreparedApprovedMapRegion,
};
#[cfg(any(
    feature = "development-live-agent",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
use pal_overlay_win::client_build_binding::{
    VerifiedSteamClientBuild, verify_tracked_steam_client,
};
#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
use pal_overlay_win::control_intent_for_hotkey;
#[cfg(feature = "development-live-agent")]
use pal_overlay_win::live_agent_runtime::{LiveAgentCommand, start_live_agent};
#[cfg(feature = "development-local-readonly-position")]
use pal_overlay_win::local_overlay_control::load_projected_pois;
#[cfg(feature = "development-local-readonly-position")]
use pal_overlay_win::local_position_runtime::{
    DevelopmentLocalPositionCommand, DevelopmentLocalPositionDiagnosticCommand,
    DevelopmentLocalPositionDiagnosticRecord, DevelopmentLocalPositionWorker,
    DevelopmentLocalPositionWorkerStart,
};
use pal_overlay_win::runtime_signals::OverlayRuntimeSignals;
#[cfg(feature = "test-harness")]
use pal_overlay_win::synthetic_preview::{
    PREVIEW_DIAMETER, SyntheticPreviewFrame, SyntheticPreviewOptions, SyntheticSurface,
};
#[cfg(feature = "approved-map-pack-runtime")]
use pal_overlay_win::{ControlIntent, FullscreenMapNavigation};
use pal_overlay_win::{
    DEFAULT_MINIMAP_SIZE_DIP, OverlayWindowHost, PhysicalRect, PumpOutcome,
    top_left_overlay_layouts,
};
#[cfg(feature = "development-live-agent")]
use pal_overlay_win::{DEV_DIAGNOSTIC_GAME_BUILD_ID, load_live_source_config};
#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
use pal_overlay_win::{
    control_client::{ControlClientEvent, ControlClientSubmit, OverlayControlClientRuntime},
    local_overlay_control::LocalOverlayControlWatcher,
    local_position_runtime::{
        DevelopmentLocalPositionWorker as LocalPositionWorker,
        DevelopmentLocalPositionWorkerStart as LocalPositionWorkerStart,
    },
    local_position_source::WINDOWS_LIVE_POSITION_PROFILE,
};
#[cfg(feature = "development-local-alignment-diagnostic")]
use pal_overlay_win::{
    local_alignment_diagnostic::{
        LocalAlignmentDiagnosticCommand, LocalAlignmentPreflight, LocalAlignmentResult,
        preflight_local_alignment_then,
    },
    local_alignment_runtime::{LocalAlignmentCollectionUpdate, LocalAlignmentCollector},
};
#[cfg(feature = "development-local-readonly-position")]
use pal_state::PositionSourceEvent;
#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
use pal_state::{PositionSource, ServerAgentPositionSource};
#[cfg(feature = "test-harness")]
use pal_state::{ReplayConfig, ReplayPositionSource};
use pal_windows::{GameWindowTracker, TrackedWindow, Win32Backend, WindowEvent};
#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
use pal_windows::{HotkeyRegistry, Win32HotkeyBackend};

// Thirty geometry probes per second keep the detached overlay visually attached during live
// window moves without doing any file, JSON, or map work on the tracking path.
const TRACK_INTERVAL: Duration = Duration::from_millis(33);
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
#[cfg(feature = "test-harness")]
const STATIC_TEST_WORLD_X: f64 = -343_155.0;
#[cfg(feature = "test-harness")]
const STATIC_TEST_WORLD_Y: f64 = 244_585.0;

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn apply_global_hotkeys(
    registry: &mut HotkeyRegistry<Win32HotkeyBackend>,
    bindings: pal_domain::HotkeyBindings,
) {
    match registry.replace(bindings) {
        Ok(report) => {
            for disabled in &report.disabled {
                eprintln!(
                    "Global hotkey {:?} is disabled: {:?}",
                    disabled.action, disabled.reason
                );
            }
        }
        Err(error) => {
            eprintln!("Global hotkeys are disabled because registration failed: {error}");
        }
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SourceCounters {
    polls: u64,
    samples: u64,
}

#[cfg(feature = "development-live-performance-diagnostic")]
struct CountingPositionSource<S> {
    source: S,
    counters: Rc<Cell<SourceCounters>>,
}

#[cfg(feature = "development-live-performance-diagnostic")]
impl<S> CountingPositionSource<S> {
    fn new(source: S, counters: Rc<Cell<SourceCounters>>) -> Self {
        Self { source, counters }
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
impl<S: PositionSource> PositionSource for CountingPositionSource<S> {
    fn poll(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, pal_state::PositionSourceError> {
        let mut counters = self.counters.get();
        counters.polls = counters.polls.saturating_add(1);
        self.counters.set(counters);

        let event = self.source.poll(now_monotonic_ms)?;
        if matches!(event, Some(PositionSourceEvent::Sample(_))) {
            let mut counters = self.counters.get();
            counters.samples = counters.samples.saturating_add(1);
            self.counters.set(counters);
        }
        Ok(event)
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LivePerformanceSummary {
    source_polls: u64,
    source_samples: u64,
    stationary_suppressed_frames: u64,
    rasterized_frames: u64,
    raster_cpu_timing_samples: u64,
    raster_cpu_timing_failures: u64,
    raster_cpu_100ns_total: u64,
    present_requests: u64,
    paint_calls: u64,
    upload_calls: u64,
    process_lifetime_ms: u64,
    final_visible: bool,
    final_freshness: Option<pal_domain::Freshness>,
}

#[cfg(feature = "development-live-performance-diagnostic")]
impl LivePerformanceSummary {
    fn write_json_line(&self, output: &mut impl io::Write) -> io::Result<()> {
        writeln!(
            output,
            concat!(
                "{{\"source_polls\":{},\"source_samples\":{},",
                "\"stationary_suppressed_frames\":{},\"rasterized_frames\":{},",
                "\"raster_cpu_timing_samples\":{},\"raster_cpu_timing_failures\":{},",
                "\"raster_cpu_100ns_total\":{},\"present_requests\":{},\"paint_calls\":{},",
                "\"upload_calls\":{},\"process_lifetime_ms\":{},\"final_visible\":{},",
                "\"final_freshness\":\"{}\"}}"
            ),
            self.source_polls,
            self.source_samples,
            self.stationary_suppressed_frames,
            self.rasterized_frames,
            self.raster_cpu_timing_samples,
            self.raster_cpu_timing_failures,
            self.raster_cpu_100ns_total,
            self.present_requests,
            self.paint_calls,
            self.upload_calls,
            self.process_lifetime_ms,
            self.final_visible,
            freshness_literal(self.final_freshness),
        )
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
struct LivePerformanceFinalizer {
    started_at: Instant,
    finished: bool,
}

#[cfg(feature = "development-live-performance-diagnostic")]
impl LivePerformanceFinalizer {
    const fn new(started_at: Instant) -> Self {
        Self {
            started_at,
            finished: false,
        }
    }

    fn finish_at(
        &mut self,
        output: &mut impl io::Write,
        mut summary: LivePerformanceSummary,
        finished_at: Instant,
    ) -> io::Result<bool> {
        if self.finished {
            return Ok(false);
        }
        self.finished = true;
        summary.process_lifetime_ms = u64::try_from(
            finished_at
                .saturating_duration_since(self.started_at)
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        summary.write_json_line(output)?;
        Ok(true)
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
fn live_performance_deadline(started_at: Instant, duration: Duration) -> Option<Instant> {
    started_at.checked_add(duration)
}

#[cfg(feature = "development-live-performance-diagnostic")]
struct LivePerformanceRun {
    finalizer: LivePerformanceFinalizer,
    source_counters: Rc<Cell<SourceCounters>>,
}

#[cfg(feature = "development-live-performance-diagnostic")]
impl LivePerformanceRun {
    fn new(started_at: Instant, source_counters: Rc<Cell<SourceCounters>>) -> Self {
        Self {
            finalizer: LivePerformanceFinalizer::new(started_at),
            source_counters,
        }
    }

    fn finish<S: PositionSource>(
        &mut self,
        runtime: Option<&ActualMapPreviewRuntime<S>>,
        overlay: Option<&OverlayWindowHost>,
        final_freshness: Option<pal_domain::Freshness>,
        finished_at: Instant,
    ) -> io::Result<bool> {
        let source = self.source_counters.get();
        let runtime_counters = runtime.map(ActualMapPreviewRuntime::performance_counters);
        let surface_counters =
            overlay.and_then(OverlayWindowHost::actual_map_surface_performance_counters);
        let paint_counters = overlay.map(OverlayWindowHost::actual_map_paint_performance_counters);
        let summary = LivePerformanceSummary {
            source_polls: source.polls,
            source_samples: source.samples,
            stationary_suppressed_frames: runtime_counters
                .map_or(0, |counters| counters.stationary_suppressed_frames),
            rasterized_frames: surface_counters.map_or(0, |counters| counters.rasterized_frames),
            raster_cpu_timing_samples: surface_counters
                .map_or(0, |counters| counters.raster_cpu_timing_samples),
            raster_cpu_timing_failures: surface_counters
                .map_or(0, |counters| counters.raster_cpu_timing_failures),
            raster_cpu_100ns_total: surface_counters
                .map_or(0, |counters| counters.raster_cpu_100ns_total),
            present_requests: paint_counters.map_or(0, |counters| counters.present_requests),
            paint_calls: paint_counters.map_or(0, |counters| counters.paint_calls),
            upload_calls: paint_counters.map_or(0, |counters| counters.upload_calls),
            process_lifetime_ms: 0,
            final_visible: overlay.is_some_and(OverlayWindowHost::is_visible),
            final_freshness,
        };
        let mut stdout = io::stdout().lock();
        self.finalizer.finish_at(&mut stdout, summary, finished_at)
    }

    fn finish_without_runtime(&mut self) -> io::Result<bool> {
        let mut stdout = io::stdout().lock();
        self.finalizer.finish_at(
            &mut stdout,
            LivePerformanceSummary::default(),
            Instant::now(),
        )
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
const fn freshness_literal(freshness: Option<pal_domain::Freshness>) -> &'static str {
    match freshness {
        None => "unknown",
        Some(pal_domain::Freshness::Live) => "live",
        Some(pal_domain::Freshness::Delayed) => "delayed",
        Some(pal_domain::Freshness::Stale) => "stale",
        Some(pal_domain::Freshness::Offline) => "offline",
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    #[cfg(feature = "development-local-alignment-diagnostic")]
    if local_alignment_diagnostic_mode_requested(&arguments) {
        return run_development_local_alignment_diagnostic(arguments);
    }
    #[cfg(feature = "development-local-readonly-position")]
    if local_position_diagnostic_mode_requested(&arguments) {
        return run_development_local_position_diagnostic(arguments);
    }
    #[cfg(feature = "development-live-performance-diagnostic")]
    if live_performance_diagnostic_mode_requested(&arguments) {
        let signals = OverlayRuntimeSignals::start()?;
        return run_development_local_live_performance_diagnostic(arguments, &signals);
    }
    #[cfg(feature = "approved-map-pack-runtime")]
    if approved_map_pack_check_requested(&arguments) {
        return run_approved_map_pack_check(arguments);
    }
    let _signals = OverlayRuntimeSignals::start()?;
    #[cfg(feature = "approved-map-pack-runtime")]
    if approved_map_pack_mode_requested(&arguments) {
        return run_approved_map_pack_preview(arguments, &_signals);
    }
    #[cfg(feature = "development-local-readonly-position")]
    if local_position_mode_requested(&arguments) {
        return run_development_local_position_preview(arguments, &_signals);
    }
    #[cfg(feature = "development-live-agent")]
    if arguments
        .first()
        .is_some_and(|argument| argument == "--live-agent-config")
    {
        return run_live_agent_preview(arguments, &_signals);
    }
    run_standard_preview(arguments)
}

fn run_standard_preview(arguments: Vec<OsString>) -> Result<(), Box<dyn Error + Send + Sync>> {
    if arguments.iter().any(|argument| argument == "--help") {
        if arguments.len() != 1 {
            return Err("--help cannot be combined with other preview arguments".into());
        }
        println!("Pal Companion overlay");
        println!("Tracks the exact Palworld-Win64-Shipping.exe game window.");
        println!("The release shell is minimal; development builds can render local map previews.");
        #[cfg(feature = "test-harness")]
        println!("Development preview: --synthetic-map [--duration-seconds 1..3600]");
        #[cfg(feature = "test-harness")]
        println!(
            "Fixed real-map test landmark: --real-map-bmp <path> \
             --allow-fixed-test-landmark [--duration-seconds 1..3600]"
        );
        #[cfg(feature = "test-harness")]
        println!(
            "Moving preview: --real-map-bmp <path> --position-replay <json> \
             --allow-development-replay [--rotation-mode north-up|heading-up]"
        );
        #[cfg(feature = "development-live-agent")]
        println!("Development live Agent: --live-agent-config <owner-only-config.toml>");
        #[cfg(feature = "development-local-readonly-position")]
        println!(
            "Development local read-only smoke: --development-local-readonly-position \
             --real-map-bmp <path> --allow-unapproved-map-smoke \
             [--rotation-mode north-up|heading-up] [--duration-seconds 1..3600]"
        );
        #[cfg(feature = "development-live-performance-diagnostic")]
        println!(
            "Development local live performance diagnostic: \
             --development-local-readonly-position \
             --development-local-live-performance-diagnostic-json \
             --real-map-bmp <path> --allow-unapproved-map-smoke \
             --duration-seconds 1..3600 [--rotation-mode north-up|heading-up]"
        );
        #[cfg(feature = "development-local-readonly-position")]
        println!(
            "Headless exact-build position JSON: \
             --development-local-position-diagnostic-json [--timeout-seconds 1..30]"
        );
        #[cfg(feature = "development-local-alignment-diagnostic")]
        println!(
            "Headless exact-build alignment JSON: \
             --development-local-alignment-diagnostic-json --real-map-bmp <path> \
             --observation-file <path> --observation-sha256 <lowercase-sha256> \
             --timeout-seconds 1..30"
        );
        return Ok(());
    }

    #[cfg(not(feature = "test-harness"))]
    if !arguments.is_empty() {
        return Err("unrecognized preview arguments; refusing to create an overlay window".into());
    }

    #[cfg(feature = "test-harness")]
    let preview_options = SyntheticPreviewOptions::parse(
        arguments.iter().map(|argument| argument.to_string_lossy()),
    )?;
    #[cfg(feature = "test-harness")]
    let deadline = preview_options
        .optional_duration()
        .and_then(|duration| Instant::now().checked_add(duration));
    #[cfg(not(feature = "test-harness"))]
    let deadline: Option<Instant> = None;
    #[cfg(feature = "test-harness")]
    let preview_diameter =
        if preview_options.synthetic_map() || preview_options.real_map_bmp().is_some() {
            PREVIEW_DIAMETER
        } else {
            DEFAULT_MINIMAP_SIZE_DIP
        };
    #[cfg(not(feature = "test-harness"))]
    let preview_diameter = DEFAULT_MINIMAP_SIZE_DIP;

    let mut tracker = GameWindowTracker::new(Win32Backend::new());
    let mut overlay = OverlayWindowHost::create()?;
    #[cfg(feature = "test-harness")]
    let preview_map_surface_size = if preview_options.real_map_bmp().is_some() {
        let Some(WindowEvent::Attached(window)) = tracker.poll()? else {
            return Err(
                "real-map development preview requires Palworld to be running first".into(),
            );
        };
        let snapshot = window.snapshot();
        let layouts = preview_layouts(
            snapshot.client_left(),
            snapshot.client_top(),
            snapshot.client_width(),
            snapshot.client_height(),
            snapshot.dpi(),
            preview_diameter,
        )?;
        let surface_size = layouts.expanded().viewport_size;
        overlay.apply_layouts(layouts)?;
        overlay.attach(&window)?;
        Some(surface_size)
    } else {
        None
    };
    overlay.set_requested_visible(initial_preview_requested_visibility())?;
    #[cfg(feature = "test-harness")]
    let mut actual_map = None;
    #[cfg(feature = "test-harness")]
    let mut actual_runtime = None;
    #[cfg(feature = "test-harness")]
    let mut runtime_started = None;
    #[cfg(feature = "test-harness")]
    let mut pending_actual_map_view = PendingActualMapView::default();
    #[cfg(feature = "test-harness")]
    if preview_options.synthetic_map() {
        let frame = SyntheticPreviewFrame::from_fixture(pal_domain::PoiFilters::default())?;
        let mut surface = SyntheticSurface::new(PREVIEW_DIAMETER)?;
        surface.rasterize(&frame);
        overlay.apply_synthetic_surface(Arc::new(surface))?;
        configure_interactive_map_preview(&mut overlay)?;
        overlay.set_requested_visible(true)?;
        eprintln!(
            "Synthetic grid preview prepared; this is not the real Palworld map or live position."
        );
    }
    #[cfg(feature = "test-harness")]
    if let Some(path) = preview_options.real_map_bmp() {
        let map = MapRaster::load_bmp_file(path)?;
        configure_interactive_map_preview(&mut overlay)?;
        let world_to_image = authoritative_main_map_world_to_image();
        let center = world_to_image.project(STATIC_TEST_WORLD_X, STATIC_TEST_WORLD_Y);
        let scale = f64::from(map.width()) / 2_048.0 * 1.25;
        let view = MapView::new(
            center.x() * f64::from(map.width()),
            center.y() * f64::from(map.height()),
            scale,
            0.0,
        )?;
        let mut surface = ActualMapSurface::new(
            preview_map_surface_size.expect("real-map preview was pre-attached"),
        )?;
        if preview_options.position_replay().is_none() {
            surface.rasterize(&map, view);
        }
        overlay.apply_actual_map_surface(surface)?;
        if preview_options.position_replay().is_none() {
            overlay.set_requested_visible(true)?;
        }
        if let Some(replay_path) = preview_options.position_replay() {
            let source = ReplayPositionSource::from_path(
                replay_path,
                ReplayConfig {
                    world_alias: "preview-world".to_owned(),
                    subject_id: b"preview-subject".to_vec(),
                    starting_generation: 1,
                    base_boot_id: b"preview-boot".to_vec(),
                    loop_playback: true,
                    starting_monotonic_ms: 0,
                },
            )?;
            let settings = pal_domain::OverlaySettings {
                rotation_mode: preview_options.rotation_mode(),
                ..pal_domain::OverlaySettings::default()
            };
            actual_runtime = Some(ActualMapPreviewRuntime::new(
                source,
                settings,
                world_to_image,
            )?);
            runtime_started = Some(Instant::now());
            eprintln!(
                "DEVELOPMENT REPLAY ONLY: simulated positions are looping; \
                 this is not your character and server live position is not connected."
            );
        } else {
            eprintln!(
                "Real Palworld map prepared at a fixed test landmark; live position is not connected."
            );
        }
        if actual_runtime.is_some() {
            actual_map = Some(map);
        }
    }
    let mut next_tracking_poll = Instant::now();
    eprintln!("Pal Companion preview is waiting for Palworld-Win64-Shipping.exe...");

    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            eprintln!("Preview duration elapsed; preview is exiting.");
            return Ok(());
        }
        if overlay.pump_messages()? == PumpOutcome::ShutdownRequested {
            eprintln!("Overlay window closed; preview is exiting.");
            return Ok(());
        }
        overlay.sync_fullscreen_consumer()?;
        let now = Instant::now();
        if now >= next_tracking_poll || overlay.reattach_requested() {
            next_tracking_poll = now + TRACK_INTERVAL;
            match tracker.poll() {
                Ok(Some(WindowEvent::Attached(window))) => {
                    attach_preview(&mut overlay, &window, preview_diameter)?;
                    eprintln!(
                        "Palworld attached; {}",
                        preview_visibility_status(window.snapshot(), overlay.is_visible())
                    );
                }
                Ok(Some(WindowEvent::Changed { current, .. })) => {
                    let was_visible = overlay.is_visible();
                    attach_preview(&mut overlay, &current, preview_diameter)?;
                    if was_visible != overlay.is_visible() {
                        eprintln!(
                            "Palworld state changed; {}",
                            preview_visibility_status(current.snapshot(), overlay.is_visible())
                        );
                    }
                }
                Ok(Some(WindowEvent::Detached { .. })) => {
                    overlay.detach()?;
                    eprintln!("Palworld detached; waiting for the game...");
                }
                Ok(None) => {}
                Err(error) => eprintln!("Palworld window probe failed: {error}"),
            }
            overlay.refresh_topmost()?;
        }
        #[cfg(feature = "test-harness")]
        if let (Some(runtime), Some(map), Some(started)) = (
            actual_runtime.as_mut(),
            actual_map.as_ref(),
            runtime_started,
        ) {
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            if let PreviewTick::Present(frame) =
                runtime.tick(elapsed_ms, map.width(), map.height())?
            {
                pending_actual_map_view.submit(frame.view());
            }
            if let Some(view) = pending_actual_map_view.take_if_visible(true) {
                let updated = overlay.update_actual_map_surface(map, view)?;
                debug_assert!(updated, "real-map surface was initialized before replay");
                overlay.set_requested_visible(true)?;
            }
        }
        thread::sleep(FRAME_INTERVAL);
    }
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn local_alignment_diagnostic_mode_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--development-local-alignment-diagnostic-json")
}

#[cfg(feature = "development-local-readonly-position")]
fn local_position_diagnostic_mode_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--development-local-position-diagnostic-json")
}

#[cfg(feature = "development-local-readonly-position")]
fn local_position_mode_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--development-local-readonly-position")
}

#[cfg(feature = "approved-map-pack-runtime")]
fn approved_map_pack_mode_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--approved-map-pack")
}

#[cfg(feature = "approved-map-pack-runtime")]
fn approved_map_pack_check_requested(arguments: &[OsString]) -> bool {
    arguments
        .first()
        .is_some_and(|argument| argument == "--approved-map-pack-check")
}

#[cfg(feature = "approved-map-pack-runtime")]
#[derive(Debug)]
struct ApprovedMapPackCommand {
    dataset_root: std::path::PathBuf,
    game_build_id: String,
}

#[cfg(feature = "approved-map-pack-runtime")]
impl ApprovedMapPackCommand {
    fn parse(arguments: &[OsString]) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let [pack_flag, dataset_root, build_flag, game_build_id] = arguments else {
            return Err(
                "approved map mode requires --approved-map-pack <absolute-root> \
                 --game-build-id <exact-build>"
                    .into(),
            );
        };
        if pack_flag != "--approved-map-pack" || build_flag != "--game-build-id" {
            return Err("approved map mode arguments are out of order or unknown".into());
        }
        let dataset_root = std::path::PathBuf::from(dataset_root);
        if !dataset_root.is_absolute() {
            return Err("approved map pack dataset root must be absolute".into());
        }
        let game_build_id = game_build_id
            .to_str()
            .filter(|value| !value.is_empty() && value.as_bytes().iter().all(u8::is_ascii_digit))
            .ok_or("approved map pack Build ID must contain only ASCII digits")?
            .to_owned();
        Ok(Self {
            dataset_root,
            game_build_id,
        })
    }

    fn parse_check(arguments: &[OsString]) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let [check_flag, dataset_root, build_flag, game_build_id] = arguments else {
            return Err(
                "approved map check requires --approved-map-pack-check <absolute-root> \
                 --game-build-id <exact-build>"
                    .into(),
            );
        };
        if check_flag != "--approved-map-pack-check" || build_flag != "--game-build-id" {
            return Err("approved map check arguments are out of order or unknown".into());
        }
        Self::parse(&[
            OsString::from("--approved-map-pack"),
            dataset_root.clone(),
            build_flag.clone(),
            game_build_id.clone(),
        ])
    }
}

#[cfg(feature = "development-live-performance-diagnostic")]
fn live_performance_diagnostic_mode_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--development-local-live-performance-diagnostic-json")
}

const fn initial_preview_requested_visibility() -> bool {
    false
}

#[cfg(any(
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ),
    test
))]
const fn runtime_overlay_visible(
    enabled: bool,
    fresh_play_position: bool,
    map_surface_applied: bool,
    browse_available: bool,
) -> bool {
    enabled && map_surface_applied && (fresh_play_position || browse_available)
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn run_development_local_alignment_diagnostic(
    arguments: Vec<OsString>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = LocalAlignmentDiagnosticCommand::parse(arguments)?;
    let deadline = Instant::now()
        .checked_add(command.timeout())
        .ok_or("the local alignment diagnostic timeout is invalid")?;
    let result = preflight_local_alignment_then(&command, |preflight| {
        run_development_local_alignment_after_preflight(preflight, deadline)
    })??;

    let mut output = io::stdout().lock();
    output.write_all(result.to_json_line().as_bytes())?;
    output.flush()?;
    Ok(())
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn local_alignment_completion_is_before_deadline(now: Instant, deadline: Instant) -> bool {
    now < deadline
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn run_development_local_alignment_after_preflight(
    preflight: LocalAlignmentPreflight,
    deadline: Instant,
) -> Result<LocalAlignmentResult, Box<dyn Error + Send + Sync>> {
    if preflight.game_build_id() != WINDOWS_LIVE_POSITION_PROFILE.build_id() {
        return Err(
            "the retained alignment evidence does not match the current live-position build".into(),
        );
    }
    if !local_alignment_completion_is_before_deadline(Instant::now(), deadline) {
        return Err("the local alignment diagnostic timed out before process access".into());
    }

    let mut tracker = GameWindowTracker::new(Win32Backend::new());
    let mut worker: Option<LocalPositionWorker> = None;
    let mut source: Option<ServerAgentPositionSource> = None;
    let mut collector = LocalAlignmentCollector::new(preflight.observation().clone());

    loop {
        if !local_alignment_completion_is_before_deadline(Instant::now(), deadline) {
            return Err(
                "the local alignment diagnostic timed out without ten stable live samples".into(),
            );
        }

        if worker.is_none() {
            let window_event = tracker
                .poll()
                .map_err(|_| "the exact Steam Palworld client is unavailable")?;
            if !local_alignment_completion_is_before_deadline(Instant::now(), deadline) {
                return Err(
                    "the local alignment diagnostic timed out during client discovery".into(),
                );
            }
            match window_event {
                Some(WindowEvent::Attached(window))
                | Some(WindowEvent::Changed {
                    current: window, ..
                }) => {
                    let binding = verify_tracked_steam_client(
                        &window,
                        WINDOWS_LIVE_POSITION_PROFILE.build_id(),
                    )?;
                    if !local_alignment_completion_is_before_deadline(Instant::now(), deadline) {
                        return Err(
                            "the local alignment diagnostic timed out during client verification"
                                .into(),
                        );
                    }
                    let started_worker = DevelopmentLocalPositionWorker::spawn(window, binding)?;
                    if !local_alignment_completion_is_before_deadline(Instant::now(), deadline) {
                        return Err(
                            "the local alignment diagnostic timed out during worker startup".into(),
                        );
                    }
                    worker = Some(started_worker);
                }
                Some(WindowEvent::Detached { .. }) => {
                    return Err("the exact Steam Palworld client detached".into());
                }
                None => {}
            }
        }

        if source.is_none()
            && let Some(worker) = worker.as_mut()
        {
            match worker.poll_start() {
                DevelopmentLocalPositionWorkerStart::Pending => {}
                DevelopmentLocalPositionWorkerStart::Ready(ready) => source = Some(ready),
                DevelopmentLocalPositionWorkerStart::Failed(_)
                | DevelopmentLocalPositionWorkerStart::Closed => {
                    return Err("the exact-build local alignment source is unavailable".into());
                }
            }
        }

        if let (Some(source), Some(worker)) = (source.as_mut(), worker.as_ref()) {
            let now_monotonic_ms = worker.elapsed_ms();
            let event = source.poll(now_monotonic_ms)?;
            match collector.consume(event, now_monotonic_ms)? {
                LocalAlignmentCollectionUpdate::Pending => {}
                LocalAlignmentCollectionUpdate::Complete(result) => {
                    if !local_alignment_completion_is_before_deadline(Instant::now(), deadline) {
                        return Err(
                            "the local alignment diagnostic timed out before completion".into()
                        );
                    }
                    return Ok(result);
                }
            }
        }

        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(feature = "development-local-readonly-position")]
fn run_development_local_position_diagnostic(
    arguments: Vec<OsString>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = DevelopmentLocalPositionDiagnosticCommand::parse(arguments)?;
    let deadline = Instant::now()
        .checked_add(command.timeout())
        .ok_or("the local position diagnostic timeout is invalid")?;
    let mut tracker = GameWindowTracker::new(Win32Backend::new());
    let mut worker: Option<DevelopmentLocalPositionWorker> = None;
    let mut source: Option<ServerAgentPositionSource> = None;

    loop {
        if Instant::now() >= deadline {
            return Err("the local position diagnostic timed out without a fresh sample".into());
        }

        if worker.is_none() {
            match tracker.poll() {
                Ok(Some(WindowEvent::Attached(window)))
                | Ok(Some(WindowEvent::Changed {
                    current: window, ..
                })) => {
                    let binding = verify_tracked_steam_client(
                        &window,
                        WINDOWS_LIVE_POSITION_PROFILE.build_id(),
                    )?;
                    worker = Some(DevelopmentLocalPositionWorker::spawn(window, binding)?);
                }
                Ok(Some(WindowEvent::Detached { .. })) => {
                    return Err("the exact Steam Palworld client detached".into());
                }
                Ok(None) => {}
                Err(_) => return Err("the exact Steam Palworld client is unavailable".into()),
            }
        }

        if source.is_none()
            && let Some(worker) = worker.as_mut()
        {
            match worker.poll_start() {
                DevelopmentLocalPositionWorkerStart::Pending => {}
                DevelopmentLocalPositionWorkerStart::Ready(ready) => source = Some(ready),
                DevelopmentLocalPositionWorkerStart::Failed(_)
                | DevelopmentLocalPositionWorkerStart::Closed => {
                    return Err(
                        "the exact-build local read-only diagnostic source is unavailable".into(),
                    );
                }
            }
        }

        if let (Some(source), Some(worker)) = (source.as_mut(), worker.as_ref()) {
            match source.poll(worker.elapsed_ms())? {
                Some(PositionSourceEvent::Sample(sample)) => {
                    let record = DevelopmentLocalPositionDiagnosticRecord::from_fresh_sample(
                        &sample,
                        worker.elapsed_ms(),
                    )?;
                    println!("{}", record.to_json_line());
                    return Ok(());
                }
                Some(
                    PositionSourceEvent::Unavailable { .. }
                    | PositionSourceEvent::Disconnected { .. }
                    | PositionSourceEvent::ClockInvalid { .. },
                ) => {
                    return Err(
                        "the exact-build local read-only diagnostic source failed closed".into(),
                    );
                }
                Some(PositionSourceEvent::Connected { .. }) | None => {}
            }
        }

        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(feature = "development-local-readonly-position")]
fn run_development_local_position_preview(
    arguments: Vec<OsString>,
    signals: &OverlayRuntimeSignals,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = DevelopmentLocalPositionCommand::parse(arguments)?;
    #[cfg(feature = "development-live-performance-diagnostic")]
    debug_assert!(!command.live_performance_diagnostic());
    let map = load_development_local_position_map(&command)?;
    let map_pois = load_projected_pois(command.poi_catalog_path(), &map)?;
    run_local_position_preview_loop(
        vec![LocalMapRegionSurface::development(
            map,
            map_pois,
            authoritative_main_map_world_to_image(),
        )],
        LocalMapApproval::DevelopmentUnapproved,
        command.duration(),
        |source| source,
        Some(signals),
        #[cfg(feature = "development-live-performance-diagnostic")]
        None,
    )
}

#[cfg(feature = "approved-map-pack-runtime")]
fn run_approved_map_pack_preview(
    arguments: Vec<OsString>,
    signals: &OverlayRuntimeSignals,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = ApprovedMapPackCommand::parse(&arguments)?;
    let prepared =
        PreparedApprovedMapPack::open_published(&command.dataset_root, &command.game_build_id)?;
    let map_surfaces = prepared
        .regions()
        .iter()
        .map(LocalMapRegionSurface::approved)
        .collect::<Vec<_>>();
    eprintln!(
        "Approved exact-build map pack loaded: build={} regions={} POIs={}.",
        prepared.build_id(),
        map_surfaces.len(),
        map_surfaces
            .iter()
            .map(|surface| surface.pois.len())
            .sum::<usize>()
    );
    run_local_position_preview_loop(
        map_surfaces,
        LocalMapApproval::Approved,
        None,
        |source| source,
        Some(signals),
        #[cfg(feature = "development-live-performance-diagnostic")]
        None,
    )
}

#[cfg(feature = "approved-map-pack-runtime")]
fn run_approved_map_pack_check(
    arguments: Vec<OsString>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = ApprovedMapPackCommand::parse_check(&arguments)?;
    let prepared =
        PreparedApprovedMapPack::open_published(&command.dataset_root, &command.game_build_id)?;
    let poi_count = prepared
        .regions()
        .iter()
        .map(|region| region.pois().len())
        .sum::<usize>();
    println!(
        "{{\"ok\":true,\"game_build_id\":\"{}\",\"regions\":{},\"pois\":{}}}",
        prepared.build_id(),
        prepared.regions().len(),
        poi_count
    );
    Ok(())
}

#[cfg(feature = "development-local-readonly-position")]
fn load_development_local_position_map(
    command: &DevelopmentLocalPositionCommand,
) -> Result<MapRaster, Box<dyn Error + Send + Sync>> {
    let map = MapRaster::load_bmp_file(command.map_path())?;
    if map.width() != 2_048 || map.height() != 2_048 {
        return Err("the unapproved local smoke map must be a 2048 by 2048 BMP".into());
    }
    Ok(map)
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalPositionPreviewTerminal {
    DeadlineExpired,
    ShutdownRequested,
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalMapApproval {
    #[cfg(feature = "development-local-readonly-position")]
    DevelopmentUnapproved,
    #[cfg(feature = "approved-map-pack-runtime")]
    Approved,
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn browse_only_mode(map_approval: LocalMapApproval) -> bool {
    match map_approval {
        #[cfg(feature = "development-local-readonly-position")]
        LocalMapApproval::DevelopmentUnapproved => false,
        #[cfg(feature = "approved-map-pack-runtime")]
        LocalMapApproval::Approved => !CURRENT_BUILD_HAS_LIVE_POSITION_PROFILE,
    }
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
const fn same_process_identity(
    previous_window_id: u64,
    previous_process_id: u32,
    current_window_id: u64,
    current_process_id: u32,
) -> bool {
    previous_window_id == current_window_id && previous_process_id == current_process_id
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn same_tracked_game_process(previous: Option<&TrackedWindow>, current: &TrackedWindow) -> bool {
    previous.is_some_and(|previous| {
        same_process_identity(
            previous.id().as_raw(),
            previous.snapshot().process_id(),
            current.id().as_raw(),
            current.snapshot().process_id(),
        )
    })
}

#[cfg(feature = "approved-map-pack-runtime")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct SearchFocus {
    surface_index: usize,
    map_x: f64,
    map_y: f64,
}

#[cfg(feature = "approved-map-pack-runtime")]
fn clear_search_focus_for_session_loss(search_focus: &mut Option<SearchFocus>) {
    *search_focus = None;
}

#[cfg(feature = "approved-map-pack-runtime")]
fn merge_live_player_marker(
    view: MapView,
    world_pose: Option<(f64, f64, Option<f32>)>,
    world_to_image: WorldToImageTransform,
    map_width: u32,
    map_height: u32,
) -> Result<MapView, pal_overlay_win::actual_map_preview::MapViewError> {
    match world_pose.and_then(|(world_x, world_y, heading)| {
        world_to_image
            .project_within_bounds(world_x, world_y)
            .map(|point| {
                let rotation_degrees = heading
                    .filter(|value| value.is_finite())
                    .map(|value| value.rem_euclid(360.0))
                    .unwrap_or(0.0);
                (
                    point.x() * f64::from(map_width),
                    point.y() * f64::from(map_height),
                    rotation_degrees,
                )
            })
    }) {
        Some((map_x, map_y, rotation_degrees)) => {
            view.with_player_map_pose(map_x, map_y, rotation_degrees)
        }
        None => Ok(view.without_player_marker()),
    }
}

#[cfg(feature = "approved-map-pack-runtime")]
fn refresh_retained_navigation_view(
    view: MapView,
    tracking_view: Option<MapView>,
    world_pose: Option<(f64, f64, Option<f32>)>,
    world_to_image: WorldToImageTransform,
    map_width: u32,
    map_height: u32,
) -> Result<MapView, pal_overlay_win::actual_map_preview::MapViewError> {
    let view = if let Some(tracking_view) = tracking_view {
        view.with_raster_transform(
            tracking_view.source_pixels_per_screen_pixel(),
            tracking_view.map_rotation_degrees(),
        )?
    } else {
        view
    };
    merge_live_player_marker(view, world_pose, world_to_image, map_width, map_height)
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
#[derive(Clone)]
struct LocalMapRegionSurface {
    map_id: String,
    region_id: String,
    priority: i32,
    map: Arc<MapRaster>,
    pois: Vec<MapPoi>,
    #[cfg(feature = "approved-map-pack-runtime")]
    search_entries: Vec<MapSearchEntry>,
    world_to_image: WorldToImageTransform,
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
impl LocalMapRegionSurface {
    #[cfg(feature = "development-local-readonly-position")]
    fn development(
        map: MapRaster,
        pois: Vec<MapPoi>,
        world_to_image: WorldToImageTransform,
    ) -> Self {
        Self {
            map_id: "MainMap".to_owned(),
            region_id: "FirstRegion".to_owned(),
            priority: 0,
            map: Arc::new(map),
            pois,
            #[cfg(feature = "approved-map-pack-runtime")]
            search_entries: Vec::new(),
            world_to_image,
        }
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    fn approved(region: &PreparedApprovedMapRegion) -> Self {
        Self {
            map_id: region.map_id().to_owned(),
            region_id: region.region_id().to_owned(),
            priority: region.priority(),
            map: region.raster_arc(),
            pois: region.pois().to_vec(),
            search_entries: region.search_entries().to_vec(),
            world_to_image: region.world_to_image(),
        }
    }

    fn contains_world(&self, world_x: f64, world_y: f64) -> bool {
        self.world_to_image
            .project_within_bounds(world_x, world_y)
            .is_some()
    }
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn select_local_map_surface_index(
    surfaces: &[LocalMapRegionSurface],
    world_x: f64,
    world_y: f64,
) -> Option<usize> {
    if !world_x.is_finite() || !world_y.is_finite() {
        return None;
    }
    let highest_priority = surfaces
        .iter()
        .filter(|surface| surface.contains_world(world_x, world_y))
        .map(|surface| surface.priority)
        .max()?;
    let mut matches = surfaces.iter().enumerate().filter(|(_, surface)| {
        surface.priority == highest_priority && surface.contains_world(world_x, world_y)
    });
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
#[derive(Clone, Copy, Debug)]
struct LocalPositionPreviewTermination {
    #[cfg(feature = "development-live-performance-diagnostic")]
    selected_at: Instant,
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
#[derive(Debug)]
struct LocalPositionPreviewFailure {
    error: Box<dyn Error + Send + Sync>,
    #[cfg(feature = "development-live-performance-diagnostic")]
    selected_at: Instant,
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
impl LocalPositionPreviewFailure {
    fn new(error: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            error,
            #[cfg(feature = "development-live-performance-diagnostic")]
            selected_at: Instant::now(),
        }
    }

    fn message(message: &'static str) -> Self {
        Self::new(message.into())
    }
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
impl<E> From<E> for LocalPositionPreviewFailure
where
    E: Error + Send + Sync + 'static,
{
    fn from(error: E) -> Self {
        Self::new(Box::new(error))
    }
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn select_local_preview_terminal(
    condition: LocalPositionPreviewTerminal,
    selected_at: Instant,
    report: impl FnOnce(&'static str),
) -> LocalPositionPreviewTermination {
    let terminal = LocalPositionPreviewTermination {
        #[cfg(feature = "development-live-performance-diagnostic")]
        selected_at,
    };
    #[cfg(not(feature = "development-live-performance-diagnostic"))]
    let _ = selected_at;
    let message = match condition {
        LocalPositionPreviewTerminal::DeadlineExpired => {
            "Local smoke duration elapsed; preview is exiting."
        }
        LocalPositionPreviewTerminal::ShutdownRequested => {
            "Overlay window closed; local smoke preview is exiting."
        }
    };
    report(message);
    terminal
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn prepare_local_map_startup<T, E>(
    approval: LocalMapApproval,
    set_requested_visible: impl FnOnce(bool) -> Result<T, E>,
    mut report: impl FnMut(&'static str),
) -> Result<(), E> {
    let _ = set_requested_visible(initial_preview_requested_visibility())?;
    match approval {
        #[cfg(feature = "development-local-readonly-position")]
        LocalMapApproval::DevelopmentUnapproved => {
            report(
                "DEVELOPMENT LOCAL READ-ONLY SMOKE ONLY: Gate B map alignment is unapproved; no game memory is written.",
            );
            report("Local smoke preview is waiting for the exact Steam Palworld client build...");
        }
        #[cfg(feature = "approved-map-pack-runtime")]
        LocalMapApproval::Approved => {
            if browse_only_mode(approval) {
                report(
                    "Approved exact-build map pack browsing is active; live position remains hidden until this client build is verified.",
                );
            } else {
                report(
                    "Approved exact-build map pack is active; waiting for the exact read-only Palworld position source.",
                );
            }
        }
    }
    Ok(())
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn apply_local_map_surface(
    overlay: &mut OverlayWindowHost,
    surface: ActualMapSurface,
    approval: LocalMapApproval,
) -> Result<(), pal_overlay_win::OverlayHostError> {
    match approval {
        #[cfg(feature = "development-local-readonly-position")]
        LocalMapApproval::DevelopmentUnapproved => {
            overlay.apply_local_unapproved_map_surface(surface)
        }
        #[cfg(feature = "approved-map-pack-runtime")]
        LocalMapApproval::Approved => overlay.apply_approved_map_surface(surface),
    }
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn update_local_map_surface(
    overlay: &mut OverlayWindowHost,
    map: &MapRaster,
    view: MapView,
    pois: &[MapPoi],
    filters: &pal_domain::PoiFilters,
    approval: LocalMapApproval,
) -> Result<bool, pal_overlay_win::OverlayHostError> {
    match approval {
        #[cfg(feature = "development-local-readonly-position")]
        LocalMapApproval::DevelopmentUnapproved => {
            overlay.update_local_unapproved_map_surface_with_pois(map, view, pois, filters)
        }
        #[cfg(feature = "approved-map-pack-runtime")]
        LocalMapApproval::Approved => {
            overlay.update_approved_map_surface_with_pois(map, view, pois, filters)
        }
    }
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn map_surface_refresh_required(
    current: &pal_domain::OverlaySettings,
    candidate: &pal_domain::OverlaySettings,
) -> bool {
    current.rotation_mode != candidate.rotation_mode
        || current.display_mode != candidate.display_mode
        || current.diameter_px != candidate.diameter_px
        || current.zoom != candidate.zoom
        || current.poi_filters != candidate.poi_filters
}

#[cfg(feature = "approved-map-pack-runtime")]
fn frame_search_entry(entry: &MapSearchEntry) -> FrameSearchEntry {
    FrameSearchEntry {
        kind: match entry.kind {
            MapSearchKind::Pal => FrameSearchKind::Pal,
            MapSearchKind::Poi => FrameSearchKind::Poi,
            MapSearchKind::Resource => FrameSearchKind::Resource,
        },
        title: entry.title.clone(),
        subtitle: entry.subtitle.clone(),
    }
}

#[cfg(feature = "approved-map-pack-runtime")]
fn filters_for_search_selection(
    current: &pal_domain::PoiFilters,
    target: &MapSearchTarget,
) -> pal_domain::PoiFilters {
    let mut next = current.clone();
    match target {
        MapSearchTarget::Pal(species_id) => {
            if !next.selected_pal_ids.iter().any(|id| id == species_id) {
                if next.selected_pal_ids.len() == 8 {
                    next.selected_pal_ids.remove(0);
                }
                next.selected_pal_ids.push(species_id.clone());
                next.selected_pal_ids.sort_unstable();
                next.selected_pal_ids.dedup();
            }
        }
        MapSearchTarget::Poi(kind) => match kind {
            MapPoiKind::FastTravel => next.fast_travel = true,
            MapPoiKind::Boss => next.boss = true,
            MapPoiKind::Wanted => next.wanted = true,
            MapPoiKind::Dungeon => next.dungeon = true,
            MapPoiKind::Supplemental | MapPoiKind::PalSpawn => {}
        },
        MapSearchTarget::Layer(layer_id) => {
            if layer_id == "boss" {
                next.boss = true;
            } else if !next.enabled_layer_ids.iter().any(|id| id == layer_id) {
                next.enabled_layer_ids.push(layer_id.clone());
                next.enabled_layer_ids.sort_unstable();
                next.enabled_layer_ids.dedup();
            }
        }
    }
    next
}

#[cfg(feature = "approved-map-pack-runtime")]
fn approved_browse_view(
    surface: &LocalMapRegionSurface,
    zoom: f32,
) -> Result<MapView, pal_overlay_win::actual_map_preview::MapViewError> {
    let scale = f64::from(surface.map.width()) / 2_048.0 * 1.25 / f64::from(zoom);
    MapView::browse(
        f64::from(surface.map.width()) * 0.5,
        f64::from(surface.map.height()) * 0.5,
        scale,
    )
}

#[cfg(any(
    feature = "development-local-readonly-position",
    feature = "approved-map-pack-runtime"
))]
fn run_local_position_preview_loop<S, F>(
    map_surfaces: Vec<LocalMapRegionSurface>,
    map_approval: LocalMapApproval,
    duration: Option<Duration>,
    mut wrap_source: F,
    signals: Option<&OverlayRuntimeSignals>,
    #[cfg(feature = "development-live-performance-diagnostic")] mut diagnostic: Option<
        LivePerformanceRun,
    >,
) -> Result<(), Box<dyn Error + Send + Sync>>
where
    S: PositionSource,
    F: FnMut(ServerAgentPositionSource) -> S,
{
    if map_surfaces.is_empty() {
        return Err("at least one verified map region is required".into());
    }
    let (initial_control, control_watcher) = LocalOverlayControlWatcher::start()?;
    let control_client = OverlayControlClientRuntime::start()?;
    let mut control_connected = false;
    let mut effective_settings_version = 0_u64;
    let mut overlay_settings = initial_control.settings.to_domain()?;
    let mut runtime_settings_version = 2_u64;
    let mut tracker = GameWindowTracker::new(Win32Backend::new());
    let mut overlay = match OverlayWindowHost::create() {
        Ok(overlay) => overlay,
        Err(error) => {
            #[cfg(feature = "development-live-performance-diagnostic")]
            if let Some(diagnostic) = diagnostic.as_mut() {
                let _ = diagnostic.finish_without_runtime();
            }
            return Err(error.into());
        }
    };
    #[cfg(feature = "approved-map-pack-runtime")]
    let overlay_search_catalog = map_surfaces
        .iter()
        .flat_map(|surface| surface.search_entries.iter().cloned())
        .collect::<Vec<_>>();
    #[cfg(feature = "approved-map-pack-runtime")]
    overlay.set_fullscreen_search_catalog(
        &overlay_search_catalog
            .iter()
            .map(frame_search_entry)
            .collect::<Vec<_>>(),
    )?;
    #[cfg(feature = "approved-map-pack-runtime")]
    let mut search_focus: Option<SearchFocus> = None;
    #[cfg(feature = "approved-map-pack-runtime")]
    let mut manual_pan_active = false;
    #[cfg(not(feature = "approved-map-pack-runtime"))]
    let search_focus: Option<()> = None;
    #[cfg(not(feature = "approved-map-pack-runtime"))]
    let manual_pan_active = false;
    let mut hotkey_registry = HotkeyRegistry::new(Win32HotkeyBackend::new());
    apply_global_hotkeys(&mut hotkey_registry, overlay_settings.hotkey_bindings);
    let mut verified_client: Option<VerifiedSteamClientBuild> = None;
    let mut tracked_window: Option<TrackedWindow> = None;
    let mut worker: Option<LocalPositionWorker> = None;
    let mut runtime: Option<ActualMapPreviewRuntime<S>> = None;
    let mut map_surface_applied = false;
    let mut active_surface_index = 0_usize;
    let mut last_presented_view: Option<MapView> = None;
    let mut surface_refresh_needed = false;
    let mut visibility_gate = PreviewVisibilityGate::default();
    let browse_only = browse_only_mode(map_approval);
    #[cfg(feature = "development-live-performance-diagnostic")]
    let deadline_epoch = diagnostic
        .as_ref()
        .map_or_else(Instant::now, |diagnostic| diagnostic.finalizer.started_at);
    #[cfg(feature = "development-live-performance-diagnostic")]
    let deadline =
        duration.and_then(|duration| live_performance_deadline(deadline_epoch, duration));
    #[cfg(not(feature = "development-live-performance-diagnostic"))]
    let deadline = duration.and_then(|duration| Instant::now().checked_add(duration));
    let mut next_tracking_poll = Instant::now();
    #[cfg(feature = "development-live-performance-diagnostic")]
    let mut latest_freshness = None;

    let result: Result<LocalPositionPreviewTermination, LocalPositionPreviewFailure> = (|| {
        prepare_local_map_startup(
            map_approval,
            |visible| overlay.set_requested_visible(visible),
            |message| eprintln!("{message}"),
        )?;
        overlay.set_input_mode(pal_domain::InputMode::Locked)?;
        overlay.set_display_mode(overlay_settings.display_mode)?;
        overlay.set_opacity(overlay_settings.opacity)?;
        overlay.set_poi_filters(overlay_settings.poi_filters.clone())?;
        eprintln!(
            "Overlay controls are live at {} ({} regions, {} POIs loaded).",
            pal_overlay_control::default_control_path().display(),
            map_surfaces.len(),
            map_surfaces
                .iter()
                .map(|surface| surface.pois.len())
                .sum::<usize>()
        );
        loop {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                let selected_at = Instant::now();
                return Ok(select_local_preview_terminal(
                    LocalPositionPreviewTerminal::DeadlineExpired,
                    selected_at,
                    |message| eprintln!("{message}"),
                ));
            }
            if overlay.pump_messages()? == PumpOutcome::ShutdownRequested {
                let selected_at = Instant::now();
                return Ok(select_local_preview_terminal(
                    LocalPositionPreviewTerminal::ShutdownRequested,
                    selected_at,
                    |message| eprintln!("{message}"),
                ));
            }
            overlay.sync_fullscreen_consumer()?;

            for action in overlay.take_hotkey_actions() {
                let intent = control_intent_for_hotkey(
                    action,
                    &overlay_settings,
                    effective_settings_version,
                );
                if control_client.submit(intent) != ControlClientSubmit::Queued {
                    control_connected = false;
                    overlay.set_input_mode(pal_domain::InputMode::Locked)?;
                }
            }

            for intent in overlay.take_control_intents() {
                if control_client.submit(intent) != ControlClientSubmit::Queued {
                    control_connected = false;
                    overlay.set_input_mode(pal_domain::InputMode::Locked)?;
                }
            }
            #[cfg(feature = "approved-map-pack-runtime")]
            if let Some(navigation) = overlay.take_fullscreen_map_navigation() {
                match navigation {
                    FullscreenMapNavigation::Pan {
                        raster_delta_x,
                        raster_delta_y,
                    } if overlay_settings.display_mode == pal_domain::DisplayMode::ExpandedMap => {
                        if let (Some(view), Some(viewport)) =
                            (last_presented_view, overlay.actual_map_surface_size())
                        {
                            let surface = &map_surfaces[active_surface_index];
                            last_presented_view = Some(view.panned_by_raster_delta(
                                f64::from(raster_delta_x),
                                f64::from(raster_delta_y),
                                viewport.width,
                                viewport.height,
                                surface.map.width(),
                                surface.map.height(),
                            )?);
                            manual_pan_active = true;
                            surface_refresh_needed = true;
                        }
                    }
                    FullscreenMapNavigation::Recenter => {
                        let tracking_view = runtime
                            .as_ref()
                            .and_then(ActualMapPreviewRuntime::last_live_tracking_view);
                        if browse_only || tracking_view.is_some() {
                            search_focus = None;
                            manual_pan_active = false;
                            if let Some(view) = tracking_view {
                                last_presented_view = Some(view);
                            }
                            surface_refresh_needed = true;
                        }
                    }
                    FullscreenMapNavigation::Pan { .. } => {}
                }
            }
            #[cfg(feature = "approved-map-pack-runtime")]
            if let Some(index) = overlay.take_fullscreen_search_selection()
                && let Some(selection) = overlay_search_catalog.get(index)
                && let Some(surface_index) = map_surfaces.iter().position(|surface| {
                    surface.map_id == selection.map_id && surface.region_id == selection.region_id
                })
            {
                let filters =
                    filters_for_search_selection(&overlay_settings.poi_filters, &selection.target);
                if filters != overlay_settings.poi_filters
                    && control_client.submit(ControlIntent::ReplacePoiFilters {
                        expected_settings_version: effective_settings_version,
                        filters,
                    }) != ControlClientSubmit::Queued
                {
                    control_connected = false;
                    overlay.set_input_mode(pal_domain::InputMode::Locked)?;
                }
                let focus = SearchFocus {
                    surface_index,
                    map_x: selection.map_x,
                    map_y: selection.map_y,
                };
                let changed_region = active_surface_index != surface_index;
                active_surface_index = surface_index;
                search_focus = Some(focus);
                manual_pan_active = false;
                let surface = &map_surfaces[active_surface_index];
                let scale = f64::from(surface.map.width()) / 2_048.0 * 1.25
                    / f64::from(overlay_settings.zoom);
                last_presented_view = Some(MapView::search_focus(
                    selection.map_x,
                    selection.map_y,
                    scale,
                )?);
                if changed_region {
                    map_surface_applied = false;
                }
                surface_refresh_needed = true;
                overlay.set_requested_visible(runtime_overlay_visible(
                    overlay_settings.enabled,
                    visibility_gate.requested_visible(),
                    map_surface_applied,
                    (browse_only || search_focus.is_some() || manual_pan_active)
                        && tracked_window.is_some(),
                ))?;
                eprintln!(
                    "Overlay search selected '{}' in {}/{}.",
                    selection.title, selection.map_id, selection.region_id
                );
            }

            if let Some(event) = control_client.take_latest() {
                match event {
                    ControlClientEvent::Connected {
                        settings_version,
                        settings: candidate,
                    }
                    | ControlClientEvent::SettingsApplied {
                        settings_version,
                        settings: candidate,
                    } => {
                        control_connected = true;
                        effective_settings_version = settings_version;
                        let diameter_changed =
                            candidate.diameter_px != overlay_settings.diameter_px;
                        let map_surface_changed =
                            map_surface_refresh_required(&overlay_settings, &candidate);
                        runtime_settings_version =
                            runtime_settings_version.checked_add(1).ok_or_else(|| {
                                LocalPositionPreviewFailure::message("settings version exhausted")
                            })?;
                        if let Some(runtime) = runtime.as_mut() {
                            runtime.apply_settings(runtime_settings_version, candidate.clone())?;
                        }
                        overlay.set_effective_settings_version(settings_version);
                        overlay.set_input_mode(candidate.input_mode)?;
                        overlay.set_display_mode(candidate.display_mode)?;
                        overlay.set_opacity(candidate.opacity)?;
                        overlay.set_poi_filters(candidate.poi_filters.clone())?;
                        if candidate.hotkey_bindings != overlay_settings.hotkey_bindings {
                            apply_global_hotkeys(&mut hotkey_registry, candidate.hotkey_bindings);
                        }
                        overlay_settings = candidate;
                        #[cfg(feature = "approved-map-pack-runtime")]
                        if overlay_settings.display_mode != pal_domain::DisplayMode::ExpandedMap {
                            search_focus = None;
                            manual_pan_active = false;
                        }
                        surface_refresh_needed |= map_surface_changed;
                        if diameter_changed && let Some(window) = tracked_window.as_ref() {
                            attach_preview(&mut overlay, window, overlay_settings.diameter_px)
                                .map_err(LocalPositionPreviewFailure::new)?;
                        }
                        overlay.set_requested_visible(runtime_overlay_visible(
                            overlay_settings.enabled,
                            visibility_gate.requested_visible(),
                            map_surface_applied,
                            (browse_only || search_focus.is_some() || manual_pan_active)
                                && tracked_window.is_some(),
                        ))?;
                    }
                    ControlClientEvent::Disconnected => {
                        control_connected = false;
                        overlay.set_input_mode(pal_domain::InputMode::Locked)?;
                    }
                }
            }

            if let Some(control) = control_watcher.take_latest() {
                let candidate = control.settings.to_domain()?;
                let diameter_changed = candidate.diameter_px != overlay_settings.diameter_px;
                let map_surface_changed =
                    map_surface_refresh_required(&overlay_settings, &candidate);
                runtime_settings_version =
                    runtime_settings_version.checked_add(1).ok_or_else(|| {
                        LocalPositionPreviewFailure::message("settings version exhausted")
                    })?;
                if let Some(runtime) = runtime.as_mut() {
                    runtime.apply_settings(runtime_settings_version, candidate.clone())?;
                }
                overlay.set_input_mode(if control_connected {
                    candidate.input_mode
                } else {
                    pal_domain::InputMode::Locked
                })?;
                overlay.set_display_mode(candidate.display_mode)?;
                overlay.set_opacity(candidate.opacity)?;
                overlay.set_poi_filters(candidate.poi_filters.clone())?;
                if candidate.hotkey_bindings != overlay_settings.hotkey_bindings {
                    apply_global_hotkeys(&mut hotkey_registry, candidate.hotkey_bindings);
                }
                overlay_settings = candidate;
                #[cfg(feature = "approved-map-pack-runtime")]
                if overlay_settings.display_mode != pal_domain::DisplayMode::ExpandedMap {
                    search_focus = None;
                    manual_pan_active = false;
                }
                surface_refresh_needed |= map_surface_changed;
                if diameter_changed && let Some(window) = tracked_window.as_ref() {
                    attach_preview(&mut overlay, window, overlay_settings.diameter_px)
                        .map_err(LocalPositionPreviewFailure::new)?;
                }
                overlay.set_requested_visible(runtime_overlay_visible(
                    overlay_settings.enabled,
                    visibility_gate.requested_visible(),
                    map_surface_applied,
                    (browse_only || search_focus.is_some() || manual_pan_active)
                        && tracked_window.is_some(),
                ))?;
            }

            let now = Instant::now();
            if now >= next_tracking_poll || overlay.reattach_requested() {
                next_tracking_poll = now + TRACK_INTERVAL;
                match tracker.poll() {
                    Ok(Some(WindowEvent::Attached(window)))
                    | Ok(Some(WindowEvent::Changed {
                        current: window, ..
                    })) => {
                        let session_changed = if browse_only {
                            !same_tracked_game_process(tracked_window.as_ref(), &window)
                        } else {
                            !client_binding_matches(&verified_client, &window)
                        };
                        if session_changed {
                            overlay.set_requested_visible(false)?;
                            if let Some(signals) = signals {
                                signals.set_position_live(false)?;
                            }
                            runtime = None;
                            drop(worker.take());
                            active_surface_index = 0;
                            map_surface_applied = false;
                            last_presented_view = None;
                            #[cfg(feature = "approved-map-pack-runtime")]
                            {
                                clear_search_focus_for_session_loss(&mut search_focus);
                                manual_pan_active = false;
                            }
                            visibility_gate = PreviewVisibilityGate::default();
                            if browse_only {
                                verified_client = None;
                                eprintln!(
                                    "Exact Palworld window attached; verified map browsing is active without requiring a live-position build profile."
                                );
                            } else {
                                let binding = verify_tracked_steam_client(
                                    &window,
                                    WINDOWS_LIVE_POSITION_PROFILE.build_id(),
                                )?;
                                worker = Some(LocalPositionWorker::spawn(
                                    window.clone(),
                                    binding.clone(),
                                )?);
                                verified_client = Some(binding);
                                eprintln!(
                                    "Exact Steam client build verified; local read-only worker is initializing off the UI thread."
                                );
                            }
                        }
                        tracked_window = Some(window.clone());
                        attach_preview(&mut overlay, &window, overlay_settings.diameter_px)
                            .map_err(LocalPositionPreviewFailure::new)?;
                        surface_refresh_needed = true;
                        overlay.set_input_mode(if control_connected {
                            overlay_settings.input_mode
                        } else {
                            pal_domain::InputMode::Locked
                        })?;
                        overlay.set_display_mode(overlay_settings.display_mode)?;
                    }
                    Ok(Some(WindowEvent::Detached { .. })) => {
                        overlay.set_requested_visible(false)?;
                        if let Some(signals) = signals {
                            signals.set_position_live(false)?;
                        }
                        runtime = None;
                        worker = None;
                        verified_client = None;
                        tracked_window = None;
                        map_surface_applied = false;
                        last_presented_view = None;
                        surface_refresh_needed = false;
                        #[cfg(feature = "approved-map-pack-runtime")]
                        {
                            clear_search_focus_for_session_loss(&mut search_focus);
                            manual_pan_active = false;
                        }
                        visibility_gate = PreviewVisibilityGate::default();
                        overlay.detach()?;
                        eprintln!("Palworld detached; waiting for the game...");
                    }
                    Ok(None) => {}
                    Err(error) => eprintln!("Palworld window probe failed: {error}"),
                }
                overlay.refresh_topmost()?;
            }

            if runtime.is_none()
                && let Some(worker) = worker.as_mut()
            {
                match worker.poll_start() {
                    LocalPositionWorkerStart::Pending => {}
                    LocalPositionWorkerStart::Ready(source) => {
                        runtime = Some(ActualMapPreviewRuntime::new(
                            wrap_source(source),
                            overlay_settings.clone(),
                            map_surfaces[active_surface_index].world_to_image,
                        )?);
                        eprintln!(
                            "Local read-only worker is ready; waiting for a fresh position sample."
                        );
                    }
                    LocalPositionWorkerStart::Failed(_) | LocalPositionWorkerStart::Closed => {
                        overlay.set_requested_visible(false)?;
                        return Err(LocalPositionPreviewFailure::message(
                            "the local read-only worker failed before producing a position source",
                        ));
                    }
                }
            }

            let elapsed_ms = worker.as_ref().map_or(0, LocalPositionWorker::elapsed_ms);
            let active_surface = &map_surfaces[active_surface_index];
            let mut tick = match (runtime.as_mut(), worker.as_ref(), verified_client.as_ref()) {
                (Some(runtime), Some(_worker), Some(_)) => runtime.tick(
                    elapsed_ms,
                    active_surface.map.width(),
                    active_surface.map.height(),
                )?,
                _ => PreviewTick::Unchanged,
            };
            let visibility_tick = tick;
            if let Some(runtime) = runtime.as_mut()
                && let Some((world_x, world_y)) = runtime.last_world_position()
                && let Some(selected_index) =
                    select_local_map_surface_index(&map_surfaces, world_x, world_y)
                && {
                    #[cfg(feature = "approved-map-pack-runtime")]
                    {
                        search_focus.is_none() && !manual_pan_active
                    }
                    #[cfg(not(feature = "approved-map-pack-runtime"))]
                    {
                        true
                    }
                }
                && selected_index != active_surface_index
            {
                active_surface_index = selected_index;
                let selected = &map_surfaces[active_surface_index];
                runtime.select_world_to_image(selected.world_to_image);
                map_surface_applied = false;
                last_presented_view = None;
                surface_refresh_needed = true;
                tick = runtime.tick(elapsed_ms, selected.map.width(), selected.map.height())?;
                eprintln!(
                    "Overlay map region switched to {}/{} ({} POIs).",
                    selected.map_id,
                    selected.region_id,
                    selected.pois.len()
                );
            }
            let active_surface = &map_surfaces[active_surface_index];
            #[cfg(feature = "development-live-performance-diagnostic")]
            if let Some(freshness) = tick.freshness() {
                latest_freshness = Some(freshness);
            }
            #[cfg(feature = "approved-map-pack-runtime")]
            if search_focus.is_some() || manual_pan_active {
                if let Some(focus) = search_focus {
                    debug_assert_eq!(focus.surface_index, active_surface_index);
                }
                if let Some(view) = last_presented_view {
                    let view = refresh_retained_navigation_view(
                        view,
                        match tick {
                            PreviewTick::Present(frame) => Some(frame.view()),
                            PreviewTick::Unchanged
                            | PreviewTick::MetadataChanged(_)
                            | PreviewTick::Unavailable(_) => None,
                        },
                        runtime
                            .as_ref()
                            .and_then(ActualMapPreviewRuntime::last_live_world_pose),
                        active_surface.world_to_image,
                        active_surface.map.width(),
                        active_surface.map.height(),
                    )?;
                    if last_presented_view != Some(view) {
                        last_presented_view = Some(view);
                        surface_refresh_needed = true;
                    }
                }
                tick = PreviewTick::Unchanged;
            }
            #[cfg(feature = "approved-map-pack-runtime")]
            if browse_only
                && tracked_window.is_some()
                && search_focus.is_none()
                && !manual_pan_active
                && (surface_refresh_needed || last_presented_view.is_none())
            {
                last_presented_view =
                    Some(approved_browse_view(active_surface, overlay_settings.zoom)?);
            }
            #[cfg(feature = "approved-map-pack-runtime")]
            if (search_focus.is_some() || manual_pan_active || browse_only)
                && tracked_window.is_some()
                && !map_surface_applied
                && let Some(view) = last_presented_view
            {
                let mut surface = ActualMapSurface::new(overlay_settings.diameter_px)?;
                surface.rasterize_with_pois(
                    &active_surface.map,
                    view,
                    &active_surface.pois,
                    &overlay_settings.poi_filters,
                );
                apply_local_map_surface(&mut overlay, surface, map_approval)?;
                let _ = update_local_map_surface(
                    &mut overlay,
                    &active_surface.map,
                    view,
                    &active_surface.pois,
                    &overlay_settings.poi_filters,
                    map_approval,
                )?;
                map_surface_applied = true;
                surface_refresh_needed = false;
            }
            if let PreviewTick::Present(frame) = tick {
                let view = frame.view();
                last_presented_view = Some(view);
                if map_surface_applied {
                    let updated = update_local_map_surface(
                        &mut overlay,
                        &active_surface.map,
                        view,
                        &active_surface.pois,
                        &overlay_settings.poi_filters,
                        map_approval,
                    )?;
                    debug_assert!(updated, "local map surface is applied before updates");
                } else {
                    let mut surface = ActualMapSurface::new(overlay_settings.diameter_px)?;
                    surface.rasterize_with_pois(
                        &active_surface.map,
                        view,
                        &active_surface.pois,
                        &overlay_settings.poi_filters,
                    );
                    apply_local_map_surface(&mut overlay, surface, map_approval)?;
                    let _ = update_local_map_surface(
                        &mut overlay,
                        &active_surface.map,
                        view,
                        &active_surface.pois,
                        &overlay_settings.poi_filters,
                        map_approval,
                    )?;
                    map_surface_applied = true;
                }
                surface_refresh_needed = false;
            } else if surface_refresh_needed
                && map_surface_applied
                && let Some(view) = last_presented_view
            {
                let updated = update_local_map_surface(
                    &mut overlay,
                    &active_surface.map,
                    view,
                    &active_surface.pois,
                    &overlay_settings.poi_filters,
                    map_approval,
                )?;
                debug_assert!(updated, "a retained view refreshes the resized map surface");
                surface_refresh_needed = false;
            }
            let _ = visibility_gate.update(visibility_tick);
            if let Some(signals) = signals {
                let live_position_stream = runtime
                    .as_ref()
                    .and_then(ActualMapPreviewRuntime::last_live_world_pose)
                    .is_some();
                signals.set_position_live(live_position_stream && map_surface_applied)?;
            }
            overlay.set_requested_visible(runtime_overlay_visible(
                overlay_settings.enabled,
                visibility_gate.requested_visible(),
                map_surface_applied,
                (browse_only || search_focus.is_some() || manual_pan_active)
                    && tracked_window.is_some(),
            ))?;
            thread::sleep(match overlay_settings.fps_profile {
                pal_domain::FpsProfile::Thirty => Duration::from_millis(33),
                pal_domain::FpsProfile::Auto | pal_domain::FpsProfile::Sixty => FRAME_INTERVAL,
            });
        }
    })();
    #[cfg(feature = "development-live-performance-diagnostic")]
    let finished_at = result.as_ref().map_or_else(
        |failure| failure.selected_at,
        |terminal| terminal.selected_at,
    );

    #[cfg(feature = "development-live-performance-diagnostic")]
    {
        let finish_result = diagnostic.as_mut().map_or(Ok(false), |diagnostic| {
            diagnostic.finish(
                runtime.as_ref(),
                Some(&overlay),
                latest_freshness,
                finished_at,
            )
        });
        match result {
            Ok(_) => {
                finish_result?;
                Ok(())
            }
            Err(failure) => {
                let _ = finish_result;
                Err(failure.error)
            }
        }
    }

    #[cfg(not(feature = "development-live-performance-diagnostic"))]
    result.map(|_| ()).map_err(|failure| failure.error)
}

#[cfg(feature = "development-live-performance-diagnostic")]
fn run_development_local_live_performance_diagnostic(
    arguments: Vec<OsString>,
    signals: &OverlayRuntimeSignals,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = DevelopmentLocalPositionCommand::parse(arguments)?;
    debug_assert!(command.live_performance_diagnostic());
    let started_at = Instant::now();
    let source_counters = Rc::new(Cell::new(SourceCounters::default()));
    let mut diagnostic = LivePerformanceRun::new(started_at, Rc::clone(&source_counters));
    let map = match load_development_local_position_map(&command) {
        Ok(map) => map,
        Err(error) => {
            let _ = diagnostic.finish_without_runtime();
            return Err(error);
        }
    };
    let map_pois = match load_projected_pois(command.poi_catalog_path(), &map) {
        Ok(pois) => pois,
        Err(error) => {
            let _ = diagnostic.finish_without_runtime();
            return Err(error.into());
        }
    };
    run_local_position_preview_loop(
        vec![LocalMapRegionSurface::development(
            map,
            map_pois,
            authoritative_main_map_world_to_image(),
        )],
        LocalMapApproval::DevelopmentUnapproved,
        command.duration(),
        move |source| CountingPositionSource::new(source, Rc::clone(&source_counters)),
        Some(signals),
        Some(diagnostic),
    )
}

#[cfg(feature = "development-live-agent")]
fn run_live_agent_preview(
    arguments: Vec<OsString>,
    signals: &OverlayRuntimeSignals,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let command = LiveAgentCommand::parse(arguments)?;
    let config = load_live_source_config(command.config_path())?;
    let verified_map = config.load_verified_map_bmp()?;
    let map = MapRaster::decode_bmp(verified_map.bytes())?;
    if map.width() != 2_048 || map.height() != 2_048 {
        return Err("the exact-build development map preview dimensions are invalid".into());
    }
    let mut tracker = GameWindowTracker::new(Win32Backend::new());
    let mut overlay = OverlayWindowHost::create()?;
    overlay.apply_live_actual_map_surface(ActualMapSurface::new(420)?)?;
    overlay.set_requested_visible(false)?;
    let mut verified_client = None;
    let mut next_tracking_poll = Instant::now();
    eprintln!("Development live Agent mode is active; Gate B map alignment is not approved.");
    eprintln!("Live preview is waiting for the exact Steam Palworld client build...");

    while verified_client.is_none() {
        if overlay.pump_messages()? == PumpOutcome::ShutdownRequested {
            eprintln!("Overlay window closed before client verification; live preview is exiting.");
            return Ok(());
        }
        overlay.sync_fullscreen_consumer()?;
        let now = Instant::now();
        if now >= next_tracking_poll || overlay.reattach_requested() {
            next_tracking_poll = now + TRACK_INTERVAL;
            match tracker.poll() {
                Ok(Some(WindowEvent::Attached(window)))
                | Ok(Some(WindowEvent::Changed {
                    current: window, ..
                })) => {
                    attach_verified_live_preview(
                        &mut overlay,
                        &window,
                        &mut verified_client,
                        false,
                    )?;
                }
                Ok(Some(WindowEvent::Detached { .. })) => {
                    overlay.detach()?;
                }
                Ok(None) => {}
                Err(error) => eprintln!("Palworld window probe failed: {error}"),
            }
            overlay.refresh_topmost()?;
        }
        thread::sleep(FRAME_INTERVAL);
    }
    eprintln!("Exact Steam client build verified; connecting to the trusted Agent...");

    let network_config = config.load_network_consumer_config()?;
    let live = start_live_agent(network_config, config.world_alias(), config.subject_id())?;
    let mut runtime = ActualMapPreviewRuntime::new(
        live.source,
        pal_domain::OverlaySettings::default(),
        authoritative_main_map_world_to_image(),
    )?;
    let monotonic_epoch = live.monotonic_epoch;
    let supervisor = live.supervisor;
    let mut visibility_gate = PreviewVisibilityGate::default();
    eprintln!("Live preview is waiting for a trusted fresh position sample...");

    loop {
        if overlay.pump_messages()? == PumpOutcome::ShutdownRequested {
            eprintln!("Overlay window closed; live preview is exiting.");
            return supervisor.shutdown().map_err(Into::into);
        }
        overlay.sync_fullscreen_consumer()?;
        let now = Instant::now();
        if now >= next_tracking_poll || overlay.reattach_requested() {
            next_tracking_poll = now + TRACK_INTERVAL;
            match tracker.poll() {
                Ok(Some(WindowEvent::Attached(window))) => {
                    if !client_binding_matches(&verified_client, &window) {
                        reset_live_preview_session(&mut overlay, &mut visibility_gate)?;
                    }
                    attach_verified_live_preview(
                        &mut overlay,
                        &window,
                        &mut verified_client,
                        visibility_gate.requested_visible(),
                    )?;
                    eprintln!(
                        "Exact Steam client build verified; Palworld attached; {}",
                        preview_visibility_status(window.snapshot(), overlay.is_visible())
                    );
                }
                Ok(Some(WindowEvent::Changed { current, .. })) => {
                    if !client_binding_matches(&verified_client, &current) {
                        reset_live_preview_session(&mut overlay, &mut visibility_gate)?;
                    }
                    attach_verified_live_preview(
                        &mut overlay,
                        &current,
                        &mut verified_client,
                        visibility_gate.requested_visible(),
                    )?;
                }
                Ok(Some(WindowEvent::Detached { .. })) => {
                    verified_client = None;
                    reset_live_preview_session(&mut overlay, &mut visibility_gate)?;
                    overlay.detach()?;
                    eprintln!("Palworld detached; waiting for the game...");
                }
                Ok(None) => {}
                Err(error) => eprintln!("Palworld window probe failed: {error}"),
            }
            overlay.refresh_topmost()?;
        }

        let tick = if verified_client.is_some() {
            runtime.tick(monotonic_epoch.elapsed_ms(), map.width(), map.height())?
        } else {
            PreviewTick::Unchanged
        };
        match tick {
            PreviewTick::Present(frame) => {
                overlay.update_live_actual_map_surface(&map, frame.view())?;
            }
            PreviewTick::Unchanged
            | PreviewTick::MetadataChanged(_)
            | PreviewTick::Unavailable(_) => {}
        }
        if let Some(requested_visible) = visibility_gate.update(tick) {
            signals.set_position_live(requested_visible)?;
            overlay.set_requested_visible(requested_visible)?;
            let freshness = tick
                .freshness()
                .expect("a visibility transition always has freshness metadata");
            if requested_visible {
                eprintln!(
                    "Live position is {freshness:?}; {}",
                    live_position_visibility_status(true, overlay.is_visible())
                );
            } else {
                eprintln!(
                    "Live position is {freshness:?}; {}",
                    live_position_visibility_status(false, overlay.is_visible())
                );
            }
        }
        if supervisor.is_finished() {
            eprintln!("Live Agent connection stopped; stale position display remains disabled.");
            return supervisor.shutdown().map_err(Into::into);
        }
        thread::sleep(FRAME_INTERVAL);
    }
}

#[cfg(feature = "development-live-agent")]
fn attach_verified_live_preview(
    overlay: &mut OverlayWindowHost,
    game: &TrackedWindow,
    verified_client: &mut Option<VerifiedSteamClientBuild>,
    restore_requested_visible: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let requires_verification = !client_binding_matches(verified_client, game);
    if requires_verification {
        overlay.set_requested_visible(false)?;
        *verified_client = Some(verify_tracked_steam_client(
            game,
            DEV_DIAGNOSTIC_GAME_BUILD_ID,
        )?);
    }
    attach_preview(overlay, game, 420)?;
    if requires_verification && restore_requested_visible {
        overlay.set_requested_visible(true)?;
    }
    Ok(())
}

#[cfg(any(
    feature = "development-live-agent",
    any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    )
))]
fn client_binding_matches(
    verified_client: &Option<VerifiedSteamClientBuild>,
    game: &TrackedWindow,
) -> bool {
    verified_client
        .as_ref()
        .is_some_and(|binding| binding.matches(game))
}

#[cfg(feature = "development-live-agent")]
fn reset_live_preview_session(
    overlay: &mut OverlayWindowHost,
    visibility_gate: &mut PreviewVisibilityGate,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    overlay.set_requested_visible(false)?;
    overlay.apply_live_actual_map_surface(ActualMapSurface::new(420)?)?;
    *visibility_gate = PreviewVisibilityGate::default();
    Ok(())
}

#[cfg(feature = "development-live-agent")]
fn live_position_visibility_status(
    requested_visible: bool,
    effective_visible: bool,
) -> &'static str {
    if !requested_visible {
        "the live overlay is hidden."
    } else if effective_visible {
        "the live overlay is visible."
    } else {
        "the live overlay is armed but suspended by the game-window state."
    }
}

fn preview_visibility_status(
    snapshot: &pal_domain::WindowSnapshot,
    effective_visible: bool,
) -> &'static str {
    if effective_visible {
        "preview overlay is visible."
    } else if snapshot.minimized() {
        "preview is suspended while the game is minimized."
    } else if !snapshot.visible() {
        "preview is suspended while the game window is hidden."
    } else if !snapshot.active() {
        "preview is suspended while the game is inactive."
    } else {
        "preview is hidden by the current overlay setting."
    }
}

fn attach_preview(
    overlay: &mut OverlayWindowHost,
    game: &TrackedWindow,
    diameter: u32,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let snapshot = game.snapshot();
    let layouts = preview_layouts(
        snapshot.client_left(),
        snapshot.client_top(),
        snapshot.client_width(),
        snapshot.client_height(),
        snapshot.dpi(),
        diameter,
    )?;
    overlay.apply_layouts(layouts)?;
    overlay.attach(game)?;
    Ok(())
}

#[cfg(feature = "test-harness")]
fn configure_interactive_map_preview(
    overlay: &mut OverlayWindowHost,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    overlay.set_display_mode(pal_domain::DisplayMode::ExpandedMap)?;
    overlay.set_input_mode(pal_domain::InputMode::PinnedInteractive)?;
    overlay.set_fullscreen_search_catalog(&[
        FrameSearchEntry {
            kind: FrameSearchKind::Resource,
            title: "석탄 광맥".to_owned(),
            subtitle: "자원".to_owned(),
        },
        FrameSearchEntry {
            kind: FrameSearchKind::Resource,
            title: "금속 광맥".to_owned(),
            subtitle: "자원".to_owned(),
        },
        FrameSearchEntry {
            kind: FrameSearchKind::Resource,
            title: "수정 광맥".to_owned(),
            subtitle: "자원".to_owned(),
        },
    ])?;
    Ok(())
}

fn preview_layouts(
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    dpi: u32,
    requested_diameter_dip: u32,
) -> Result<pal_overlay_win::TopLeftOverlayLayouts, pal_overlay_win::TopLeftLayoutError> {
    top_left_overlay_layouts(
        PhysicalRect::new(left, top, width, height),
        dpi,
        requested_diameter_dip,
    )
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "development-live-performance-diagnostic")]
    use std::{cell::Cell, collections::VecDeque, rc::Rc};

    #[cfg(feature = "development-live-agent")]
    use super::live_position_visibility_status;
    #[cfg(feature = "development-local-alignment-diagnostic")]
    use super::local_alignment_completion_is_before_deadline;
    #[cfg(feature = "development-local-readonly-position")]
    use super::local_position_mode_requested;
    #[cfg(feature = "approved-map-pack-runtime")]
    use super::{
        ApprovedMapPackCommand, SearchFocus, approved_map_pack_mode_requested,
        clear_search_focus_for_session_loss, merge_live_player_marker,
    };
    #[cfg(feature = "development-live-performance-diagnostic")]
    use super::{
        CountingPositionSource, LivePerformanceFinalizer, LivePerformanceSummary,
        LocalPositionPreviewFailure, LocalPositionPreviewTerminal, SourceCounters,
        freshness_literal, live_performance_deadline, select_local_preview_terminal,
    };
    #[cfg(any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ))]
    use super::{LocalMapApproval, prepare_local_map_startup, same_process_identity};
    use super::{
        initial_preview_requested_visibility, preview_visibility_status, runtime_overlay_visible,
    };
    #[cfg(any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ))]
    use pal_domain::Freshness;
    use pal_domain::WindowSnapshot;
    #[cfg(feature = "development-live-performance-diagnostic")]
    use pal_domain::{PositionSample, SampleClock};
    #[cfg(any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ))]
    use pal_overlay_win::actual_map_runtime::{PreviewTick, PreviewVisibilityGate};
    #[cfg(feature = "development-live-performance-diagnostic")]
    use pal_state::{PositionSource, PositionSourceError, PositionSourceEvent};

    fn snapshot(visible: bool, active: bool, minimized: bool) -> WindowSnapshot {
        WindowSnapshot::new(1, 0, 0, 1_920, 1_080, 96, visible, active, minimized)
            .expect("valid window snapshot")
    }

    #[cfg(feature = "development-local-alignment-diagnostic")]
    #[test]
    fn alignment_completion_accepts_only_times_strictly_before_the_deadline() {
        let started_at = std::time::Instant::now();
        let deadline = started_at + std::time::Duration::from_secs(1);

        assert!(local_alignment_completion_is_before_deadline(
            deadline - std::time::Duration::from_nanos(1),
            deadline
        ));
        assert!(!local_alignment_completion_is_before_deadline(
            deadline, deadline
        ));
        assert!(!local_alignment_completion_is_before_deadline(
            deadline + std::time::Duration::from_nanos(1),
            deadline
        ));
    }

    #[test]
    fn status_never_claims_visibility_for_an_inactive_attachment() {
        assert_eq!(
            preview_visibility_status(&snapshot(true, false, false), false),
            "preview is suspended while the game is inactive."
        );
        assert_eq!(
            preview_visibility_status(&snapshot(true, true, false), true),
            "preview overlay is visible."
        );
    }

    #[test]
    fn status_distinguishes_minimized_hidden_and_user_hidden_states() {
        assert_eq!(
            preview_visibility_status(&snapshot(true, false, true), false),
            "preview is suspended while the game is minimized."
        );
        assert_eq!(
            preview_visibility_status(&snapshot(false, false, false), false),
            "preview is suspended while the game window is hidden."
        );
        assert_eq!(
            preview_visibility_status(&snapshot(true, true, false), false),
            "preview is hidden by the current overlay setting."
        );
    }

    #[cfg(feature = "development-local-readonly-position")]
    #[test]
    fn local_readonly_dispatch_recognizes_its_opt_in_in_any_argument_position() {
        assert!(local_position_mode_requested(&[
            "--real-map-bmp".into(),
            "map.bmp".into(),
            "--development-local-readonly-position".into(),
            "--allow-unapproved-map-smoke".into(),
        ]));
        assert!(!local_position_mode_requested(&[
            "--real-map-bmp".into(),
            "map.bmp".into(),
            "--allow-unapproved-map-smoke".into(),
        ]));
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    #[test]
    fn approved_map_dispatch_requires_an_absolute_root_and_exact_build() {
        let root = std::env::current_dir().unwrap();
        let arguments = vec![
            "--approved-map-pack".into(),
            root.into_os_string(),
            "--game-build-id".into(),
            "24181527".into(),
        ];
        assert!(approved_map_pack_mode_requested(&arguments));
        let command = ApprovedMapPackCommand::parse(&arguments).expect("valid approved command");
        assert_eq!(command.game_build_id, "24181527");

        assert!(
            ApprovedMapPackCommand::parse(&[
                "--approved-map-pack".into(),
                "relative".into(),
                "--game-build-id".into(),
                "24181527".into(),
            ])
            .is_err()
        );
        assert!(
            ApprovedMapPackCommand::parse(&[
                "--approved-map-pack".into(),
                std::env::current_dir().unwrap().into_os_string(),
                "--game-build-id".into(),
                "not-a-build".into(),
            ])
            .is_err()
        );
    }

    #[cfg(all(
        feature = "development-local-readonly-position",
        not(feature = "test-harness")
    ))]
    #[test]
    fn unknown_standard_arguments_fail_before_overlay_creation() {
        assert!(super::run_standard_preview(vec!["--unknown".into()]).is_err());
    }

    #[cfg(not(feature = "test-harness"))]
    #[test]
    fn help_does_not_allow_unknown_standard_arguments_before_overlay_creation() {
        assert!(super::run_standard_preview(vec!["--help".into()]).is_ok());
        for arguments in [
            vec!["--help".into(), "--bogus".into()],
            vec!["--bogus".into(), "--help".into()],
        ] {
            assert!(super::run_standard_preview(arguments).is_err());
        }
    }

    #[test]
    fn a_preview_shell_starts_hidden_until_renderable_content_is_applied() {
        assert!(!initial_preview_requested_visibility());
    }

    #[test]
    fn runtime_overlay_requires_live_position_or_attached_browse_mode() {
        assert!(runtime_overlay_visible(true, true, true, false));
        assert!(runtime_overlay_visible(true, false, true, true));
        assert!(!runtime_overlay_visible(true, false, true, false));
        assert!(!runtime_overlay_visible(true, true, false, false));
        assert!(!runtime_overlay_visible(false, true, true, false));
    }

    #[cfg(any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ))]
    #[test]
    fn raster_settings_refresh_a_retained_map_surface() {
        let current = pal_domain::OverlaySettings::default();

        let mut filters = current.clone();
        filters.poi_filters.wanted = !filters.poi_filters.wanted;
        assert!(super::map_surface_refresh_required(&current, &filters));

        let mut rotation = current.clone();
        rotation.rotation_mode = pal_domain::RotationMode::HeadingUp;
        assert!(super::map_surface_refresh_required(&current, &rotation));

        let mut zoom = current.clone();
        zoom.zoom = 1.25;
        assert!(super::map_surface_refresh_required(&current, &zoom));

        let mut opacity = current.clone();
        opacity.opacity = 0.7;
        assert!(!super::map_surface_refresh_required(&current, &opacity));
    }

    #[cfg(any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ))]
    #[test]
    fn browse_session_identity_ignores_window_state_changes_but_not_process_replacement() {
        assert!(same_process_identity(0x100, 42, 0x100, 42));
        assert!(!same_process_identity(0x100, 42, 0x100, 43));
        assert!(!same_process_identity(0x100, 42, 0x101, 42));
    }

    #[cfg(any(
        feature = "development-local-readonly-position",
        feature = "approved-map-pack-runtime"
    ))]
    #[test]
    fn an_unavailable_world_hides_even_when_content_remains_renderable() {
        let mut gate = PreviewVisibilityGate::default();
        assert_eq!(
            gate.update(PreviewTick::MetadataChanged(Freshness::Live)),
            Some(true)
        );
        assert!(runtime_overlay_visible(
            true,
            gate.requested_visible(),
            true,
            false,
        ));

        assert_eq!(
            gate.update(PreviewTick::Unavailable(Freshness::Stale)),
            Some(false)
        );
        assert!(!runtime_overlay_visible(
            true,
            gate.requested_visible(),
            true,
            false,
        ));
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    #[test]
    fn session_loss_clears_a_retained_search_focus() {
        let mut search_focus = Some(SearchFocus {
            surface_index: 1,
            map_x: 100.0,
            map_y: 200.0,
        });

        clear_search_focus_for_session_loss(&mut search_focus);

        assert_eq!(search_focus, None);
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    #[test]
    fn searched_expanded_map_keeps_and_refreshes_the_live_player_marker() {
        let focused =
            pal_overlay_win::actual_map_preview::MapView::search_focus(1_000.0, 2_000.0, 1.25)
                .unwrap();
        let region = pal_overlay_win::actual_map_preview::WorldToImageTransform::from_world_bounds(
            0.0, 0.0, 100.0, 100.0,
        )
        .expect("region transform");
        assert_eq!(focused.player_map_pose(), None);

        let live = merge_live_player_marker(
            focused,
            Some((25.0, 75.0, Some(405.0))),
            region,
            2_000,
            4_000,
        )
        .unwrap();
        assert_eq!(live.center_x(), 1_000.0);
        assert_eq!(live.center_y(), 2_000.0);
        assert_eq!(live.player_map_pose(), Some((1_500.0, 3_000.0, 45.0)));

        let outside_region =
            merge_live_player_marker(live, Some((125.0, 75.0, Some(90.0))), region, 2_000, 4_000)
                .unwrap();
        assert_eq!(outside_region.player_map_pose(), None);

        let unavailable =
            merge_live_player_marker(outside_region, None, region, 2_000, 4_000).unwrap();
        assert_eq!(unavailable.player_map_pose(), None);
    }

    #[cfg(feature = "development-local-readonly-position")]
    #[test]
    fn local_smoke_hides_the_shell_before_announcing_startup() {
        let observed = std::cell::RefCell::new(Vec::new());

        prepare_local_map_startup(
            LocalMapApproval::DevelopmentUnapproved,
            |_| {
                observed.borrow_mut().push("hidden");
                Ok::<(), std::convert::Infallible>(())
            },
            |message| observed.borrow_mut().push(message),
        )
        .unwrap();

        assert_eq!(
            observed.into_inner(),
            [
                "hidden",
                "DEVELOPMENT LOCAL READ-ONLY SMOKE ONLY: Gate B map alignment is unapproved; no game memory is written.",
                "Local smoke preview is waiting for the exact Steam Palworld client build...",
            ]
        );
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    #[test]
    fn approved_map_startup_never_reports_an_unapproved_surface() {
        let observed = std::cell::RefCell::new(Vec::new());

        prepare_local_map_startup(
            LocalMapApproval::Approved,
            |_| {
                observed.borrow_mut().push("hidden");
                Ok::<(), std::convert::Infallible>(())
            },
            |message| observed.borrow_mut().push(message),
        )
        .unwrap();

        let observed = observed.into_inner();
        assert_eq!(observed[0], "hidden");
        assert!(
            observed[1].contains("Approved exact-build map pack"),
            "approved mode must identify its strict map boundary"
        );
        assert!(
            !observed
                .iter()
                .any(|message| message.contains("unapproved"))
        );
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    struct FixturePositionSource {
        events: VecDeque<Option<PositionSourceEvent>>,
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    impl PositionSource for FixturePositionSource {
        fn poll(
            &mut self,
            _now_monotonic_ms: u64,
        ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
            Ok(self.events.pop_front().unwrap_or(None))
        }
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn counting_position_source_counts_polls_and_samples() {
        let sample = PositionSample::new(
            "world",
            b"subject",
            b"boot",
            1,
            1,
            1.0,
            2.0,
            3.0,
            Some(4.0),
            SampleClock::received_with_age(0, 0),
        )
        .expect("valid fixture sample");
        let counters = Rc::new(Cell::new(SourceCounters::default()));
        let mut source = CountingPositionSource::new(
            FixturePositionSource {
                events: VecDeque::from([Some(PositionSourceEvent::Sample(sample)), None]),
            },
            Rc::clone(&counters),
        );

        assert!(source.poll(0).unwrap().is_some());
        assert!(source.poll(0).unwrap().is_none());
        assert_eq!(
            counters.get(),
            SourceCounters {
                polls: 2,
                samples: 1,
            }
        );
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn zero_live_performance_summary_has_fixed_json_schema() {
        let mut output = Vec::new();
        LivePerformanceSummary::default()
            .write_json_line(&mut output)
            .expect("write summary");

        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "{\"source_polls\":0,\"source_samples\":0,",
                "\"stationary_suppressed_frames\":0,\"rasterized_frames\":0,",
                "\"raster_cpu_timing_samples\":0,\"raster_cpu_timing_failures\":0,",
                "\"raster_cpu_100ns_total\":0,\"present_requests\":0,\"paint_calls\":0,",
                "\"upload_calls\":0,\"process_lifetime_ms\":0,\"final_visible\":false,",
                "\"final_freshness\":\"unknown\"}\n"
            )
        );
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn final_freshness_uses_only_permitted_lowercase_literals() {
        assert_eq!(freshness_literal(None), "unknown");
        assert_eq!(freshness_literal(Some(Freshness::Live)), "live");
        assert_eq!(freshness_literal(Some(Freshness::Delayed)), "delayed");
        assert_eq!(freshness_literal(Some(Freshness::Stale)), "stale");
        assert_eq!(freshness_literal(Some(Freshness::Offline)), "offline");
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn shutdown_finalization_is_idempotent_and_emits_one_summary() {
        let started_at = std::time::Instant::now();
        let mut finalizer = LivePerformanceFinalizer::new(started_at);
        let mut output = Vec::new();

        assert!(
            finalizer
                .finish_at(
                    &mut output,
                    LivePerformanceSummary::default(),
                    started_at + std::time::Duration::from_millis(125),
                )
                .unwrap()
        );
        assert!(
            !finalizer
                .finish_at(
                    &mut output,
                    LivePerformanceSummary::default(),
                    started_at + std::time::Duration::from_secs(1),
                )
                .unwrap()
        );

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert!(output.contains("\"process_lifetime_ms\":125"));
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn diagnostic_deadline_and_lifetime_share_the_post_parse_epoch() {
        let started_at = std::time::Instant::now();
        let deadline =
            live_performance_deadline(started_at, std::time::Duration::from_secs(3)).unwrap();
        let mut finalizer = LivePerformanceFinalizer::new(started_at);
        let mut output = Vec::new();

        finalizer
            .finish_at(&mut output, LivePerformanceSummary::default(), deadline)
            .unwrap();

        assert_eq!(deadline.duration_since(started_at).as_secs(), 3);
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("\"process_lifetime_ms\":3000")
        );
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn terminal_time_is_selected_before_reporting_side_effects() {
        let selected_at = std::time::Instant::now();
        let reported = std::cell::Cell::new(false);

        let terminal = select_local_preview_terminal(
            LocalPositionPreviewTerminal::DeadlineExpired,
            selected_at,
            |_| reported.set(true),
        );

        assert!(reported.get());
        assert_eq!(terminal.selected_at, selected_at);
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn runtime_error_time_is_captured_during_error_propagation() {
        fn fail() -> Result<(), LocalPositionPreviewFailure> {
            Err(std::io::Error::other("fixture runtime error"))?;
            Ok(())
        }

        let before = std::time::Instant::now();
        let failure = fail().unwrap_err();
        let after = std::time::Instant::now();

        assert!(failure.selected_at >= before);
        assert!(failure.selected_at <= after);
    }

    #[cfg(feature = "development-live-agent")]
    #[test]
    fn live_status_never_reports_visible_when_the_gate_or_window_hides_it() {
        assert_eq!(
            live_position_visibility_status(false, false),
            "the live overlay is hidden."
        );
        assert_eq!(
            live_position_visibility_status(true, false),
            "the live overlay is armed but suspended by the game-window state."
        );
        assert_eq!(
            live_position_visibility_status(true, true),
            "the live overlay is visible."
        );
    }
}
