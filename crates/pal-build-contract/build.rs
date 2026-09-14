use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let contract_path = manifest_dir.join("../../contracts/runtime-builds.json");
    println!("cargo:rerun-if-changed={}", contract_path.display());

    let bytes = fs::read(&contract_path).expect("read runtime build contract");
    let contract: serde_json::Value =
        serde_json::from_slice(&bytes).expect("parse runtime build contract");
    assert_eq!(contract["schema_version"].as_u64(), Some(1));
    let current = build_id(&contract, "current_game_build_id");
    let local_position = build_id(&contract, "windows_live_position_profile_build_id");
    let generated = format!(
        concat!(
            "pub const CURRENT_GAME_BUILD_ID: &str = {current:?};\n",
            "pub const CURRENT_GAME_BUILD_ID_U64: u64 = {current_u64};\n",
            "pub const CURRENT_STEAM_GAME_BUILD_ID: &str = {steam_current:?};\n",
            "pub const WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID: &str = {local_position:?};\n",
            "pub const WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID_U64: u64 = {local_position_u64};\n",
            "pub const CURRENT_BUILD_HAS_LIVE_POSITION_PROFILE: bool = {profile_current};\n"
        ),
        current = current,
        current_u64 = current.parse::<u64>().expect("current build fits u64"),
        steam_current = format!("steam:{current}"),
        local_position = local_position,
        local_position_u64 = local_position
            .parse::<u64>()
            .expect("position profile build fits u64"),
        profile_current = current == local_position,
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("runtime_builds.rs");
    fs::write(output, generated).expect("write generated runtime build contract");
}

fn build_id<'a>(contract: &'a serde_json::Value, field: &str) -> &'a str {
    let value = contract[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} must be a string"));
    assert!(
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
        "{field} must contain only ASCII digits"
    );
    value
}
