use std::collections::BTreeSet;

fn dependency_names(section: &str) -> BTreeSet<&str> {
    section
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let key = line
                .split_once('=')
                .expect("dependency line must contain '='")
                .0
                .trim();
            key.strip_suffix(".workspace").unwrap_or(key)
        })
        .collect()
}

fn manifest_section<'a>(manifest: &'a str, name: &str) -> &'a str {
    let header = format!("[{name}]");
    let start = manifest
        .find(&header)
        .unwrap_or_else(|| panic!("missing [{name}] section"))
        + header.len();
    let remainder = &manifest[start..];
    let end = remainder.find("\n[").unwrap_or(remainder.len());
    &remainder[..end]
}

#[test]
fn runtime_dependencies_stay_transport_storage_and_os_free() {
    let manifest = include_str!("../Cargo.toml");
    assert_eq!(
        dependency_names(manifest_section(manifest, "dependencies")),
        BTreeSet::from(["pal-data-types", "prost", "sha2", "thiserror"]),
    );
}

#[test]
fn build_dependencies_are_only_prost_and_vendored_protoc() {
    let manifest = include_str!("../Cargo.toml");
    assert_eq!(
        dependency_names(manifest_section(manifest, "build-dependencies")),
        BTreeSet::from(["prost-build", "protoc-bin-vendored"]),
    );
}

#[test]
fn build_script_passes_vendored_protoc_without_mutating_the_environment() {
    let build_script = include_str!("../build.rs");
    assert!(build_script.contains(".protoc_executable("));
    assert!(!build_script.contains("set_var("));
    for schema in [
        "common.proto",
        "local.proto",
        "telemetry.proto",
        "catalog.proto",
        "profile.proto",
        "analysis.proto",
        "sync.proto",
    ] {
        assert!(build_script.contains(schema), "missing schema: {schema}");
    }
}

#[test]
fn only_v2_is_generated_and_exported_for_runtime_use() {
    let build_script = include_str!("../build.rs");
    let exports = include_str!("../src/lib.rs");
    assert!(build_script.contains("palcompanion/v2"));
    assert!(!build_script.contains("palcompanion/v1"));
    assert!(exports.contains("pub mod v2"));
    assert!(!exports.contains("pub mod v1"));
}

#[test]
fn forbidden_runtime_dependencies_are_absent_from_the_manifest() {
    let dependencies = manifest_section(include_str!("../Cargo.toml"), "dependencies");
    for forbidden in [
        "tonic", "tokio", "windows", "rusqlite", "reqwest", "serde", "http",
    ] {
        assert!(
            !dependencies.contains(forbidden),
            "forbidden runtime dependency: {forbidden}"
        );
    }
}
