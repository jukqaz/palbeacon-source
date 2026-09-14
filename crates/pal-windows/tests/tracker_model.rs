use pal_windows::{
    FakeWindowBackend, GameWindowTracker, MonitorId, PALWORLD_IMAGE_NAME, PhysicalClientRect,
    TrackerError, WindowBackendError, WindowBackendOperation, WindowEvent, WindowId,
    WindowObservation, WindowObservationParts,
};

fn observation(id: u64, process_id: u32) -> WindowObservation {
    WindowObservation::for_test(WindowObservationParts {
        id: WindowId::from_raw(id),
        monitor_id: MonitorId::from_raw(1),
        process_id,
        image_path: format!(r"C:\Games\Palworld\Pal\Binaries\Win64\{PALWORLD_IMAGE_NAME}"),
        window_title: "Palworld".to_owned(),
        client_rect: PhysicalClientRect::new(100, 200, 1_280, 720),
        dpi: 96,
        visible: true,
        top_level: true,
        foreground: true,
        minimized: false,
        inspectable: true,
    })
}

fn attached(event: Option<WindowEvent>) -> pal_windows::TrackedWindow {
    match event {
        Some(WindowEvent::Attached(window)) => window,
        other => panic!("expected Attached, got {other:?}"),
    }
}

fn changed(event: Option<WindowEvent>) -> (pal_windows::TrackedWindow, pal_windows::TrackedWindow) {
    match event {
        Some(WindowEvent::Changed { previous, current }) => (previous, current),
        other => panic!("expected Changed, got {other:?}"),
    }
}

#[test]
fn no_candidate_produces_no_event() {
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::default());
    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn visible_top_level_exact_shipping_image_attaches() {
    let mut tracker =
        GameWindowTracker::new(FakeWindowBackend::with_windows([observation(0xBEEF, 42)]));

    let window = attached(tracker.poll().unwrap());
    assert_eq!(window.id(), WindowId::from_raw(0xBEEF));
    assert_eq!(window.monitor_id(), MonitorId::from_raw(1));
    assert_eq!(window.snapshot().process_id(), 42);
    assert_eq!(
        window.image_path(),
        r"C:\Games\Palworld\Pal\Binaries\Win64\Palworld-Win64-Shipping.exe"
    );
    assert_eq!(window.snapshot().client_left(), 100);
    assert_eq!(window.snapshot().client_top(), 200);
    assert_eq!(window.snapshot().client_width(), 1_280);
    assert_eq!(window.snapshot().client_height(), 720);
}

#[test]
fn debug_output_redacts_the_full_process_image_path() {
    let candidate = observation(0xBEEF, 42);
    let observation_debug = format!("{candidate:?}");
    assert!(observation_debug.contains("<redacted>"));
    assert!(!observation_debug.contains(r"C:\Games\Palworld"));

    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([candidate]));
    let tracked = attached(tracker.poll().unwrap());
    let tracked_debug = format!("{tracked:?}");
    assert!(tracked_debug.contains("<redacted>"));
    assert!(!tracked_debug.contains(r"C:\Games\Palworld"));
}

#[test]
fn an_image_path_change_is_reported_without_losing_the_full_path() {
    let backend = FakeWindowBackend::with_windows([observation(0xBEEF, 42)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());

    let moved = observation(0xBEEF, 42).with_image_path_for_test(
        r"D:\SteamLibrary\steamapps\common\Palworld\Pal\Binaries\Win64\Palworld-Win64-Shipping.exe"
            .to_owned(),
    );
    probe.replace_windows([moved]);

    let (_, current) = changed(tracker.poll().unwrap());
    assert_eq!(
        current.image_path(),
        r"D:\SteamLibrary\steamapps\common\Palworld\Pal\Binaries\Win64\Palworld-Win64-Shipping.exe"
    );
}

#[test]
fn hidden_child_lookalike_and_title_only_candidates_are_rejected() {
    let mut hidden = observation(1, 1);
    hidden = hidden.with_visible_for_test(false);
    let mut child = observation(2, 2);
    child = child.with_top_level_for_test(false);
    let mut lookalike = observation(3, 3);
    lookalike = lookalike
        .with_image_path_for_test(r"C:\Games\Palworld-Win64-Shipping.exe.backup".to_owned());
    let mut prefixed = observation(4, 4);
    prefixed =
        prefixed.with_image_path_for_test(r"C:\Games\Not-Palworld-Win64-Shipping.exe".to_owned());
    let mut title_only = observation(5, 5);
    title_only = title_only
        .with_image_path_for_test(r"C:\Games\Other.exe".to_owned())
        .with_window_title_for_test(PALWORLD_IMAGE_NAME.to_owned());

    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([
        hidden, child, lookalike, prefixed, title_only,
    ]));

    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn exact_image_basename_matching_is_case_insensitive() {
    let candidate = observation(7, 7)
        .with_image_path_for_test(r"C:\Games\PALWORLD-WIN64-SHIPPING.EXE".to_owned());
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([candidate]));

    assert_eq!(
        attached(tracker.poll().unwrap()).id(),
        WindowId::from_raw(7)
    );
}

#[test]
fn new_candidate_order_is_foreground_then_area_then_smallest_id() {
    let mut background_huge = observation(1, 1)
        .with_foreground_for_test(false)
        .with_client_rect_for_test(PhysicalClientRect::new(0, 0, 4_000, 4_000));
    let foreground_small =
        observation(9, 9).with_client_rect_for_test(PhysicalClientRect::new(0, 0, 500, 500));
    let foreground_large_high_id =
        observation(8, 8).with_client_rect_for_test(PhysicalClientRect::new(0, 0, 1_000, 900));
    let foreground_large_low_id =
        observation(4, 4).with_client_rect_for_test(PhysicalClientRect::new(0, 0, 900, 1_000));
    background_huge = background_huge.with_monitor_for_test(MonitorId::from_raw(2));

    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([
        foreground_large_high_id,
        background_huge,
        foreground_small,
        foreground_large_low_id,
    ]));

    assert_eq!(
        attached(tracker.poll().unwrap()).id(),
        WindowId::from_raw(4)
    );
}

#[test]
fn current_valid_hwnd_is_retained_without_enumerating_again() {
    let backend = FakeWindowBackend::with_windows([observation(10, 10)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    let enumerations_after_attach = probe.calls().enumerate_top_level;

    let current = observation(10, 10);
    let better_new_candidate =
        observation(1, 1).with_client_rect_for_test(PhysicalClientRect::new(0, 0, 4_000, 4_000));
    probe.replace_windows([current, better_new_candidate]);

    assert_eq!(tracker.poll().unwrap(), None);
    assert_eq!(probe.calls().enumerate_top_level, enumerations_after_attach);
    assert_eq!(probe.calls().inspected, vec![WindowId::from_raw(10)]);
}

#[test]
fn current_inspection_failure_falls_back_to_enumeration_and_attaches_replacement() {
    let backend = FakeWindowBackend::with_windows([observation(50, 50)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.fail_inspection(
        WindowId::from_raw(50),
        WindowBackendError::new(WindowBackendOperation::Inspect, 6),
    );
    probe.replace_windows([observation(51, 51)]);

    let replacement = attached(tracker.poll().unwrap());

    assert_eq!(replacement.id(), WindowId::from_raw(51));
    assert_eq!(replacement.snapshot().process_id(), 51);
    assert_eq!(probe.calls().enumerate_top_level, 2);
}

#[test]
fn current_inspection_failure_does_not_mask_typed_enumeration_failure() {
    let backend = FakeWindowBackend::with_windows([observation(52, 52)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.fail_inspection(
        WindowId::from_raw(52),
        WindowBackendError::new(WindowBackendOperation::Inspect, 6),
    );
    probe.fail_enumeration(WindowBackendError::new(
        WindowBackendOperation::EnumerateTopLevel,
        5,
    ));

    assert_eq!(
        tracker.poll().unwrap_err(),
        TrackerError::Backend(WindowBackendError::new(
            WindowBackendOperation::EnumerateTopLevel,
            5,
        ))
    );
}

#[test]
fn identical_repoll_produces_no_event() {
    let backend = FakeWindowBackend::with_windows([observation(11, 11)]);
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());

    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn client_move_publishes_exactly_one_changed_event() {
    let backend = FakeWindowBackend::with_windows([observation(12, 12)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.replace_windows([observation(12, 12)
        .with_client_rect_for_test(PhysicalClientRect::new(-300, 440, 1_280, 720))]);

    let (previous, current) = changed(tracker.poll().unwrap());
    assert_eq!(previous.snapshot().client_left(), 100);
    assert_eq!(current.snapshot().client_left(), -300);
    assert_eq!(current.snapshot().client_top(), 440);
    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn resize_publishes_exactly_one_changed_event() {
    let backend = FakeWindowBackend::with_windows([observation(13, 13)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.replace_windows([observation(13, 13)
        .with_client_rect_for_test(PhysicalClientRect::new(100, 200, 1_920, 1_080))]);

    let (_, current) = changed(tracker.poll().unwrap());
    assert_eq!(current.snapshot().client_width(), 1_920);
    assert_eq!(current.snapshot().client_height(), 1_080);
    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn each_dpi_transition_publishes_one_changed_event() {
    let backend = FakeWindowBackend::with_windows([observation(14, 14)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());

    for dpi in [120, 144] {
        probe.replace_windows([observation(14, 14).with_dpi_for_test(dpi)]);
        let (_, current) = changed(tracker.poll().unwrap());
        assert_eq!(current.snapshot().dpi(), dpi);
        assert_eq!(tracker.poll().unwrap(), None);
    }
}

#[test]
fn foreground_change_publishes_changed() {
    let backend = FakeWindowBackend::with_windows([observation(15, 15)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.replace_windows([observation(15, 15).with_foreground_for_test(false)]);

    let (_, current) = changed(tracker.poll().unwrap());
    assert!(!current.snapshot().active());
}

#[test]
fn minimize_and_restore_each_publish_changed_while_remaining_attached() {
    let backend = FakeWindowBackend::with_windows([observation(16, 16)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());

    probe.replace_windows([observation(16, 16).with_minimized_for_test(true)]);
    let (_, minimized) = changed(tracker.poll().unwrap());
    assert!(minimized.snapshot().minimized());

    probe.replace_windows([observation(16, 16).with_minimized_for_test(false)]);
    let (_, restored) = changed(tracker.poll().unwrap());
    assert!(!restored.snapshot().minimized());
}

#[test]
fn minimized_current_window_remains_attached_when_win32_reports_it_not_visible() {
    let backend = FakeWindowBackend::with_windows([observation(160, 160)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());

    probe.replace_windows([observation(160, 160)
        .with_visible_for_test(false)
        .with_minimized_for_test(true)
        .with_client_rect_for_test(PhysicalClientRect::new(0, 1_364, 0, 0))]);

    let (_, minimized) = changed(tracker.poll().unwrap());
    assert_eq!(minimized.id(), WindowId::from_raw(160));
    assert!(minimized.snapshot().minimized());
    assert!(!minimized.snapshot().visible());
    assert_eq!(minimized.snapshot().client_left(), 100);
    assert_eq!(minimized.snapshot().client_top(), 200);
    assert_eq!(minimized.snapshot().client_width(), 1_280);
    assert_eq!(minimized.snapshot().client_height(), 720);
}

#[test]
fn monitor_change_publishes_changed() {
    let backend = FakeWindowBackend::with_windows([observation(17, 17)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.replace_windows([observation(17, 17).with_monitor_for_test(MonitorId::from_raw(99))]);

    let (_, current) = changed(tracker.poll().unwrap());
    assert_eq!(current.monitor_id(), MonitorId::from_raw(99));
}

#[test]
fn negative_physical_client_origin_is_preserved() {
    let candidate = observation(18, 18)
        .with_client_rect_for_test(PhysicalClientRect::new(-2_560, -240, 1_920, 1_080));
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([candidate]));

    let current = attached(tracker.poll().unwrap());
    assert_eq!(current.snapshot().client_left(), -2_560);
    assert_eq!(current.snapshot().client_top(), -240);
}

#[test]
fn disappearance_publishes_detached_exactly_once() {
    let backend = FakeWindowBackend::with_windows([observation(19, 19)]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.replace_windows([]);

    let previous = match tracker.poll().unwrap() {
        Some(WindowEvent::Detached { previous }) => previous,
        other => panic!("expected Detached, got {other:?}"),
    };
    assert_eq!(previous.id(), WindowId::from_raw(19));
    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn restarted_process_or_hwnd_publishes_new_attached_without_old_geometry() {
    let backend = FakeWindowBackend::with_windows([
        observation(20, 20).with_client_rect_for_test(PhysicalClientRect::new(1, 2, 300, 400))
    ]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    attached(tracker.poll().unwrap());
    probe.replace_windows([observation(21, 99)
        .with_client_rect_for_test(PhysicalClientRect::new(700, 800, 1_600, 900))]);

    let replacement = attached(tracker.poll().unwrap());
    assert_eq!(replacement.id(), WindowId::from_raw(21));
    assert_eq!(replacement.snapshot().process_id(), 99);
    assert_eq!(replacement.snapshot().client_left(), 700);
    assert_eq!(replacement.snapshot().client_top(), 800);
}

#[test]
fn zero_visible_width_height_or_dpi_candidates_are_rejected() {
    let candidates = [
        observation(30, 30).with_client_rect_for_test(PhysicalClientRect::new(0, 0, 0, 720)),
        observation(31, 31).with_client_rect_for_test(PhysicalClientRect::new(0, 0, 1_280, 0)),
        observation(32, 32).with_dpi_for_test(0),
    ];
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows(candidates));

    assert_eq!(tracker.poll().unwrap(), None);
}

#[test]
fn inaccessible_candidates_are_skipped_and_enumeration_failure_is_typed_and_sanitized() {
    let inaccessible = observation(40, 40).with_inspectable_for_test(false);
    let valid = observation(41, 41);
    let backend = FakeWindowBackend::with_windows([inaccessible, valid]);
    let probe = backend.clone();
    let mut tracker = GameWindowTracker::new(backend);
    assert_eq!(
        attached(tracker.poll().unwrap()).id(),
        WindowId::from_raw(41)
    );

    probe.replace_windows([]);
    changed_or_detached(&mut tracker);
    probe.fail_enumeration(WindowBackendError::new(
        WindowBackendOperation::EnumerateTopLevel,
        5,
    ));
    let error = tracker.poll().unwrap_err();
    assert_eq!(
        error,
        TrackerError::Backend(WindowBackendError::new(
            WindowBackendOperation::EnumerateTopLevel,
            5
        ))
    );
    let rendered = error.to_string();
    assert!(rendered.contains("enumerate_top_level"));
    assert!(rendered.contains('5'));
    assert!(!rendered.contains("Palworld"));
    assert!(!rendered.contains(r"C:\"));
}

fn changed_or_detached(tracker: &mut GameWindowTracker<FakeWindowBackend>) {
    match tracker.poll().unwrap() {
        Some(WindowEvent::Detached { .. }) => {}
        other => panic!("expected detach before forcing enumeration failure, got {other:?}"),
    }
}
