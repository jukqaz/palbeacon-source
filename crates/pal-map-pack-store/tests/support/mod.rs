#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256};

use pal_map_pack_store::{MapPackStore, MapRegionPack};

pub const MAIN_INDEX: &str = "regions/mainmap/firstregion/tile-index.json";
pub const MAIN_TRANSFORM: &str = "regions/mainmap/firstregion/transform.json";
pub const MAIN_TILES: &str = "regions/mainmap/firstregion/tiles";
pub const TREE_INDEX: &str = "regions/tree/dummyregion/tile-index.json";
pub const TREE_TRANSFORM: &str = "regions/tree/dummyregion/transform.json";

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

pub fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("map-pack-valid")
}

pub struct TempPack {
    root: PathBuf,
}

pub struct PublishedPack {
    root: PathBuf,
}

impl PublishedPack {
    pub fn valid(label: &str) -> Self {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pal-map-published-{label}-{}-{serial}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale published fixture");
        }
        let relative_version = ".versions/24181527-fixture";
        let version = root.join(relative_version);
        copy_tree(&fixture_root(), &version);
        let manifest_hash =
            sha256_hex(&fs::read(version.join("manifest.json")).expect("read manifest"));
        fs::write(
            root.join("24181527.active.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "game_build_id": "24181527",
                "relative_version_path": relative_version,
                "manifest_sha256": manifest_hash,
            }))
            .expect("serialize active pointer"),
        )
        .expect("write active pointer");
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn pointer_path(&self) -> PathBuf {
        self.root.join("24181527.active.json")
    }
}

impl TempPack {
    pub fn copy_valid(label: &str) -> Self {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pal-map-pack-store-{label}-{}-{serial}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale temp fixture");
        }
        copy_tree(&fixture_root(), &root);
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn read_json(&self, relative: &str) -> Value {
        serde_json::from_slice(&fs::read(self.path(relative)).expect("read fixture JSON"))
            .expect("parse fixture JSON")
    }

    pub fn write_json(&self, relative: &str, value: &Value) {
        fs::write(
            self.path(relative),
            serde_json::to_vec_pretty(value).expect("serialize fixture JSON"),
        )
        .expect("write fixture JSON");
    }

    pub fn refresh_manifest_file_hash(&self, relative: &str, field: &str) {
        let hash = sha256_hex(&fs::read(self.path(relative)).expect("read hashed fixture file"));
        let mut manifest = self.read_json("manifest.json");
        if field == "pois_sha256" {
            manifest[field] = Value::String(hash);
        } else {
            let region = manifest["map_regions"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|region| {
                    region["tile_index_relative_path"].as_str() == Some(relative)
                        || region["transform_relative_path"].as_str() == Some(relative)
                })
                .expect("manifest region references hashed fixture file");
            region[field] = Value::String(hash);
        }
        self.write_json("manifest.json", &manifest);
    }
}

pub fn main_region_pack(store: &MapPackStore) -> &MapRegionPack {
    store
        .select_region_pack(0.0, 0.0)
        .expect("MainMap fixture coordinate selects a region pack")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("write lowercase hex");
    }
    encoded
}

pub fn lowercase_hex(bytes: [u8; 32]) -> String {
    sha256_hex_text(bytes)
}

fn sha256_hex_text(bytes: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("write lowercase hex");
    }
    encoded
}

impl Drop for TempPack {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Drop for PublishedPack {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create fixture directory");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let destination = target.join(entry.file_name());
        if entry.file_type().expect("read fixture type").is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy fixture file");
        }
    }
}

pub fn assert_sanitized(error: &impl std::fmt::Display, root: &Path) {
    let text = error.to_string();
    assert!(
        !text.contains(&root.display().to_string()),
        "error leaked absolute fixture path: {text}"
    );
    assert!(!text.contains('{'), "error leaked raw JSON: {text}");
    assert!(
        !text.contains("synthetic-fast"),
        "error leaked raw JSON value: {text}"
    );
}
