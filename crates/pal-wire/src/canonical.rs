use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

use pal_data_types::{PersonalSnapshotId, ProjectionId};
use prost::Message;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::v2::{
    self, BaseFacility, BaseSnapshot, BaseStorageItem, CloudBaseEntryV1,
    CloudBaseProjectionContentV1, CloudBaseProjectionPageV1, CloudCharacterProjectionContentV1,
    CloudCharacterProjectionPageV1, CloudEquipmentProjectionContentV1,
    CloudEquipmentProjectionPageV1, CloudInventoryProjectionContentV1,
    CloudInventoryProjectionPageV1, CloudPalProjectionContentV1, CloudPalProjectionPageV1,
    CloudProfileProjectionContentV1, CloudProfileProjectionDescriptorV1,
    CloudProfileProjectionIdentityV1, CloudProfileProjectionPageDescriptorV1,
    CloudProfileProjectionPageV1, CloudProfileProjectionSectionDescriptorV1,
    CloudProfileSectionIdentityV1, CloudProgressionProjectionContentV1,
    CloudProgressionProjectionPageV1, EquipmentSlotKind, InventoryContainerKind, PalContainerKind,
    PalGender, PersonalSnapshotContent, ProjectionCompleteness, ProjectionSectionKind,
    ProjectionSectionStateV1, SnapshotCompleteness, SnapshotSectionKind,
    cloud_profile_projection_page_v1,
};

pub const MAX_PROJECTION_PAGE_ENTRIES: usize = 250;
pub const MAX_PROJECTION_PAGE_ENCODED_LEN: usize = 4_194_304;
pub const MAX_PROJECTION_PAGE_COUNT: usize = 2_048;
pub const MAX_CANONICAL_CONTENT_LEN: usize = 134_217_728;

const MAX_INVENTORY_ENTRIES: usize = 100_000;
const MAX_EQUIPMENT_ENTRIES: usize = 256;
const MAX_PAL_ENTRIES: usize = 10_000;
const MAX_PASSIVE_IDS_PER_PAL: usize = 64;
const MAX_ACTIVE_SKILL_IDS_PER_PAL: usize = 256;
const MAX_LEARNED_SKILL_IDS_PER_PAL: usize = 256;
const MAX_BASE_ENTRIES: usize = 256;
const MAX_FACILITIES_PER_BASE: usize = 4_096;
const MAX_FACILITIES_TOTAL: usize = 100_000;
const MAX_ASSIGNED_PALS_TOTAL: usize = 10_000;
const MAX_STORAGE_ITEMS_PER_BASE: usize = 8_192;
const MAX_STORAGE_ITEMS_TOTAL: usize = 100_000;
const MAX_PROGRESSION_ENTRIES: usize = 250_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CanonicalError {
    #[error("{field} is required")]
    MissingField { field: &'static str },
    #[error("{field} must be {expected} bytes, but was {actual}")]
    InvalidLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("{field} is not a canonical text identifier")]
    InvalidTextId { field: &'static str },
    #[error("{field} has unsupported enum value {value}")]
    InvalidEnum { field: &'static str, value: i32 },
    #[error("{field} has invalid value: {reason}")]
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    #[error("{field} contains {actual} entries, maximum is {maximum}")]
    TooManyEntries {
        field: &'static str,
        maximum: usize,
        actual: usize,
    },
    #[error("duplicate semantic key in {field}: {stable_key:?}")]
    DuplicateSemanticKey {
        field: &'static str,
        stable_key: Vec<u8>,
    },
    #[error("integer overflow while canonicalizing {field}")]
    IntegerOverflow { field: &'static str },
    #[error("canonical content is {encoded_length} bytes, maximum is {maximum}")]
    ContentTooLarge {
        encoded_length: usize,
        maximum: usize,
    },
    #[error("one {section:?} projection entry ({stable_key:?}) encodes to {encoded_length} bytes")]
    ProjectionEntryTooLarge {
        section: ProjectionSectionKind,
        stable_key: Vec<u8>,
        encoded_length: usize,
    },
    #[error("projection has {actual} pages, maximum is {maximum}")]
    TooManyProjectionPages { actual: usize, maximum: usize },
    #[error("projection pages encode to {encoded_length} bytes, maximum is {maximum}")]
    ProjectionTransportTooLarge { encoded_length: u64, maximum: u64 },
}

#[derive(Clone, Debug)]
pub struct PaginatedCloudProfile {
    pub pages: Vec<CloudProfileProjectionPageV1>,
    pub descriptor: CloudProfileProjectionDescriptorV1,
}

impl Deref for PaginatedCloudProfile {
    type Target = [CloudProfileProjectionPageV1];

    fn deref(&self) -> &Self::Target {
        &self.pages
    }
}

pub fn personal_snapshot_id(
    content: &PersonalSnapshotContent,
) -> Result<PersonalSnapshotId, CanonicalError> {
    let bytes = canonical_snapshot_content(content)?;
    let mut digest = Sha256::new();
    digest.update(b"palcompanion.snapshot.v1\0");
    digest.update(bytes);
    Ok(PersonalSnapshotId::from_bytes(digest.finalize().into()))
}

pub fn canonical_snapshot_content(
    content: &PersonalSnapshotContent,
) -> Result<Vec<u8>, CanonicalError> {
    validate_snapshot(content)?;
    let mut canonical = content.clone();

    canonical
        .inventory
        .sort_by_key(|slot| (slot.container_kind, slot.container_ordinal, slot.slot_index));
    canonical
        .equipment
        .sort_by_key(|slot| (slot.slot_kind, slot.slot_index));
    canonical
        .pals
        .sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    for pal in &mut canonical.pals {
        sort_dedup_strings(&mut pal.passive_ids);
        sort_dedup_strings(&mut pal.active_skill_ids);
    }
    canonical
        .bases
        .sort_by(|left, right| left.base_id.cmp(&right.base_id));
    for base in &mut canonical.bases {
        canonicalize_snapshot_base(base)?;
    }
    if let Some(progression) = canonical.progression.as_mut() {
        for ids in [
            &mut progression.technology_ids,
            &mut progression.fast_travel_ids,
            &mut progression.tower_ids,
            &mut progression.boss_ids,
            &mut progression.mission_ids,
            &mut progression.paldex_capture_ids,
            &mut progression.map_region_ids,
            &mut progression.collectible_ids,
            &mut progression.base_research_ids,
        ] {
            sort_dedup_strings(ids);
        }
    }
    canonical.section_states.sort_by_key(|state| state.section);

    let bytes = canonical.encode_to_vec();
    check_content_length(bytes.len())?;
    Ok(bytes)
}

fn validate_snapshot(content: &PersonalSnapshotContent) -> Result<(), CanonicalError> {
    if content.schema_major != 1 {
        return Err(CanonicalError::InvalidValue {
            field: "schema_major",
            reason: "expected schema major 1",
        });
    }
    validate_text_id("game_build_id", &content.game_build_id)?;
    validate_len("dataset_manifest_id", &content.dataset_manifest_id, 32)?;
    validate_text_id("world_id", &content.world_id)?;
    validate_len("owner_subject_id", &content.owner_subject_id, 32)?;
    let overall = snapshot_completeness("completeness", content.completeness)?;

    check_count("inventory", content.inventory.len(), MAX_INVENTORY_ENTRIES)?;
    check_count("equipment", content.equipment.len(), MAX_EQUIPMENT_ENTRIES)?;
    check_count("pals", content.pals.len(), MAX_PAL_ENTRIES)?;
    check_count("bases", content.bases.len(), MAX_BASE_ENTRIES)?;

    let mut inventory_keys = BTreeSet::new();
    for slot in &content.inventory {
        inventory_container_kind("inventory.container_kind", slot.container_kind)?;
        validate_text_id("inventory.item_id", &slot.item_id)?;
        let key = inventory_key(slot.container_kind, slot.container_ordinal, slot.slot_index);
        reject_duplicate(&mut inventory_keys, "inventory", key)?;
    }

    let mut equipment_keys = BTreeSet::new();
    for slot in &content.equipment {
        equipment_slot_kind("equipment.slot_kind", slot.slot_kind)?;
        validate_text_id("equipment.item_id", &slot.item_id)?;
        let key = equipment_key(slot.slot_kind, slot.slot_index);
        reject_duplicate(&mut equipment_keys, "equipment", key)?;
    }

    let mut pal_ids = BTreeSet::new();
    let mut pal_locations = BTreeSet::new();
    for pal in &content.pals {
        validate_len("pals.instance_id", &pal.instance_id, 16)?;
        validate_text_id("pals.species_id", &pal.species_id)?;
        check_count(
            "pals.passive_ids",
            pal.passive_ids.len(),
            MAX_PASSIVE_IDS_PER_PAL,
        )?;
        check_count(
            "pals.active_skill_ids",
            pal.active_skill_ids.len(),
            MAX_ACTIVE_SKILL_IDS_PER_PAL,
        )?;
        validate_text_ids("pals.passive_ids", &pal.passive_ids)?;
        validate_text_ids("pals.active_skill_ids", &pal.active_skill_ids)?;
        if let Some(gender) = &pal.gender
            && !matches!(gender.as_str(), "male" | "female" | "genderless")
        {
            return Err(CanonicalError::InvalidValue {
                field: "pals.gender",
                reason: "expected male, female, or genderless",
            });
        }
        reject_duplicate(&mut pal_ids, "pals.instance_id", pal.instance_id.clone())?;
        let location = pal.location.as_ref().ok_or(CanonicalError::MissingField {
            field: "pals.location",
        })?;
        pal_container_kind("pals.location.container_kind", location.container_kind)?;
        validate_base_location(
            "pals.location.base_id",
            location.container_kind,
            location.base_id.as_deref(),
        )?;
        let key = pal_location_key(
            location.container_kind,
            location.container_ordinal,
            location.slot_index,
            location.base_id.as_deref(),
        );
        reject_duplicate(&mut pal_locations, "pals.location", key)?;
    }

    let mut base_ids = BTreeSet::new();
    for base in &content.bases {
        validate_snapshot_base(base)?;
        reject_duplicate(&mut base_ids, "bases.base_id", base.base_id.clone())?;
    }
    if let Some(progression) = &content.progression {
        for (field, ids) in [
            ("progression.technology_ids", &progression.technology_ids),
            ("progression.fast_travel_ids", &progression.fast_travel_ids),
            ("progression.tower_ids", &progression.tower_ids),
            ("progression.boss_ids", &progression.boss_ids),
            ("progression.mission_ids", &progression.mission_ids),
            (
                "progression.paldex_capture_ids",
                &progression.paldex_capture_ids,
            ),
            ("progression.map_region_ids", &progression.map_region_ids),
            ("progression.collectible_ids", &progression.collectible_ids),
            (
                "progression.base_research_ids",
                &progression.base_research_ids,
            ),
        ] {
            validate_text_ids(field, ids)?;
        }
    }
    validate_snapshot_section_states(content, overall)?;
    Ok(())
}

fn validate_snapshot_base(base: &BaseSnapshot) -> Result<(), CanonicalError> {
    validate_len("bases.base_id", &base.base_id, 16)?;
    validate_text_id("bases.location_id", &base.location_id)?;
    for facility in &base.facilities {
        validate_text_id("bases.facilities.building_id", &facility.building_id)?;
        if facility.count == 0 {
            return Err(CanonicalError::InvalidValue {
                field: "bases.facilities.count",
                reason: "count must be positive",
            });
        }
    }
    for assigned in &base.assigned_pal_instance_ids {
        validate_len("bases.assigned_pal_instance_ids", assigned, 16)?;
    }
    for item in &base.storage_items {
        validate_text_id("bases.storage_items.item_id", &item.item_id)?;
        if item.quantity == 0 {
            return Err(CanonicalError::InvalidValue {
                field: "bases.storage_items.quantity",
                reason: "quantity must be positive",
            });
        }
    }
    Ok(())
}

fn canonicalize_snapshot_base(base: &mut BaseSnapshot) -> Result<(), CanonicalError> {
    let mut facilities = BTreeMap::<String, u32>::new();
    for facility in base.facilities.drain(..) {
        let count = facilities.entry(facility.building_id).or_default();
        *count = count
            .checked_add(facility.count)
            .ok_or(CanonicalError::IntegerOverflow {
                field: "bases.facilities.count",
            })?;
    }
    base.facilities = facilities
        .into_iter()
        .map(|(building_id, count)| BaseFacility { building_id, count })
        .collect();
    base.assigned_pal_instance_ids.sort();
    base.assigned_pal_instance_ids.dedup();
    let mut storage = BTreeMap::<String, u64>::new();
    for item in base.storage_items.drain(..) {
        let quantity = storage.entry(item.item_id).or_default();
        *quantity = quantity
            .checked_add(item.quantity)
            .ok_or(CanonicalError::IntegerOverflow {
                field: "bases.storage_items.quantity",
            })?;
    }
    base.storage_items = storage
        .into_iter()
        .map(|(item_id, quantity)| BaseStorageItem { item_id, quantity })
        .collect();
    Ok(())
}

fn validate_snapshot_section_states(
    content: &PersonalSnapshotContent,
    overall: SnapshotCompleteness,
) -> Result<(), CanonicalError> {
    if content.section_states.len() != 6 {
        return Err(CanonicalError::InvalidValue {
            field: "section_states",
            reason: "exactly six section states are required",
        });
    }
    let mut sections = BTreeSet::new();
    let mut states = Vec::with_capacity(6);
    for state in &content.section_states {
        let section = snapshot_section_kind("section_states.section", state.section)?;
        let completeness =
            snapshot_completeness("section_states.completeness", state.completeness)?;
        validate_snapshot_counts(
            completeness,
            state.expected_record_count,
            state.observed_record_count,
        )?;
        if !sections.insert(section as i32) {
            return Err(CanonicalError::DuplicateSemanticKey {
                field: "section_states.section",
                stable_key: (section as i32).to_be_bytes().to_vec(),
            });
        }
        if completeness == SnapshotCompleteness::Unavailable
            && snapshot_section_has_content(content, section)
        {
            return Err(CanonicalError::InvalidValue {
                field: "section_states.completeness",
                reason: "unavailable sections must contain no entries",
            });
        }
        states.push(completeness);
    }
    let expected_sections = (SnapshotSectionKind::Character as i32
        ..=SnapshotSectionKind::Progression as i32)
        .collect::<BTreeSet<_>>();
    if sections != expected_sections {
        return Err(CanonicalError::InvalidValue {
            field: "section_states.section",
            reason: "all six non-zero section kinds are required",
        });
    }

    let derived = if states
        .iter()
        .all(|state| *state == SnapshotCompleteness::Unavailable)
    {
        SnapshotCompleteness::Unavailable
    } else if states.contains(&SnapshotCompleteness::Unknown) {
        SnapshotCompleteness::Unknown
    } else if states
        .iter()
        .all(|state| *state == SnapshotCompleteness::Complete)
    {
        SnapshotCompleteness::Complete
    } else {
        SnapshotCompleteness::Partial
    };
    if overall != derived {
        return Err(CanonicalError::InvalidValue {
            field: "completeness",
            reason: "overall completeness does not match the six section states",
        });
    }
    Ok(())
}

fn snapshot_section_has_content(
    content: &PersonalSnapshotContent,
    section: SnapshotSectionKind,
) -> bool {
    match section {
        SnapshotSectionKind::Character => content.character.is_some(),
        SnapshotSectionKind::Inventory => !content.inventory.is_empty(),
        SnapshotSectionKind::Equipment => !content.equipment.is_empty(),
        SnapshotSectionKind::Pals => !content.pals.is_empty(),
        SnapshotSectionKind::Bases => !content.bases.is_empty(),
        SnapshotSectionKind::Progression => content.progression.is_some(),
        SnapshotSectionKind::Unspecified => false,
    }
}

fn validate_snapshot_counts(
    completeness: SnapshotCompleteness,
    expected: u32,
    observed: u32,
) -> Result<(), CanonicalError> {
    let valid = match completeness {
        SnapshotCompleteness::Complete => expected == observed,
        SnapshotCompleteness::Partial => expected > 0 && observed <= expected,
        SnapshotCompleteness::Unavailable => expected == 0 && observed == 0,
        SnapshotCompleteness::Unknown => expected == 0,
        SnapshotCompleteness::Unspecified => false,
    };
    if valid {
        Ok(())
    } else {
        Err(CanonicalError::InvalidValue {
            field: "section_states",
            reason: "section counts do not match completeness semantics",
        })
    }
}

pub fn canonical_cloud_profile_projection_content(
    content: &CloudProfileProjectionContentV1,
) -> Result<Vec<u8>, CanonicalError> {
    let canonical = canonicalize_cloud_profile_sections(content.clone())?;
    let bytes = canonical.encode_to_vec();
    check_content_length(bytes.len())?;
    Ok(bytes)
}

pub fn canonical_cloud_profile_projection_identity(
    content: &CloudProfileProjectionContentV1,
) -> Result<Vec<u8>, CanonicalError> {
    let canonical = canonicalize_cloud_profile_sections(content.clone())?;
    let sections = canonical_profile_section_identities(&canonical)?;
    Ok(CloudProfileProjectionIdentityV1 {
        schema_version: canonical.schema_version,
        dataset_manifest_id: canonical.dataset_manifest_id,
        game_build_id: canonical.game_build_id,
        sections,
    }
    .encode_to_vec())
}

pub fn cloud_profile_projection_id(
    content: &CloudProfileProjectionContentV1,
) -> Result<ProjectionId, CanonicalError> {
    let bytes = canonical_cloud_profile_projection_identity(content)?;
    Ok(ProjectionId::from_bytes(Sha256::digest(bytes).into()))
}

fn canonicalize_cloud_profile_sections(
    mut content: CloudProfileProjectionContentV1,
) -> Result<CloudProfileProjectionContentV1, CanonicalError> {
    validate_cloud_profile_projection(&content)?;

    let inventory = content
        .inventory
        .as_mut()
        .expect("validation requires inventory");
    inventory.entries.sort_by_key(|entry| {
        (
            entry.container_kind,
            entry.container_ordinal,
            entry.slot_index,
        )
    });

    let equipment = content
        .equipment
        .as_mut()
        .expect("validation requires equipment");
    equipment
        .entries
        .sort_by_key(|entry| (entry.slot_kind, entry.slot_index));

    let pals = content.pals.as_mut().expect("validation requires pals");
    pals.entries
        .sort_by(|left, right| left.pal_instance_id.cmp(&right.pal_instance_id));
    for pal in &mut pals.entries {
        sort_dedup_strings(&mut pal.passive_ids);
        sort_dedup_strings(&mut pal.active_skill_ids);
        sort_dedup_strings(&mut pal.learned_skill_ids);
    }

    let bases = content.bases.as_mut().expect("validation requires bases");
    bases
        .entries
        .sort_by(|left, right| left.base_id.cmp(&right.base_id));
    for base in &mut bases.entries {
        canonicalize_cloud_base(base)?;
    }

    content
        .progression
        .as_mut()
        .expect("validation requires progression")
        .entries
        .sort_by(|left, right| {
            (left.kind, left.unlock_id.as_bytes()).cmp(&(right.kind, right.unlock_id.as_bytes()))
        });

    let encoded_length = content.encoded_len();
    check_content_length(encoded_length)?;
    Ok(content)
}

fn validate_cloud_profile_projection(
    content: &CloudProfileProjectionContentV1,
) -> Result<(), CanonicalError> {
    if content.schema_version != 1 {
        return Err(CanonicalError::InvalidValue {
            field: "schema_version",
            reason: "expected schema version 1",
        });
    }
    validate_len("dataset_manifest_id", &content.dataset_manifest_id, 32)?;
    validate_text_id("game_build_id", &content.game_build_id)?;

    validate_character_section(required(&content.character, "character")?)?;
    validate_inventory_section(required(&content.inventory, "inventory")?)?;
    validate_equipment_section(required(&content.equipment, "equipment")?)?;
    validate_pal_section(required(&content.pals, "pals")?)?;
    validate_base_section(required(&content.bases, "bases")?)?;
    validate_progression_section(required(&content.progression, "progression")?)?;
    Ok(())
}

fn validate_character_section(
    section: &CloudCharacterProjectionContentV1,
) -> Result<(), CanonicalError> {
    let entry_count = usize::from(section.character.is_some());
    validate_projection_state(required(&section.state, "character.state")?, entry_count)?;
    Ok(())
}

fn validate_inventory_section(
    section: &CloudInventoryProjectionContentV1,
) -> Result<(), CanonicalError> {
    check_count(
        "inventory.entries",
        section.entries.len(),
        MAX_INVENTORY_ENTRIES,
    )?;
    validate_projection_state(
        required(&section.state, "inventory.state")?,
        section.entries.len(),
    )?;
    let mut keys = BTreeSet::new();
    for entry in &section.entries {
        inventory_container_kind("inventory.entries.container_kind", entry.container_kind)?;
        validate_text_id("inventory.entries.item_id", &entry.item_id)?;
        reject_duplicate(
            &mut keys,
            "inventory.entries",
            inventory_key(
                entry.container_kind,
                entry.container_ordinal,
                entry.slot_index,
            ),
        )?;
    }
    Ok(())
}

fn validate_equipment_section(
    section: &CloudEquipmentProjectionContentV1,
) -> Result<(), CanonicalError> {
    check_count(
        "equipment.entries",
        section.entries.len(),
        MAX_EQUIPMENT_ENTRIES,
    )?;
    validate_projection_state(
        required(&section.state, "equipment.state")?,
        section.entries.len(),
    )?;
    let mut keys = BTreeSet::new();
    for entry in &section.entries {
        equipment_slot_kind("equipment.entries.slot_kind", entry.slot_kind)?;
        validate_text_id("equipment.entries.item_id", &entry.item_id)?;
        reject_duplicate(
            &mut keys,
            "equipment.entries",
            equipment_key(entry.slot_kind, entry.slot_index),
        )?;
    }
    Ok(())
}

fn validate_pal_section(section: &CloudPalProjectionContentV1) -> Result<(), CanonicalError> {
    check_count("pals.entries", section.entries.len(), MAX_PAL_ENTRIES)?;
    validate_projection_state(
        required(&section.state, "pals.state")?,
        section.entries.len(),
    )?;
    let mut ids = BTreeSet::new();
    let mut locations = BTreeSet::new();
    for pal in &section.entries {
        validate_len("pals.entries.pal_instance_id", &pal.pal_instance_id, 16)?;
        validate_text_id("pals.entries.species_id", &pal.species_id)?;
        pal_gender("pals.entries.gender", pal.gender)?;
        check_count(
            "pals.entries.passive_ids",
            pal.passive_ids.len(),
            MAX_PASSIVE_IDS_PER_PAL,
        )?;
        check_count(
            "pals.entries.active_skill_ids",
            pal.active_skill_ids.len(),
            MAX_ACTIVE_SKILL_IDS_PER_PAL,
        )?;
        check_count(
            "pals.entries.learned_skill_ids",
            pal.learned_skill_ids.len(),
            MAX_LEARNED_SKILL_IDS_PER_PAL,
        )?;
        validate_text_ids("pals.entries.passive_ids", &pal.passive_ids)?;
        validate_text_ids("pals.entries.active_skill_ids", &pal.active_skill_ids)?;
        validate_text_ids("pals.entries.learned_skill_ids", &pal.learned_skill_ids)?;
        pal_container_kind("pals.entries.container_kind", pal.container_kind)?;
        validate_base_location(
            "pals.entries.base_id",
            pal.container_kind,
            pal.base_id.as_deref(),
        )?;
        reject_duplicate(
            &mut ids,
            "pals.entries.pal_instance_id",
            pal.pal_instance_id.clone(),
        )?;
        reject_duplicate(
            &mut locations,
            "pals.entries.location",
            pal_location_key(
                pal.container_kind,
                pal.container_ordinal,
                pal.slot_index,
                pal.base_id.as_deref(),
            ),
        )?;
    }
    Ok(())
}

fn validate_base_section(section: &CloudBaseProjectionContentV1) -> Result<(), CanonicalError> {
    check_count("bases.entries", section.entries.len(), MAX_BASE_ENTRIES)?;
    validate_projection_state(
        required(&section.state, "bases.state")?,
        section.entries.len(),
    )?;
    let mut base_ids = BTreeSet::new();
    let mut facilities_total = 0usize;
    let mut assigned_total = 0usize;
    let mut storage_total = 0usize;
    for base in &section.entries {
        validate_len("bases.entries.base_id", &base.base_id, 16)?;
        validate_text_id("bases.entries.location_id", &base.location_id)?;
        check_count(
            "bases.entries.facilities",
            base.facilities.len(),
            MAX_FACILITIES_PER_BASE,
        )?;
        check_count(
            "bases.entries.storage_items",
            base.storage_items.len(),
            MAX_STORAGE_ITEMS_PER_BASE,
        )?;
        facilities_total = facilities_total.checked_add(base.facilities.len()).ok_or(
            CanonicalError::IntegerOverflow {
                field: "bases.entries.facilities",
            },
        )?;
        assigned_total = assigned_total
            .checked_add(base.assigned_pal_instance_ids.len())
            .ok_or(CanonicalError::IntegerOverflow {
                field: "bases.entries.assigned_pal_instance_ids",
            })?;
        storage_total = storage_total.checked_add(base.storage_items.len()).ok_or(
            CanonicalError::IntegerOverflow {
                field: "bases.entries.storage_items",
            },
        )?;
        for facility in &base.facilities {
            validate_text_id(
                "bases.entries.facilities.building_id",
                &facility.building_id,
            )?;
            if facility.count == 0 {
                return Err(CanonicalError::InvalidValue {
                    field: "bases.entries.facilities.count",
                    reason: "count must be positive",
                });
            }
        }
        for assigned in &base.assigned_pal_instance_ids {
            validate_len("bases.entries.assigned_pal_instance_ids", assigned, 16)?;
        }
        for item in &base.storage_items {
            validate_text_id("bases.entries.storage_items.item_id", &item.item_id)?;
            if item.quantity == 0 {
                return Err(CanonicalError::InvalidValue {
                    field: "bases.entries.storage_items.quantity",
                    reason: "quantity must be positive",
                });
            }
        }
        reject_duplicate(&mut base_ids, "bases.entries.base_id", base.base_id.clone())?;
    }
    check_count(
        "bases.entries.facilities",
        facilities_total,
        MAX_FACILITIES_TOTAL,
    )?;
    check_count(
        "bases.entries.assigned_pal_instance_ids",
        assigned_total,
        MAX_ASSIGNED_PALS_TOTAL,
    )?;
    check_count(
        "bases.entries.storage_items",
        storage_total,
        MAX_STORAGE_ITEMS_TOTAL,
    )?;
    Ok(())
}

fn validate_progression_section(
    section: &CloudProgressionProjectionContentV1,
) -> Result<(), CanonicalError> {
    check_count(
        "progression.entries",
        section.entries.len(),
        MAX_PROGRESSION_ENTRIES,
    )?;
    validate_projection_state(
        required(&section.state, "progression.state")?,
        section.entries.len(),
    )?;
    let mut keys = BTreeSet::new();
    for entry in &section.entries {
        progression_unlock_kind("progression.entries.kind", entry.kind)?;
        validate_text_id("progression.entries.unlock_id", &entry.unlock_id)?;
        reject_duplicate(
            &mut keys,
            "progression.entries",
            progression_key(entry.kind, &entry.unlock_id),
        )?;
    }
    Ok(())
}

fn validate_projection_state(
    state: &ProjectionSectionStateV1,
    actual_entry_count: usize,
) -> Result<(), CanonicalError> {
    let actual_u32 =
        u32::try_from(actual_entry_count).map_err(|_| CanonicalError::IntegerOverflow {
            field: "state.projected_entry_count",
        })?;
    if state.projected_entry_count != actual_u32 {
        return Err(CanonicalError::InvalidValue {
            field: "state.projected_entry_count",
            reason: "must equal the actual top-level entry count",
        });
    }
    let completeness = projection_completeness("state.completeness", state.completeness)?;
    let counts_valid = match completeness {
        ProjectionCompleteness::Complete => {
            state.source_expected_record_count == state.source_observed_record_count
        }
        ProjectionCompleteness::Partial => {
            state.source_expected_record_count > 0
                && state.source_observed_record_count <= state.source_expected_record_count
        }
        ProjectionCompleteness::Unavailable => {
            state.source_expected_record_count == 0
                && state.source_observed_record_count == 0
                && state.projected_entry_count == 0
        }
        ProjectionCompleteness::Unknown => state.source_expected_record_count == 0,
        ProjectionCompleteness::Unspecified => false,
    };
    if counts_valid {
        Ok(())
    } else {
        Err(CanonicalError::InvalidValue {
            field: "state",
            reason: "counts do not match projection completeness semantics",
        })
    }
}

fn canonicalize_cloud_base(base: &mut CloudBaseEntryV1) -> Result<(), CanonicalError> {
    let mut facilities = BTreeMap::<String, u32>::new();
    for facility in base.facilities.drain(..) {
        let count = facilities.entry(facility.building_id).or_default();
        *count = count
            .checked_add(facility.count)
            .ok_or(CanonicalError::IntegerOverflow {
                field: "bases.entries.facilities.count",
            })?;
    }
    base.facilities = facilities
        .into_iter()
        .map(|(building_id, count)| v2::CloudBaseFacilityV1 { building_id, count })
        .collect();
    base.assigned_pal_instance_ids.sort();
    base.assigned_pal_instance_ids.dedup();
    let mut storage = BTreeMap::<String, u64>::new();
    for item in base.storage_items.drain(..) {
        let quantity = storage.entry(item.item_id).or_default();
        *quantity = quantity
            .checked_add(item.quantity)
            .ok_or(CanonicalError::IntegerOverflow {
                field: "bases.entries.storage_items.quantity",
            })?;
    }
    base.storage_items = storage
        .into_iter()
        .map(|(item_id, quantity)| v2::CloudBaseStorageItemV1 { item_id, quantity })
        .collect();
    Ok(())
}

fn canonical_profile_section_identities(
    content: &CloudProfileProjectionContentV1,
) -> Result<Vec<CloudProfileSectionIdentityV1>, CanonicalError> {
    let character = required(&content.character, "character")?;
    let inventory = required(&content.inventory, "inventory")?;
    let equipment = required(&content.equipment, "equipment")?;
    let pals = required(&content.pals, "pals")?;
    let bases = required(&content.bases, "bases")?;
    let progression = required(&content.progression, "progression")?;
    Ok(vec![
        section_identity(
            ProjectionSectionKind::Character,
            required(&character.state, "character.state")?,
            character,
        ),
        section_identity(
            ProjectionSectionKind::Inventory,
            required(&inventory.state, "inventory.state")?,
            inventory,
        ),
        section_identity(
            ProjectionSectionKind::Equipment,
            required(&equipment.state, "equipment.state")?,
            equipment,
        ),
        section_identity(
            ProjectionSectionKind::Pals,
            required(&pals.state, "pals.state")?,
            pals,
        ),
        section_identity(
            ProjectionSectionKind::Bases,
            required(&bases.state, "bases.state")?,
            bases,
        ),
        section_identity(
            ProjectionSectionKind::Progression,
            required(&progression.state, "progression.state")?,
            progression,
        ),
    ])
}

fn section_identity<M: Message>(
    section: ProjectionSectionKind,
    state: &ProjectionSectionStateV1,
    content: &M,
) -> CloudProfileSectionIdentityV1 {
    CloudProfileSectionIdentityV1 {
        section: section as i32,
        state: Some(*state),
        section_content_sha256: Sha256::digest(content.encode_to_vec()).to_vec(),
    }
}

pub fn paginate_cloud_profile(
    content: &CloudProfileProjectionContentV1,
) -> Result<PaginatedCloudProfile, CanonicalError> {
    reject_raw_oversized_entries(content)?;
    let canonical = canonicalize_cloud_profile_sections(content.clone())?;
    let projection_id = cloud_profile_projection_id(&canonical)?;
    let section_identities = canonical_profile_section_identities(&canonical)?;
    let projection_bytes = projection_id.as_bytes();

    let character = required(&canonical.character, "character")?;
    let character_entries = character.character.iter().cloned().collect::<Vec<_>>();
    let (mut pages, character_pages) = paginate_entries(
        projection_bytes,
        ProjectionSectionKind::Character,
        &character_entries,
        |entries| {
            cloud_profile_projection_page_v1::Payload::Character(CloudCharacterProjectionPageV1 {
                entries,
            })
        },
        |_| vec![0x01],
    )?;

    let inventory = required(&canonical.inventory, "inventory")?;
    let (mut section_pages, inventory_pages) = paginate_entries(
        projection_bytes,
        ProjectionSectionKind::Inventory,
        &inventory.entries,
        |entries| {
            cloud_profile_projection_page_v1::Payload::Inventory(CloudInventoryProjectionPageV1 {
                entries,
            })
        },
        |entry| {
            inventory_key(
                entry.container_kind,
                entry.container_ordinal,
                entry.slot_index,
            )
        },
    )?;
    pages.append(&mut section_pages);

    let equipment = required(&canonical.equipment, "equipment")?;
    let (mut section_pages, equipment_pages) = paginate_entries(
        projection_bytes,
        ProjectionSectionKind::Equipment,
        &equipment.entries,
        |entries| {
            cloud_profile_projection_page_v1::Payload::Equipment(CloudEquipmentProjectionPageV1 {
                entries,
            })
        },
        |entry| equipment_key(entry.slot_kind, entry.slot_index),
    )?;
    pages.append(&mut section_pages);

    let pals = required(&canonical.pals, "pals")?;
    let (mut section_pages, pal_pages) = paginate_entries(
        projection_bytes,
        ProjectionSectionKind::Pals,
        &pals.entries,
        |entries| {
            cloud_profile_projection_page_v1::Payload::Pals(CloudPalProjectionPageV1 { entries })
        },
        |entry| entry.pal_instance_id.clone(),
    )?;
    pages.append(&mut section_pages);

    let bases = required(&canonical.bases, "bases")?;
    let (mut section_pages, base_pages) = paginate_entries(
        projection_bytes,
        ProjectionSectionKind::Bases,
        &bases.entries,
        |entries| {
            cloud_profile_projection_page_v1::Payload::Bases(CloudBaseProjectionPageV1 { entries })
        },
        |entry| entry.base_id.clone(),
    )?;
    pages.append(&mut section_pages);

    let progression = required(&canonical.progression, "progression")?;
    let (mut section_pages, progression_pages) = paginate_entries(
        projection_bytes,
        ProjectionSectionKind::Progression,
        &progression.entries,
        |entries| {
            cloud_profile_projection_page_v1::Payload::Progression(
                CloudProgressionProjectionPageV1 { entries },
            )
        },
        |entry| progression_key(entry.kind, &entry.unlock_id),
    )?;
    pages.append(&mut section_pages);

    if pages.len() > MAX_PROJECTION_PAGE_COUNT {
        return Err(CanonicalError::TooManyProjectionPages {
            actual: pages.len(),
            maximum: MAX_PROJECTION_PAGE_COUNT,
        });
    }

    let page_sets = [
        character_pages,
        inventory_pages,
        equipment_pages,
        pal_pages,
        base_pages,
        progression_pages,
    ];
    let states = [
        required(&character.state, "character.state")?,
        required(&inventory.state, "inventory.state")?,
        required(&equipment.state, "equipment.state")?,
        required(&pals.state, "pals.state")?,
        required(&bases.state, "bases.state")?,
        required(&progression.state, "progression.state")?,
    ];

    let mut section_descriptors = Vec::with_capacity(6);
    let mut total_page_count = 0usize;
    let mut total_entry_count = 0usize;
    let mut total_encoded_length = 0u64;
    for ((identity, page_descriptors), state) in
        section_identities.into_iter().zip(page_sets).zip(states)
    {
        let section_encoded_length = page_descriptors.iter().try_fold(0u64, |total, page| {
            total.checked_add(u64::from(page.encoded_length)).ok_or(
                CanonicalError::IntegerOverflow {
                    field: "descriptor.total_encoded_length",
                },
            )
        })?;
        total_page_count += page_descriptors.len();
        total_entry_count += page_descriptors
            .iter()
            .map(|page| page.entry_count as usize)
            .sum::<usize>();
        total_encoded_length = total_encoded_length
            .checked_add(section_encoded_length)
            .ok_or(CanonicalError::IntegerOverflow {
                field: "descriptor.total_encoded_length",
            })?;
        section_descriptors.push(CloudProfileProjectionSectionDescriptorV1 {
            section: identity.section,
            state: Some(*state),
            section_content_sha256: identity.section_content_sha256,
            page_count: u32::try_from(page_descriptors.len()).map_err(|_| {
                CanonicalError::IntegerOverflow {
                    field: "descriptor.sections.page_count",
                }
            })?,
            total_encoded_length: section_encoded_length,
            pages: page_descriptors,
        });
    }

    if total_encoded_length > MAX_CANONICAL_CONTENT_LEN as u64 {
        return Err(CanonicalError::ProjectionTransportTooLarge {
            encoded_length: total_encoded_length,
            maximum: MAX_CANONICAL_CONTENT_LEN as u64,
        });
    }

    let descriptor = CloudProfileProjectionDescriptorV1 {
        projection_id: projection_bytes.to_vec(),
        section_count: 6,
        total_page_count: u32::try_from(total_page_count).map_err(|_| {
            CanonicalError::IntegerOverflow {
                field: "descriptor.total_page_count",
            }
        })?,
        total_entry_count: u32::try_from(total_entry_count).map_err(|_| {
            CanonicalError::IntegerOverflow {
                field: "descriptor.total_entry_count",
            }
        })?,
        total_encoded_length,
        sections: section_descriptors,
    };
    Ok(PaginatedCloudProfile { pages, descriptor })
}

fn paginate_entries<T, BuildPayload, SortKey>(
    projection_id: &[u8; 32],
    section: ProjectionSectionKind,
    entries: &[T],
    build_payload: BuildPayload,
    sort_key: SortKey,
) -> Result<
    (
        Vec<CloudProfileProjectionPageV1>,
        Vec<CloudProfileProjectionPageDescriptorV1>,
    ),
    CanonicalError,
>
where
    T: Clone,
    BuildPayload: Fn(Vec<T>) -> cloud_profile_projection_page_v1::Payload,
    SortKey: Fn(&T) -> Vec<u8>,
{
    let mut pages = Vec::new();
    let mut descriptors = Vec::new();
    let mut current = Vec::<T>::new();
    let mut current_keys = Vec::<Vec<u8>>::new();
    let mut page_index = 0u32;

    for entry in entries {
        let key = sort_key(entry);
        let mut candidate = current.clone();
        candidate.push(entry.clone());
        let candidate_page = projection_page(
            projection_id,
            section,
            page_index,
            build_payload(candidate.clone()),
        );
        let over_count = candidate.len() > MAX_PROJECTION_PAGE_ENTRIES;
        let over_bytes = candidate_page.encoded_len() > MAX_PROJECTION_PAGE_ENCODED_LEN;
        if over_count || over_bytes {
            if current.is_empty() {
                return Err(CanonicalError::ProjectionEntryTooLarge {
                    section,
                    stable_key: key,
                    encoded_length: candidate_page.encoded_len(),
                });
            }
            close_page(
                projection_id,
                section,
                page_index,
                std::mem::take(&mut current),
                std::mem::take(&mut current_keys),
                &build_payload,
                &mut pages,
                &mut descriptors,
            )?;
            page_index = page_index
                .checked_add(1)
                .ok_or(CanonicalError::IntegerOverflow {
                    field: "page_index",
                })?;
            current.push(entry.clone());
            current_keys.push(key.clone());
            let single_page = projection_page(
                projection_id,
                section,
                page_index,
                build_payload(current.clone()),
            );
            if single_page.encoded_len() > MAX_PROJECTION_PAGE_ENCODED_LEN {
                return Err(CanonicalError::ProjectionEntryTooLarge {
                    section,
                    stable_key: key,
                    encoded_length: single_page.encoded_len(),
                });
            }
        } else {
            current = candidate;
            current_keys.push(key);
        }
    }

    if !current.is_empty() {
        close_page(
            projection_id,
            section,
            page_index,
            current,
            current_keys,
            &build_payload,
            &mut pages,
            &mut descriptors,
        )?;
    }
    Ok((pages, descriptors))
}

#[allow(clippy::too_many_arguments)]
fn close_page<T, BuildPayload>(
    projection_id: &[u8; 32],
    section: ProjectionSectionKind,
    page_index: u32,
    entries: Vec<T>,
    keys: Vec<Vec<u8>>,
    build_payload: &BuildPayload,
    pages: &mut Vec<CloudProfileProjectionPageV1>,
    descriptors: &mut Vec<CloudProfileProjectionPageDescriptorV1>,
) -> Result<(), CanonicalError>
where
    BuildPayload: Fn(Vec<T>) -> cloud_profile_projection_page_v1::Payload,
{
    let entry_count = entries.len();
    let page = projection_page(projection_id, section, page_index, build_payload(entries));
    let encoded = page.encode_to_vec();
    if entry_count > MAX_PROJECTION_PAGE_ENTRIES || encoded.len() > MAX_PROJECTION_PAGE_ENCODED_LEN
    {
        return Err(CanonicalError::InvalidValue {
            field: "projection.page",
            reason: "closed page exceeds a paging bound",
        });
    }
    descriptors.push(CloudProfileProjectionPageDescriptorV1 {
        section: section as i32,
        page_index,
        entry_count: u32::try_from(entry_count).map_err(|_| CanonicalError::IntegerOverflow {
            field: "descriptor.pages.entry_count",
        })?,
        encoded_length: u32::try_from(encoded.len()).map_err(|_| {
            CanonicalError::IntegerOverflow {
                field: "descriptor.pages.encoded_length",
            }
        })?,
        sha256: Sha256::digest(&encoded).to_vec(),
        first_sort_key: keys.first().cloned().unwrap_or_default(),
        last_sort_key: keys.last().cloned().unwrap_or_default(),
    });
    pages.push(page);
    Ok(())
}

fn projection_page(
    projection_id: &[u8; 32],
    section: ProjectionSectionKind,
    page_index: u32,
    payload: cloud_profile_projection_page_v1::Payload,
) -> CloudProfileProjectionPageV1 {
    CloudProfileProjectionPageV1 {
        projection_id: projection_id.to_vec(),
        section: section as i32,
        page_index,
        payload: Some(payload),
    }
}

fn reject_raw_oversized_entries(
    content: &CloudProfileProjectionContentV1,
) -> Result<(), CanonicalError> {
    let placeholder_id = [0u8; 32];
    if let Some(character) = &content.character
        && let Some(entry) = &character.character
    {
        reject_raw_entry(
            &placeholder_id,
            ProjectionSectionKind::Character,
            vec![0x01],
            cloud_profile_projection_page_v1::Payload::Character(CloudCharacterProjectionPageV1 {
                entries: vec![*entry],
            }),
        )?;
    }
    if let Some(inventory) = &content.inventory {
        for entry in &inventory.entries {
            reject_raw_entry(
                &placeholder_id,
                ProjectionSectionKind::Inventory,
                inventory_key(
                    entry.container_kind,
                    entry.container_ordinal,
                    entry.slot_index,
                ),
                cloud_profile_projection_page_v1::Payload::Inventory(
                    CloudInventoryProjectionPageV1 {
                        entries: vec![entry.clone()],
                    },
                ),
            )?;
        }
    }
    if let Some(equipment) = &content.equipment {
        for entry in &equipment.entries {
            reject_raw_entry(
                &placeholder_id,
                ProjectionSectionKind::Equipment,
                equipment_key(entry.slot_kind, entry.slot_index),
                cloud_profile_projection_page_v1::Payload::Equipment(
                    CloudEquipmentProjectionPageV1 {
                        entries: vec![entry.clone()],
                    },
                ),
            )?;
        }
    }
    if let Some(pals) = &content.pals {
        for entry in &pals.entries {
            reject_raw_entry(
                &placeholder_id,
                ProjectionSectionKind::Pals,
                entry.pal_instance_id.clone(),
                cloud_profile_projection_page_v1::Payload::Pals(CloudPalProjectionPageV1 {
                    entries: vec![entry.clone()],
                }),
            )?;
        }
    }
    if let Some(bases) = &content.bases {
        for entry in &bases.entries {
            reject_raw_entry(
                &placeholder_id,
                ProjectionSectionKind::Bases,
                entry.base_id.clone(),
                cloud_profile_projection_page_v1::Payload::Bases(CloudBaseProjectionPageV1 {
                    entries: vec![entry.clone()],
                }),
            )?;
        }
    }
    if let Some(progression) = &content.progression {
        for entry in &progression.entries {
            reject_raw_entry(
                &placeholder_id,
                ProjectionSectionKind::Progression,
                progression_key(entry.kind, &entry.unlock_id),
                cloud_profile_projection_page_v1::Payload::Progression(
                    CloudProgressionProjectionPageV1 {
                        entries: vec![entry.clone()],
                    },
                ),
            )?;
        }
    }
    Ok(())
}

fn reject_raw_entry(
    projection_id: &[u8; 32],
    section: ProjectionSectionKind,
    stable_key: Vec<u8>,
    payload: cloud_profile_projection_page_v1::Payload,
) -> Result<(), CanonicalError> {
    let encoded_length = projection_page(projection_id, section, 0, payload).encoded_len();
    if encoded_length > MAX_PROJECTION_PAGE_ENCODED_LEN {
        Err(CanonicalError::ProjectionEntryTooLarge {
            section,
            stable_key,
            encoded_length,
        })
    } else {
        Ok(())
    }
}

fn required<'a, T>(value: &'a Option<T>, field: &'static str) -> Result<&'a T, CanonicalError> {
    value.as_ref().ok_or(CanonicalError::MissingField { field })
}

fn validate_len(field: &'static str, value: &[u8], expected: usize) -> Result<(), CanonicalError> {
    if value.len() == expected {
        Ok(())
    } else {
        Err(CanonicalError::InvalidLength {
            field,
            expected,
            actual: value.len(),
        })
    }
}

fn validate_text_id(field: &'static str, value: &str) -> Result<(), CanonicalError> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || !value.is_ascii()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        Err(CanonicalError::InvalidTextId { field })
    } else {
        Ok(())
    }
}

fn validate_text_ids(field: &'static str, values: &[String]) -> Result<(), CanonicalError> {
    for value in values {
        validate_text_id(field, value)?;
    }
    Ok(())
}

fn check_count(field: &'static str, actual: usize, maximum: usize) -> Result<(), CanonicalError> {
    if actual <= maximum {
        Ok(())
    } else {
        Err(CanonicalError::TooManyEntries {
            field,
            maximum,
            actual,
        })
    }
}

fn check_content_length(encoded_length: usize) -> Result<(), CanonicalError> {
    if encoded_length <= MAX_CANONICAL_CONTENT_LEN {
        Ok(())
    } else {
        Err(CanonicalError::ContentTooLarge {
            encoded_length,
            maximum: MAX_CANONICAL_CONTENT_LEN,
        })
    }
}

fn reject_duplicate(
    keys: &mut BTreeSet<Vec<u8>>,
    field: &'static str,
    stable_key: Vec<u8>,
) -> Result<(), CanonicalError> {
    if keys.insert(stable_key.clone()) {
        Ok(())
    } else {
        Err(CanonicalError::DuplicateSemanticKey { field, stable_key })
    }
}

fn sort_dedup_strings(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

fn inventory_key(container_kind: i32, container_ordinal: u32, slot_index: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(12);
    key.extend_from_slice(&(container_kind as u32).to_be_bytes());
    key.extend_from_slice(&container_ordinal.to_be_bytes());
    key.extend_from_slice(&slot_index.to_be_bytes());
    key
}

fn equipment_key(slot_kind: i32, slot_index: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(8);
    key.extend_from_slice(&(slot_kind as u32).to_be_bytes());
    key.extend_from_slice(&slot_index.to_be_bytes());
    key
}

fn pal_location_key(
    container_kind: i32,
    container_ordinal: u32,
    slot_index: u32,
    base_id: Option<&[u8]>,
) -> Vec<u8> {
    let mut key = Vec::with_capacity(29);
    key.extend_from_slice(&(container_kind as u32).to_be_bytes());
    key.extend_from_slice(&container_ordinal.to_be_bytes());
    key.extend_from_slice(&slot_index.to_be_bytes());
    match base_id {
        Some(value) => {
            key.push(1);
            key.extend_from_slice(value);
        }
        None => key.push(0),
    }
    key
}

fn progression_key(kind: i32, unlock_id: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(5 + unlock_id.len());
    key.extend_from_slice(&(kind as u32).to_be_bytes());
    key.push(0);
    key.extend_from_slice(unlock_id.as_bytes());
    key
}

fn validate_base_location(
    field: &'static str,
    container_kind: i32,
    base_id: Option<&[u8]>,
) -> Result<(), CanonicalError> {
    if container_kind == PalContainerKind::Base as i32 {
        let base_id = base_id.ok_or(CanonicalError::MissingField { field })?;
        validate_len(field, base_id, 16)
    } else if base_id.is_some() {
        Err(CanonicalError::InvalidValue {
            field,
            reason: "base_id is allowed only for a base-assigned Pal",
        })
    } else {
        Ok(())
    }
}

fn snapshot_completeness(
    field: &'static str,
    value: i32,
) -> Result<SnapshotCompleteness, CanonicalError> {
    nonzero_enum(field, value, SnapshotCompleteness::try_from)
}

fn snapshot_section_kind(
    field: &'static str,
    value: i32,
) -> Result<SnapshotSectionKind, CanonicalError> {
    nonzero_enum(field, value, SnapshotSectionKind::try_from)
}

fn inventory_container_kind(
    field: &'static str,
    value: i32,
) -> Result<InventoryContainerKind, CanonicalError> {
    nonzero_enum(field, value, InventoryContainerKind::try_from)
}

fn equipment_slot_kind(
    field: &'static str,
    value: i32,
) -> Result<EquipmentSlotKind, CanonicalError> {
    nonzero_enum(field, value, EquipmentSlotKind::try_from)
}

fn pal_container_kind(field: &'static str, value: i32) -> Result<PalContainerKind, CanonicalError> {
    nonzero_enum(field, value, PalContainerKind::try_from)
}

fn pal_gender(field: &'static str, value: i32) -> Result<PalGender, CanonicalError> {
    nonzero_enum(field, value, PalGender::try_from)
}

fn projection_completeness(
    field: &'static str,
    value: i32,
) -> Result<ProjectionCompleteness, CanonicalError> {
    nonzero_enum(field, value, ProjectionCompleteness::try_from)
}

fn progression_unlock_kind(
    field: &'static str,
    value: i32,
) -> Result<v2::CloudProgressionUnlockKind, CanonicalError> {
    nonzero_enum(field, value, v2::CloudProgressionUnlockKind::try_from)
}

fn nonzero_enum<T, E, Parse>(
    field: &'static str,
    value: i32,
    parse: Parse,
) -> Result<T, CanonicalError>
where
    T: Copy,
    E: std::fmt::Debug,
    Parse: FnOnce(i32) -> Result<T, E>,
{
    if value == 0 {
        return Err(CanonicalError::InvalidEnum { field, value });
    }
    parse(value).map_err(|_| CanonicalError::InvalidEnum { field, value })
}
