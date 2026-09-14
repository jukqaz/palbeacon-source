#![cfg(windows)]

use pal_windows_ipc::{
    CORE_PIPE_NAME,
    handshake::SessionPolicy,
    security::{current_user_only_sddl, current_user_sid_string},
    server::PipeListener,
};

#[test]
fn core_pipe_uses_exact_endpoint_and_current_user_only_dacl() {
    assert_eq!(CORE_PIPE_NAME, r"\\.\pipe\PalBeacon.Core.v2");

    let sid = current_user_sid_string().expect("current process SID");
    let sddl = current_user_only_sddl().expect("protected SDDL");
    assert_eq!(sddl, format!("D:P(A;;GA;;;{sid})"));

    for forbidden in [";;;SY)", ";;;BA)", ";;;BU)", ";;;WD)", ";;;AN)"] {
        assert!(!sddl.contains(forbidden), "forbidden trustee in DACL");
    }
}

#[test]
fn protected_local_only_pipe_can_be_created() {
    let endpoint = format!(
        r"\\.\pipe\PalBeacon.CurrentUserAcl.Test.{}",
        std::process::id()
    );
    let listener = PipeListener::bind(&endpoint, SessionPolicy::management_ui(), 1)
        .expect("protected pipe creation");
    assert_eq!(listener.endpoint(), endpoint);
}
