use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use pal_wire::v2::{
    AcquisitionKind, AcquisitionMethodV1, AcquisitionTargetV1, AnalyzeBaseRequest,
    AnalyzeProgressionRequest, BaseFacility, BaseSnapshot, BaseStorageItem, BreedingRuleV1,
    CapturedCageMethodV1, CatalogBuildingRecord, CatalogSpeciesRecord, CatalogTechnologyRecord,
    CharacterProfile, CloudBaseEntryV1, CloudBaseFacilityV1, CloudBaseProjectionContentV1,
    CloudBaseStorageItemV1, CloudCharacterEntryV1, CloudCharacterProjectionContentV1,
    CloudEquipmentEntryV1, CloudEquipmentProjectionContentV1, CloudInventoryEntryV1,
    CloudInventoryProjectionContentV1, CloudPalEntryV1, CloudPalProjectionContentV1,
    CloudProfileProjectionCaptureV1, CloudProfileProjectionContentV1,
    CloudProgressionProjectionContentV1, CloudProgressionUnlockKind, CloudProgressionUnlockV1,
    CompanionRequest, CompanionRequestSetV1, CompareFarmingMethodsRequest,
    CompareOwnedMountsRequest, ContainerCount, DatasetCapabilityStatus, DatasetCapabilityV1,
    DistributionScope, EquipmentSlot, EquipmentSlotKind, EvaluateOwnedPalRequest,
    FindOwnedBreedingPlanRequest, GetOwnedPalRequest, InventoryContainerKind, InventorySlot,
    KindCount, ListOwnedPalsRequest, NormalizedSaveSet, OwnedPal, PalContainerKind,
    PalDropMethodV1, PalGender, PalLocation, PersonalSnapshotContent, PrivateCatalogFactV1,
    PrivateCloudAcquisitionGraphV1, PrivateCloudBreedingGraphV1, PrivateCloudCatalogManifestV1,
    PrivateCloudCatalogPageV1, PrivateCloudFileDescriptorV1, PrivateCloudProgressionGraphV1,
    PrivateProgressionEdgeV1, PrivateSourceProvenanceV1, ProgressionSnapshot,
    ProjectionCompleteness, ProjectionSectionKind, ProjectionSectionStateV1, ReadContext,
    SaveCensus, SaveSourceRole, SearchCatalogRequest, SnapshotCompleteness, SnapshotSectionKind,
    SnapshotSectionState, acquisition_method_v1, acquisition_target_v1, companion_request,
    private_catalog_fact_v1,
};
use pal_wire::{
    canonical_cloud_profile_projection_content, canonical_cloud_profile_projection_identity,
    canonical_snapshot_content, cloud_profile_projection_id, paginate_cloud_profile,
    personal_snapshot_id,
};
use prost::Message;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
enum Mode {
    Check,
    Write,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mode = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [value] if value == "-Check" => Mode::Check,
        [value] if value == "-Write" => Mode::Write,
        _ => return Err("usage: write-wire-goldens -Check | -Write".into()),
    };

    let workspace_root = workspace_root()?;
    let fixture_root = workspace_root.join("tests/fixtures/contracts/wire-v1");
    let local_fixture_root = workspace_root.join("tests/fixtures/local-data");
    let fixtures = build_fixtures()?;
    let local_fixtures = build_local_fixtures(&fixtures)?;
    match mode {
        Mode::Check => {
            check_fixtures(&fixture_root, &fixtures)?;
            check_fixtures(&local_fixture_root, &local_fixtures)?;
        }
        Mode::Write => {
            write_fixtures(&fixture_root, &fixtures)?;
            write_fixtures(&local_fixture_root, &local_fixtures)?;
        }
    }
    Ok(())
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR was not provided by Cargo")?,
    );
    Ok(manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("pal-wire must live under <workspace>/crates")?
        .to_path_buf())
}

fn build_fixtures() -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let snapshot = snapshot_fixture();
    let snapshot_bytes = canonical_snapshot_content(&snapshot)?;
    let snapshot_id = personal_snapshot_id(&snapshot)?;
    let request = request_fixture(snapshot_id.as_bytes());
    let all_requests = all_tool_requests_fixture();
    let normalized_save = normalized_save_fixture(&snapshot);
    let census = save_census_fixture();
    let private_catalog_page = private_catalog_page_fixture();
    let cloud = cloud_fixture();
    let cloud_content_bytes = canonical_cloud_profile_projection_content(&cloud)?;
    let cloud_identity_bytes = canonical_cloud_profile_projection_identity(&cloud)?;
    let projection_id = cloud_profile_projection_id(&cloud)?;
    let paginated = paginate_cloud_profile(&cloud)?;

    let capture_a = capture_fixture(
        projection_id.as_bytes(),
        snapshot_id.as_bytes(),
        1,
        10,
        1_000,
        None,
        0,
    );
    let capture_b = capture_fixture(
        projection_id.as_bytes(),
        &[0xb2; 32],
        2,
        20,
        2_000,
        Some(vec![0xc3; 32]),
        7,
    );

    let mut files = BTreeMap::new();
    files.insert(
        "source.json".to_owned(),
        concat!(
            "{\n",
            "  \"schema_major\": 1,\n",
            "  \"fixture_kind\": \"synthetic-palcompanion-wire-v1\",\n",
            "  \"contains_private_data\": false\n",
            "}\n"
        )
        .as_bytes()
        .to_vec(),
    );
    files.insert("snapshot-content.pb".to_owned(), snapshot_bytes);
    files.insert(
        "snapshot-content.sha256".to_owned(),
        snapshot_id.as_bytes().to_vec(),
    );
    files.insert("companion-request.pb".to_owned(), request.encode_to_vec());
    files.insert(
        "all-tool-requests.pb".to_owned(),
        all_requests.encode_to_vec(),
    );
    files.insert(
        "normalized-save.pb".to_owned(),
        normalized_save.encode_to_vec(),
    );
    files.insert("save-census.pb".to_owned(), census.encode_to_vec());
    files.insert(
        "private-cloud-catalog-page.pb".to_owned(),
        private_catalog_page.encode_to_vec(),
    );
    files.insert(
        "cloud-profile-projection-content.pb".to_owned(),
        cloud_content_bytes,
    );
    files.insert(
        "cloud-profile-projection-identity.pb".to_owned(),
        cloud_identity_bytes,
    );
    files.insert(
        "cloud-profile-projection-id.sha256".to_owned(),
        projection_id.as_bytes().to_vec(),
    );
    files.insert(
        "cloud-profile-projection-capture-a.pb".to_owned(),
        capture_a.encode_to_vec(),
    );
    files.insert(
        "cloud-profile-projection-capture-b.pb".to_owned(),
        capture_b.encode_to_vec(),
    );
    for (section, name) in [
        (ProjectionSectionKind::Character, "character"),
        (ProjectionSectionKind::Inventory, "inventory"),
        (ProjectionSectionKind::Equipment, "equipment"),
        (ProjectionSectionKind::Pals, "pals"),
        (ProjectionSectionKind::Bases, "bases"),
        (ProjectionSectionKind::Progression, "progression"),
    ] {
        let page = paginated
            .pages
            .iter()
            .find(|page| page.section == section as i32 && page.page_index == 0)
            .ok_or("synthetic fixture must produce one page for every section")?;
        files.insert(
            format!("cloud-profile-page-{name}.pb"),
            page.encode_to_vec(),
        );
    }
    files.insert(
        "cloud-profile-projection-descriptor.pb".to_owned(),
        paginated.descriptor.encode_to_vec(),
    );
    files.insert("manifest.json".to_owned(), manifest_bytes(&files));
    Ok(files)
}

fn build_local_fixtures(
    wire: &BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let required = |name: &str| -> Result<Vec<u8>, Box<dyn Error>> {
        wire.get(name)
            .cloned()
            .ok_or_else(|| format!("wire fixture is missing: {name}").into())
    };
    let mut files = BTreeMap::new();

    for (source, target) in [
        ("normalized-save.pb", "save-synthetic-v1/normalized-save.pb"),
        ("save-census.pb", "save-synthetic-v1/census.pb"),
        (
            "snapshot-content.pb",
            "save-synthetic-v1/snapshot-content.pb",
        ),
        (
            "snapshot-content.sha256",
            "save-synthetic-v1/personal-snapshot-id.sha256",
        ),
        (
            "cloud-profile-projection-content.pb",
            "save-synthetic-v1/cloud-profile-projection-content.pb",
        ),
        (
            "cloud-profile-projection-identity.pb",
            "save-synthetic-v1/cloud-profile-projection-identity.pb",
        ),
        (
            "cloud-profile-projection-id.sha256",
            "save-synthetic-v1/cloud-profile-projection-id.sha256",
        ),
        (
            "cloud-profile-projection-capture-a.pb",
            "save-synthetic-v1/cloud-profile-projection-capture.pb",
        ),
        (
            "cloud-profile-projection-descriptor.pb",
            "save-synthetic-v1/cloud-profile-projection-descriptor.pb",
        ),
    ] {
        files.insert(target.to_owned(), required(source)?);
    }
    for name in [
        "character",
        "inventory",
        "equipment",
        "pals",
        "bases",
        "progression",
    ] {
        files.insert(
            format!("save-synthetic-v1/pages/{name}.pb"),
            required(&format!("cloud-profile-page-{name}.pb"))?,
        );
    }
    files.insert(
        "save-synthetic-v1/cloud-profile-redaction-oracle.json".to_owned(),
        concat!(
            "{\n",
            "  \"schema_major\": 1,\n",
            "  \"included\": [\n",
            "    \"character.numeric_allocations\",\n",
            "    \"inventory.item_id_quantity\",\n",
            "    \"equipment.item_id_numeric_state\",\n",
            "    \"pals.stable_ids_skills_passives_upgrades_flags_location\",\n",
            "    \"bases.aggregate_facilities_assigned_pals_storage\",\n",
            "    \"progression.stable_typed_unlock_ids\"\n",
            "  ],\n",
            "  \"excluded\": [\n",
            "    \"owner_subject_id\",\n",
            "    \"world_subject_id\",\n",
            "    \"player_subject_id\",\n",
            "    \"nicknames\",\n",
            "    \"raw_account_ids\",\n",
            "    \"save_paths\",\n",
            "    \"raw_save_bytes\",\n",
            "    \"private_notes\"\n",
            "  ]\n",
            "}\n"
        )
        .as_bytes()
        .to_vec(),
    );

    let candidate = private_candidate_fixtures()?;
    for (name, bytes) in candidate {
        files.insert(format!("private-cloud-catalog-v1/{name}"), bytes);
    }
    Ok(files)
}

fn private_candidate_fixtures() -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let projection_id = vec![0xd1; 32];
    let page = private_catalog_page_fixture();
    let breeding = PrivateCloudBreedingGraphV1 {
        projection_id: projection_id.clone(),
        rules: vec![BreedingRuleV1 {
            rule_id: "breeding:fixture".to_owned(),
            parent_a_species_id: "FixturePal".to_owned(),
            parent_b_species_id: "FixtureParent".to_owned(),
            child_species_id: "FixtureChild".to_owned(),
            source_id: "fixture:source".to_owned(),
        }],
    };
    let acquisition = PrivateCloudAcquisitionGraphV1 {
        projection_id: projection_id.clone(),
        methods: vec![
            AcquisitionMethodV1 {
                schema_major: 1,
                game_build_id: "steam:24181527".to_owned(),
                source_id: "fixture:source".to_owned(),
                entity_version: "steam:24181527".to_owned(),
                method_id: "acq:fixture:drop".to_owned(),
                kind: AcquisitionKind::PalDrop as i32,
                target: Some(AcquisitionTargetV1 {
                    target: Some(acquisition_target_v1::Target::ItemId(
                        "FixtureItem".to_owned(),
                    )),
                }),
                location_id: Some("loc:fixture".to_owned()),
                method: Some(acquisition_method_v1::Method::PalDrop(PalDropMethodV1 {
                    pal_id: "FixturePal".to_owned(),
                    minimum_quantity: 1,
                    maximum_quantity: 2,
                    probability_ppm: 500_000,
                })),
            },
            AcquisitionMethodV1 {
                schema_major: 1,
                game_build_id: "steam:24181527".to_owned(),
                source_id: "fixture:source".to_owned(),
                entity_version: "steam:24181527".to_owned(),
                method_id: "acq:fixture:cage".to_owned(),
                kind: AcquisitionKind::CapturedCage as i32,
                target: Some(AcquisitionTargetV1 {
                    target: Some(acquisition_target_v1::Target::SpeciesId(
                        "FixturePal".to_owned(),
                    )),
                }),
                location_id: Some("loc:fixture".to_owned()),
                method: Some(acquisition_method_v1::Method::CapturedCage(
                    CapturedCageMethodV1 {
                        method_id: "acq:fixture:cage".to_owned(),
                        cage_id: "cage:fixture".to_owned(),
                        location_id: "loc:fixture".to_owned(),
                        species_id: "FixturePal".to_owned(),
                        minimum_level: 10,
                        maximum_level: 20,
                        source_weight: 25,
                        probability_ppm: Some(250_000),
                        probability_provenance: "normalized_fixture_weight".to_owned(),
                    },
                )),
            },
        ],
    };
    let progression = PrivateCloudProgressionGraphV1 {
        projection_id: projection_id.clone(),
        technologies: vec![CatalogTechnologyRecord {
            technology_id: "TechFixture".to_owned(),
            level: 10,
            prerequisite_ids: vec![],
        }],
        buildings: vec![CatalogBuildingRecord {
            building_id: "BuildingFixture".to_owned(),
            technology_id: Some("TechFixture".to_owned()),
        }],
        edges: vec![PrivateProgressionEdgeV1 {
            prerequisite_id: "TechFixture".to_owned(),
            unlocked_id: "BuildingFixture".to_owned(),
        }],
    };

    let mut payloads = BTreeMap::from([
        ("pages/catalog-000000.pb".to_owned(), page.encode_to_vec()),
        ("graphs/breeding.pb".to_owned(), breeding.encode_to_vec()),
        (
            "graphs/acquisition.pb".to_owned(),
            acquisition.encode_to_vec(),
        ),
        (
            "graphs/progression.pb".to_owned(),
            progression.encode_to_vec(),
        ),
    ]);
    let descriptors = payloads
        .iter()
        .enumerate()
        .map(|(ordinal, (path, bytes))| PrivateCloudFileDescriptorV1 {
            relative_path: path.clone(),
            ordinal: u32::try_from(ordinal).expect("fixture ordinal fits"),
            encoded_length: u64::try_from(bytes.len()).expect("fixture length fits"),
            row_count: 1,
            first_stable_key: path.as_bytes().to_vec(),
            last_stable_key: path.as_bytes().to_vec(),
            sha256: Sha256::digest(bytes).to_vec(),
        })
        .collect::<Vec<_>>();
    let capabilities = [
        "pals",
        "skills",
        "items",
        "recipes",
        "acquisition",
        "breeding",
        "technologies",
        "buildings",
        "locations",
        "pois",
        "aliases",
        "localization",
        "sources",
    ]
    .into_iter()
    .map(|capability_id| DatasetCapabilityV1 {
        capability_id: capability_id.to_owned(),
        status: DatasetCapabilityStatus::Complete as i32,
        source_ids: vec!["fixture:source".to_owned()],
        diagnostic_codes: vec![],
    })
    .collect();
    let logical_fact_hash = {
        let mut hash = Sha256::new();
        for bytes in payloads.values() {
            hash.update(bytes);
        }
        hash.finalize().to_vec()
    };
    let manifest = PrivateCloudCatalogManifestV1 {
        schema_major: 1,
        game_build_id: "steam:24181527".to_owned(),
        dataset_manifest_id: vec![0x21; 32],
        projection_id: projection_id.clone(),
        distribution_scope: DistributionScope::PrivateTenantCandidate as i32,
        sources: vec![PrivateSourceProvenanceV1 {
            source_id: "fixture:source".to_owned(),
            source_kind: "synthetic".to_owned(),
            source_hash: vec![0x91; 32],
            verified: true,
        }],
        capabilities,
        files: descriptors,
        enforced_exclusions: vec![
            "game_assets".to_owned(),
            "map_images".to_owned(),
            "full_localized_descriptions".to_owned(),
            "personal_state".to_owned(),
            "absolute_paths".to_owned(),
        ],
        logical_fact_hash,
    };
    payloads.insert("manifest.pb".to_owned(), manifest.encode_to_vec());

    let mut hashes = String::new();
    for (path, bytes) in &payloads {
        hashes.push_str(&format!("{}  {path}\n", hex(&Sha256::digest(bytes))));
    }
    payloads.insert("hashes.sha256".to_owned(), hashes.into_bytes());
    payloads.insert(
        "operator-approval.json".to_owned(),
        format!(
            concat!(
                "{{\n",
                "  \"schema_major\": 1,\n",
                "  \"approval_id\": \"synthetic-fixture-approval\",\n",
                "  \"approved_manifest_id\": \"{}\",\n",
                "  \"approved_capabilities\": [\"pals\", \"skills\", \"items\", \"recipes\", ",
                "\"acquisition\", \"breeding\", \"technologies\", \"buildings\", \"locations\", ",
                "\"pois\", \"aliases\", \"localization\", \"sources\"],\n",
                "  \"reviewer_key_id\": \"fixture-reviewer-key\",\n",
                "  \"decision_time\": \"2000-01-01T00:00:00Z\",\n",
                "  \"statement\": \"Internal synthetic release gate only; not copyright or license authorization.\"\n",
                "}}\n"
            ),
            hex(&Sha256::digest(
                payloads
                    .get("manifest.pb")
                    .expect("manifest was inserted")
            ))
        )
        .into_bytes(),
    );
    Ok(payloads)
}

fn manifest_bytes(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut manifest = String::from("{\n  \"schema_major\": 1,\n  \"sha256\": {\n");
    for (index, (name, bytes)) in files.iter().enumerate() {
        let separator = if index + 1 == files.len() { "" } else { "," };
        manifest.push_str(&format!(
            "    \"{name}\": \"{}\"{separator}\n",
            hex(&Sha256::digest(bytes))
        ));
    }
    manifest.push_str("  }\n}\n");
    manifest.into_bytes()
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

fn check_fixtures(root: &Path, expected: &BTreeMap<String, Vec<u8>>) -> Result<(), Box<dyn Error>> {
    for (name, bytes) in expected {
        let path = root.join(name);
        let actual = fs::read(&path).map_err(|error| {
            format!(
                "fixture is missing or unreadable ({}): {error}",
                path.display()
            )
        })?;
        if actual != *bytes {
            return Err(
                format!("fixture differs from canonical output: {}", path.display()).into(),
            );
        }
    }
    Ok(())
}

fn write_fixtures(root: &Path, fixtures: &BTreeMap<String, Vec<u8>>) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root)?;
    for (name, bytes) in fixtures {
        write_atomic(&root.join(name), bytes)?;
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("fixture")
    ));
    fs::write(&temporary, bytes)?;
    if path.exists() {
        if fs::read(path)? == bytes {
            fs::remove_file(temporary)?;
            return Ok(());
        }
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn complete_snapshot_state(section: SnapshotSectionKind, count: u32) -> SnapshotSectionState {
    SnapshotSectionState {
        section: section as i32,
        completeness: SnapshotCompleteness::Complete as i32,
        expected_record_count: count,
        observed_record_count: count,
    }
}

fn snapshot_fixture() -> PersonalSnapshotContent {
    PersonalSnapshotContent {
        schema_major: 1,
        game_build_id: "steam_2026.07.20".to_owned(),
        dataset_manifest_id: vec![0x21; 32],
        world_id: "fixture_world".to_owned(),
        owner_subject_id: vec![0x31; 32],
        completeness: SnapshotCompleteness::Complete as i32,
        character: Some(character_fixture()),
        inventory: vec![InventorySlot {
            container_kind: InventoryContainerKind::Character as i32,
            container_ordinal: 1,
            slot_index: 3,
            item_id: "PalSphere".to_owned(),
            quantity: 12,
            durability: Some(99),
            rarity: Some(2),
        }],
        equipment: vec![EquipmentSlot {
            slot_kind: EquipmentSlotKind::Weapon as i32,
            slot_index: 2,
            item_id: "Crossbow".to_owned(),
            durability: Some(71),
            rarity: Some(1),
            ammunition: Some(16),
        }],
        pals: vec![OwnedPal {
            instance_id: vec![0x41; 16],
            species_id: "Quivern".to_owned(),
            container_ordinal: 3,
            level: 27,
            iv_hp: Some(91),
            iv_attack: Some(82),
            iv_defense: Some(73),
            passive_ids: vec!["Swift".to_owned(), "Artisan".to_owned()],
            active_skill_ids: vec!["DragonCannon".to_owned(), "AcidRain".to_owned()],
            location: Some(PalLocation {
                container_kind: PalContainerKind::Base as i32,
                container_ordinal: 3,
                slot_index: 5,
                base_id: Some(vec![0x51; 16]),
            }),
            gender: Some("female".to_owned()),
            condensation_rank: 2,
            learned_skill_ids: vec!["AirBlade".to_owned(), "DragonCannon".to_owned()],
            soul_hp: 3,
            soul_attack: 4,
            soul_defense: 5,
            soul_work_speed: 6,
            trust: 77,
            is_lucky: true,
            is_boss: true,
            is_alpha: true,
            is_awakened: true,
            is_mutated: true,
            slot_index: 5,
            base_id: Some(vec![0x51; 16]),
        }],
        bases: vec![BaseSnapshot {
            base_id: vec![0x51; 16],
            location_id: "WindsweptHills".to_owned(),
            level: 15,
            facilities: vec![BaseFacility {
                building_id: "BerryFarm".to_owned(),
                count: 3,
            }],
            assigned_pal_instance_ids: vec![vec![0x41; 16]],
            storage_items: vec![BaseStorageItem {
                item_id: "Wood".to_owned(),
                quantity: 300,
            }],
        }],
        progression: Some(ProgressionSnapshot {
            technology_ids: vec!["Tech_A".to_owned()],
            fast_travel_ids: vec!["FT_1".to_owned()],
            tower_ids: vec![],
            boss_ids: vec![],
            mission_ids: vec![],
            paldex_capture_ids: vec![],
            map_region_ids: vec![],
            collectible_ids: vec![],
            base_research_ids: vec![],
        }),
        section_states: vec![
            complete_snapshot_state(SnapshotSectionKind::Character, 1),
            complete_snapshot_state(SnapshotSectionKind::Inventory, 1),
            complete_snapshot_state(SnapshotSectionKind::Equipment, 1),
            complete_snapshot_state(SnapshotSectionKind::Pals, 1),
            complete_snapshot_state(SnapshotSectionKind::Bases, 4),
            complete_snapshot_state(SnapshotSectionKind::Progression, 2),
        ],
    }
}

fn character_fixture() -> CharacterProfile {
    CharacterProfile {
        level: 42,
        experience: 123_456,
        allocated_hp: 8,
        allocated_stamina: 5,
        allocated_attack: 4,
        allocated_defense: 3,
        allocated_work_speed: 2,
        allocated_carry_weight: 7,
        technology_points: 9,
        ancient_technology_points: 1,
        world_subject_id: Some(vec![0x81; 32]),
        player_subject_id: Some(vec![0x82; 32]),
    }
}

fn request_fixture(snapshot_id: &[u8; 32]) -> CompanionRequest {
    CompanionRequest {
        trace_id: vec![0x11; 16],
        query: Some(companion_request::Query::ListOwnedPals(
            ListOwnedPalsRequest {
                context: Some(ReadContext {
                    game_build_id: "steam_2026.07.20".to_owned(),
                    dataset_manifest_id: vec![0x21; 32],
                    personal_snapshot_id: snapshot_id.to_vec(),
                    solver_version: "fixture_solver_v1".to_owned(),
                    deadline_ms: 2_000,
                    node_budget: 10_000,
                }),
                species_id: Some("Quivern".to_owned()),
                cursor: None,
                page_size: 25,
            },
        )),
    }
}

fn all_tool_requests_fixture() -> CompanionRequestSetV1 {
    let queries = [
        companion_request::Query::ListOwnedPals(ListOwnedPalsRequest::default()),
        companion_request::Query::GetOwnedPal(GetOwnedPalRequest::default()),
        companion_request::Query::FindOwnedBreedingPlan(FindOwnedBreedingPlanRequest::default()),
        companion_request::Query::SearchCatalog(SearchCatalogRequest::default()),
        companion_request::Query::CompareFarmingMethods(CompareFarmingMethodsRequest::default()),
        companion_request::Query::AnalyzeBase(AnalyzeBaseRequest::default()),
        companion_request::Query::AnalyzeProgression(AnalyzeProgressionRequest::default()),
        companion_request::Query::CompareOwnedMounts(CompareOwnedMountsRequest::default()),
        companion_request::Query::EvaluateOwnedPal(EvaluateOwnedPalRequest::default()),
    ];
    CompanionRequestSetV1 {
        requests: queries
            .into_iter()
            .enumerate()
            .map(|(index, query)| CompanionRequest {
                trace_id: vec![u8::try_from(index + 1).expect("fixture index fits"); 16],
                query: Some(query),
            })
            .collect(),
    }
}

fn normalized_save_fixture(snapshot: &PersonalSnapshotContent) -> NormalizedSaveSet {
    NormalizedSaveSet {
        schema_major: snapshot.schema_major,
        game_build_id: snapshot.game_build_id.clone(),
        source_install_id: vec![0x71; 16],
        world_id: snapshot.world_id.clone(),
        owner_subject_id: snapshot.owner_subject_id.clone(),
        character: snapshot.character.clone(),
        inventory: snapshot.inventory.clone(),
        equipment: snapshot.equipment.clone(),
        pals: snapshot.pals.clone(),
        bases: snapshot.bases.clone(),
        progression: snapshot.progression.clone(),
        required_roles_seen: vec![SaveSourceRole::Level as i32, SaveSourceRole::Player as i32],
    }
}

fn save_census_fixture() -> SaveCensus {
    SaveCensus {
        schema_major: 1,
        character_count: 1,
        inventory_counts: vec![ContainerCount {
            container_kind: InventoryContainerKind::Character as i32,
            container_ordinal: 1,
            record_count: 1,
        }],
        equipment_count: 1,
        owned_pal_count: 1,
        owned_pal_instance_ids: vec![vec![0x41; 16]],
        base_count: 1,
        facility_count: 1,
        progression_counts: vec![
            KindCount {
                kind_id: "technology".to_owned(),
                record_count: 1,
            },
            KindCount {
                kind_id: "fast_travel".to_owned(),
                record_count: 1,
            },
        ],
        required_roles_seen: vec![SaveSourceRole::Level as i32, SaveSourceRole::Player as i32],
        base_assigned_pal_count: 1,
        base_storage_slot_count: 1,
    }
}

fn private_catalog_page_fixture() -> PrivateCloudCatalogPageV1 {
    PrivateCloudCatalogPageV1 {
        projection_id: vec![0xd1; 32],
        page_index: 0,
        facts: vec![PrivateCatalogFactV1 {
            source_id: "fixture:pal".to_owned(),
            entity_version: "steam:24181527".to_owned(),
            fact: Some(private_catalog_fact_v1::Fact::Species(
                CatalogSpeciesRecord {
                    species_id: "FixturePal".to_owned(),
                    breeding_power: Some(123),
                    work_type_ids: vec!["transport".to_owned()],
                    element_ids: vec!["dragon".to_owned()],
                    hp: Some(100),
                    attack: Some(90),
                    defense: Some(80),
                    ride_sprint_speed: Some(1_400),
                    stamina: Some(220),
                },
            )),
        }],
    }
}

fn complete_projection_state(source_count: u32, projected_count: u32) -> ProjectionSectionStateV1 {
    ProjectionSectionStateV1 {
        completeness: ProjectionCompleteness::Complete as i32,
        source_expected_record_count: source_count,
        source_observed_record_count: source_count,
        projected_entry_count: projected_count,
    }
}

fn cloud_fixture() -> CloudProfileProjectionContentV1 {
    CloudProfileProjectionContentV1 {
        schema_version: 1,
        dataset_manifest_id: vec![0x21; 32],
        game_build_id: "steam_2026.07.20".to_owned(),
        character: Some(CloudCharacterProjectionContentV1 {
            state: Some(complete_projection_state(1, 1)),
            character: Some(CloudCharacterEntryV1 {
                level: 42,
                experience: 123_456,
                allocated_hp: 8,
                allocated_stamina: 5,
                allocated_attack: 4,
                allocated_defense: 3,
                allocated_work_speed: 2,
                allocated_carry_weight: 7,
                technology_points: 9,
                ancient_technology_points: 1,
            }),
        }),
        inventory: Some(CloudInventoryProjectionContentV1 {
            state: Some(complete_projection_state(1, 1)),
            entries: vec![CloudInventoryEntryV1 {
                container_kind: InventoryContainerKind::Character as i32,
                container_ordinal: 1,
                slot_index: 3,
                item_id: "PalSphere".to_owned(),
                quantity: 12,
                durability: Some(99),
                rarity: Some(2),
            }],
        }),
        equipment: Some(CloudEquipmentProjectionContentV1 {
            state: Some(complete_projection_state(1, 1)),
            entries: vec![CloudEquipmentEntryV1 {
                slot_kind: EquipmentSlotKind::Weapon as i32,
                slot_index: 2,
                item_id: "Crossbow".to_owned(),
                durability: Some(71),
                rarity: Some(1),
                ammunition: Some(16),
            }],
        }),
        pals: Some(CloudPalProjectionContentV1 {
            state: Some(complete_projection_state(1, 1)),
            entries: vec![CloudPalEntryV1 {
                pal_instance_id: vec![0x41; 16],
                species_id: "Quivern".to_owned(),
                level: 27,
                gender: PalGender::Female as i32,
                iv_hp: Some(91),
                iv_attack: Some(82),
                iv_defense: Some(73),
                passive_ids: vec!["Swift".to_owned(), "Artisan".to_owned()],
                active_skill_ids: vec!["DragonCannon".to_owned(), "AcidRain".to_owned()],
                condensation_rank: 2,
                soul_hp: 3,
                soul_attack: 4,
                soul_defense: 5,
                soul_work_speed: 6,
                container_kind: PalContainerKind::Base as i32,
                learned_skill_ids: vec!["AirBlade".to_owned(), "DragonCannon".to_owned()],
                trust: 77,
                is_lucky: true,
                is_boss: true,
                is_alpha: true,
                is_awakened: true,
                is_mutated: true,
                container_ordinal: 3,
                slot_index: 5,
                base_id: Some(vec![0x51; 16]),
            }],
        }),
        bases: Some(CloudBaseProjectionContentV1 {
            state: Some(complete_projection_state(4, 1)),
            entries: vec![CloudBaseEntryV1 {
                base_id: vec![0x51; 16],
                location_id: "WindsweptHills".to_owned(),
                level: 15,
                facilities: vec![CloudBaseFacilityV1 {
                    building_id: "BerryFarm".to_owned(),
                    count: 3,
                }],
                assigned_pal_instance_ids: vec![vec![0x41; 16]],
                storage_items: vec![CloudBaseStorageItemV1 {
                    item_id: "Wood".to_owned(),
                    quantity: 300,
                }],
            }],
        }),
        progression: Some(CloudProgressionProjectionContentV1 {
            state: Some(complete_projection_state(2, 2)),
            entries: vec![
                CloudProgressionUnlockV1 {
                    kind: CloudProgressionUnlockKind::Technology as i32,
                    unlock_id: "Tech_A".to_owned(),
                },
                CloudProgressionUnlockV1 {
                    kind: CloudProgressionUnlockKind::FastTravel as i32,
                    unlock_id: "FT_1".to_owned(),
                },
            ],
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_fixture(
    projection_id: &[u8; 32],
    source_snapshot_id: &[u8; 32],
    source_generation: u64,
    source_sequence: u64,
    observed_at_unix_ms: u64,
    base_active_projection_id: Option<Vec<u8>>,
    base_commit_revision: u64,
) -> CloudProfileProjectionCaptureV1 {
    CloudProfileProjectionCaptureV1 {
        world_id: "fixture_world".to_owned(),
        source_install_id: vec![0x71; 16],
        source_generation,
        source_sequence,
        observed_at_unix_ms,
        source_personal_snapshot_id: source_snapshot_id.to_vec(),
        base_active_projection_id,
        base_commit_revision,
        projection_id: projection_id.to_vec(),
    }
}
