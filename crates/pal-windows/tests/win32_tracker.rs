#![cfg(windows)]

use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use pal_windows::{
    GameWindowTracker, Win32Backend, WindowBackend, WindowBackendError, WindowEvent, WindowId,
    WindowObservation, enable_per_monitor_v2,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    PostMessageW, SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindowAsync, WM_CLOSE,
};

const WAIT_LIMIT: Duration = Duration::from_secs(10);

#[test]
fn foreground_wait_is_needed_only_after_an_accepted_request_for_an_inactive_window() {
    assert!(!should_wait_for_foreground_event(true, true));
    assert!(should_wait_for_foreground_event(false, true));
    assert!(!should_wait_for_foreground_event(false, false));
    assert!(!should_wait_for_foreground_event(true, false));
}

#[test]
#[ignore = "requires the prebuilt Palworld-Win64-Shipping.exe helper"]
fn tracks_real_win32_window_lifecycle() {
    enable_per_monitor_v2().expect("test process must use Per-Monitor-V2 DPI awareness");
    let executable = helper_executable();
    let mut child = ChildGuard::spawn(&executable);
    let ready = child.read_ready();
    assert_eq!(ready.process_id, child.id());

    let hwnd = ready.hwnd as usize as HWND;
    let backend = PinnedPidBackend {
        inner: Win32Backend::new(),
        process_id: ready.process_id,
    };
    let mut tracker = GameWindowTracker::new(backend);

    let attached = wait_for_event(&mut tracker, "attach", |event| {
        matches!(
            event,
            WindowEvent::Attached(window) if window.id().as_raw() == ready.hwnd
        )
    });
    let initial = current_window(&attached);

    // SAFETY: hwnd belongs to the helper process owned by this test.
    assert_ne!(
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                240,
                180,
                900,
                620,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        },
        0,
        "SetWindowPos failed"
    );
    let moved = wait_for_event(&mut tracker, "move/resize", |event| {
        matches!(
            event,
            WindowEvent::Changed { previous, current }
                if previous.snapshot() == initial.snapshot()
                    && current.snapshot().client_width() != initial.snapshot().client_width()
                    && current.snapshot().client_height() != initial.snapshot().client_height()
        )
    });
    let moved_window = current_window(&moved);

    // SAFETY: hwnd is a live helper window.
    assert_ne!(
        unsafe { ShowWindowAsync(hwnd, SW_MINIMIZE) },
        0,
        "ShowWindowAsync(SW_MINIMIZE) failed"
    );
    let minimized = wait_for_event(&mut tracker, "minimize", |event| {
        matches!(
            event,
            WindowEvent::Changed { current, .. } if current.snapshot().minimized()
        )
    });
    assert!(current_window(&minimized).snapshot().minimized());

    // SAFETY: hwnd is a live helper window.
    assert_ne!(
        unsafe { ShowWindowAsync(hwnd, SW_RESTORE) },
        0,
        "ShowWindowAsync(SW_RESTORE) failed"
    );
    let restored = wait_for_event(&mut tracker, "restore", |event| {
        matches!(
            event,
            WindowEvent::Changed { current, .. } if !current.snapshot().minimized()
        )
    });
    assert!(!current_window(&restored).snapshot().minimized());
    assert_eq!(
        current_window(&restored).snapshot().client_width(),
        moved_window.snapshot().client_width()
    );

    // Foreground permission can be denied by desktop policy. Assert the event only when User32
    // accepts the request.
    // SAFETY: hwnd is a live helper window.
    let already_active = current_window(&restored).snapshot().active();
    let foreground_request_accepted = unsafe { SetForegroundWindow(hwnd) } != 0;
    if should_wait_for_foreground_event(already_active, foreground_request_accepted) {
        let active = wait_for_event(&mut tracker, "foreground", |event| {
            matches!(
                event,
                WindowEvent::Changed { current, .. } if current.snapshot().active()
            ) || matches!(
                event,
                WindowEvent::Attached(current) if current.snapshot().active()
            )
        });
        assert!(current_window(&active).snapshot().active());
    }

    // SAFETY: posting WM_CLOSE does not transfer ownership of hwnd.
    assert_ne!(
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) },
        0,
        "PostMessageW(WM_CLOSE) failed"
    );
    let detached = wait_for_event(&mut tracker, "detach", |event| {
        matches!(
            event,
            WindowEvent::Detached { previous } if previous.id().as_raw() == ready.hwnd
        )
    });
    assert!(matches!(detached, WindowEvent::Detached { .. }));
    child.wait_for_exit();
}

const fn should_wait_for_foreground_event(already_active: bool, request_accepted: bool) -> bool {
    request_accepted && !already_active
}

fn helper_executable() -> PathBuf {
    let path = PathBuf::from(
        env::var_os("PAL_TEST_WINDOW_EXE")
            .expect("PAL_TEST_WINDOW_EXE must point to a prebuilt helper"),
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Palworld-Win64-Shipping.exe"),
        "helper must exercise the exact production process-image match"
    );
    path
}

#[derive(Clone, Copy, Debug)]
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

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn read_ready(&self) -> Ready {
        let line = self
            .ready_rx
            .recv_timeout(WAIT_LIMIT)
            .expect("helper did not become ready before timeout");
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("READY"));
        let process_id = fields
            .next()
            .expect("READY process id")
            .parse()
            .expect("numeric process id");
        let hwnd = fields
            .next()
            .expect("READY HWND")
            .parse()
            .expect("numeric HWND");
        assert_eq!(fields.next(), None);
        Ready { process_id, hwnd }
    }

    fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + WAIT_LIMIT;
        while Instant::now() < deadline {
            if self.child.try_wait().expect("wait helper").is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("helper did not exit after WM_CLOSE");
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
        if let Some(event) = tracker.poll().expect("poll tracker") {
            eprintln!("{stage}: {event:?}");
            if predicate(&event) {
                return event;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("expected {stage} tracker event before timeout");
}

fn current_window(event: &WindowEvent) -> &pal_windows::TrackedWindow {
    match event {
        WindowEvent::Attached(current) | WindowEvent::Changed { current, .. } => current,
        WindowEvent::Detached { .. } => panic!("detached event has no current window"),
    }
}
