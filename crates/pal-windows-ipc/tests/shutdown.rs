#![cfg(windows)]

use std::{
    io, thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_windows_ipc::{handshake::SessionPolicy, server::PipeListener};
use tokio_util::sync::CancellationToken;

fn endpoint() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        r"\\.\pipe\PalBeacon.Test.shutdown.{}.{nonce}",
        std::process::id()
    )
}

#[test]
fn cancellation_releases_a_blocked_accept_and_the_pipe_name() {
    let endpoint = endpoint();
    let listener =
        PipeListener::bind(&endpoint, SessionPolicy::management_ui(), 1).expect("initial bind");
    let cancel = CancellationToken::new();
    let accept = thread::spawn({
        let cancel = cancel.clone();
        move || listener.accept(&cancel)
    });

    thread::sleep(Duration::from_millis(50));
    cancel.cancel();
    let error = accept
        .join()
        .expect("accept thread")
        .expect_err("cancelled accept");
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);

    let rebound = PipeListener::bind(&endpoint, SessionPolicy::management_ui(), 1)
        .expect("rebind after stop");
    drop(rebound);
}
