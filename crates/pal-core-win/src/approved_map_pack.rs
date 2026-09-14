//! Fail-closed activation boundary for a locally published, Gate-B-qualified map pack.
//!
//! `MapPackStore::open_published` resolves only the exact Build active pointer, binds the
//! manifest hash, verifies every region artifact, and rejects transforms that do not carry the
//! required 10-reference plus 5-sealed-holdout evidence. Keeping this check in Core prevents a
//! launcher or Overlay command line from asserting that an arbitrary BMP is approved.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pal_map_pack_store::{MapPackError, MapPackStore};
use thiserror::Error;

const MAIN_MAP_ID: &str = "MainMap";
const MAIN_REGION_ID: &str = "FirstRegion";
const TREE_MAP_ID: &str = "Tree";
const TREE_REGION_ID: &str = "DummyRegion";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedMapPackIdentity {
    dataset_root: PathBuf,
    game_build_id: String,
    canonical_pack_sha256: [u8; 32],
    main_transform_sha256: [u8; 32],
    tree_transform_sha256: [u8; 32],
}

impl ApprovedMapPackIdentity {
    pub fn verify_published(
        dataset_root: impl AsRef<Path>,
        expected_build: &str,
    ) -> Result<(Self, Arc<MapPackStore>), ApprovedMapPackError> {
        let dataset_root = dataset_root.as_ref();
        if !dataset_root.is_absolute() {
            return Err(ApprovedMapPackError::DatasetRootNotAbsolute);
        }

        let store = MapPackStore::open_published(dataset_root, expected_build)?;
        let main = store
            .map_regions()
            .iter()
            .find(|region| region.map_id() == MAIN_MAP_ID && region.region_id() == MAIN_REGION_ID)
            .ok_or(ApprovedMapPackError::MissingRequiredRegion {
                map_id: MAIN_MAP_ID,
                region_id: MAIN_REGION_ID,
            })?;
        let tree = store
            .map_regions()
            .iter()
            .find(|region| region.map_id() == TREE_MAP_ID && region.region_id() == TREE_REGION_ID)
            .ok_or(ApprovedMapPackError::MissingRequiredRegion {
                map_id: TREE_MAP_ID,
                region_id: TREE_REGION_ID,
            })?;

        let identity = Self {
            dataset_root: dataset_root.to_path_buf(),
            game_build_id: expected_build.to_owned(),
            canonical_pack_sha256: store.canonical_pack_hash(),
            main_transform_sha256: *main.transform_sha256(),
            tree_transform_sha256: *tree.transform_sha256(),
        };
        Ok((identity, store))
    }

    pub fn dataset_root(&self) -> &Path {
        &self.dataset_root
    }

    pub fn game_build_id(&self) -> &str {
        &self.game_build_id
    }

    pub const fn canonical_pack_sha256(&self) -> [u8; 32] {
        self.canonical_pack_sha256
    }

    pub const fn main_transform_sha256(&self) -> [u8; 32] {
        self.main_transform_sha256
    }

    pub const fn tree_transform_sha256(&self) -> [u8; 32] {
        self.tree_transform_sha256
    }
}

#[derive(Debug, Error)]
pub enum ApprovedMapPackError {
    #[error("approved map dataset root must be an absolute path")]
    DatasetRootNotAbsolute,
    #[error("approved map pack is invalid: {0}")]
    InvalidPack(#[from] MapPackError),
    #[error("approved map pack is missing required region {map_id}/{region_id}")]
    MissingRequiredRegion {
        map_id: &'static str,
        region_id: &'static str,
    },
}

pub fn encode_sha256(value: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in value {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
