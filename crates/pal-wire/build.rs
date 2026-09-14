use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR was not provided by Cargo")?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .ok_or("pal-wire must live under <workspace>/crates")?;
    let proto_root = workspace_root.join("proto");
    let schema_root = proto_root.join("palcompanion/v2");
    let schemas = [
        schema_root.join("common.proto"),
        schema_root.join("local.proto"),
        schema_root.join("telemetry.proto"),
        schema_root.join("catalog.proto"),
        schema_root.join("profile.proto"),
        schema_root.join("analysis.proto"),
        schema_root.join("sync.proto"),
        schema_root.join("map.proto"),
    ];

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.compile_protos(&schemas, &[proto_root])?;

    for schema in schemas {
        println!("cargo:rerun-if-changed={}", schema.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
