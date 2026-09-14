use std::sync::Arc;

use pal_domain::{DisplayMode, HotkeyAction, InputMode, OverlayAction};
#[cfg(feature = "test-harness")]
use pal_overlay_win::OverlayHostOperation;
#[cfg(feature = "test-harness")]
use pal_overlay_win::actual_map_preview::{ActualMapSurface, MapRaster, MapView};
use pal_overlay_win::{
    ControlIntent, HitTest, HostUpdate, OverlayHostError, OverlayWindowHost, PhysicalPoint,
    PhysicalRect, PumpOutcome, SuspendReason, TopLeftOverlayLayouts, renderer::RendererKind,
    top_left_overlay_layouts,
};
use pal_render::{FilterAction, GateBadge, RailAction, RenderSnapshot, build_chrome_plan};
use pal_windows::{
    FakeWindowBackend, GameWindowTracker, MonitorId, PhysicalClientRect, TrackedWindow,
    WindowEvent, WindowId, WindowObservation, WindowObservationParts, action_registration_id,
};
#[cfg(feature = "test-harness")]
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetForegroundWindow, GetLayeredWindowAttributes, GetWindowLongPtrW, GetWindowRect,
    HTCLIENT, HTTRANSPARENT, HWND_NOTOPMOST, IsWindow, IsWindowVisible, LWA_COLORKEY,
    MA_NOACTIVATE, PostMessageW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SendMessageW,
    SetWindowPos, WM_CLOSE, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

fn tracked() -> TrackedWindow {
    let observation = WindowObservation::for_test(WindowObservationParts {
        id: WindowId::from_raw(7),
        monitor_id: MonitorId::from_raw(9),
        process_id: 42,
        image_path: r"C:\Palworld-Win64-Shipping.exe".to_owned(),
        window_title: "not trusted".to_owned(),
        client_rect: PhysicalClientRect::new(-900, 40, 800, 600),
        dpi: 96,
        visible: true,
        top_level: true,
        foreground: true,
        minimized: false,
        inspectable: true,
    });
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([observation]));
    match tracker.poll().expect("tracking") {
        Some(WindowEvent::Attached(window)) => window,
        event => panic!("expected attached window, got {event:?}"),
    }
}

fn layouts() -> TopLeftOverlayLayouts {
    top_left_overlay_layouts(PhysicalRect::new(-900, 40, 800, 600), 96, 240)
        .expect("fixture layouts")
}

fn lparam(point: PhysicalPoint) -> isize {
    let x = u16::from_ne_bytes((point.x as i16).to_ne_bytes()) as u32;
    let y = u16::from_ne_bytes((point.y as i16).to_ne_bytes()) as u32;
    isize::try_from(x | (y << 16)).expect("packed coordinates")
}

#[test]
fn host_has_exact_nonactivating_styles_and_only_toggles_transparent() {
    let foreground = unsafe { GetForegroundWindow() };
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    assert_eq!(host.attach(&tracked()), Ok(HostUpdate::Changed));

    let base = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED;
    let locked = unsafe { GetWindowLongPtrW(host.raw_hwnd(), GWL_EXSTYLE) as u32 };
    assert_eq!(locked, base | WS_EX_TRANSPARENT);
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);

    host.set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");
    let interactive = unsafe { GetWindowLongPtrW(host.raw_hwnd(), GWL_EXSTYLE) as u32 };
    assert_eq!(interactive, base);
    assert_eq!(locked ^ interactive, WS_EX_TRANSPARENT);
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
}

#[test]
fn refresh_topmost_restores_the_overlay_after_a_fullscreen_z_order_rebuild() {
    let foreground = unsafe { GetForegroundWindow() };
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    host.attach(&tracked()).expect("attach");

    assert_ne!(
        unsafe {
            SetWindowPos(
                host.raw_hwnd(),
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        },
        0
    );
    assert_eq!(
        unsafe { GetWindowLongPtrW(host.raw_hwnd(), GWL_EXSTYLE) as u32 } & WS_EX_TOPMOST,
        0
    );

    host.refresh_topmost().expect("restore topmost band");

    assert_ne!(
        unsafe { GetWindowLongPtrW(host.raw_hwnd(), GWL_EXSTYLE) as u32 } & WS_EX_TOPMOST,
        0
    );
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
}

#[test]
fn layered_window_starts_with_the_renderer_background_color_key() {
    let host = OverlayWindowHost::create().expect("create overlay");
    let mut color_key = u32::MAX;
    let mut alpha = 0_u8;
    let mut flags = 0_u32;

    assert_ne!(
        unsafe {
            GetLayeredWindowAttributes(host.raw_hwnd(), &mut color_key, &mut alpha, &mut flags)
        },
        0
    );
    assert_eq!(color_key, pal_overlay_win::PREVIEW_COLOR_KEY);
    assert_eq!(flags, LWA_COLORKEY);
}

#[test]
fn minimap_hwnd_is_bounded_to_the_map_region_and_preserves_screen_hit_testing() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    host.attach(&tracked()).expect("attach");

    let mut window_rect = unsafe { std::mem::zeroed() };
    assert_ne!(
        unsafe { GetWindowRect(host.raw_hwnd(), &mut window_rect) },
        0
    );
    assert_eq!(
        (
            window_rect.left,
            window_rect.top,
            window_rect.right - window_rect.left,
            window_rect.bottom - window_rect.top,
        ),
        (-892, 48, 224, 224)
    );

    let center = PhysicalPoint::new(-780, 160);
    let corner = PhysicalPoint::new(-891, 49);
    assert_eq!(
        unsafe { SendMessageW(host.raw_hwnd(), WM_NCHITTEST, 0, lparam(center)) },
        HTTRANSPARENT as isize
    );

    host.set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");
    assert_eq!(
        unsafe { SendMessageW(host.raw_hwnd(), WM_NCHITTEST, 0, lparam(center)) },
        HTCLIENT as isize
    );
    assert_eq!(
        unsafe { SendMessageW(host.raw_hwnd(), WM_NCHITTEST, 0, lparam(corner)) },
        HTTRANSPARENT as isize
    );
}

#[test]
fn mouse_activation_is_refused_and_preview_needs_no_snapshot() {
    let foreground = unsafe { GetForegroundWindow() };
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    host.attach(&tracked()).expect("attach");

    assert_ne!(unsafe { IsWindowVisible(host.raw_hwnd()) }, 0);
    assert_eq!(
        unsafe { SendMessageW(host.raw_hwnd(), WM_MOUSEACTIVATE, 0, 0) },
        MA_NOACTIVATE as isize
    );
    assert_eq!(host.pump_messages(), Ok(PumpOutcome::Continue));
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
}

#[test]
fn host_diagnostics_identify_the_gdi_path_as_correctness_only() {
    let host = OverlayWindowHost::create().expect("create overlay");

    assert_eq!(host.renderer_kind(), RendererKind::GdiCorrectnessPreview);
    assert!(!host.renderer_kind().is_production_acceptance_renderer());
}

#[test]
fn interactive_preview_records_delivered_clicks_for_cross_process_proof() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    host.attach(&tracked()).expect("attach");
    host.set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");

    assert_eq!(host.received_clicks(), 0);
    unsafe { SendMessageW(host.raw_hwnd(), WM_LBUTTONDOWN, 0, 0) };
    assert_eq!(host.received_clicks(), 1);
}

#[test]
fn interactive_expanded_rail_queues_a_versioned_semantic_intent() {
    let responsive_layouts = layouts();
    let expanded = responsive_layouts.expanded();
    let surface = expanded.surface_shape.bounds();
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(responsive_layouts).expect("layouts");
    host.attach(&tracked()).expect("attach");
    host.set_display_mode(DisplayMode::ExpandedMap)
        .expect("expanded");
    host.set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");
    host.set_effective_settings_version(7);

    let screen = pal_overlay_win::gdi_correctness_renderer::GdiPaintPlan::from_layout(
        expanded,
        build_chrome_plan(
            DisplayMode::ExpandedMap,
            InputMode::PinnedInteractive,
            pal_domain::Freshness::Live,
            GateBadge::Approved,
        ),
    )
    .rail_glyph_center(RailAction::ZoomIn)
    .expect("zoom-in center");
    let client = PhysicalPoint::new(screen.x - surface.left, screen.y - surface.top);
    unsafe { SendMessageW(host.raw_hwnd(), WM_LBUTTONUP, 0, lparam(client)) };

    assert_eq!(
        host.take_control_intents(),
        vec![ControlIntent::Action {
            expected_settings_version: 7,
            action: OverlayAction::ZoomIn,
        }]
    );
}

#[test]
fn interactive_filter_strip_queues_the_matching_core_action() {
    let responsive_layouts = layouts();
    let expanded = responsive_layouts.expanded();
    let surface = expanded.surface_shape.bounds();
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(responsive_layouts).expect("layouts");
    host.attach(&tracked()).expect("attach");
    host.set_display_mode(DisplayMode::ExpandedMap)
        .expect("expanded");
    host.set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");
    host.set_effective_settings_version(12);

    let rect = pal_overlay_win::gdi_correctness_renderer::filter_button_rect(
        expanded,
        FilterAction::Wanted,
    )
    .expect("wanted filter rect");
    let client = PhysicalPoint::new(
        rect.left - surface.left + i32::try_from(rect.width).unwrap() / 2,
        rect.top - surface.top + i32::try_from(rect.height).unwrap() / 2,
    );
    unsafe { SendMessageW(host.raw_hwnd(), WM_LBUTTONUP, 0, lparam(client)) };

    assert_eq!(
        host.take_control_intents(),
        vec![ControlIntent::Action {
            expected_settings_version: 12,
            action: OverlayAction::ToggleWanted,
        }]
    );
}

#[test]
fn registered_global_hotkey_messages_are_delivered_as_semantic_actions() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    let action = HotkeyAction::RotationToggle;
    assert_ne!(
        unsafe {
            PostMessageW(
                host.raw_hwnd(),
                WM_HOTKEY,
                usize::try_from(action_registration_id(action)).expect("registration id"),
                0,
            )
        },
        0
    );

    assert_eq!(host.pump_messages(), Ok(PumpOutcome::Continue));
    assert_eq!(host.take_hotkey_actions(), vec![action]);
}

#[cfg(feature = "test-harness")]
#[test]
fn actual_map_update_records_raster_present_paint_and_upload() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    let responsive_layouts = layouts();
    host.apply_layouts(responsive_layouts).expect("layouts");
    host.attach(&tracked()).expect("attach");
    host.apply_actual_map_surface(
        ActualMapSurface::new(responsive_layouts.mini().viewport_size).expect("surface"),
    )
    .expect("apply actual map surface");
    assert_ne!(unsafe { UpdateWindow(host.raw_hwnd()) }, 0);

    let baseline_paint = host.actual_map_paint_performance_counters();
    let baseline_raster = host
        .actual_map_surface_performance_counters()
        .expect("actual map surface counters");
    let map = MapRaster::new(4, 4, vec![0x0001_0101; 16]).expect("map");
    let view = MapView::new(1.5, 1.5, 1.0, 0.0).expect("view");

    assert_eq!(host.update_actual_map_surface(&map, view), Ok(true));
    assert_ne!(unsafe { UpdateWindow(host.raw_hwnd()) }, 0);

    let paint = host.actual_map_paint_performance_counters();
    let raster = host
        .actual_map_surface_performance_counters()
        .expect("actual map surface counters");
    assert_eq!(
        raster.rasterized_frames - baseline_raster.rasterized_frames,
        1
    );
    assert_eq!(paint.present_requests - baseline_paint.present_requests, 1);
    assert_eq!(paint.paint_calls - baseline_paint.paint_calls, 1);
    assert_eq!(paint.upload_calls - baseline_paint.upload_calls, 1);
}

#[cfg(feature = "test-harness")]
#[test]
fn static_surface_size_mismatch_is_rejected_without_uploading() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    host.attach(&tracked()).expect("attach");
    let baseline = host.actual_map_paint_performance_counters();

    let error = host
        .apply_actual_map_surface(ActualMapSurface::new(16).expect("surface"))
        .expect_err("static mismatch must fail closed");

    assert_eq!(error.operation(), OverlayHostOperation::Renderer);
    assert_eq!(host.actual_map_paint_performance_counters(), baseline);
    assert_eq!(host.actual_map_surface_size(), None);
}

#[cfg(feature = "test-harness")]
#[test]
fn expanded_update_resizes_to_the_exact_map_viewport_before_rasterizing() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    let responsive_layouts = layouts();
    host.apply_layouts(responsive_layouts).expect("layouts");
    host.attach(&tracked()).expect("attach");
    host.apply_actual_map_surface(
        ActualMapSurface::new(responsive_layouts.mini().viewport_size).expect("surface"),
    )
    .expect("apply surface");
    host.set_display_mode(DisplayMode::ExpandedMap)
        .expect("expanded mode");
    let before = host.actual_map_paint_performance_counters();
    assert_ne!(unsafe { UpdateWindow(host.raw_hwnd()) }, 0);
    let after_mismatch_paint = host.actual_map_paint_performance_counters();
    assert_eq!(after_mismatch_paint.upload_calls, before.upload_calls);

    let map = MapRaster::new(600, 600, vec![0; 600 * 600]).expect("black map");
    let view = MapView::new(300.0, 300.0, 1.0, 0.0).expect("view");
    assert_eq!(host.update_actual_map_surface(&map, view), Ok(true));

    assert_eq!(
        host.actual_map_surface_size(),
        Some(responsive_layouts.expanded().viewport_size)
    );
    let expanded_viewport = responsive_layouts.expanded().viewport_size;
    assert_eq!(
        host.actual_map_surface_pixel(expanded_viewport.width / 4, expanded_viewport.height / 4),
        Some(0)
    );
}

#[test]
fn hide_suspend_and_mode_changes_use_public_contract() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    host.apply_layouts(layouts()).expect("layouts");
    host.attach(&tracked()).expect("attach");

    assert_eq!(
        host.set_display_mode(DisplayMode::ExpandedMap),
        Ok(HostUpdate::Changed)
    );
    assert_eq!(host.set_requested_visible(false), Ok(HostUpdate::Changed));
    assert_eq!(unsafe { IsWindowVisible(host.raw_hwnd()) }, 0);
    host.suspend(SuspendReason::Explicit).expect("suspend");
}

#[test]
fn render_snapshot_method_has_required_arc_contract() {
    let _method: fn(&mut OverlayWindowHost, Arc<RenderSnapshot>) -> Result<u64, OverlayHostError> =
        OverlayWindowHost::apply_snapshot;
}

#[test]
fn hit_test_enum_is_stable_for_wndproc_mapping() {
    assert_ne!(HitTest::Client, HitTest::Transparent);
}

#[test]
fn close_message_reports_shutdown_instead_of_leaving_a_stale_hwnd_loop() {
    let mut host = OverlayWindowHost::create().expect("create overlay");
    let hwnd = host.raw_hwnd();

    unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0) };

    assert_eq!(host.pump_messages(), Ok(PumpOutcome::ShutdownRequested));
    assert_eq!(unsafe { IsWindow(hwnd) }, 0);
}
