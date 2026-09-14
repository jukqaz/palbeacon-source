use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const MANIFEST_PATH: &str = "/assets/public/catalog/public-web-manifest.v1.json";

const EXPECTED_SCHEMA: &str = "pal-public-web-catalog-v1";
const EXPECTED_POLICY: &str = "public-metadata-policy-v1";
const REQUIRED_COUNTS: [&str; 8] = [
    "pals",
    "breeding_species",
    "active_skills",
    "passive_skills",
    "items",
    "buildings",
    "technologies",
    "humans",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PublicCatalogManifest {
    schema: String,
    policy: String,
    dataset_version: String,
    game_build_id: String,
    verified: bool,
    counts: BTreeMap<String, u64>,
    files: Vec<PublicCatalogFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PublicCatalogFile {
    path: String,
    bytes: u64,
    sha256: String,
}

impl PublicCatalogManifest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema != EXPECTED_SCHEMA || self.policy != EXPECTED_POLICY || !self.verified {
            return Err("public catalog manifest identity is not verified".to_owned());
        }
        let Some(build_id) = self.game_build_id.strip_prefix("steam:") else {
            return Err("public catalog game build is not a Steam build".to_owned());
        };
        if build_id.is_empty() || !build_id.bytes().all(|value| value.is_ascii_digit()) {
            return Err("public catalog game build is invalid".to_owned());
        }
        let expected_dataset_version = format!("public-web-catalog-steam-{build_id}-v1");
        if self.dataset_version != expected_dataset_version {
            return Err("public catalog dataset version does not match its game build".to_owned());
        }
        for name in REQUIRED_COUNTS {
            if self.counts.get(name).copied().unwrap_or_default() == 0 {
                return Err(format!("public catalog count is empty: {name}"));
            }
        }
        if self.files.is_empty() {
            return Err("public catalog file list is empty".to_owned());
        }
        for file in &self.files {
            if file.path.is_empty()
                || file.bytes == 0
                || file.sha256.len() != 64
                || !file.sha256.bytes().all(|value| value.is_ascii_hexdigit())
            {
                return Err(format!(
                    "public catalog file record is invalid: {}",
                    file.path
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn into_status_json(self) -> Value {
        json!({
            "dataset": {
                "dataset_version": self.dataset_version,
                "game_build_id": self.game_build_id,
                "schema_version": self.schema,
                "verified": self.verified,
                "source": "worker_static_assets",
                "manifest_path": MANIFEST_PATH,
                "file_count": self.files.len(),
                "counts": self.counts,
            },
            "game_build_validation": {
                "required": false,
                "scope": "personal_projection_only"
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PublicCatalogManifest;

    fn fixture() -> PublicCatalogManifest {
        serde_json::from_str(
            r#"{
              "schema":"pal-public-web-catalog-v1",
              "policy":"public-metadata-policy-v1",
              "dataset_version":"public-web-catalog-steam-24467282-v1",
              "game_build_id":"steam:24467282",
              "verified":true,
              "counts":{
                "pals":288,"breeding_species":288,"active_skills":324,
                "passive_skills":420,"items":2466,"buildings":498,
                "technologies":588,"humans":433
              },
              "files":[{
                "path":"pals.json","bytes":100,
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
              }]
            }"#,
        )
        .expect("catalog manifest fixture")
    }

    #[test]
    fn validates_exact_build_static_catalog_manifest() {
        let manifest = fixture();
        manifest.validate().expect("valid catalog manifest");
        let status = manifest.into_status_json();
        assert_eq!(
            status["dataset"]["dataset_version"],
            "public-web-catalog-steam-24467282-v1"
        );
        assert_eq!(status["dataset"]["source"], "worker_static_assets");
    }

    #[test]
    fn rejects_build_and_dataset_mismatch() {
        let mut manifest = fixture();
        manifest.dataset_version = "public-web-catalog-steam-24181527-v1".to_owned();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn rejects_empty_required_counts() {
        let mut manifest = fixture();
        manifest.counts.insert("items".to_owned(), 0);
        assert!(manifest.validate().is_err());
    }
}
