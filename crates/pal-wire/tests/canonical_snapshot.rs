use pal_wire::v2::{
    BaseFacility, BaseSnapshot, BaseStorageItem, CharacterProfile, CloudBaseEntryV1,
    CloudBaseFacilityV1, CloudBaseProjectionContentV1, CloudBaseStorageItemV1,
    CloudCharacterEntryV1, CloudCharacterProjectionContentV1, CloudEquipmentEntryV1,
    CloudEquipmentProjectionContentV1, CloudInventoryEntryV1, CloudInventoryProjectionContentV1,
    CloudPalEntryV1, CloudPalProjectionContentV1, CloudProfileProjectionCaptureV1,
    CloudProfileProjectionContentV1, CloudProfileProjectionIdentityV1,
    CloudProfileProjectionPageV1, CloudProgressionProjectionContentV1, CloudProgressionUnlockKind,
    CloudProgressionUnlockV1, EquipmentSlot, EquipmentSlotKind, InventoryContainerKind,
    InventorySlot, OwnedPal, PalContainerKind, PalGender, PalLocation, PersonalSnapshotContent,
    ProgressionSnapshot, ProjectionCompleteness, ProjectionSectionKind, ProjectionSectionStateV1,
    SnapshotCapture, SnapshotCompleteness, SnapshotDiagnostic, SnapshotSectionKind,
    SnapshotSectionState, cloud_profile_projection_page_v1,
};
use pal_wire::{
    CanonicalError, MAX_PROJECTION_PAGE_ENCODED_LEN, MAX_PROJECTION_PAGE_ENTRIES,
    canonical_cloud_profile_projection_identity, canonical_snapshot_content,
    cloud_profile_projection_id, paginate_cloud_profile, personal_snapshot_id,
};
use prost::Message;
use sha2::{Digest, Sha256};

fn complete_state(section: SnapshotSectionKind, record_count: u32) -> SnapshotSectionState {
    SnapshotSectionState {
        section: section as i32,
        completeness: SnapshotCompleteness::Complete as i32,
        expected_record_count: record_count,
        observed_record_count: record_count,
    }
}

fn fixture_snapshot_content() -> PersonalSnapshotContent {
    PersonalSnapshotContent {
        schema_major: 1,
        game_build_id: "steam_2026.07.20".to_owned(),
        dataset_manifest_id: vec![0x21; 32],
        world_id: "fixture_world".to_owned(),
        owner_subject_id: vec![0x31; 32],
        completeness: SnapshotCompleteness::Complete as i32,
        character: Some(CharacterProfile {
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
            world_subject_id: None,
            player_subject_id: None,
        }),
        inventory: vec![
            InventorySlot {
                container_kind: InventoryContainerKind::Character as i32,
                container_ordinal: 2,
                slot_index: 8,
                item_id: "Berry".to_owned(),
                quantity: 20,
                durability: None,
                rarity: None,
            },
            InventorySlot {
                container_kind: InventoryContainerKind::Character as i32,
                container_ordinal: 1,
                slot_index: 3,
                item_id: "PalSphere".to_owned(),
                quantity: 12,
                durability: Some(99),
                rarity: Some(2),
            },
        ],
        equipment: vec![
            EquipmentSlot {
                slot_kind: EquipmentSlotKind::Weapon as i32,
                slot_index: 2,
                item_id: "Crossbow".to_owned(),
                durability: Some(71),
                rarity: Some(1),
                ammunition: Some(16),
            },
            EquipmentSlot {
                slot_kind: EquipmentSlotKind::Helmet as i32,
                slot_index: 1,
                item_id: "ClothHat".to_owned(),
                durability: Some(44),
                rarity: None,
                ammunition: None,
            },
        ],
        pals: vec![OwnedPal {
            instance_id: vec![0x41; 16],
            species_id: "Quivern".to_owned(),
            container_ordinal: 1,
            level: 27,
            iv_hp: Some(91),
            iv_attack: Some(82),
            iv_defense: Some(73),
            passive_ids: vec!["Swift".to_owned(), "Artisan".to_owned(), "Swift".to_owned()],
            active_skill_ids: vec![
                "DragonCannon".to_owned(),
                "AcidRain".to_owned(),
                "AcidRain".to_owned(),
            ],
            location: Some(PalLocation {
                container_kind: PalContainerKind::Base as i32,
                container_ordinal: 3,
                slot_index: 5,
                base_id: Some(vec![0x51; 16]),
            }),
            gender: Some("female".to_owned()),
            condensation_rank: 2,
            ..OwnedPal::default()
        }],
        bases: vec![BaseSnapshot {
            base_id: vec![0x51; 16],
            location_id: "WindsweptHills".to_owned(),
            level: 15,
            facilities: vec![BaseFacility {
                building_id: "BerryFarm".to_owned(),
                count: 2,
            }],
            assigned_pal_instance_ids: vec![vec![0x41; 16]],
            storage_items: vec![BaseStorageItem {
                item_id: "Wood".to_owned(),
                quantity: 300,
            }],
        }],
        progression: Some(ProgressionSnapshot {
            technology_ids: vec![
                "Tech_B".to_owned(),
                "Tech_A".to_owned(),
                "Tech_A".to_owned(),
            ],
            fast_travel_ids: vec!["FT_2".to_owned(), "FT_1".to_owned()],
            tower_ids: vec![],
            boss_ids: vec!["Boss_1".to_owned()],
            mission_ids: vec![],
            paldex_capture_ids: vec!["Quivern".to_owned()],
            map_region_ids: vec![],
            collectible_ids: vec![],
            base_research_ids: vec![],
        }),
        section_states: vec![
            complete_state(SnapshotSectionKind::Progression, 6),
            complete_state(SnapshotSectionKind::Bases, 4),
            complete_state(SnapshotSectionKind::Pals, 1),
            complete_state(SnapshotSectionKind::Equipment, 2),
            complete_state(SnapshotSectionKind::Inventory, 2),
            complete_state(SnapshotSectionKind::Character, 1),
        ],
    }
}

fn complete_projection_state(
    source_records: u32,
    projected_entries: u32,
) -> ProjectionSectionStateV1 {
    ProjectionSectionStateV1 {
        completeness: ProjectionCompleteness::Complete as i32,
        source_expected_record_count: source_records,
        source_observed_record_count: source_records,
        projected_entry_count: projected_entries,
    }
}

fn fixture_cloud_profile_projection_content() -> CloudProfileProjectionContentV1 {
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
            state: Some(complete_projection_state(2, 2)),
            entries: vec![
                CloudInventoryEntryV1 {
                    container_kind: InventoryContainerKind::Character as i32,
                    container_ordinal: 2,
                    slot_index: 8,
                    item_id: "Berry".to_owned(),
                    quantity: 20,
                    durability: None,
                    rarity: None,
                },
                CloudInventoryEntryV1 {
                    container_kind: InventoryContainerKind::Character as i32,
                    container_ordinal: 1,
                    slot_index: 3,
                    item_id: "PalSphere".to_owned(),
                    quantity: 12,
                    durability: Some(99),
                    rarity: Some(2),
                },
            ],
        }),
        equipment: Some(CloudEquipmentProjectionContentV1 {
            state: Some(complete_projection_state(2, 2)),
            entries: vec![
                CloudEquipmentEntryV1 {
                    slot_kind: EquipmentSlotKind::Weapon as i32,
                    slot_index: 2,
                    item_id: "Crossbow".to_owned(),
                    durability: Some(71),
                    rarity: Some(1),
                    ammunition: Some(16),
                },
                CloudEquipmentEntryV1 {
                    slot_kind: EquipmentSlotKind::Helmet as i32,
                    slot_index: 1,
                    item_id: "ClothHat".to_owned(),
                    durability: Some(44),
                    rarity: None,
                    ammunition: None,
                },
            ],
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
                passive_ids: vec!["Swift".to_owned(), "Artisan".to_owned(), "Swift".to_owned()],
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
            state: Some(complete_projection_state(6, 1)),
            entries: vec![CloudBaseEntryV1 {
                base_id: vec![0x51; 16],
                location_id: "WindsweptHills".to_owned(),
                level: 15,
                facilities: vec![
                    CloudBaseFacilityV1 {
                        building_id: "BerryFarm".to_owned(),
                        count: 1,
                    },
                    CloudBaseFacilityV1 {
                        building_id: "BerryFarm".to_owned(),
                        count: 2,
                    },
                ],
                assigned_pal_instance_ids: vec![vec![0x41; 16], vec![0x41; 16]],
                storage_items: vec![
                    CloudBaseStorageItemV1 {
                        item_id: "Wood".to_owned(),
                        quantity: 100,
                    },
                    CloudBaseStorageItemV1 {
                        item_id: "Wood".to_owned(),
                        quantity: 200,
                    },
                ],
            }],
        }),
        progression: Some(CloudProgressionProjectionContentV1 {
            state: Some(complete_projection_state(2, 2)),
            entries: vec![
                CloudProgressionUnlockV1 {
                    kind: CloudProgressionUnlockKind::FastTravel as i32,
                    unlock_id: "FT_1".to_owned(),
                },
                CloudProgressionUnlockV1 {
                    kind: CloudProgressionUnlockKind::Technology as i32,
                    unlock_id: "Tech_A".to_owned(),
                },
            ],
        }),
    }
}

fn page_entry_count(page: &CloudProfileProjectionPageV1) -> usize {
    match page.payload.as_ref().unwrap() {
        cloud_profile_projection_page_v1::Payload::Character(value) => value.entries.len(),
        cloud_profile_projection_page_v1::Payload::Inventory(value) => value.entries.len(),
        cloud_profile_projection_page_v1::Payload::Equipment(value) => value.entries.len(),
        cloud_profile_projection_page_v1::Payload::Pals(value) => value.entries.len(),
        cloud_profile_projection_page_v1::Payload::Bases(value) => value.entries.len(),
        cloud_profile_projection_page_v1::Payload::Progression(value) => value.entries.len(),
    }
}

fn inventory_sort_key(container_kind: i32, ordinal: u32, slot: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(12);
    key.extend_from_slice(&(container_kind as u32).to_be_bytes());
    key.extend_from_slice(&ordinal.to_be_bytes());
    key.extend_from_slice(&slot.to_be_bytes());
    key
}

#[test]
fn snapshot_id_uses_the_exact_domain_separator_and_canonical_bytes() {
    let content = fixture_snapshot_content();
    let canonical = canonical_snapshot_content(&content).unwrap();
    let mut expected = Sha256::new();
    expected.update(b"palcompanion.snapshot.v1\0");
    expected.update(&canonical);

    let expected: [u8; 32] = expected.finalize().into();
    assert_eq!(
        personal_snapshot_id(&content).unwrap().as_bytes(),
        &expected
    );
    assert_eq!(
        PersonalSnapshotContent::decode(canonical.as_slice())
            .unwrap()
            .encode_to_vec(),
        canonical
    );
}

#[test]
fn set_and_semantic_entry_order_are_canonical() {
    let baseline = fixture_snapshot_content();
    let mut reordered = baseline.clone();
    reordered.inventory.reverse();
    reordered.equipment.reverse();
    reordered.pals[0].passive_ids.reverse();
    reordered.pals[0].active_skill_ids.reverse();
    reordered.section_states.reverse();
    let progression = reordered.progression.as_mut().unwrap();
    progression.technology_ids.reverse();
    progression.fast_travel_ids.reverse();

    assert_eq!(
        personal_snapshot_id(&baseline).unwrap().as_bytes(),
        personal_snapshot_id(&reordered).unwrap().as_bytes(),
        "reordering entries without changing their semantic keys must preserve identity"
    );
}

#[test]
fn changing_an_inventory_slot_key_changes_snapshot_identity() {
    let baseline = fixture_snapshot_content();
    let mut changed = baseline.clone();
    changed.inventory[0].slot_index += 1;

    assert_ne!(
        personal_snapshot_id(&baseline).unwrap().as_bytes(),
        personal_snapshot_id(&changed).unwrap().as_bytes()
    );
}

#[test]
fn actual_capture_metadata_changes_do_not_change_content_identity() {
    let content = fixture_snapshot_content();
    let first = SnapshotCapture {
        content: Some(content.clone()),
        personal_snapshot_id: vec![0x61; 32],
        observed_at_unix_ms: 1_000,
        source_sequence: 7,
        parser_revision: "parser_a".to_owned(),
        diagnostics: vec![],
    };
    let mut second = first.clone();
    second.observed_at_unix_ms = 9_000;
    second.source_sequence = 99;
    second.parser_revision = "parser_b".to_owned();
    second.diagnostics.push(SnapshotDiagnostic {
        code: "fixture_warning".to_owned(),
        section: SnapshotSectionKind::Inventory as i32,
    });

    assert_ne!(first.encode_to_vec(), second.encode_to_vec());
    assert_eq!(
        personal_snapshot_id(first.content.as_ref().unwrap())
            .unwrap()
            .as_bytes(),
        personal_snapshot_id(second.content.as_ref().unwrap())
            .unwrap()
            .as_bytes()
    );
}

#[test]
fn duplicate_semantic_slots_are_rejected() {
    let mut content = fixture_snapshot_content();
    let mut duplicate = content.inventory[0].clone();
    duplicate.item_id = "DifferentItem".to_owned();
    content.inventory.push(duplicate);
    assert!(canonical_snapshot_content(&content).is_err());
}

#[test]
fn snapshot_requires_all_six_section_states_and_exact_hash_widths() {
    let mut missing_section = fixture_snapshot_content();
    missing_section.section_states.pop();
    assert!(canonical_snapshot_content(&missing_section).is_err());

    let mut bad_dataset_id = fixture_snapshot_content();
    bad_dataset_id.dataset_manifest_id.pop();
    assert!(canonical_snapshot_content(&bad_dataset_id).is_err());
}

#[test]
fn cloud_identity_is_exactly_six_ordered_section_hashes() {
    let content = fixture_cloud_profile_projection_content();
    let identity_bytes = canonical_cloud_profile_projection_identity(&content).unwrap();
    let identity = CloudProfileProjectionIdentityV1::decode(identity_bytes.as_slice()).unwrap();

    assert_eq!(identity.sections.len(), 6);
    assert_eq!(
        identity
            .sections
            .iter()
            .map(|section| section.section)
            .collect::<Vec<_>>(),
        (ProjectionSectionKind::Character as i32..=ProjectionSectionKind::Progression as i32)
            .collect::<Vec<_>>()
    );
    assert!(
        identity
            .sections
            .iter()
            .all(|section| section.section_content_sha256.len() == 32)
    );
    let expected: [u8; 32] = Sha256::digest(&identity_bytes).into();
    assert_eq!(
        cloud_profile_projection_id(&content).unwrap().as_bytes(),
        &expected
    );
}

#[test]
fn all_six_cloud_sections_and_slot_keys_are_identity_bearing() {
    let baseline = fixture_cloud_profile_projection_content();
    let baseline_id = *cloud_profile_projection_id(&baseline).unwrap().as_bytes();
    let mut changes = Vec::new();

    let mut character = baseline.clone();
    character
        .character
        .as_mut()
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .level += 1;
    changes.push(character);
    let mut inventory = baseline.clone();
    inventory.inventory.as_mut().unwrap().entries[0].slot_index += 1;
    changes.push(inventory);
    let mut equipment = baseline.clone();
    equipment.equipment.as_mut().unwrap().entries[0].slot_index += 1;
    changes.push(equipment);
    let mut pals = baseline.clone();
    pals.pals.as_mut().unwrap().entries[0].trust += 1;
    changes.push(pals);
    let mut bases = baseline.clone();
    bases.bases.as_mut().unwrap().entries[0].level += 1;
    changes.push(bases);
    let mut progression = baseline.clone();
    progression.progression.as_mut().unwrap().entries[0].unlock_id = "FT_2".to_owned();
    changes.push(progression);

    for changed in changes {
        assert_ne!(
            *cloud_profile_projection_id(&changed).unwrap().as_bytes(),
            baseline_id
        );
    }
}

#[test]
fn cloud_vector_reordering_with_unchanged_semantic_keys_preserves_identity() {
    let baseline = fixture_cloud_profile_projection_content();
    let mut reordered = baseline.clone();
    reordered.inventory.as_mut().unwrap().entries.reverse();
    reordered.equipment.as_mut().unwrap().entries.reverse();
    reordered.pals.as_mut().unwrap().entries[0]
        .passive_ids
        .reverse();
    reordered.bases.as_mut().unwrap().entries[0]
        .facilities
        .reverse();
    reordered.bases.as_mut().unwrap().entries[0]
        .storage_items
        .reverse();
    reordered.progression.as_mut().unwrap().entries.reverse();

    assert_eq!(
        cloud_profile_projection_id(&baseline).unwrap().as_bytes(),
        cloud_profile_projection_id(&reordered).unwrap().as_bytes()
    );
}

#[test]
fn capture_page_and_descriptor_metadata_are_projection_identity_excluded() {
    let content = fixture_cloud_profile_projection_content();
    let projection_id = *cloud_profile_projection_id(&content).unwrap().as_bytes();
    let first = CloudProfileProjectionCaptureV1 {
        world_id: "fixture_world".to_owned(),
        source_install_id: vec![0x71; 16],
        source_generation: 1,
        source_sequence: 2,
        observed_at_unix_ms: 3,
        source_personal_snapshot_id: vec![0x81; 32],
        base_active_projection_id: None,
        base_commit_revision: 4,
        projection_id: projection_id.to_vec(),
    };
    let mut second = first.clone();
    second.source_generation = 91;
    second.source_sequence = 92;
    second.observed_at_unix_ms = 93;
    second.source_personal_snapshot_id = vec![0x91; 32];
    second.base_active_projection_id = Some(vec![0xa1; 32]);
    second.base_commit_revision = 94;

    let mut first_pages = paginate_cloud_profile(&content).unwrap();
    let mut second_pages = paginate_cloud_profile(&content).unwrap();
    second_pages.descriptor.total_encoded_length += 1;
    first_pages.pages[0].page_index += 100;

    assert_ne!(first.encode_to_vec(), second.encode_to_vec());
    assert_ne!(
        first_pages.descriptor.encode_to_vec(),
        second_pages.descriptor.encode_to_vec()
    );
    assert_eq!(
        *cloud_profile_projection_id(&content).unwrap().as_bytes(),
        projection_id
    );
}

#[test]
fn cloud_pal_retains_web_visible_state_and_semantic_location() {
    let content = fixture_cloud_profile_projection_content();
    let pal = &content.pals.as_ref().unwrap().entries[0];
    assert!(!pal.learned_skill_ids.is_empty());
    assert!(pal.trust > 0);
    assert!(pal.is_lucky && pal.is_boss && pal.is_alpha && pal.is_awakened && pal.is_mutated);
    assert!(pal.container_ordinal > 0);
    assert!(pal.slot_index > 0);
    assert_eq!(pal.base_id.as_ref().unwrap().len(), 16);

    let schema = include_str!("../../../proto/palcompanion/v2/sync.proto")
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<String>();
    for forbidden in [
        "character_name",
        "nickname",
        "raw_account_id",
        "save_path",
        "private_note",
        "diagnostics",
        "owner_subject_id",
    ] {
        assert!(!schema.contains(forbidden), "forbidden field: {forbidden}");
    }
}

#[test]
fn typed_pages_and_descriptors_are_bounded_hashed_and_complete() {
    let mut content = fixture_cloud_profile_projection_content();
    let inventory = content.inventory.as_mut().unwrap();
    inventory.entries.clear();
    for slot in 0..(MAX_PROJECTION_PAGE_ENTRIES as u32 + 1) {
        inventory.entries.push(CloudInventoryEntryV1 {
            container_kind: InventoryContainerKind::Character as i32,
            container_ordinal: 1,
            slot_index: slot,
            item_id: format!("Item_{slot}"),
            quantity: 1,
            durability: None,
            rarity: None,
        });
    }
    inventory.state = Some(complete_projection_state(
        MAX_PROJECTION_PAGE_ENTRIES as u32 + 1,
        MAX_PROJECTION_PAGE_ENTRIES as u32 + 1,
    ));

    let paginated = paginate_cloud_profile(&content).unwrap();
    assert_eq!(paginated.descriptor.section_count, 6);
    assert_eq!(paginated.descriptor.sections.len(), 6);
    assert_eq!(
        paginated
            .descriptor
            .sections
            .iter()
            .map(|section| section.section)
            .collect::<Vec<_>>(),
        (ProjectionSectionKind::Character as i32..=ProjectionSectionKind::Progression as i32)
            .collect::<Vec<_>>()
    );
    assert!(
        paginated
            .pages
            .iter()
            .all(|page| page.encoded_len() <= MAX_PROJECTION_PAGE_ENCODED_LEN)
    );
    assert!(
        paginated
            .pages
            .iter()
            .all(|page| page_entry_count(page) <= MAX_PROJECTION_PAGE_ENTRIES)
    );
    let inventory_descriptor = &paginated.descriptor.sections[1];
    assert_eq!(inventory_descriptor.pages.len(), 2);
    assert_eq!(
        inventory_descriptor.pages[0].first_sort_key,
        inventory_sort_key(1, 1, 0)
    );
    assert_eq!(
        inventory_descriptor.pages[1].last_sort_key,
        inventory_sort_key(1, 1, MAX_PROJECTION_PAGE_ENTRIES as u32)
    );
    for descriptor in paginated
        .descriptor
        .sections
        .iter()
        .flat_map(|section| &section.pages)
    {
        let page = paginated
            .pages
            .iter()
            .find(|page| {
                page.section == descriptor.section && page.page_index == descriptor.page_index
            })
            .unwrap();
        assert_eq!(descriptor.entry_count as usize, page_entry_count(page));
        assert_eq!(descriptor.encoded_length as usize, page.encoded_len());
        assert_eq!(
            descriptor.sha256,
            Sha256::digest(page.encode_to_vec()).to_vec()
        );
    }
}

#[test]
fn duplicate_cloud_semantic_keys_are_rejected() {
    let mut content = fixture_cloud_profile_projection_content();
    let inventory = content.inventory.as_mut().unwrap();
    inventory.entries.push(inventory.entries[0].clone());
    inventory.state = Some(complete_projection_state(3, 3));
    assert!(cloud_profile_projection_id(&content).is_err());
}

#[test]
fn a_single_oversized_typed_entry_is_rejected_deterministically() {
    let mut content = fixture_cloud_profile_projection_content();
    content.bases.as_mut().unwrap().entries[0].location_id =
        "x".repeat(MAX_PROJECTION_PAGE_ENCODED_LEN + 1);
    assert!(matches!(
        paginate_cloud_profile(&content),
        Err(CanonicalError::ProjectionEntryTooLarge {
            section: ProjectionSectionKind::Bases,
            ..
        })
    ));
}
