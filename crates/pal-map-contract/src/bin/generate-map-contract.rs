use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pal_map_contract::ValidatedMapContract;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("generate-map-contract: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mode = env::args().nth(1).ok_or_else(usage)?;
    if env::args().nth(2).is_some() || !matches!(mode.as_str(), "--check" | "--write") {
        return Err(usage());
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("pal-map-contract must live under <workspace>/crates")?
        .to_path_buf();
    let asset_root = workspace.join("assets/palbeacon/game/map");
    let taxonomy_path = asset_root.join("poi-terminology.ko.v1.json");
    let poi_path = asset_root.join("pois.v1.json");
    let taxonomy = fs::read(&taxonomy_path)
        .map_err(|error| format!("could not read {}: {error}", taxonomy_path.display()))?;
    let pois = fs::read(&poi_path)
        .map_err(|error| format!("could not read {}: {error}", poi_path.display()))?;
    let contract = ValidatedMapContract::parse(&taxonomy, &pois)?;

    let outputs = [
        (
            workspace.join("contracts/map/v1/manifest.json"),
            json_bytes(&contract.manifest)?,
        ),
        (
            workspace.join("assets/palbeacon/game/map/search-index.ko.v1.json"),
            json_bytes(&contract.build_search_index())?,
        ),
    ];
    let mut stale = Vec::new();
    for (path, bytes) in outputs {
        if mode == "--check" {
            match fs::read(&path) {
                Ok(current) if current == bytes => {}
                _ => stale.push(path),
            }
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
            }
            fs::write(&path, bytes)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "generated map contract is stale:\n{}",
            stale
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

fn json_bytes(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("could not serialize generated JSON: {error}"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn usage() -> String {
    "usage: generate-map-contract --check | --write".to_owned()
}
