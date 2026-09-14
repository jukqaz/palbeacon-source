use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

mod knowledge;
mod quality;

use knowledge::{KnowledgeProjectionCounts, SourceArtifact};
use quality::{QualityReportBuilder, ReferenceIndex};

const BASE_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0001_private_companion.sql");
const SEARCH_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0002_catalog_search.sql");
const DETAIL_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0003_catalog_species_detail.sql");
const SKILLS_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0004_catalog_skills.sql");
const ITEMS_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0005_catalog_items_detail.sql");
const BUILD_POLICY_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0006_game_build_compatibility.sql");
const KNOWLEDGE_GRAPH_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0007_knowledge_graph_wiki.sql");
const KNOWLEDGE_OPERATIONS_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0008_knowledge_operations.sql");
const KNOWLEDGE_OPERATOR_REVIEW_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0009_knowledge_operator_review.sql");
const KNOWLEDGE_REVIEW_PAGINATION_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0010_knowledge_review_pagination.sql");
const DATASET_SCOPE_MIGRATION: &str =
    include_str!("../../../cloudflare/migrations/0011_dataset_scope.sql");
const POST_MIGRATION_TRIGGERS: &str =
    include_str!("../../../cloudflare/schema/post_migration_triggers.sql");
const CURRENT_MIGRATIONS: [&str; 11] = [
    BASE_MIGRATION,
    SEARCH_MIGRATION,
    DETAIL_MIGRATION,
    SKILLS_MIGRATION,
    ITEMS_MIGRATION,
    BUILD_POLICY_MIGRATION,
    KNOWLEDGE_GRAPH_MIGRATION,
    KNOWLEDGE_OPERATIONS_MIGRATION,
    KNOWLEDGE_OPERATOR_REVIEW_MIGRATION,
    KNOWLEDGE_REVIEW_PAGINATION_MIGRATION,
    DATASET_SCOPE_MIGRATION,
];
const EXPECTED_BREEDING_SHA256: &str =
    "e0f3a3eeca656ff506f4c1307397cad1bf156680d8c90ca6250ea790f11b38bb";
const EXPECTED_ITEM_CATALOG_SHA256: &str =
    "8045124bbbd04700cbfb0834944534bb8858788a84dd0a4898d4c56f2912765b";
const EXPECTED_ITEM_MAPPING_SHA256: &str =
    "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0";
const DEFAULT_EXPECTED_SPECIES_COUNT: usize = 299;
const EXPECTED_ITEM_COUNT: usize = 2_466;
const EXPECTED_RECIPE_COUNT: usize = 1_414;
const EXPECTED_PAL_DROP_COUNT: usize = 3_374;
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const SPECIES_CHUNK_SIZE: usize = 100;
const SKILL_CHUNK_SIZE: usize = 200;
const PASSIVE_CHUNK_SIZE: usize = 200;
const ITEM_CHUNK_SIZE: usize = 200;
const RECIPE_CHUNK_SIZE: usize = 200;
const ACQUISITION_CHUNK_SIZE: usize = 250;
const ALIAS_CHUNK_SIZE: usize = 250;
const BREEDING_CHUNK_SIZE: usize = 500;
const KNOWLEDGE_COMPILER_SCHEMA_VERSION: &str = "8";
const KNOWLEDGE_QUALITY_REPORT_PATH: &str = "knowledge-data-quality.json";

#[derive(Debug)]
struct Options {
    catalog_worker: PathBuf,
    catalog_dir: PathBuf,
    breeding_file: PathBuf,
    active_skills_file: PathBuf,
    active_skills_ko_file: PathBuf,
    passive_skills_file: PathBuf,
    passive_skills_ko_file: PathBuf,
    item_catalog_file: PathBuf,
    output_dir: PathBuf,
    dataset_version: String,
    game_version: String,
    game_build_id: String,
    expected_species_count: usize,
    expected_item_count: usize,
    expected_recipe_count: usize,
    expected_pal_drop_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogEnvelope {
    status: String,
    quality: String,
    source_id: String,
    matched_count: usize,
    returned_count: usize,
    records: Vec<SpeciesRecord>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct SpeciesRecord {
    species_id: String,
    name_ko: Option<String>,
    paldex_number: Option<u32>,
    rarity: Option<u32>,
    #[serde(default)]
    elements: Vec<LocalizedValue>,
    hp: Option<i64>,
    attack: Option<i64>,
    defense: Option<i64>,
    run_speed: Option<i64>,
    ride_sprint_speed: Option<i64>,
    transport_speed: Option<i64>,
    stamina: Option<i64>,
    food_amount: Option<i64>,
    nocturnal: bool,
    #[serde(default)]
    work_suitability: Vec<WorkSuitability>,
    #[serde(default)]
    learned_skills: Vec<LearnedSkill>,
    #[serde(default)]
    guaranteed_passives: Vec<LocalizedValue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LocalizedValue {
    id: String,
    name_ko: String,
}

#[derive(Clone, Debug, Deserialize)]
struct WorkSuitability {
    id: String,
    level: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LearnedSkill {
    skill_id: String,
    name_ko: String,
    learn_level: i64,
    element_ko: Option<String>,
    power: Option<i64>,
    cool_time: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ActiveSkillRecord {
    element: String,
    #[serde(rename = "type")]
    skill_kind: String,
    power: i64,
    min_range: i64,
    max_range: i64,
    cool_time: f64,
    #[serde(default)]
    effects: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LocalizedText {
    localized_name: String,
    description: Option<String>,
}

type ActiveSkills = BTreeMap<String, ActiveSkillRecord>;
type PassiveSkills = BTreeMap<String, serde_json::Value>;
type LocalizedTexts = BTreeMap<String, LocalizedText>;

#[derive(Debug, Deserialize)]
struct ItemCatalogRoot {
    schema_version: u32,
    game_build_id: String,
    mapping_sha256: String,
    contract_review_id: String,
    verified: bool,
    items: Vec<ItemSourceRecord>,
    recipes: Vec<ItemRecipeSource>,
    pal_drops: Vec<PalDropSource>,
}

#[derive(Debug, Deserialize)]
struct ItemSourceRecord {
    item_id: String,
    name_ko: String,
    description_ko: Option<String>,
    type_a: String,
    type_b: String,
    price: i64,
    weight_milli: i64,
    maximum_stack_count: i64,
    rarity: i64,
    rank: i64,
    icon_name: String,
    legal_in_game: bool,
    localization_fallback: bool,
}

#[derive(Debug, Deserialize)]
struct ItemRecipeSource {
    recipe_id: String,
    output_item_id: String,
    output_quantity: i64,
    ingredients: Vec<ItemRecipeIngredientSource>,
    work_amount: i64,
    unlock_item_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ItemRecipeIngredientSource {
    item_id: String,
    quantity: i64,
}

#[derive(Debug, Deserialize)]
struct PalDropSource {
    method_id: String,
    source_row_id: String,
    slot: i64,
    pal_id: String,
    level: i64,
    item_id: String,
    minimum_quantity: i64,
    maximum_quantity: i64,
    probability_ppm: i64,
}

#[derive(Debug)]
struct SkillSources {
    active: ActiveSkills,
    active_ko: LocalizedTexts,
    passive: PassiveSkills,
    passive_ko: LocalizedTexts,
}

#[derive(Debug)]
struct SeedInputs {
    catalog: CatalogEnvelope,
    breeding: BreedingRoot,
    skills: SkillSources,
    item_catalog: ItemCatalogRoot,
    item_catalog_hash: String,
    source_hash: String,
    source_artifacts: Vec<SourceArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BreedingRoot {
    breeding: Vec<BreedingRule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BreedingRule {
    parent1_internal_name: String,
    parent1_gender: String,
    parent2_internal_name: String,
    parent2_gender: String,
    child_internal_name: String,
}

#[derive(Debug, Serialize)]
struct SeedManifest {
    schema: &'static str,
    dataset_version: String,
    game_version: String,
    game_build_id: String,
    verified: bool,
    activated: bool,
    sqlite_verified: bool,
    source_id: String,
    source_hash: String,
    catalog_quality: String,
    catalog_warnings: Vec<String>,
    species_count: usize,
    active_skill_count: usize,
    passive_count: usize,
    item_count: usize,
    recipe_count: usize,
    pal_drop_count: usize,
    item_catalog_sha256: String,
    item_contract_review_id: String,
    localized_skill_fallback_count: usize,
    localized_passive_fallback_count: usize,
    localized_name_fallback_count: usize,
    alias_count: usize,
    breeding_rule_count: usize,
    graph_node_count: usize,
    graph_edge_count: usize,
    wiki_page_count: usize,
    knowledge_error_count: usize,
    missing_active_skill_edge_count: usize,
    missing_passive_edge_count: usize,
    missing_drop_species_edge_count: usize,
    missing_unlock_item_edge_count: usize,
    canonicalized_reference_count: usize,
    duplicate_reference_count: usize,
    knowledge_error_reduction_count: usize,
    knowledge_quality_report: SeedFile,
    skipped_gender_specific_rule_count: usize,
    skipped_unknown_species_rule_count: usize,
    files: Vec<SeedFile>,
}

#[derive(Debug, Serialize)]
struct SeedFile {
    path: String,
    sha256: String,
    bytes: usize,
}

#[derive(Debug)]
struct GeneratedSeed {
    files: BTreeMap<String, Vec<u8>>,
    quality_report: Vec<u8>,
    manifest: SeedManifest,
}

#[derive(Debug)]
struct ItemSeedRows {
    items: Vec<String>,
    recipes: Vec<String>,
    acquisitions: Vec<String>,
    contract_review_id: String,
}

#[derive(Debug)]
struct SqlCounts {
    species: usize,
    active_skills: usize,
    passives: usize,
    items: usize,
    recipes: usize,
    acquisitions: usize,
    aliases: usize,
    breeding_rules: usize,
    knowledge: KnowledgeProjectionCounts,
}

#[derive(Debug, PartialEq, Eq)]
struct ObservedSqlCounts {
    species: usize,
    active_skills: usize,
    passives: usize,
    items: usize,
    recipes: usize,
    acquisitions: usize,
    aliases: usize,
    breeding_rules: usize,
    graph_nodes: usize,
    graph_edges: usize,
    wiki_pages: usize,
    knowledge_manifests: usize,
    inactive_datasets: usize,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pal-cloud-seed: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let options = parse_options(env::args_os().skip(1))?;
    validate_options(&options)?;
    let catalog_bytes = collect_catalog(&options)?;
    let catalog: CatalogEnvelope = serde_json::from_slice(&catalog_bytes)
        .map_err(|error| format!("catalog worker returned invalid JSON: {error}"))?;
    let breeding_bytes = fs::read(&options.breeding_file)
        .map_err(|error| format!("cannot read breeding data: {error}"))?;
    let breeding_hash = sha256_hex(&breeding_bytes);
    if breeding_hash != EXPECTED_BREEDING_SHA256 {
        return Err(format!(
            "breeding data hash mismatch: expected {EXPECTED_BREEDING_SHA256}, got {breeding_hash}"
        ));
    }
    let breeding: BreedingRoot = serde_json::from_slice(&breeding_bytes)
        .map_err(|error| format!("breeding data is invalid: {error}"))?;
    let active_skills_bytes = read_bounded_file(&options.active_skills_file, "active skills")?;
    let active_skills_ko_bytes =
        read_bounded_file(&options.active_skills_ko_file, "Korean active skills")?;
    let passive_skills_bytes = read_bounded_file(&options.passive_skills_file, "passive skills")?;
    let passive_skills_ko_bytes =
        read_bounded_file(&options.passive_skills_ko_file, "Korean passive skills")?;
    let active_skills: ActiveSkills = parse_json_source(&active_skills_bytes, "active skills")?;
    let active_skills_ko: LocalizedTexts =
        parse_json_source(&active_skills_ko_bytes, "Korean active skills")?;
    let passive_skills: PassiveSkills = parse_json_source(&passive_skills_bytes, "passive skills")?;
    let passive_skills_ko: LocalizedTexts =
        parse_json_source(&passive_skills_ko_bytes, "Korean passive skills")?;
    let item_catalog_bytes = read_bounded_file(&options.item_catalog_file, "item catalog")?;
    let item_catalog_hash = sha256_hex(&item_catalog_bytes);
    if item_catalog_hash != EXPECTED_ITEM_CATALOG_SHA256 {
        return Err(format!(
            "item catalog hash mismatch: expected {EXPECTED_ITEM_CATALOG_SHA256}, got {item_catalog_hash}"
        ));
    }
    let item_catalog: ItemCatalogRoot = parse_json_source(&item_catalog_bytes, "item catalog")?;
    let source_hash = sha256_hex(
        &[
            catalog_bytes.as_slice(),
            breeding_bytes.as_slice(),
            active_skills_bytes.as_slice(),
            active_skills_ko_bytes.as_slice(),
            passive_skills_bytes.as_slice(),
            passive_skills_ko_bytes.as_slice(),
            item_catalog_bytes.as_slice(),
        ]
        .concat(),
    );
    let source_artifacts = vec![
        SourceArtifact {
            artifact_id: "artifact:species-catalog",
            artifact_kind: "catalog",
            logical_source_ref: "catalog:species".to_owned(),
            sha256: sha256_hex(&catalog_bytes),
            byte_size: catalog_bytes.len(),
            evidence_quality: catalog_evidence_quality(&catalog.quality),
        },
        SourceArtifact {
            artifact_id: "artifact:breeding",
            artifact_kind: "game_extract",
            logical_source_ref: format!("game-files:steam:{}:breeding", options.game_build_id),
            sha256: breeding_hash,
            byte_size: breeding_bytes.len(),
            evidence_quality: "exact",
        },
        SourceArtifact {
            artifact_id: "artifact:active-skills",
            artifact_kind: "game_extract",
            logical_source_ref: format!("game-files:steam:{}:active-skills", options.game_build_id),
            sha256: sha256_hex(&active_skills_bytes),
            byte_size: active_skills_bytes.len(),
            evidence_quality: "exact",
        },
        SourceArtifact {
            artifact_id: "artifact:active-skills-ko",
            artifact_kind: "localization",
            logical_source_ref: format!(
                "game-files:steam:{}:ko-active-skills",
                options.game_build_id
            ),
            sha256: sha256_hex(&active_skills_ko_bytes),
            byte_size: active_skills_ko_bytes.len(),
            evidence_quality: "exact",
        },
        SourceArtifact {
            artifact_id: "artifact:passive-skills",
            artifact_kind: "game_extract",
            logical_source_ref: format!(
                "game-files:steam:{}:passive-skills",
                options.game_build_id
            ),
            sha256: sha256_hex(&passive_skills_bytes),
            byte_size: passive_skills_bytes.len(),
            evidence_quality: "exact",
        },
        SourceArtifact {
            artifact_id: "artifact:passive-skills-ko",
            artifact_kind: "localization",
            logical_source_ref: format!(
                "game-files:steam:{}:ko-passive-skills",
                options.game_build_id
            ),
            sha256: sha256_hex(&passive_skills_ko_bytes),
            byte_size: passive_skills_ko_bytes.len(),
            evidence_quality: "exact",
        },
        SourceArtifact {
            artifact_id: "artifact:item-catalog",
            artifact_kind: "game_extract",
            logical_source_ref: format!(
                "game-files:steam:{}:items-recipes-drops",
                options.game_build_id
            ),
            sha256: item_catalog_hash.clone(),
            byte_size: item_catalog_bytes.len(),
            evidence_quality: "exact",
        },
    ];
    let generated = generate_seed(
        &options,
        SeedInputs {
            catalog,
            breeding,
            skills: SkillSources {
                active: active_skills,
                active_ko: active_skills_ko,
                passive: passive_skills,
                passive_ko: passive_skills_ko,
            },
            item_catalog,
            item_catalog_hash,
            source_hash,
            source_artifacts,
        },
    )?;
    write_seed(&options.output_dir, generated)
}

fn catalog_evidence_quality(quality: &str) -> &'static str {
    match quality {
        "exact" => "exact",
        "measured" => "measured",
        _ => "unknown",
    }
}

fn read_bounded_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read {label}: {error}"))?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(format!("{label} exceeds 16 MiB"));
    }
    Ok(bytes)
}

fn parse_json_source<T: for<'de> Deserialize<'de>>(bytes: &[u8], label: &str) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("{label} is invalid: {error}"))
}

fn parse_options(
    mut arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<Options, String> {
    let mut values = BTreeMap::<String, String>::new();
    while let Some(key) = arguments.next() {
        let key = key
            .into_string()
            .map_err(|_| "arguments must be valid Unicode".to_owned())?;
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {key}"))?
            .into_string()
            .map_err(|_| format!("value for {key} must be valid Unicode"))?;
        if values.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate argument {key}"));
        }
    }
    let required = |name: &str| {
        values
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing required argument {name}"))
    };
    let expected_species_count = values
        .get("--expected-species-count")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| "--expected-species-count must be an integer".to_owned())
        })
        .transpose()?
        .unwrap_or(DEFAULT_EXPECTED_SPECIES_COUNT);
    for key in values.keys() {
        if ![
            "--catalog-worker",
            "--catalog-dir",
            "--breeding-file",
            "--active-skills-file",
            "--active-skills-ko-file",
            "--passive-skills-file",
            "--passive-skills-ko-file",
            "--item-catalog-file",
            "--output-dir",
            "--dataset-version",
            "--game-version",
            "--game-build-id",
            "--expected-species-count",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unknown argument {key}"));
        }
    }
    Ok(Options {
        catalog_worker: PathBuf::from(required("--catalog-worker")?),
        catalog_dir: PathBuf::from(required("--catalog-dir")?),
        breeding_file: PathBuf::from(required("--breeding-file")?),
        active_skills_file: PathBuf::from(required("--active-skills-file")?),
        active_skills_ko_file: PathBuf::from(required("--active-skills-ko-file")?),
        passive_skills_file: PathBuf::from(required("--passive-skills-file")?),
        passive_skills_ko_file: PathBuf::from(required("--passive-skills-ko-file")?),
        item_catalog_file: PathBuf::from(required("--item-catalog-file")?),
        output_dir: PathBuf::from(required("--output-dir")?),
        dataset_version: required("--dataset-version")?,
        game_version: required("--game-version")?,
        game_build_id: required("--game-build-id")?,
        expected_species_count,
        expected_item_count: EXPECTED_ITEM_COUNT,
        expected_recipe_count: EXPECTED_RECIPE_COUNT,
        expected_pal_drop_count: EXPECTED_PAL_DROP_COUNT,
    })
}

fn validate_options(options: &Options) -> Result<(), String> {
    for (name, path) in [
        ("--catalog-worker", &options.catalog_worker),
        ("--catalog-dir", &options.catalog_dir),
        ("--breeding-file", &options.breeding_file),
        ("--active-skills-file", &options.active_skills_file),
        ("--active-skills-ko-file", &options.active_skills_ko_file),
        ("--passive-skills-file", &options.passive_skills_file),
        ("--passive-skills-ko-file", &options.passive_skills_ko_file),
        ("--item-catalog-file", &options.item_catalog_file),
        ("--output-dir", &options.output_dir),
    ] {
        if !path.is_absolute() {
            return Err(format!("{name} must be absolute"));
        }
    }
    if !options.catalog_worker.is_file() {
        return Err("--catalog-worker does not exist".to_owned());
    }
    if !options.catalog_dir.is_dir() {
        return Err("--catalog-dir does not exist".to_owned());
    }
    if !options.breeding_file.is_file() {
        return Err("--breeding-file does not exist".to_owned());
    }
    for (name, path) in [
        ("--active-skills-file", &options.active_skills_file),
        ("--active-skills-ko-file", &options.active_skills_ko_file),
        ("--passive-skills-file", &options.passive_skills_file),
        ("--passive-skills-ko-file", &options.passive_skills_ko_file),
        ("--item-catalog-file", &options.item_catalog_file),
    ] {
        if !path.is_file() {
            return Err(format!("{name} does not exist"));
        }
    }
    if options.output_dir.exists() {
        return Err("--output-dir already exists; refusing to overwrite seed files".to_owned());
    }
    for (name, value) in [
        ("--dataset-version", options.dataset_version.as_str()),
        ("--game-version", options.game_version.as_str()),
        ("--game-build-id", options.game_build_id.as_str()),
    ] {
        if value.is_empty()
            || value.len() > 128
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".:_-".contains(character))
        {
            return Err(format!("{name} contains unsupported characters"));
        }
    }
    if options.expected_species_count == 0 || options.expected_species_count > 1024 {
        return Err("--expected-species-count must be between 1 and 1024".to_owned());
    }
    Ok(())
}

fn collect_catalog(options: &Options) -> Result<Vec<u8>, String> {
    let mut child = Command::new(&options.catalog_worker)
        .arg("--catalog-dir")
        .arg(&options.catalog_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start catalog worker: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "catalog worker stdin is unavailable".to_owned())?
        .write_all(br#"{"schema_version":1,"query":"","limit":512}"#)
        .map_err(|error| format!("cannot request the catalog: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot wait for catalog worker: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "catalog worker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if output.stdout.len() > MAX_CATALOG_BYTES {
        return Err("catalog worker output exceeds 16 MiB".to_owned());
    }
    Ok(output.stdout)
}

fn generate_seed(options: &Options, inputs: SeedInputs) -> Result<GeneratedSeed, String> {
    let SeedInputs {
        catalog,
        breeding,
        skills,
        item_catalog,
        item_catalog_hash,
        source_hash,
        source_artifacts,
    } = inputs;
    if catalog.status != "ok" {
        return Err(format!("catalog status is {}", catalog.status));
    }
    if catalog.records.len() != options.expected_species_count
        || catalog.matched_count != catalog.records.len()
        || catalog.returned_count != catalog.records.len()
    {
        return Err(format!(
            "catalog count mismatch: expected {}, records {}, matched {}, returned {}",
            options.expected_species_count,
            catalog.records.len(),
            catalog.matched_count,
            catalog.returned_count
        ));
    }
    if skills.active.is_empty() {
        return Err("active skill source is empty".to_owned());
    }
    if skills.passive.is_empty() {
        return Err("passive skill source is empty".to_owned());
    }

    let mut active_skill_ids = BTreeSet::new();
    for raw_skill_id in skills.active.keys() {
        let skill_id = canonical_active_skill_id(raw_skill_id);
        validate_id("skill_id", skill_id)?;
        if !active_skill_ids.insert(skill_id.to_owned()) {
            return Err(format!("duplicate canonical active skill id {skill_id}"));
        }
    }
    let active_skill_index = ReferenceIndex::new(active_skill_ids.iter().cloned());
    let passive_skill_index = ReferenceIndex::new(skills.passive.keys().cloned());
    let mut quality_report = QualityReportBuilder::new(
        &options.dataset_version,
        &options.game_build_id,
        &source_hash,
    );

    let mut species_ids = BTreeSet::new();
    let mut species_names = BTreeMap::new();
    let mut species_rows = Vec::with_capacity(catalog.records.len());
    let mut alias_rows = BTreeSet::new();
    let mut localized_name_fallback_count = 0;
    for record in &catalog.records {
        validate_id("species_id", &record.species_id)?;
        if !species_ids.insert(record.species_id.clone()) {
            return Err(format!("duplicate species id {}", record.species_id));
        }
        let name_ko = record.name_ko.as_deref().unwrap_or(&record.species_id);
        if record.name_ko.is_none() {
            localized_name_fallback_count += 1;
        }
        species_names.insert(record.species_id.clone(), name_ko.to_owned());
        let element_json = serde_json::to_string(
            &record
                .elements
                .iter()
                .map(|element| normalize_token(&element.id))
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())?;
        let work_json = serde_json::to_string(
            &record
                .work_suitability
                .iter()
                .map(|work| (normalize_token(&work.id), work.level))
                .collect::<BTreeMap<_, _>>(),
        )
        .map_err(|error| error.to_string())?;
        let learned_skills =
            canonicalize_learned_skills(record, &active_skill_index, &mut quality_report)?;
        let learned_skills_json =
            serde_json::to_string(&learned_skills).map_err(|error| error.to_string())?;
        let guaranteed_passives =
            canonicalize_guaranteed_passives(record, &passive_skill_index, &mut quality_report);
        let guaranteed_passives_json =
            serde_json::to_string(&guaranteed_passives).map_err(|error| error.to_string())?;
        species_rows.push(format!(
            "INSERT INTO catalog_species \
             (dataset_version, species_id, name_ko, name_en, element_json, work_json, \
              base_hp, base_attack, base_defense, ride_sprint_speed, ride_stamina, \
              source_ref, paldex_number, rarity, run_speed, transport_speed, food_amount, \
              nocturnal, learned_skills_json, guaranteed_passives_json) VALUES \
             ({}, {}, {}, NULL, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {});",
            sql_text(&options.dataset_version),
            sql_text(&record.species_id),
            sql_text(name_ko),
            sql_text(&element_json),
            sql_text(&work_json),
            sql_optional_i64(record.hp),
            sql_optional_i64(record.attack),
            sql_optional_i64(record.defense),
            sql_optional_i64(record.ride_sprint_speed),
            sql_optional_i64(record.stamina),
            sql_text(&catalog.source_id),
            sql_optional_u32(record.paldex_number),
            sql_optional_u32(record.rarity),
            sql_optional_i64(record.run_speed),
            sql_optional_i64(record.transport_speed),
            sql_optional_i64(record.food_amount),
            i64::from(record.nocturnal),
            sql_text(&learned_skills_json),
            sql_text(&guaranteed_passives_json),
        ));
        add_catalog_alias(
            &mut alias_rows,
            &options.dataset_version,
            "species",
            &record.species_id,
            name_ko,
        );
        add_catalog_alias(
            &mut alias_rows,
            &options.dataset_version,
            "species",
            &record.species_id,
            &record.species_id,
        );
        if let Some(number) = record.paldex_number {
            for alias in [
                number.to_string(),
                format!("No.{number}"),
                format!("도감 {number}"),
            ] {
                add_catalog_alias(
                    &mut alias_rows,
                    &options.dataset_version,
                    "species",
                    &record.species_id,
                    &alias,
                );
            }
        }
    }

    let mut active_skill_rows = Vec::with_capacity(skills.active.len());
    let mut localized_skill_fallback_count = 0;
    for (raw_skill_id, skill) in &skills.active {
        let skill_id = canonical_active_skill_id(raw_skill_id);
        let localized = skills
            .active_ko
            .get(raw_skill_id)
            .or_else(|| skills.active_ko.get(skill_id));
        let name_ko = localized
            .map(|text| text.localized_name.trim())
            .filter(|name| !name.is_empty())
            .unwrap_or(skill_id);
        if localized.is_none() {
            localized_skill_fallback_count += 1;
        }
        let effects_json =
            serde_json::to_string(&skill.effects).map_err(|error| error.to_string())?;
        active_skill_rows.push(format!(
            "INSERT INTO catalog_active_skills \
             (dataset_version, skill_id, name_ko, description_ko, element, skill_kind, \
              power, min_range, max_range, cool_time, effects_json, source_ref) VALUES \
             ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {});",
            sql_text(&options.dataset_version),
            sql_text(skill_id),
            sql_text(name_ko),
            sql_optional_text(localized.and_then(|text| text.description.as_deref())),
            sql_text(&skill.element),
            sql_text(&skill.skill_kind),
            skill.power,
            skill.min_range,
            skill.max_range,
            skill.cool_time,
            sql_text(&effects_json),
            sql_text(&catalog.source_id),
        ));
        add_catalog_alias(
            &mut alias_rows,
            &options.dataset_version,
            "active_skill",
            skill_id,
            name_ko,
        );
        add_catalog_alias(
            &mut alias_rows,
            &options.dataset_version,
            "active_skill",
            skill_id,
            skill_id,
        );
    }

    let mut passive_rows = Vec::with_capacity(skills.passive.len());
    let mut localized_passive_fallback_count = 0;
    for (passive_id, effect) in &skills.passive {
        validate_id("passive_id", passive_id)?;
        let localized = skills.passive_ko.get(passive_id);
        let name_ko = localized
            .map(|text| text.localized_name.trim())
            .filter(|name| !name.is_empty())
            .unwrap_or(passive_id);
        if localized.is_none() {
            localized_passive_fallback_count += 1;
        }
        let effect_json = serde_json::to_string(&effect).map_err(|error| error.to_string())?;
        passive_rows.push(format!(
            "INSERT INTO catalog_passives \
             (dataset_version, passive_id, name_ko, description_ko, effect_json) VALUES \
             ({}, {}, {}, {}, {});",
            sql_text(&options.dataset_version),
            sql_text(passive_id),
            sql_text(name_ko),
            sql_optional_text(localized.and_then(|text| text.description.as_deref())),
            sql_text(&effect_json),
        ));
        add_catalog_alias(
            &mut alias_rows,
            &options.dataset_version,
            "passive",
            passive_id,
            name_ko,
        );
        add_catalog_alias(
            &mut alias_rows,
            &options.dataset_version,
            "passive",
            passive_id,
            passive_id,
        );
    }

    let item_seed = generate_item_seed_rows(
        options,
        &item_catalog,
        &item_catalog_hash,
        &species_names,
        &ReferenceIndex::new(species_ids.iter().cloned()),
        &mut quality_report,
        &mut alias_rows,
    )?;

    let mut rules = BTreeSet::new();
    let mut skipped_gender_specific_rule_count = 0;
    let mut skipped_unknown_species_rule_count = 0;
    for rule in &breeding.breeding {
        if rule.parent1_gender != "WILDCARD" || rule.parent2_gender != "WILDCARD" {
            skipped_gender_specific_rule_count += 1;
            continue;
        }
        if !species_ids.contains(&rule.parent1_internal_name)
            || !species_ids.contains(&rule.parent2_internal_name)
            || !species_ids.contains(&rule.child_internal_name)
        {
            skipped_unknown_species_rule_count += 1;
            continue;
        }
        let (parent_a, parent_b) = if rule.parent1_internal_name <= rule.parent2_internal_name {
            (
                rule.parent1_internal_name.clone(),
                rule.parent2_internal_name.clone(),
            )
        } else {
            (
                rule.parent2_internal_name.clone(),
                rule.parent1_internal_name.clone(),
            )
        };
        rules.insert((parent_a, parent_b, rule.child_internal_name.clone()));
    }
    let breeding_rows = rules
        .iter()
        .map(|(parent_a, parent_b, child)| {
            format!(
                "INSERT INTO breeding_rules \
                 (dataset_version, parent_a_species_id, parent_b_species_id, \
                  child_species_id, rule_kind) VALUES ({}, {}, {}, {}, 'exact');",
                sql_text(&options.dataset_version),
                sql_text(parent_a),
                sql_text(parent_b),
                sql_text(child),
            )
        })
        .collect::<Vec<_>>();
    let canonical_node_count = species_rows.len()
        + active_skill_rows.len()
        + passive_rows.len()
        + item_seed.items.len()
        + item_seed.recipes.len()
        + breeding_rows.len();
    let wiki_page_count = canonical_node_count - breeding_rows.len();
    let expected_knowledge = KnowledgeProjectionCounts {
        graph_nodes: canonical_node_count + wiki_page_count,
        graph_edges: 0,
        wiki_pages: wiki_page_count,
        error_count: 0,
        missing_active_skill_edges: 0,
        missing_passive_edges: 0,
        missing_drop_species_edges: 0,
        missing_unlock_item_edges: 0,
    };

    let mut files = BTreeMap::new();
    files.insert(
        "0000_dataset.sql".to_owned(),
        sql_document(&[format!(
            "INSERT INTO data_versions \
             (dataset_version, game_version, game_build_id, schema_version, source_hash, \
               verified, activated_at) VALUES ({}, {}, {}, {}, {}, 0, NULL);",
            sql_text(&options.dataset_version),
            sql_text(&options.game_version),
            sql_text(&options.game_build_id),
            sql_text(KNOWLEDGE_COMPILER_SCHEMA_VERSION),
            sql_text(&source_hash),
        )])
        .into_bytes(),
    );
    add_chunked_sql(
        &mut files,
        "0100_species",
        &species_rows,
        SPECIES_CHUNK_SIZE,
    );
    add_chunked_sql(
        &mut files,
        "0140_active_skills",
        &active_skill_rows,
        SKILL_CHUNK_SIZE,
    );
    add_chunked_sql(
        &mut files,
        "0160_passives",
        &passive_rows,
        PASSIVE_CHUNK_SIZE,
    );
    add_chunked_sql(&mut files, "0180_items", &item_seed.items, ITEM_CHUNK_SIZE);
    let alias_rows = alias_rows.into_iter().collect::<Vec<_>>();
    add_chunked_sql(&mut files, "0200_aliases", &alias_rows, ALIAS_CHUNK_SIZE);
    add_chunked_sql(
        &mut files,
        "0240_recipes",
        &item_seed.recipes,
        RECIPE_CHUNK_SIZE,
    );
    add_chunked_sql(
        &mut files,
        "0260_acquisitions",
        &item_seed.acquisitions,
        ACQUISITION_CHUNK_SIZE,
    );
    add_chunked_sql(
        &mut files,
        "0300_breeding",
        &breeding_rows,
        BREEDING_CHUNK_SIZE,
    );
    files.insert(
        "0400_knowledge.sql".to_owned(),
        knowledge::projection_sql(options, &source_hash, &source_artifacts).into_bytes(),
    );

    let file_manifest = files
        .iter()
        .map(|(path, bytes)| SeedFile {
            path: path.clone(),
            sha256: sha256_hex(bytes),
            bytes: bytes.len(),
        })
        .collect();
    let knowledge_counts = verify_generated_sql(
        &files,
        &SqlCounts {
            species: species_rows.len(),
            active_skills: active_skill_rows.len(),
            passives: passive_rows.len(),
            items: item_seed.items.len(),
            recipes: item_seed.recipes.len(),
            acquisitions: item_seed.acquisitions.len(),
            aliases: alias_rows.len(),
            breeding_rules: breeding_rows.len(),
            knowledge: expected_knowledge,
        },
    )?;
    let quality_report = quality_report.finish(knowledge_counts.error_count);
    let expected_error_count = quality_report.summary.unresolved_reference_count
        + quality_report.summary.ambiguous_reference_count;
    if expected_error_count != knowledge_counts.error_count {
        return Err(format!(
            "quality report expected {expected_error_count} unresolved graph endpoints, \
             but SQLite verification found {}",
            knowledge_counts.error_count
        ));
    }
    let mut quality_report_bytes =
        serde_json::to_vec_pretty(&quality_report).map_err(|error| error.to_string())?;
    quality_report_bytes.push(b'\n');
    let knowledge_quality_report = SeedFile {
        path: KNOWLEDGE_QUALITY_REPORT_PATH.to_owned(),
        sha256: sha256_hex(&quality_report_bytes),
        bytes: quality_report_bytes.len(),
    };
    let manifest = SeedManifest {
        schema: "pal-companion-cloud-seed-v1",
        dataset_version: options.dataset_version.clone(),
        game_version: options.game_version.clone(),
        game_build_id: options.game_build_id.clone(),
        verified: false,
        activated: false,
        sqlite_verified: true,
        source_id: catalog.source_id,
        source_hash,
        catalog_quality: catalog.quality,
        catalog_warnings: catalog.warnings,
        species_count: species_rows.len(),
        active_skill_count: active_skill_rows.len(),
        passive_count: passive_rows.len(),
        item_count: item_seed.items.len(),
        recipe_count: item_seed.recipes.len(),
        pal_drop_count: item_seed.acquisitions.len(),
        item_catalog_sha256: item_catalog_hash,
        item_contract_review_id: item_seed.contract_review_id,
        localized_skill_fallback_count,
        localized_passive_fallback_count,
        localized_name_fallback_count,
        alias_count: alias_rows.len(),
        breeding_rule_count: breeding_rows.len(),
        graph_node_count: knowledge_counts.graph_nodes,
        graph_edge_count: knowledge_counts.graph_edges,
        wiki_page_count: knowledge_counts.wiki_pages,
        knowledge_error_count: knowledge_counts.error_count,
        missing_active_skill_edge_count: knowledge_counts.missing_active_skill_edges,
        missing_passive_edge_count: knowledge_counts.missing_passive_edges,
        missing_drop_species_edge_count: knowledge_counts.missing_drop_species_edges,
        missing_unlock_item_edge_count: knowledge_counts.missing_unlock_item_edges,
        canonicalized_reference_count: quality_report.summary.canonicalized_match_count,
        duplicate_reference_count: quality_report.summary.duplicate_reference_count,
        knowledge_error_reduction_count: quality_report.summary.error_book_reduction_count,
        knowledge_quality_report,
        skipped_gender_specific_rule_count,
        skipped_unknown_species_rule_count,
        files: file_manifest,
    };
    Ok(GeneratedSeed {
        files,
        quality_report: quality_report_bytes,
        manifest,
    })
}

fn canonical_active_skill_id(raw_skill_id: &str) -> &str {
    raw_skill_id
        .strip_prefix("EPalWazaID::")
        .unwrap_or(raw_skill_id)
}

fn canonicalize_learned_skills(
    record: &SpeciesRecord,
    active_skill_index: &ReferenceIndex,
    quality_report: &mut QualityReportBuilder,
) -> Result<Vec<LearnedSkill>, String> {
    let mut canonical = Vec::with_capacity(record.learned_skills.len());
    let mut fingerprints_by_skill = BTreeMap::<String, String>::new();
    for source_skill in &record.learned_skills {
        let resolution = active_skill_index.resolve(&source_skill.skill_id, Some("EPalWazaID::"));
        let mut skill = source_skill.clone();
        skill.skill_id.clone_from(&resolution.output_id);
        let fingerprint = serde_json::to_string(&skill).map_err(|error| error.to_string())?;
        if let Some(existing) = fingerprints_by_skill.get(&skill.skill_id) {
            if existing != &fingerprint {
                return Err(format!(
                    "species {} has conflicting learned skill records for {}",
                    record.species_id, skill.skill_id
                ));
            }
            quality_report.record_duplicate(
                "catalog_species.learned_skills",
                "active_skill",
                &record.species_id,
                &source_skill.skill_id,
                &skill.skill_id,
            );
            continue;
        }
        fingerprints_by_skill.insert(skill.skill_id.clone(), fingerprint);
        quality_report.record_resolution(
            "catalog_species.learned_skills",
            "active_skill",
            &record.species_id,
            &source_skill.skill_id,
            &resolution,
        );
        canonical.push(skill);
    }
    Ok(canonical)
}

fn canonicalize_guaranteed_passives(
    record: &SpeciesRecord,
    passive_skill_index: &ReferenceIndex,
    quality_report: &mut QualityReportBuilder,
) -> Vec<String> {
    let mut canonical = Vec::with_capacity(record.guaranteed_passives.len());
    let mut seen = BTreeSet::new();
    for passive in &record.guaranteed_passives {
        let resolution = passive_skill_index.resolve(&passive.id, None);
        if !seen.insert(resolution.output_id.clone()) {
            quality_report.record_duplicate(
                "catalog_species.guaranteed_passives",
                "passive",
                &record.species_id,
                &passive.id,
                &resolution.output_id,
            );
            continue;
        }
        quality_report.record_resolution(
            "catalog_species.guaranteed_passives",
            "passive",
            &record.species_id,
            &passive.id,
            &resolution,
        );
        canonical.push(resolution.output_id);
    }
    canonical
}

fn generate_item_seed_rows(
    options: &Options,
    catalog: &ItemCatalogRoot,
    catalog_hash: &str,
    species_names: &BTreeMap<String, String>,
    species_index: &ReferenceIndex,
    quality_report: &mut QualityReportBuilder,
    alias_rows: &mut BTreeSet<String>,
) -> Result<ItemSeedRows, String> {
    if catalog_hash != EXPECTED_ITEM_CATALOG_SHA256
        || catalog.schema_version != 1
        || !catalog.verified
        || (catalog.game_build_id != options.game_build_id
            && catalog.game_build_id != format!("steam:{}", options.game_build_id))
        || catalog.mapping_sha256 != EXPECTED_ITEM_MAPPING_SHA256
        || catalog.contract_review_id.trim().is_empty()
    {
        return Err("item catalog identity or verification gate failed".to_owned());
    }
    if catalog.items.len() != options.expected_item_count
        || catalog.recipes.len() != options.expected_recipe_count
        || catalog.pal_drops.len() != options.expected_pal_drop_count
    {
        return Err(format!(
            "item catalog count mismatch: expected ({}, {}, {}), got ({}, {}, {})",
            options.expected_item_count,
            options.expected_recipe_count,
            options.expected_pal_drop_count,
            catalog.items.len(),
            catalog.recipes.len(),
            catalog.pal_drops.len()
        ));
    }

    let mut item_ids = BTreeSet::new();
    let mut item_names = BTreeMap::new();
    let mut item_rows = Vec::with_capacity(catalog.items.len());
    for item in &catalog.items {
        validate_id("item_id", &item.item_id)?;
        if !item_ids.insert(item.item_id.clone()) {
            return Err(format!("duplicate item id {}", item.item_id));
        }
        if item.price < 0
            || item.weight_milli < 0
            || item.maximum_stack_count < 0
            || item.rarity < 0
            || item.rank < 0
        {
            return Err(format!(
                "item {} has a negative numeric field",
                item.item_id
            ));
        }
        let name_ko = if item.name_ko.trim().is_empty() {
            item.item_id.as_str()
        } else {
            item.name_ko.trim()
        };
        item_names.insert(item.item_id.clone(), name_ko.to_owned());
        item_rows.push(format!(
            "INSERT INTO catalog_items \
             (dataset_version, item_id, name_ko, name_en, category, price, \
              description_ko, subcategory, weight_milli, maximum_stack_count, \
              rarity, rank, icon_name, legal_in_game, localization_fallback, source_ref) \
             VALUES ({}, {}, {}, NULL, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {});",
            sql_text(&options.dataset_version),
            sql_text(&item.item_id),
            sql_text(name_ko),
            sql_text(&item.type_a),
            item.price,
            sql_optional_text(item.description_ko.as_deref()),
            sql_text(&item.type_b),
            item.weight_milli,
            item.maximum_stack_count,
            item.rarity,
            item.rank,
            sql_text(&item.icon_name),
            i64::from(item.legal_in_game),
            i64::from(item.localization_fallback),
            sql_text(&format!(
                "game-files:steam:{}:DT_ItemDataTable",
                options.game_build_id
            )),
        ));
        add_catalog_alias(
            alias_rows,
            &options.dataset_version,
            "item",
            &item.item_id,
            name_ko,
        );
        add_catalog_alias(
            alias_rows,
            &options.dataset_version,
            "item",
            &item.item_id,
            &item.item_id,
        );
    }
    let item_index = ReferenceIndex::new(item_ids.iter().cloned());

    let mut recipe_ids = BTreeSet::new();
    let mut recipe_rows = Vec::with_capacity(catalog.recipes.len());
    for recipe in &catalog.recipes {
        validate_id("recipe_id", &recipe.recipe_id)?;
        if !recipe_ids.insert(recipe.recipe_id.clone()) {
            return Err(format!("duplicate recipe id {}", recipe.recipe_id));
        }
        if !item_ids.contains(&recipe.output_item_id) {
            return Err(format!(
                "recipe {} references unknown output item {}",
                recipe.recipe_id, recipe.output_item_id
            ));
        }
        if recipe.output_quantity <= 0 || recipe.work_amount < 0 {
            return Err(format!(
                "recipe {} has invalid output or work amount",
                recipe.recipe_id
            ));
        }
        let ingredients = recipe
            .ingredients
            .iter()
            .map(|ingredient| {
                if ingredient.quantity <= 0 || !item_ids.contains(&ingredient.item_id) {
                    return Err(format!(
                        "recipe {} has invalid ingredient {}",
                        recipe.recipe_id, ingredient.item_id
                    ));
                }
                Ok(serde_json::json!({
                    "item_id": &ingredient.item_id,
                    "name_ko": item_names
                        .get(&ingredient.item_id)
                        .cloned()
                        .unwrap_or_else(|| ingredient.item_id.clone()),
                    "quantity": ingredient.quantity
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let ingredients_json =
            serde_json::to_string(&ingredients).map_err(|error| error.to_string())?;
        let unlock_item_id = recipe.unlock_item_id.as_deref().map(|raw_unlock_item_id| {
            let resolution = item_index.resolve(raw_unlock_item_id, None);
            quality_report.record_resolution(
                "item_catalog.recipes.unlock_item_id",
                "item",
                &recipe.recipe_id,
                raw_unlock_item_id,
                &resolution,
            );
            resolution.output_id
        });
        recipe_rows.push(format!(
            "INSERT INTO catalog_recipes \
             (dataset_version, recipe_id, output_item_id, output_quantity, ingredients_json, \
              work_amount, unlock_item_id, source_ref) VALUES ({}, {}, {}, {}, {}, {}, {}, {});",
            sql_text(&options.dataset_version),
            sql_text(&recipe.recipe_id),
            sql_text(&recipe.output_item_id),
            recipe.output_quantity,
            sql_text(&ingredients_json),
            recipe.work_amount,
            sql_optional_text(unlock_item_id.as_deref()),
            sql_text(&format!(
                "game-files:steam:{}:DT_ItemRecipe",
                options.game_build_id
            )),
        ));
    }

    let mut acquisition_ids = BTreeSet::new();
    let mut acquisition_rows = Vec::with_capacity(catalog.pal_drops.len());
    for drop in &catalog.pal_drops {
        validate_id("method_id", &drop.method_id)?;
        validate_id("pal_id", &drop.pal_id)?;
        if !acquisition_ids.insert(drop.method_id.clone()) {
            return Err(format!("duplicate acquisition method {}", drop.method_id));
        }
        if !item_ids.contains(&drop.item_id)
            || drop.minimum_quantity < 0
            || drop.maximum_quantity < drop.minimum_quantity
            || !(1..=1_000_000).contains(&drop.probability_ppm)
        {
            return Err(format!("invalid Pal drop {}", drop.method_id));
        }
        let species_resolution = species_index.resolve(&drop.pal_id, None);
        quality_report.record_resolution(
            "item_catalog.pal_drops",
            "species",
            &drop.method_id,
            &drop.pal_id,
            &species_resolution,
        );
        let requirements_json = serde_json::to_string(&serde_json::json!({
            "pal_id": &species_resolution.output_id,
            "source_pal_id": &drop.pal_id,
            "reference_resolution": species_resolution.method(),
            "pal_name_ko": species_names.get(&species_resolution.output_id),
            "level": drop.level,
            "source_row_id": drop.source_row_id,
            "slot": drop.slot,
            "probability_ppm": drop.probability_ppm
        }))
        .map_err(|error| error.to_string())?;
        acquisition_rows.push(format!(
            "INSERT INTO acquisition_methods \
             (dataset_version, method_id, item_id, method_kind, source_entity_id, \
              location_id, quantity_min, quantity_max, probability, cost, \
              requirements_json, evidence_ref) VALUES \
             ({}, {}, {}, 'pal_drop', {}, NULL, {}, {}, {:.6}, NULL, {}, {});",
            sql_text(&options.dataset_version),
            sql_text(&drop.method_id),
            sql_text(&drop.item_id),
            sql_text(&species_resolution.output_id),
            drop.minimum_quantity,
            drop.maximum_quantity,
            drop.probability_ppm as f64 / 1_000_000.0,
            sql_text(&requirements_json),
            sql_text(&format!(
                "game-files:steam:{}:DT_PalDropItem",
                options.game_build_id
            )),
        ));
    }

    Ok(ItemSeedRows {
        items: item_rows,
        recipes: recipe_rows,
        acquisitions: acquisition_rows,
        contract_review_id: catalog.contract_review_id.clone(),
    })
}

fn validate_id(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_:-".contains(character))
    {
        return Err(format!("{label} contains unsupported characters: {value}"));
    }
    Ok(())
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_alias(value: &str) -> String {
    value
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn add_catalog_alias(
    rows: &mut BTreeSet<String>,
    dataset_version: &str,
    entity_kind: &str,
    entity_id: &str,
    alias: &str,
) {
    let normalized = normalize_alias(alias);
    if normalized.is_empty() {
        return;
    }
    rows.insert(format!(
        "INSERT INTO catalog_aliases \
         (dataset_version, entity_kind, entity_id, language, alias, normalized_alias) \
         VALUES ({}, {}, {}, 'ko', {}, {});",
        sql_text(dataset_version),
        sql_text(entity_kind),
        sql_text(entity_id),
        sql_text(alias),
        sql_text(&normalized),
    ));
}

fn sql_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn sql_optional_i64(value: Option<i64>) -> String {
    value.map_or_else(|| "NULL".to_owned(), |value| value.to_string())
}

fn sql_optional_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "NULL".to_owned(), |value| value.to_string())
}

fn sql_optional_text(value: Option<&str>) -> String {
    value
        .filter(|text| !text.trim().is_empty())
        .map_or_else(|| "NULL".to_owned(), sql_text)
}

fn sql_document(rows: &[String]) -> String {
    format!(
        "-- Generated by pal-cloud-seed. Do not edit.\n\
         PRAGMA foreign_keys=ON;\nBEGIN IMMEDIATE;\n{}\nCOMMIT;\n",
        rows.join("\n")
    )
}

fn add_chunked_sql(
    files: &mut BTreeMap<String, Vec<u8>>,
    prefix: &str,
    rows: &[String],
    chunk_size: usize,
) {
    for (index, chunk) in rows.chunks(chunk_size).enumerate() {
        files.insert(
            format!("{prefix}_{index:04}.sql"),
            sql_document(chunk).into_bytes(),
        );
    }
}

fn write_seed(output_dir: &Path, mut generated: GeneratedSeed) -> Result<(), String> {
    fs::create_dir_all(output_dir)
        .map_err(|error| format!("cannot create output directory: {error}"))?;
    for (path, bytes) in &generated.files {
        fs::write(output_dir.join(path), bytes)
            .map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    fs::write(
        output_dir.join(KNOWLEDGE_QUALITY_REPORT_PATH),
        &generated.quality_report,
    )
    .map_err(|error| format!("cannot write {KNOWLEDGE_QUALITY_REPORT_PATH}: {error}"))?;
    generated
        .manifest
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    let manifest = serde_json::to_vec_pretty(&generated.manifest)
        .map_err(|error| format!("cannot encode seed manifest: {error}"))?;
    fs::write(output_dir.join("manifest.json"), manifest)
        .map_err(|error| format!("cannot write manifest.json: {error}"))?;
    println!(
        "generated {} species, {} active skills, {} passives, {} items, {} recipes, \
         {} Pal drops, {} aliases, {} breeding rules, {} graph nodes, {} graph edges, \
         {} Wiki pages and {} knowledge errors in {}",
        generated.manifest.species_count,
        generated.manifest.active_skill_count,
        generated.manifest.passive_count,
        generated.manifest.item_count,
        generated.manifest.recipe_count,
        generated.manifest.pal_drop_count,
        generated.manifest.alias_count,
        generated.manifest.breeding_rule_count,
        generated.manifest.graph_node_count,
        generated.manifest.graph_edge_count,
        generated.manifest.wiki_page_count,
        generated.manifest.knowledge_error_count,
        output_dir.display()
    );
    println!(
        "reference normalization reduced predicted knowledge errors by {} using {} \
         deterministic canonical matches; report: {}",
        generated.manifest.knowledge_error_reduction_count,
        generated.manifest.canonicalized_reference_count,
        output_dir.join(KNOWLEDGE_QUALITY_REPORT_PATH).display()
    );
    println!("dataset remains unverified and inactive");
    Ok(())
}

fn verify_generated_sql(
    files: &BTreeMap<String, Vec<u8>>,
    expected: &SqlCounts,
) -> Result<KnowledgeProjectionCounts, String> {
    let connection = rusqlite::Connection::open_in_memory()
        .map_err(|error| format!("cannot create SQL verification database: {error}"))?;
    apply_current_schema(&connection)
        .map_err(|error| format!("catalog schema verification failed: {error}"))?;
    for (path, bytes) in files {
        let sql = std::str::from_utf8(bytes)
            .map_err(|error| format!("{path} is not valid UTF-8: {error}"))?;
        connection
            .execute_batch(sql)
            .map_err(|error| format!("{path} failed SQLite verification: {error}"))?;
    }
    let counts = connection
        .query_row(
            "SELECT \
               (SELECT COUNT(*) FROM catalog_species), \
               (SELECT COUNT(*) FROM catalog_active_skills), \
               (SELECT COUNT(*) FROM catalog_passives), \
               (SELECT COUNT(*) FROM catalog_items), \
               (SELECT COUNT(*) FROM catalog_recipes), \
               (SELECT COUNT(*) FROM acquisition_methods), \
               (SELECT COUNT(*) FROM catalog_aliases), \
               (SELECT COUNT(*) FROM breeding_rules), \
               (SELECT COUNT(*) FROM knowledge_graph_nodes), \
               (SELECT COUNT(*) FROM knowledge_graph_edges), \
               (SELECT COUNT(*) FROM knowledge_wiki_pages), \
               (SELECT COUNT(*) FROM knowledge_dataset_manifests), \
               (SELECT COUNT(*) FROM data_versions WHERE verified=0 AND activated_at IS NULL)",
            [],
            |row| {
                Ok(ObservedSqlCounts {
                    species: row.get::<_, i64>(0)? as usize,
                    active_skills: row.get::<_, i64>(1)? as usize,
                    passives: row.get::<_, i64>(2)? as usize,
                    items: row.get::<_, i64>(3)? as usize,
                    recipes: row.get::<_, i64>(4)? as usize,
                    acquisitions: row.get::<_, i64>(5)? as usize,
                    aliases: row.get::<_, i64>(6)? as usize,
                    breeding_rules: row.get::<_, i64>(7)? as usize,
                    graph_nodes: row.get::<_, i64>(8)? as usize,
                    graph_edges: row.get::<_, i64>(9)? as usize,
                    wiki_pages: row.get::<_, i64>(10)? as usize,
                    knowledge_manifests: row.get::<_, i64>(11)? as usize,
                    inactive_datasets: row.get::<_, i64>(12)? as usize,
                })
            },
        )
        .map_err(|error| format!("cannot inspect SQL verification database: {error}"))?;
    if counts.species != expected.species
        || counts.active_skills != expected.active_skills
        || counts.passives != expected.passives
        || counts.items != expected.items
        || counts.recipes != expected.recipes
        || counts.acquisitions != expected.acquisitions
        || counts.aliases != expected.aliases
        || counts.breeding_rules != expected.breeding_rules
        || counts.graph_nodes != expected.knowledge.graph_nodes
        || counts.wiki_pages != expected.knowledge.wiki_pages
        || counts.knowledge_manifests != 1
        || counts.inactive_datasets != 1
    {
        return Err(format!(
            "SQL verification count mismatch: expected catalog \
             ({}, {}, {}, {}, {}, {}, {}, {}), graph nodes {}, Wiki pages {}, \
             one manifest and one inactive dataset; got {counts:?}",
            expected.species,
            expected.active_skills,
            expected.passives,
            expected.items,
            expected.recipes,
            expected.acquisitions,
            expected.aliases,
            expected.breeding_rules,
            expected.knowledge.graph_nodes,
            expected.knowledge.wiki_pages,
        ));
    }
    let manifest_state = connection
        .query_row(
            "SELECT graph_node_count, graph_edge_count, wiki_page_count,
                    publication_status,
                    (SELECT COUNT(*) FROM knowledge_error_book errors
                     WHERE errors.dataset_version=manifest.dataset_version
                       AND errors.status IN ('open', 'accepted')),
                    (SELECT COUNT(*) FROM knowledge_error_book errors
                     WHERE errors.dataset_version=manifest.dataset_version
                       AND errors.entry_id LIKE 'error:missing-active-skill:%'),
                    (SELECT COUNT(*) FROM knowledge_error_book errors
                     WHERE errors.dataset_version=manifest.dataset_version
                       AND errors.entry_id LIKE 'error:missing-passive:%'),
                    (SELECT COUNT(*) FROM knowledge_error_book errors
                     WHERE errors.dataset_version=manifest.dataset_version
                       AND errors.entry_id LIKE 'error:missing-drop-species:%'),
                    (SELECT COUNT(*) FROM knowledge_error_book errors
                     WHERE errors.dataset_version=manifest.dataset_version
                       AND errors.entry_id LIKE 'error:missing-unlock-item:%')
             FROM knowledge_dataset_manifests manifest LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, i64>(2)? as usize,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)? as usize,
                    row.get::<_, i64>(5)? as usize,
                    row.get::<_, i64>(6)? as usize,
                    row.get::<_, i64>(7)? as usize,
                    row.get::<_, i64>(8)? as usize,
                ))
            },
        )
        .map_err(|error| format!("cannot inspect knowledge manifest: {error}"))?;
    if manifest_state.0 != counts.graph_nodes
        || manifest_state.1 != counts.graph_edges
        || manifest_state.2 != counts.wiki_pages
        || (manifest_state.4 == 0 && manifest_state.3 != "validated")
        || (manifest_state.4 > 0 && manifest_state.3 != "candidate")
    {
        return Err(format!(
            "knowledge manifest state does not match compiled data: {manifest_state:?}"
        ));
    }
    if manifest_state.4 != manifest_state.5 + manifest_state.6 + manifest_state.7 + manifest_state.8
    {
        return Err(format!(
            "knowledge Error Book breakdown is incomplete: {manifest_state:?}"
        ));
    }
    let foreign_key_errors = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("cannot run foreign-key verification: {error}"))?;
    if foreign_key_errors != 0 {
        return Err(format!(
            "SQL verification found {foreign_key_errors} foreign-key errors"
        ));
    }
    Ok(KnowledgeProjectionCounts {
        graph_nodes: counts.graph_nodes,
        graph_edges: counts.graph_edges,
        wiki_pages: counts.wiki_pages,
        error_count: manifest_state.4,
        missing_active_skill_edges: manifest_state.5,
        missing_passive_edges: manifest_state.6,
        missing_drop_species_edges: manifest_state.7,
        missing_unlock_item_edges: manifest_state.8,
    })
}

fn apply_current_schema(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    for migration in CURRENT_MIGRATIONS {
        connection.execute_batch(migration)?;
    }
    connection.execute_batch(POST_MIGRATION_TRIGGERS)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn catalog_quality_never_promotes_unknown_evidence() {
        assert_eq!(catalog_evidence_quality("exact"), "exact");
        assert_eq!(catalog_evidence_quality("measured"), "measured");
        assert_eq!(catalog_evidence_quality("unknown"), "unknown");
        assert_eq!(catalog_evidence_quality("pinned-private-alpha"), "unknown");
    }

    #[test]
    fn knowledge_source_sql_preserves_artifact_evidence_quality() {
        let sql = knowledge::projection_sql(
            &options(),
            &"a".repeat(64),
            &[SourceArtifact {
                artifact_id: "artifact:species-catalog",
                artifact_kind: "catalog",
                logical_source_ref: "catalog:species".to_owned(),
                sha256: "b".repeat(64),
                byte_size: 42,
                evidence_quality: "unknown",
            }],
        );

        assert!(sql.contains("'unknown'"));
        assert!(!sql.contains("42, 'exact'"));
    }

    fn options() -> Options {
        Options {
            catalog_worker: PathBuf::from("unused"),
            catalog_dir: PathBuf::from("unused"),
            breeding_file: PathBuf::from("unused"),
            active_skills_file: PathBuf::from("unused"),
            active_skills_ko_file: PathBuf::from("unused"),
            passive_skills_file: PathBuf::from("unused"),
            passive_skills_ko_file: PathBuf::from("unused"),
            item_catalog_file: PathBuf::from("unused"),
            output_dir: PathBuf::from("unused"),
            dataset_version: "fixture-1".to_owned(),
            game_version: "unverified".to_owned(),
            game_build_id: "24181527".to_owned(),
            expected_species_count: 2,
            expected_item_count: 1,
            expected_recipe_count: 1,
            expected_pal_drop_count: 1,
        }
    }

    fn fixture_catalog() -> CatalogEnvelope {
        CatalogEnvelope {
            status: "ok".to_owned(),
            quality: "unknown".to_owned(),
            source_id: "fixture".to_owned(),
            matched_count: 2,
            returned_count: 2,
            records: vec![
                SpeciesRecord {
                    species_id: "SkyDragon".to_owned(),
                    name_ko: Some("페스키".to_owned()),
                    paldex_number: Some(95),
                    rarity: Some(8),
                    elements: vec![LocalizedValue {
                        id: "Dragon".to_owned(),
                        name_ko: "용".to_owned(),
                    }],
                    hp: Some(105),
                    attack: Some(100),
                    defense: Some(100),
                    run_speed: Some(900),
                    ride_sprint_speed: Some(1400),
                    transport_speed: Some(450),
                    stamina: Some(220),
                    food_amount: Some(7),
                    nocturnal: false,
                    work_suitability: vec![WorkSuitability {
                        id: "Transport".to_owned(),
                        level: 3,
                    }],
                    learned_skills: vec![LearnedSkill {
                        skill_id: "DragonWave".to_owned(),
                        name_ko: "용의 파동".to_owned(),
                        learn_level: 1,
                        element_ko: Some("용".to_owned()),
                        power: Some(55),
                        cool_time: Some(10.0),
                    }],
                    guaranteed_passives: vec![],
                },
                SpeciesRecord {
                    species_id: "Anubis".to_owned(),
                    name_ko: Some("아누비스".to_owned()),
                    paldex_number: Some(100),
                    rarity: Some(10),
                    elements: vec![LocalizedValue {
                        id: "Earth".to_owned(),
                        name_ko: "땅".to_owned(),
                    }],
                    hp: Some(120),
                    attack: Some(130),
                    defense: Some(100),
                    run_speed: Some(800),
                    ride_sprint_speed: None,
                    transport_speed: Some(480),
                    stamina: Some(100),
                    food_amount: Some(6),
                    nocturnal: false,
                    work_suitability: vec![WorkSuitability {
                        id: "Mining".to_owned(),
                        level: 3,
                    }],
                    learned_skills: vec![],
                    guaranteed_passives: vec![],
                },
            ],
            warnings: vec!["BUILD_MATCH_NOT_PROVEN".to_owned()],
        }
    }

    fn fixture_active_skills() -> (ActiveSkills, LocalizedTexts) {
        (
            BTreeMap::from([(
                "EPalWazaID::DragonWave".to_owned(),
                ActiveSkillRecord {
                    element: "Dragon".to_owned(),
                    skill_kind: "Shot".to_owned(),
                    power: 80,
                    min_range: 0,
                    max_range: 200,
                    cool_time: 4.0,
                    effects: vec![serde_json::json!({
                        "type": "Burn",
                        "value": 50
                    })],
                },
            )]),
            BTreeMap::from([(
                "EPalWazaID::DragonWave".to_owned(),
                LocalizedText {
                    localized_name: "용의 파동".to_owned(),
                    description: Some("용의 힘을 방출한다.".to_owned()),
                },
            )]),
        )
    }

    fn fixture_passives() -> (PassiveSkills, LocalizedTexts) {
        (
            BTreeMap::from([(
                "Swift".to_owned(),
                serde_json::json!({
                    "rank": 3,
                    "effects": [{"type": "MoveSpeed", "value": 30}]
                }),
            )]),
            BTreeMap::from([(
                "Swift".to_owned(),
                LocalizedText {
                    localized_name: "신속".to_owned(),
                    description: Some("이동 속도 +30%".to_owned()),
                },
            )]),
        )
    }

    fn fixture_items() -> ItemCatalogRoot {
        ItemCatalogRoot {
            schema_version: 1,
            game_build_id: "24181527".to_owned(),
            mapping_sha256: EXPECTED_ITEM_MAPPING_SHA256.to_owned(),
            contract_review_id: "review:item-catalog:fixture".to_owned(),
            verified: true,
            items: vec![ItemSourceRecord {
                item_id: "PalOil".to_owned(),
                name_ko: "고급 팰 기름".to_owned(),
                description_ko: Some("고품질 기름.".to_owned()),
                type_a: "Material".to_owned(),
                type_b: "Material".to_owned(),
                price: 300,
                weight_milli: 200,
                maximum_stack_count: 9999,
                rarity: 1,
                rank: 1,
                icon_name: "PalOil".to_owned(),
                legal_in_game: true,
                localization_fallback: false,
            }],
            recipes: vec![ItemRecipeSource {
                recipe_id: "PalOil".to_owned(),
                output_item_id: "PalOil".to_owned(),
                output_quantity: 1,
                ingredients: vec![ItemRecipeIngredientSource {
                    item_id: "PalOil".to_owned(),
                    quantity: 1,
                }],
                work_amount: 100,
                unlock_item_id: None,
            }],
            pal_drops: vec![PalDropSource {
                method_id: "pal-drop:SkyDragon:1".to_owned(),
                source_row_id: "SkyDragon".to_owned(),
                slot: 1,
                pal_id: "SkyDragon".to_owned(),
                level: 20,
                item_id: "PalOil".to_owned(),
                minimum_quantity: 1,
                maximum_quantity: 2,
                probability_ppm: 500_000,
            }],
        }
    }

    #[test]
    fn generated_seed_applies_and_stays_inactive() {
        let breeding = BreedingRoot {
            breeding: vec![BreedingRule {
                parent1_internal_name: "Anubis".to_owned(),
                parent1_gender: "WILDCARD".to_owned(),
                parent2_internal_name: "SkyDragon".to_owned(),
                parent2_gender: "WILDCARD".to_owned(),
                child_internal_name: "SkyDragon".to_owned(),
            }],
        };
        let (active_skills, active_skills_ko) = fixture_active_skills();
        let (passive_skills, passive_skills_ko) = fixture_passives();
        let generated = generate_seed(
            &options(),
            SeedInputs {
                catalog: fixture_catalog(),
                breeding,
                skills: SkillSources {
                    active: active_skills,
                    active_ko: active_skills_ko,
                    passive: passive_skills,
                    passive_ko: passive_skills_ko,
                },
                item_catalog: fixture_items(),
                item_catalog_hash: EXPECTED_ITEM_CATALOG_SHA256.to_owned(),
                source_hash: "f".repeat(64),
                source_artifacts: Vec::new(),
            },
        )
        .unwrap();
        assert!(!generated.manifest.verified);
        assert!(!generated.manifest.activated);
        assert!(generated.manifest.sqlite_verified);
        assert_eq!(generated.manifest.species_count, 2);
        assert_eq!(generated.manifest.active_skill_count, 1);
        assert_eq!(generated.manifest.passive_count, 1);
        assert_eq!(generated.manifest.item_count, 1);
        assert_eq!(generated.manifest.recipe_count, 1);
        assert_eq!(generated.manifest.pal_drop_count, 1);
        assert_eq!(generated.manifest.breeding_rule_count, 1);
        assert_eq!(generated.manifest.graph_node_count, 13);
        assert_eq!(generated.manifest.wiki_page_count, 6);
        assert_eq!(generated.manifest.knowledge_error_count, 0);
        assert_eq!(generated.manifest.canonicalized_reference_count, 0);
        assert_eq!(generated.manifest.duplicate_reference_count, 0);
        let quality_report: serde_json::Value =
            serde_json::from_slice(&generated.quality_report).unwrap();
        assert_eq!(quality_report["summary"]["input_reference_count"], 2);
        assert_eq!(
            quality_report["summary"]["verified_error_book_count_after_normalization"],
            0
        );
        assert_eq!(
            generated.manifest.knowledge_quality_report.sha256,
            sha256_hex(&generated.quality_report)
        );

        let connection = Connection::open_in_memory().unwrap();
        apply_current_schema(&connection).unwrap();
        for bytes in generated.files.values() {
            connection
                .execute_batch(std::str::from_utf8(bytes).unwrap())
                .unwrap();
        }
        let row = connection
            .query_row(
                "SELECT COUNT(*), MAX(paldex_number), MAX(activated_at IS NOT NULL) \
                 FROM catalog_species JOIN data_versions USING (dataset_version)",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row, (2, 100, 0));
        let elements: String = connection
            .query_row(
                "SELECT element_json FROM catalog_species WHERE species_id='SkyDragon'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(elements, "[\"dragon\"]");
        let knowledge = connection
            .query_row(
                "SELECT graph_node_count, graph_edge_count, wiki_page_count
                 FROM knowledge_dataset_manifests WHERE dataset_version='fixture-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            knowledge,
            (
                generated.manifest.graph_node_count as i64,
                generated.manifest.graph_edge_count as i64,
                generated.manifest.wiki_page_count as i64,
            )
        );
    }

    #[test]
    fn sql_text_escapes_quotes() {
        assert_eq!(sql_text("King's Pal"), "'King''s Pal'");
    }

    #[test]
    fn duplicate_species_are_rejected() {
        let mut catalog = fixture_catalog();
        catalog.records[1].species_id = "SkyDragon".to_owned();
        let (active_skills, active_skills_ko) = fixture_active_skills();
        let (passive_skills, passive_skills_ko) = fixture_passives();
        let error = generate_seed(
            &options(),
            SeedInputs {
                catalog,
                breeding: BreedingRoot { breeding: vec![] },
                skills: SkillSources {
                    active: active_skills,
                    active_ko: active_skills_ko,
                    passive: passive_skills,
                    passive_ko: passive_skills_ko,
                },
                item_catalog: fixture_items(),
                item_catalog_hash: EXPECTED_ITEM_CATALOG_SHA256.to_owned(),
                source_hash: "f".repeat(64),
                source_artifacts: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(error.contains("duplicate species"));
    }

    #[test]
    fn deterministic_reference_normalization_and_deduplication_are_reported() {
        let mut catalog = fixture_catalog();
        catalog.records[0].learned_skills[0].skill_id = "dragonwave".to_owned();
        let duplicate_skill = catalog.records[0].learned_skills[0].clone();
        catalog.records[0].learned_skills.push(duplicate_skill);
        catalog.records[0].guaranteed_passives = vec![
            LocalizedValue {
                id: "Swift".to_owned(),
                name_ko: "신속".to_owned(),
            },
            LocalizedValue {
                id: "Swift".to_owned(),
                name_ko: "신속".to_owned(),
            },
        ];
        let mut items = fixture_items();
        items.pal_drops[0].pal_id = "skydragon".to_owned();
        let (active_skills, active_skills_ko) = fixture_active_skills();
        let (passive_skills, passive_skills_ko) = fixture_passives();
        let generated = generate_seed(
            &options(),
            SeedInputs {
                catalog,
                breeding: BreedingRoot { breeding: vec![] },
                skills: SkillSources {
                    active: active_skills,
                    active_ko: active_skills_ko,
                    passive: passive_skills,
                    passive_ko: passive_skills_ko,
                },
                item_catalog: items,
                item_catalog_hash: EXPECTED_ITEM_CATALOG_SHA256.to_owned(),
                source_hash: "f".repeat(64),
                source_artifacts: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(generated.manifest.knowledge_error_count, 0);
        assert_eq!(generated.manifest.canonicalized_reference_count, 2);
        assert_eq!(generated.manifest.duplicate_reference_count, 2);
        assert_eq!(generated.manifest.knowledge_error_reduction_count, 2);
        let report: serde_json::Value = serde_json::from_slice(&generated.quality_report).unwrap();
        assert_eq!(report["summary"]["input_reference_count"], 5);
        assert_eq!(
            report["summary"]["distinct_reference_count_after_deduplication"],
            3
        );
        assert_eq!(
            report["summary"]["predicted_error_book_count_before_normalization"],
            2
        );
        assert_eq!(
            report["summary"]["verified_error_book_count_after_normalization"],
            0
        );

        let acquisition_sql = generated
            .files
            .iter()
            .find(|(path, _)| path.starts_with("0260_acquisitions"))
            .map(|(_, bytes)| std::str::from_utf8(bytes).unwrap())
            .unwrap();
        assert!(acquisition_sql.contains("'SkyDragon'"));
        assert!(acquisition_sql.contains("\"source_pal_id\":\"skydragon\""));
        assert!(acquisition_sql.contains("\"reference_resolution\":\"unique_ascii_case_fold\""));
    }

    #[test]
    fn unresolved_variant_is_reported_without_inventing_a_species_relation() {
        let mut items = fixture_items();
        items.pal_drops[0].pal_id = "BOSS_SkyDragon".to_owned();
        let (active_skills, active_skills_ko) = fixture_active_skills();
        let (passive_skills, passive_skills_ko) = fixture_passives();
        let generated = generate_seed(
            &options(),
            SeedInputs {
                catalog: fixture_catalog(),
                breeding: BreedingRoot { breeding: vec![] },
                skills: SkillSources {
                    active: active_skills,
                    active_ko: active_skills_ko,
                    passive: passive_skills,
                    passive_ko: passive_skills_ko,
                },
                item_catalog: items,
                item_catalog_hash: EXPECTED_ITEM_CATALOG_SHA256.to_owned(),
                source_hash: "f".repeat(64),
                source_artifacts: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(generated.manifest.knowledge_error_count, 1);
        assert_eq!(generated.manifest.missing_drop_species_edge_count, 1);
        assert_eq!(generated.manifest.canonicalized_reference_count, 0);
        let report: serde_json::Value = serde_json::from_slice(&generated.quality_report).unwrap();
        assert_eq!(
            report["unresolved_by_cause"]["boss_variant_absent_from_species_catalog"],
            1
        );
        assert_eq!(report["unresolved_by_severity"]["warning"], 1);
        let acquisition_sql = generated
            .files
            .iter()
            .find(|(path, _)| path.starts_with("0260_acquisitions"))
            .map(|(_, bytes)| std::str::from_utf8(bytes).unwrap())
            .unwrap();
        assert!(acquisition_sql.contains("'BOSS_SkyDragon'"));
        assert!(!acquisition_sql.contains("'SkyDragon', NULL"));
    }
}
