#![cfg(all(windows, feature = "development-local-readonly-position"))]

const SOURCE: &str = include_str!("../src/local_position_win32.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/local_position_runtime.rs");

#[test]
fn adapter_contains_only_read_only_process_capabilities() {
    for forbidden in [
        "WriteProcessMemory",
        "NtWriteVirtualMemory",
        "VirtualAllocEx",
        "CreateRemoteThread",
        "DebugActiveProcess",
        "OpenProcess(",
        "PROCESS_VM_WRITE",
        "PROCESS_VM_OPERATION",
        "PROCESS_ALL_ACCESS",
        "AdjustTokenPrivileges",
        "SeDebugPrivilege",
        "LoadLibrary",
        "SetWindowsHookEx",
        "FILE_SHARE_WRITE",
        "FILE_SHARE_DELETE",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "forbidden process capability in adapter: {forbidden}"
        );
    }
}

#[test]
fn adapter_reuses_the_audited_boundary_and_streams_the_image() {
    for required in [
        "ReadOnlyProcess",
        "LocalPositionMemoryReader",
        "FILE_FLAG_OPEN_REPARSE_POINT",
        "Sha256",
    ] {
        assert!(SOURCE.contains(required), "missing invariant: {required}");
    }
}

#[test]
fn exact_image_open_and_hash_are_owned_by_the_worker_boundary() {
    assert!(
        RUNTIME_SOURCE.contains("Self::spawn_with_cancellable_reader_and_loop_observer(")
            && RUNTIME_SOURCE
                .contains("open_windows_live_position_reader(&window, &verified_build, cancelled)",),
        "the exact-build reader open must be initiated by the local worker"
    );
    assert!(
        SOURCE.contains("open_and_hash_exact_image(")
            && SOURCE.contains("const HASH_BUFFER_BYTES: usize = 64 * 1024"),
        "the 161MB exact image must remain stream-hashed by the reader boundary"
    );
}

#[test]
fn query_identity_brackets_hash_before_opening_process_memory() {
    let body = function_body(SOURCE, "pub fn open_windows_live_position_reader(");
    let guard = body
        .find("QueryOnlyProcessGuard::open_tracked")
        .expect("query-only process guard");
    let image = body
        .find("open_and_hash_exact_image(")
        .expect("exact image verification");
    let sha = body
        .find("require_exact_sha256(")
        .expect("exact image digest comparison");
    let cancellation_after_sha = body[sha..]
        .find("check_cancelled(cancelled)?")
        .map(|offset| sha + offset)
        .expect("cancellation boundary after exact digest comparison");
    let process = body
        .find("open_read_only_tracked")
        .expect("read-only process open");

    assert!(
        guard < image
            && image < sha
            && sha < cancellation_after_sha
            && cancellation_after_sha < process,
        "query identity and cancellation must bracket the hash before process-memory access"
    );
}

#[test]
fn worker_uses_deadline_waiting_and_stop_unparks_it() {
    for required in [
        "const WORKER_SAMPLE_INTERVAL: Duration = Duration::from_millis(100)",
        "thread::park_timeout(next_sample_deadline.duration_since(now))",
        "worker.thread().unpark();",
    ] {
        assert!(
            RUNTIME_SOURCE.contains(required),
            "missing worker cadence or immediate-stop invariant: {required}"
        );
    }
}

#[test]
fn stream_hash_checks_worker_cancellation_at_each_chunk_boundary() {
    for required in [
        "sync::atomic::{AtomicBool, Ordering}",
        "fn check_cancelled(cancelled: &AtomicBool)",
    ] {
        assert!(
            SOURCE.contains(required),
            "missing hash cancellation invariant: {required}"
        );
    }

    let body = function_body(SOURCE, "fn stream_exact_image_reader_sha256");
    let loop_start = body
        .find("while remaining != 0 {")
        .expect("bounded hash loop");
    let loop_cancel = body[loop_start..]
        .find("check_cancelled(cancelled)?;")
        .map(|offset| loop_start + offset)
        .expect("cancellation check at each chunk boundary");
    let trailing = body
        .find("let mut trailing")
        .expect("trailing-byte verification");
    let trailing_cancel = body[..trailing]
        .rfind("check_cancelled(cancelled)?;")
        .expect("cancellation check before trailing-byte verification");

    assert!(loop_start < loop_cancel && loop_cancel <= trailing_cancel);
}

fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source.find(signature).expect("function signature");
    let body_start = source[start..].find('{').unwrap() + start;
    let mut depth = 0_u32;
    for (offset, byte) in source.as_bytes()[body_start..].iter().copied().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[body_start..=body_start + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function body")
}
