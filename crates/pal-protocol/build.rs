use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR was not provided by Cargo")?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .ok_or("pal-protocol must live under <workspace>/crates")?;
    let proto_root = workspace_root.join("proto");
    let telemetry = proto_root.join("palcompanion/v2/telemetry.proto");
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .extern_path(".palcompanion.v2", "::pal_wire::v2")
        .compile_with_config(config, &[telemetry], &[proto_root])?;

    println!("cargo:rerun-if-changed=../../proto/palcompanion/v2/common.proto");
    println!("cargo:rerun-if-changed=../../proto/palcompanion/v2/telemetry.proto");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
