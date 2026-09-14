use std::convert::Infallible;

use pal_domain::{DisplayMode, InputMode};
use pal_overlay_win::{
    HostPlan, HostUpdate, OverlayHostController, OverlayLifecycle, OverlayShellBackend,
    PhysicalRect, SuspendReason, TopLeftOverlayLayouts, top_left_overlay_layouts,
};
use pal_windows::{
    FakeWindowBackend, GameWindowTracker, MonitorId, PhysicalClientRect, TrackedWindow,
    WindowEvent, WindowId, WindowObservation, WindowObservationParts,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakeError {
    Apply,
}

#[derive(Default)]
struct FakeShell {
    plans: Vec<HostPlan>,
    fail_closed_hides: usize,
    fail_next_apply: bool,
}

impl OverlayShellBackend for FakeShell {
    type Error = FakeError;

    fn apply_plan(&mut self, plan: HostPlan) -> Result<(), Self::Error> {
        self.plans.push(plan);
        if std::mem::take(&mut self.fail_next_apply) {
            Err(FakeError::Apply)
        } else {
            Ok(())
        }
    }

    fn hide_fail_closed(&mut self) -> Result<(), Self::Error> {
        self.fail_closed_hides += 1;
        Ok(())
    }
}

fn layouts() -> TopLeftOverlayLayouts {
    top_left_overlay_layouts(PhysicalRect::new(-1_000, 40, 1_000, 700), 96, 288)
        .expect("fixture layouts")
}

fn tracked(active: bool, minimized: bool) -> TrackedWindow {
    let observation = WindowObservation::for_test(WindowObservationParts {
        id: WindowId::from_raw(7),
        monitor_id: MonitorId::from_raw(9),
        process_id: 42,
        image_path: r"C:\Palworld-Win64-Shipping.exe".to_owned(),
        window_title: "not trusted".to_owned(),
        client_rect: PhysicalClientRect::new(-1_000, 40, 1_000, 700),
        dpi: 96,
        visible: true,
        top_level: true,
        foreground: active,
        minimized,
        inspectable: true,
    });
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([observation]));
    match tracker.poll().expect("tracking must succeed") {
        Some(WindowEvent::Attached(window)) => window,
        event => panic!("expected attachment, got {event:?}"),
    }
}

fn host() -> OverlayHostController<FakeShell> {
    OverlayHostController::new(FakeShell::default(), layouts())
}

#[test]
fn active_attachment_becomes_visible_locked_and_topmost_without_activation() {
    let mut host = host();

    assert_eq!(host.attach(&tracked(true, false)), Ok(HostUpdate::Changed));
    assert_eq!(host.lifecycle(), OverlayLifecycle::Visible);
    assert!(host.is_visible());
    assert_eq!(host.input_mode(), InputMode::Locked);
    let plan = host.backend().plans.last().expect("a plan");
    assert!(plan.visible());
    assert_eq!(plan.window_rect(), PhysicalRect::new(-990, 50, 266, 266));
    assert!(plan.transparent());
    assert!(plan.topmost());
    assert!(plan.no_activate());
}

#[test]
fn minimized_attachment_is_suspended_and_locked() {
    let mut host = host();

    assert_eq!(host.attach(&tracked(false, true)), Ok(HostUpdate::Changed));
    assert_eq!(
        host.lifecycle(),
        OverlayLifecycle::Suspended(SuspendReason::Minimized)
    );
    assert!(!host.is_visible());
    assert_eq!(host.input_mode(), InputMode::Locked);
    assert!(!host.backend().plans.last().expect("a plan").visible());
}

#[test]
fn visibility_preferences_cannot_escape_suspension_without_a_fresh_active_snapshot() {
    let mut host = host();
    host.attach(&tracked(false, true))
        .expect("minimized attachment");

    assert_eq!(host.set_requested_visible(false), Ok(HostUpdate::Changed));
    assert_eq!(
        host.lifecycle(),
        OverlayLifecycle::Suspended(SuspendReason::Minimized)
    );
    assert!(!host.backend().plans.last().expect("plan").visible());

    assert_eq!(host.set_requested_visible(true), Ok(HostUpdate::Changed));
    assert_eq!(
        host.lifecycle(),
        OverlayLifecycle::Suspended(SuspendReason::Minimized)
    );
    assert!(!host.backend().plans.last().expect("plan").visible());

    host.attach(&tracked(true, false))
        .expect("fresh active attachment");
    assert_eq!(host.lifecycle(), OverlayLifecycle::Visible);
    assert!(host.backend().plans.last().expect("plan").visible());
}

#[test]
fn same_state_update_does_not_call_backend() {
    let mut host = host();
    host.attach(&tracked(true, false)).expect("attach");
    let calls = host.backend().plans.len();

    assert_eq!(
        host.set_input_mode(InputMode::Locked),
        Ok(HostUpdate::Unchanged)
    );
    assert_eq!(host.backend().plans.len(), calls);
}

#[test]
fn hidden_suspended_and_detached_states_force_locked() {
    let mut host = host();
    host.attach(&tracked(true, false)).expect("attach");
    host.set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");

    host.set_requested_visible(false).expect("hide");
    assert_eq!(host.lifecycle(), OverlayLifecycle::HiddenAttached);
    assert_eq!(host.input_mode(), InputMode::Locked);

    host.set_requested_visible(true).expect("show");
    host.set_input_mode(InputMode::LayoutEdit).expect("edit");
    host.suspend(SuspendReason::Inactive).expect("suspend");
    assert_eq!(
        host.lifecycle(),
        OverlayLifecycle::Suspended(SuspendReason::Inactive)
    );
    assert_eq!(host.input_mode(), InputMode::Locked);

    host.detach().expect("detach");
    assert_eq!(host.lifecycle(), OverlayLifecycle::Detached);
    assert_eq!(host.input_mode(), InputMode::Locked);
}

#[test]
fn style_failure_hides_fail_closed_and_returns_original_error() {
    let mut host = host();
    host.attach(&tracked(true, false)).expect("attach");
    host.backend_mut().fail_next_apply = true;

    assert_eq!(
        host.set_input_mode(InputMode::PinnedInteractive),
        Err(FakeError::Apply)
    );
    assert_eq!(host.backend().fail_closed_hides, 1);
    assert_eq!(host.lifecycle(), OverlayLifecycle::HiddenAttached);
    assert_eq!(host.input_mode(), InputMode::Locked);
}

#[test]
fn display_mode_uses_precomputed_top_left_expanded_layout_and_is_idempotent() {
    let mut host = host();
    host.attach(&tracked(true, false)).expect("attach");

    assert_eq!(
        host.set_display_mode(DisplayMode::ExpandedMap),
        Ok(HostUpdate::Changed)
    );
    let plan = host.backend().plans.last().expect("plan");
    assert_eq!(plan.display_mode(), DisplayMode::ExpandedMap);
    assert_eq!(plan.layout(), layouts().expanded());
    assert_eq!(plan.window_rect(), PhysicalRect::new(-940, 82, 880, 616));

    let calls = host.backend().plans.len();
    assert_eq!(
        host.set_display_mode(DisplayMode::ExpandedMap),
        Ok(HostUpdate::Unchanged)
    );
    assert_eq!(host.backend().plans.len(), calls);

    host.set_display_mode(DisplayMode::MiniMap)
        .expect("restore minimap");
    assert_eq!(
        host.backend().plans.last().expect("plan").layout(),
        layouts().mini()
    );
}

#[test]
fn applying_new_precomputed_layouts_switches_both_modes_without_centering() {
    let mut host = host();
    host.attach(&tracked(true, false)).expect("attach");
    let replacement = top_left_overlay_layouts(PhysicalRect::new(-500, 100, 1_200, 800), 144, 360)
        .expect("replacement layouts");

    assert_eq!(host.apply_layouts(replacement), Ok(HostUpdate::Changed));
    assert_eq!(
        host.backend().plans.last().expect("mini plan").layout(),
        replacement.mini()
    );

    host.set_display_mode(DisplayMode::ExpandedMap)
        .expect("expanded");
    assert_eq!(
        host.backend().plans.last().expect("expanded plan").layout(),
        replacement.expanded()
    );
}

#[test]
fn backend_trait_can_use_infallible_errors() {
    struct Noop;
    impl OverlayShellBackend for Noop {
        type Error = Infallible;

        fn apply_plan(&mut self, _plan: HostPlan) -> Result<(), Self::Error> {
            Ok(())
        }

        fn hide_fail_closed(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    let _host = OverlayHostController::new(Noop, layouts());
}

#[test]
fn suspending_a_detached_host_is_an_idempotent_noop() {
    let mut host = host();

    host.suspend(SuspendReason::Explicit)
        .expect("detached suspend");

    assert_eq!(host.lifecycle(), OverlayLifecycle::Detached);
    assert_eq!(host.input_mode(), InputMode::Locked);
    assert!(host.backend().plans.is_empty());
}

#[test]
fn repeated_visibility_request_while_detached_is_unchanged() {
    let mut host = host();

    assert_eq!(host.set_requested_visible(true), Ok(HostUpdate::Unchanged));
    assert_eq!(host.set_requested_visible(true), Ok(HostUpdate::Unchanged));
    assert!(host.backend().plans.is_empty());
}
