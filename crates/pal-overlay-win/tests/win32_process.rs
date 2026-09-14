#![cfg(windows)]

use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use pal_domain::InputMode;
use pal_overlay_win::{
    OverlayWindowHost, PhysicalPoint, PhysicalRect, PreviewPixelRole, PumpOutcome,
    TopLeftOverlayLayouts, preview_pixel_role, top_left_overlay_layouts,
};
use pal_windows::{
    GameWindowTracker, TrackedWindow, Win32Backend, WindowBackend, WindowBackendError, WindowEvent,
    WindowId, WindowObservation, enable_per_monitor_v2,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetForegroundWindow, GetWindowRect, IsWindowVisible,
    PostMessageW, SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SetCursorPos,
    SetWindowPos, ShowWindowAsync, WM_CLOSE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const WAIT_LIMIT: Duration = Duration::from_secs(10);
const STATIC_CLASS: [u16; 7] = [
    b'S' as u16,
    b'T' as u16,
    b'A' as u16,
    b'T' as u16,
    b'I' as u16,
    b'C' as u16,
    0,
];
const PROBE_TITLE: [u16; 6] = [
    b'P' as u16,
    b'R' as u16,
    b'O' as u16,
    b'B' as u16,
    b'E' as u16,
    0,
];

#[test]
#[ignore = "requires PAL_TEST_WINDOW_EXE and foreground desktop input"]
fn overlay_follows_real_game_window_and_preserves_cross_process_input() {
    enable_per_monitor_v2().expect("test process must use PMv2");
    let mut child = ChildGuard::spawn(&helper_executable());
    let ready = child.read_ready();
    let game_hwnd = ready.hwnd as usize as HWND;
    let backend = PinnedPidBackend {
        inner: Win32Backend::new(),
        process_id: ready.process_id,
    };
    let mut tracker = GameWindowTracker::new(backend);
    let attached = wait_for_event(
        &mut tracker,
        "attach",
        |event| matches!(event, WindowEvent::Attached(window) if window.id().as_raw() == ready.hwnd),
    );
    let mut current = current_window(&attached).clone();
    if !current.snapshot().active() {
        click(
            current.snapshot().client_left() + 40,
            current.snapshot().client_top() + 40,
            1,
        );
        current = current_window(&wait_for_event(&mut tracker, "active", |event| {
            matches!(
                event,
                WindowEvent::Changed { current, .. } if current.snapshot().active()
            )
        }))
        .clone();
    }

    let mut overlay = OverlayWindowHost::create().expect("overlay");
    apply_current(&mut overlay, &current);
    assert_ne!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);

    let layout = layouts_for(&current).mini();
    let map = layout.map_viewport_rect;
    let outside = (
        current.snapshot().client_left() + 20,
        current.snapshot().client_top() + 20,
    );
    let interactive_target = (
        map.left + i32::try_from(map.width * 3 / 4).expect("interior x"),
        map.top + i32::try_from(map.height * 3 / 4).expect("interior y"),
    );
    assert_eq!(
        preview_pixel_role(
            layout,
            PhysicalPoint::new(interactive_target.0, interactive_target.1)
        ),
        PreviewPixelRole::OpaqueMapInterior
    );

    // Keep the overlay visible while another process owns foreground, then prove a click outside
    // its HRGN activates the separate game helper.
    let probe = create_focus_probe();
    click(30, 30, 1);
    wait_until("physical click activates focus probe", || {
        assert_eq!(
            overlay.pump_messages().expect("pump focus probe"),
            PumpOutcome::Continue
        );
        unsafe { GetForegroundWindow() == probe }
    });
    click(outside.0, outside.1, 1);
    wait_until("outside-region click activates game", || unsafe {
        GetForegroundWindow() == game_hwnd
    });
    unsafe { DestroyWindow(probe) };

    // Locked mode must pass one hundred physical clicks through without activating the overlay.
    let before = overlay.received_clicks();
    click(interactive_target.0, interactive_target.1, 100);
    let _ = overlay.pump_messages().expect("pump locked clicks");
    assert_eq!(overlay.received_clicks(), before);
    assert_eq!(unsafe { GetForegroundWindow() }, game_hwnd);

    // Interactive mode receives its in-region click but MA_NOACTIVATE preserves game focus.
    overlay
        .set_input_mode(InputMode::PinnedInteractive)
        .expect("interactive");
    click(interactive_target.0, interactive_target.1, 1);
    wait_until("interactive overlay click", || {
        let _ = overlay.pump_messages().expect("pump interactive click");
        overlay.received_clicks() == before + 1
    });
    assert_eq!(unsafe { GetForegroundWindow() }, game_hwnd);

    // Once the tracker observes a different foreground window, the overlay must hide. A fresh
    // active snapshot is the only event allowed to restore it.
    let inactive_probe = create_focus_probe();
    click(30, 30, 1);
    wait_until("inactive probe owns foreground", || {
        assert_eq!(
            overlay.pump_messages().expect("pump inactive probe"),
            PumpOutcome::Continue
        );
        unsafe { GetForegroundWindow() == inactive_probe }
    });
    current = current_window(&wait_for_event(&mut tracker, "inactive", |event| {
        matches!(
            event,
            WindowEvent::Changed { current, .. } if !current.snapshot().active()
        )
    }))
    .clone();
    apply_current(&mut overlay, &current);
    assert_eq!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);

    click(
        current.snapshot().client_left() + 40,
        current.snapshot().client_top() + 40,
        1,
    );
    current = current_window(&wait_for_event(&mut tracker, "active restore", |event| {
        matches!(
            event,
            WindowEvent::Changed { current, .. } if current.snapshot().active()
        )
    }))
    .clone();
    apply_current(&mut overlay, &current);
    assert_ne!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);
    unsafe { DestroyWindow(inactive_probe) };

    // Move/resize alignment is based on the physical game client rect.
    assert_ne!(
        unsafe {
            SetWindowPos(
                game_hwnd,
                std::ptr::null_mut(),
                260,
                190,
                940,
                640,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        },
        0
    );
    current = current_window(&wait_for_event(&mut tracker, "move", |event| {
        matches!(event, WindowEvent::Changed { current, .. }
            if current.snapshot().client_left() != 0 && !current.snapshot().minimized())
    }))
    .clone();
    apply_current(&mut overlay, &current);
    assert_overlay_alignment(&overlay, &current);

    assert_ne!(unsafe { ShowWindowAsync(game_hwnd, SW_MINIMIZE) }, 0);
    current = current_window(&wait_for_event(&mut tracker, "minimize", |event| {
        matches!(event, WindowEvent::Changed { current, .. } if current.snapshot().minimized())
    }))
    .clone();
    overlay.attach(&current).expect("suspend minimized");
    assert_eq!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);

    assert_ne!(unsafe { ShowWindowAsync(game_hwnd, SW_RESTORE) }, 0);
    thread::sleep(Duration::from_millis(100));
    click(
        current.snapshot().client_left() + 40,
        current.snapshot().client_top() + 40,
        1,
    );
    current = current_window(&wait_for_event(&mut tracker, "restore", |event| {
        matches!(
            event,
            WindowEvent::Changed { current, .. }
                if !current.snapshot().minimized() && current.snapshot().active()
        )
    }))
    .clone();
    apply_current(&mut overlay, &current);
    assert_ne!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);

    // Start the replacement while the first HWND still exists so the test can prove that the
    // tracker/overlay bind to a distinct native window after detach.
    let mut replacement_child = ChildGuard::spawn(&helper_executable());
    let replacement_ready = replacement_child.read_ready();
    assert_ne!(replacement_ready.hwnd, ready.hwnd);
    let replacement_backend = PinnedPidBackend {
        inner: Win32Backend::new(),
        process_id: replacement_ready.process_id,
    };
    let mut replacement_tracker = GameWindowTracker::new(replacement_backend);
    let replacement_attached =
        wait_for_event(&mut replacement_tracker, "replacement attach", |event| {
            matches!(
                event,
                WindowEvent::Attached(window)
                    if window.id().as_raw() == replacement_ready.hwnd
            )
        });
    let mut replacement_current = current_window(&replacement_attached).clone();

    assert_ne!(unsafe { PostMessageW(game_hwnd, WM_CLOSE, 0, 0) }, 0);
    let detached = wait_for_event(&mut tracker, "detach", |event| {
        matches!(event, WindowEvent::Detached { .. })
    });
    assert!(matches!(detached, WindowEvent::Detached { .. }));
    overlay.detach().expect("detach overlay");
    assert_eq!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);
    child.wait_for_exit();

    if !replacement_current.snapshot().active() {
        click(
            replacement_current.snapshot().client_left() + 40,
            replacement_current.snapshot().client_top() + 40,
            1,
        );
        replacement_current = current_window(&wait_for_event(
            &mut replacement_tracker,
            "replacement active",
            |event| {
                matches!(
                    event,
                    WindowEvent::Changed { current, .. } if current.snapshot().active()
                )
            },
        ))
        .clone();
    }
    apply_current(&mut overlay, &replacement_current);
    assert_ne!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);
    assert_overlay_alignment(&overlay, &replacement_current);

    let replacement_hwnd = replacement_ready.hwnd as usize as HWND;
    assert_ne!(unsafe { PostMessageW(replacement_hwnd, WM_CLOSE, 0, 0) }, 0);
    assert!(matches!(
        wait_for_event(&mut replacement_tracker, "replacement detach", |event| {
            matches!(event, WindowEvent::Detached { .. })
        }),
        WindowEvent::Detached { .. }
    ));
    overlay.detach().expect("detach replacement overlay");
    assert_eq!(unsafe { IsWindowVisible(overlay.raw_hwnd()) }, 0);
    replacement_child.wait_for_exit();
}

fn layouts_for(window: &TrackedWindow) -> TopLeftOverlayLayouts {
    let snapshot = window.snapshot();
    top_left_overlay_layouts(
        PhysicalRect::new(
            snapshot.client_left(),
            snapshot.client_top(),
            snapshot.client_width(),
            snapshot.client_height(),
        ),
        snapshot.dpi(),
        240,
    )
    .expect("tracked helper window fits the exact overlay layouts")
}

fn apply_current(overlay: &mut OverlayWindowHost, window: &TrackedWindow) {
    overlay.apply_layouts(layouts_for(window)).expect("layouts");
    overlay.attach(window).expect("attach");
}

fn assert_overlay_alignment(overlay: &OverlayWindowHost, window: &TrackedWindow) {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    assert_ne!(unsafe { GetWindowRect(overlay.raw_hwnd(), &mut rect) }, 0);
    let expected = layouts_for(window).mini().surface_shape.bounds();
    assert!((rect.left - expected.left).abs() <= 1);
    assert!((rect.top - expected.top).abs() <= 1);
    assert!((rect.right - rect.left - expected.width as i32).abs() <= 1);
    assert!((rect.bottom - rect.top - expected.height as i32).abs() <= 1);
}

fn click(x: i32, y: i32, count: usize) {
    assert_ne!(unsafe { SetCursorPos(x, y) }, 0);
    let inputs = [
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: MOUSEEVENTF_LEFTDOWN,
                    ..MOUSEINPUT::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: MOUSEEVENTF_LEFTUP,
                    ..MOUSEINPUT::default()
                },
            },
        },
    ];
    for _ in 0..count {
        assert_eq!(
            unsafe {
                SendInput(
                    u32::try_from(inputs.len()).expect("input count"),
                    inputs.as_ptr(),
                    i32::try_from(std::mem::size_of::<INPUT>()).expect("INPUT size"),
                )
            },
            2
        );
    }
    thread::sleep(Duration::from_millis(100));
}

fn create_focus_probe() -> HWND {
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            STATIC_CLASS.as_ptr(),
            PROBE_TITLE.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            10,
            10,
            120,
            100,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert!(!hwnd.is_null());
    hwnd
}

fn helper_executable() -> PathBuf {
    let path = PathBuf::from(
        env::var_os("PAL_TEST_WINDOW_EXE")
            .expect("PAL_TEST_WINDOW_EXE must point to the prebuilt helper"),
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Palworld-Win64-Shipping.exe")
    );
    path
}

#[derive(Clone, Copy)]
struct Ready {
    process_id: u32,
    hwnd: u64,
}

struct ChildGuard {
    child: Child,
    ready_rx: mpsc::Receiver<String>,
}

impl ChildGuard {
    fn spawn(executable: &PathBuf) -> Self {
        let mut child = Command::new(executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch helper");
        let stdout = child.stdout.take().expect("capture helper stdout");
        let (ready_tx, ready_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = ready_tx.send(line);
        });
        Self { child, ready_rx }
    }

    fn read_ready(&self) -> Ready {
        let line = self
            .ready_rx
            .recv_timeout(WAIT_LIMIT)
            .expect("helper ready timeout");
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("READY"));
        let process_id = fields.next().expect("pid").parse().expect("numeric pid");
        let hwnd = fields.next().expect("hwnd").parse().expect("numeric hwnd");
        Ready { process_id, hwnd }
    }

    fn wait_for_exit(&mut self) {
        wait_until("helper exit", || {
            self.child.try_wait().expect("wait helper").is_some()
        });
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct PinnedPidBackend<B> {
    inner: B,
    process_id: u32,
}

impl<B: WindowBackend> WindowBackend for PinnedPidBackend<B> {
    fn inspect(&mut self, id: WindowId) -> Result<Option<WindowObservation>, WindowBackendError> {
        Ok(self
            .inner
            .inspect(id)?
            .filter(|window| window.process_id() == self.process_id))
    }

    fn enumerate_top_level(&mut self) -> Result<Vec<WindowObservation>, WindowBackendError> {
        Ok(self
            .inner
            .enumerate_top_level()?
            .into_iter()
            .filter(|window| window.process_id() == self.process_id)
            .collect())
    }
}

fn wait_for_event<B: WindowBackend>(
    tracker: &mut GameWindowTracker<B>,
    stage: &str,
    predicate: impl Fn(&WindowEvent) -> bool,
) -> WindowEvent {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        if let Some(event) = tracker.poll().expect("poll tracker")
            && predicate(&event)
        {
            return event;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("expected {stage} event");
}

fn current_window(event: &WindowEvent) -> &TrackedWindow {
    match event {
        WindowEvent::Attached(current) | WindowEvent::Changed { current, .. } => current,
        WindowEvent::Detached { .. } => panic!("detached has no current window"),
    }
}

fn wait_until(stage: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("{stage} timed out");
}
