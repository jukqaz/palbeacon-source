//! Validated map taxonomy and deterministic search-index generation.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTRACT_DOMAIN: &[u8] = b"palbeacon.map-contract.v1\0";

#[derive(Clone, Debug, Deserialize)]
pub struct TerminologyDocument {
    pub schema_version: u32,
    pub language: String,
    pub game_build_id: String,
    pub groups: Vec<TaxonomyGroup>,
    pub terms: Vec<TaxonomyTerm>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TaxonomyGroup {
    pub id: String,
    pub label_ko: String,
    pub description_ko: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TaxonomyTerm {
    pub id: String,
    pub group_id: String,
    pub label_ko: String,
    pub generic_title_ko: String,
    pub description_ko: String,
    #[serde(default)]
    pub aliases_ko: Vec<String>,
    pub verification: String,
    #[serde(default = "default_filter_visibility")]
    pub filter_visibility: String,
    #[serde(default)]
    pub merged_into: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PoiDocument {
    pub schema_version: u32,
    pub game_build_id: String,
    pub poi_count: usize,
    pub pois: Vec<PoiRecord>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PoiRecord {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    #[serde(default)]
    pub entity_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MapContractManifest {
    pub schema_version: u32,
    pub language: String,
    pub game_build_id: String,
    pub contract_sha256: String,
    pub taxonomy_sha256: String,
    pub poi_sha256: String,
    pub taxonomy_group_count: usize,
    pub poi_kind_count: usize,
    pub poi_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchIndex {
    pub schema_version: u32,
    pub language: String,
    pub game_build_id: String,
    pub contract_sha256: String,
    pub documents: Vec<SearchDocument>,
    pub postings: BTreeMap<String, Vec<u32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchDocument {
    pub id: String,
    pub kind: String,
    pub label_ko: String,
    pub normalized_text: String,
}

#[derive(Clone, Debug)]
pub struct ValidatedMapContract {
    pub terminology: TerminologyDocument,
    pub pois: PoiDocument,
    pub manifest: MapContractManifest,
}

impl ValidatedMapContract {
    pub fn parse(terminology_bytes: &[u8], poi_bytes: &[u8]) -> Result<Self, String> {
        let terminology: TerminologyDocument = serde_json::from_slice(terminology_bytes)
            .map_err(|error| format!("invalid POI terminology JSON: {error}"))?;
        let pois: PoiDocument = serde_json::from_slice(poi_bytes)
            .map_err(|error| format!("invalid POI JSON: {error}"))?;
        validate(&terminology, &pois)?;

        let taxonomy_sha256 = digest_hex(terminology_bytes);
        let poi_sha256 = digest_hex(poi_bytes);
        let game_build_id = normalize_build_id(&terminology.game_build_id).to_owned();
        let contract_sha256 = contract_digest(
            &game_build_id,
            &hex_to_bytes(&taxonomy_sha256)?,
            &hex_to_bytes(&poi_sha256)?,
        );
        let manifest = MapContractManifest {
            schema_version: 1,
            language: terminology.language.clone(),
            game_build_id,
            contract_sha256,
            taxonomy_sha256,
            poi_sha256,
            taxonomy_group_count: terminology.groups.len(),
            poi_kind_count: terminology.terms.len(),
            poi_count: pois.pois.len(),
        };
        Ok(Self {
            terminology,
            pois,
            manifest,
        })
    }

    pub fn build_search_index(&self) -> SearchIndex {
        let terms = self
            .terminology
            .terms
            .iter()
            .map(|term| (term.id.as_str(), term))
            .collect::<BTreeMap<_, _>>();
        let mut pois = self.pois.pois.iter().collect::<Vec<_>>();
        pois.sort_unstable_by(|left, right| left.id.cmp(&right.id));

        let mut documents = Vec::with_capacity(pois.len());
        let mut postings = BTreeMap::<String, BTreeSet<u32>>::new();
        for poi in pois {
            let term = terms[poi.kind.as_str()];
            let searchable = std::iter::once(poi.display_name.as_str())
                .chain(poi.entity_id.as_deref())
                .chain(std::iter::once(term.label_ko.as_str()))
                .chain(std::iter::once(term.generic_title_ko.as_str()))
                .chain(term.aliases_ko.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let normalized_text = normalize_search_text(&searchable);
            let document_index = u32::try_from(documents.len()).expect("POI count fits in u32");
            for gram in grams(&normalized_text) {
                postings.entry(gram).or_default().insert(document_index);
            }
            documents.push(SearchDocument {
                id: poi.id.clone(),
                kind: poi.kind.clone(),
                label_ko: poi.display_name.clone(),
                normalized_text,
            });
        }
        SearchIndex {
            schema_version: 1,
            language: "ko".to_owned(),
            game_build_id: self.manifest.game_build_id.clone(),
            contract_sha256: self.manifest.contract_sha256.clone(),
            documents,
            postings: postings
                .into_iter()
                .map(|(gram, indexes)| (gram, indexes.into_iter().collect()))
                .collect(),
        }
    }
}

pub fn normalize_search_text(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| !character.is_whitespace())
        .collect()
}

pub fn grams(value: &str) -> BTreeSet<String> {
    let characters = value.chars().collect::<Vec<_>>();
    let mut grams = BTreeSet::new();
    for character in &characters {
        grams.insert(format!("1:{character}"));
    }
    for pair in characters.windows(2) {
        grams.insert(format!("2:{}{}", pair[0], pair[1]));
    }
    grams
}

fn validate(terminology: &TerminologyDocument, pois: &PoiDocument) -> Result<(), String> {
    if terminology.schema_version != 1 || terminology.language != "ko" {
        return Err("POI terminology must be Korean schema version 1".to_owned());
    }
    if pois.schema_version != 2 {
        return Err("POI data must use schema version 2".to_owned());
    }
    if normalize_build_id(&terminology.game_build_id) != normalize_build_id(&pois.game_build_id) {
        return Err(format!(
            "map build mismatch: taxonomy={} POIs={}",
            terminology.game_build_id, pois.game_build_id
        ));
    }
    if pois.poi_count != pois.pois.len() {
        return Err(format!(
            "declared POI count {} does not match {} rows",
            pois.poi_count,
            pois.pois.len()
        ));
    }
    let group_ids = unique_ids(
        terminology.groups.iter().map(|group| group.id.as_str()),
        "taxonomy group",
    )?;
    let term_ids = unique_ids(
        terminology.terms.iter().map(|term| term.id.as_str()),
        "POI kind",
    )?;
    unique_ids(pois.pois.iter().map(|poi| poi.id.as_str()), "POI")?;
    for group in &terminology.groups {
        require_korean_text("group label", &group.id, &group.label_ko)?;
        require_korean_text("group description", &group.id, &group.description_ko)?;
    }
    for term in &terminology.terms {
        if !group_ids.contains(term.group_id.as_str()) {
            return Err(format!(
                "POI kind {} references missing group {}",
                term.id, term.group_id
            ));
        }
        require_korean_text("POI label", &term.id, &term.label_ko)?;
        require_korean_text("POI title", &term.id, &term.generic_title_ko)?;
        require_korean_text("POI description", &term.id, &term.description_ko)?;
        if !matches!(
            term.verification.as_str(),
            "reviewed" | "needs_game_l10n_review"
        ) {
            return Err(format!("POI kind {} has invalid verification", term.id));
        }
        if !matches!(
            term.filter_visibility.as_str(),
            "visible" | "merged" | "hidden_until_verified"
        ) {
            return Err(format!(
                "POI kind {} has invalid filter visibility",
                term.id
            ));
        }
        if term.verification != "reviewed" && term.filter_visibility == "visible" {
            return Err(format!(
                "unreviewed POI kind {} is publicly visible",
                term.id
            ));
        }
        if term.filter_visibility == "merged" {
            let target = term
                .merged_into
                .as_deref()
                .ok_or_else(|| format!("merged POI kind {} has no target", term.id))?;
            if target == term.id || !term_ids.contains(target) {
                return Err(format!("merged POI kind {} has invalid target", term.id));
            }
        } else if term.merged_into.is_some() {
            return Err(format!(
                "non-merged POI kind {} has a merge target",
                term.id
            ));
        }
    }
    for poi in &pois.pois {
        if !term_ids.contains(poi.kind.as_str()) {
            return Err(format!(
                "POI {} uses unregistered kind {}",
                poi.id, poi.kind
            ));
        }
        require_korean_text("POI display name", &poi.id, &poi.display_name)?;
    }
    Ok(())
}

fn unique_ids<'a>(
    values: impl Iterator<Item = &'a str>,
    label: &str,
) -> Result<BTreeSet<&'a str>, String> {
    let mut exact = BTreeSet::new();
    let mut folded = BTreeMap::<String, &str>::new();
    for value in values {
        if value.is_empty() || !exact.insert(value) {
            return Err(format!("{label} ID is empty or duplicated: {value}"));
        }
        let key = value.to_lowercase();
        if let Some(previous) = folded.insert(key, value) {
            return Err(format!(
                "{label} IDs collide case-insensitively: {previous} and {value}"
            ));
        }
    }
    Ok(exact)
}

fn require_korean_text(label: &str, id: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} is missing for {id}"));
    }
    Ok(())
}

fn normalize_build_id(value: &str) -> &str {
    value.rsplit(':').next().unwrap_or(value)
}

fn default_filter_visibility() -> String {
    "visible".to_owned()
}

fn contract_digest(game_build_id: &str, taxonomy_hash: &[u8], poi_hash: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CONTRACT_DOMAIN);
    hasher.update(game_build_id.as_bytes());
    hasher.update([0]);
    hasher.update(taxonomy_hash);
    hasher.update(poi_hash);
    hex(&hasher.finalize())
}

fn digest_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_to_bytes(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex digest has odd length".to_owned());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|error| format!("invalid hex digest: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grams_cover_unigrams_and_adjacent_bigrams() {
        assert_eq!(
            grams("보스"),
            BTreeSet::from(["1:보".to_owned(), "1:스".to_owned(), "2:보스".to_owned()])
        );
    }

    #[test]
    fn normalization_is_case_and_whitespace_insensitive() {
        assert_eq!(normalize_search_text(" Field  보스\n"), "field보스");
    }
}
