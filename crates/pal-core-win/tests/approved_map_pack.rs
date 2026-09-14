use std::fs;
use std::path::{Path, PathBuf};

use pal_core_win::approved_map_pack::{ApprovedMapPackIdentity, encode_sha256};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir_in;

const BUILD: &str = "24181527";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("map-pack-valid")
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination_path);
        } else {
            fs::copy(entry.path(), destination_path).unwrap();
        }
    }
}

fn published_fixture() -> tempfile::TempDir {
    let root = tempdir_in(".").unwrap();
    let version_relative = ".versions/24181527-approved-fixture";
    let version = root.path().join(version_relative);
    copy_tree(&fixture_root(), &version);
    let manifest = fs::read(version.join("manifest.json")).unwrap();
    let manifest_sha256 = encode_sha256(Sha256::digest(&manifest).into());
    fs::write(
        root.path().join(format!("{BUILD}.active.json")),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "game_build_id": BUILD,
            "relative_version_path": version_relative,
            "manifest_sha256": manifest_sha256,
        }))
        .unwrap(),
    )
    .unwrap();
    root
}

#[test]
fn exact_build_published_pack_exposes_hash_bound_identity() {
    let published = published_fixture();
    let root = published.path().canonicalize().unwrap();
    let (identity, store) = ApprovedMapPackIdentity::verify_published(&root, BUILD).unwrap();

    assert_eq!(identity.dataset_root(), root);
    assert_eq!(identity.game_build_id(), BUILD);
    assert_eq!(
        identity.canonical_pack_sha256(),
        store.canonical_pack_hash()
    );
    assert_ne!(identity.main_transform_sha256(), [0; 32]);
    assert_ne!(identity.tree_transform_sha256(), [0; 32]);
    assert_ne!(
        identity.main_transform_sha256(),
        identity.tree_transform_sha256()
    );
}

#[test]
fn relative_root_wrong_build_and_tampered_pointer_fail_closed() {
    assert!(ApprovedMapPackIdentity::verify_published("relative", BUILD).is_err());

    let published = published_fixture();
    let root = published.path().canonicalize().unwrap();
    assert!(ApprovedMapPackIdentity::verify_published(&root, "99999999").is_err());

    let pointer = root.join(format!("{BUILD}.active.json"));
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&pointer).unwrap()).unwrap();
    value["manifest_sha256"] = json!("0".repeat(64));
    fs::write(pointer, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(ApprovedMapPackIdentity::verify_published(&root, BUILD).is_err());
}
