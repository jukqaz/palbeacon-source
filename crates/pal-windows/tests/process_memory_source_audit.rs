#![cfg(all(windows, feature = "development-readonly-process-memory"))]

const SOURCE: &str = include_str!("../src/process_memory.rs");

#[test]
fn boundary_requests_only_the_two_read_only_process_rights() {
    assert!(
        SOURCE.contains("const PROCESS_ACCESS: u32 = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;")
    );
    assert!(SOURCE.contains("OpenProcess(PROCESS_ACCESS, 0, process_id)"));
}

#[test]
fn boundary_has_no_mutation_injection_or_privilege_escalation_symbols() {
    let forbidden_symbols = [
        "WriteProcessMemory",
        "VirtualAllocEx",
        "CreateRemoteThread",
        "AdjustTokenPrivileges",
        "OpenProcessToken",
        "PROCESS_VM_WRITE",
        "PROCESS_VM_OPERATION",
        "PROCESS_CREATE_THREAD",
        "PROCESS_ALL_ACCESS",
        "SeDebugPrivilege",
    ];

    for symbol in forbidden_symbols {
        assert!(
            !SOURCE.contains(symbol),
            "read-only boundary must not reference forbidden symbol {symbol}"
        );
    }
}
