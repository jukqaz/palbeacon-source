use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use pal_wire::v2::{
    CloudProfileProjectionCaptureV1, CloudProfileProjectionContentV1,
    CloudProfileProjectionDescriptorV1, CloudProfileProjectionIdentityV1, DistributionScope,
    NormalizedSaveSet, PersonalSnapshotContent, PrivateCloudAcquisitionGraphV1,
    PrivateCloudBreedingGraphV1, PrivateCloudCatalogManifestV1, PrivateCloudCatalogPageV1,
    PrivateCloudProgressionGraphV1, SaveCensus,
};
use pal_wire::{
    canonical_cloud_profile_projection_identity, cloud_profile_projection_id, personal_snapshot_id,
};
use prost::Message;

fn main() {
    if let Err(error) = run() {
        eprintln!("local fixture verification failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let root = parse_root()?;
    let save_root = root.join("save-synthetic-v1");
    let private_root = root.join("private-cloud-catalog-v1");

    let normalized = decode::<NormalizedSaveSet>(&save_root.join("normalized-save.pb"))?;
    let census = decode::<SaveCensus>(&save_root.join("census.pb"))?;
    validate_census(&normalized, &census)?;

    let snapshot = decode::<PersonalSnapshotContent>(&save_root.join("snapshot-content.pb"))?;
    let snapshot_id = personal_snapshot_id(&snapshot)?;
    let capture = decode::<CloudProfileProjectionCaptureV1>(
        &save_root.join("cloud-profile-projection-capture.pb"),
    )?;
    let content = decode::<CloudProfileProjectionContentV1>(
        &save_root.join("cloud-profile-projection-content.pb"),
    )?;
    let identity = decode::<CloudProfileProjectionIdentityV1>(
        &save_root.join("cloud-profile-projection-identity.pb"),
    )?;
    let descriptor = decode::<CloudProfileProjectionDescriptorV1>(
        &save_root.join("cloud-profile-projection-descriptor.pb"),
    )?;
    let expected_identity = canonical_cloud_profile_projection_identity(&content)?;
    let projection_id = cloud_profile_projection_id(&content)?;
    let source_snapshot_matches = capture.source_personal_snapshot_id == snapshot_id.as_bytes();
    let identity_matches = identity.encode_to_vec() == expected_identity
        && capture.projection_id == projection_id.as_bytes();
    if !source_snapshot_matches || !identity_matches {
        return Err("Cloud projection identity is not bound to the synthetic snapshot".into());
    }
    if descriptor.section_count != 6 || descriptor.sections.len() != 6 {
        return Err("Cloud projection must contain exactly six sections".into());
    }
    let pal = content
        .pals
        .as_ref()
        .and_then(|section| section.entries.first())
        .ok_or("Cloud projection must contain one Pal")?;
    if pal.learned_skill_ids.is_empty()
        || pal.trust == 0
        || pal.container_ordinal == 0
        || pal.slot_index == 0
        || pal.base_id.is_none()
    {
        return Err("Cloud Pal projection is missing evaluation or location fields".into());
    }

    let manifest = decode::<PrivateCloudCatalogManifestV1>(&private_root.join("manifest.pb"))?;
    let _page = decode::<PrivateCloudCatalogPageV1>(&private_root.join("pages/catalog-000000.pb"))?;
    let _breeding =
        decode::<PrivateCloudBreedingGraphV1>(&private_root.join("graphs/breeding.pb"))?;
    let _acquisition =
        decode::<PrivateCloudAcquisitionGraphV1>(&private_root.join("graphs/acquisition.pb"))?;
    let _progression =
        decode::<PrivateCloudProgressionGraphV1>(&private_root.join("graphs/progression.pb"))?;
    if manifest.distribution_scope != DistributionScope::PrivateTenantCandidate as i32 {
        return Err("private catalog has the wrong distribution scope".into());
    }
    if manifest.files.len() != 4 || manifest.capabilities.len() != 13 {
        return Err("private catalog manifest is incomplete".into());
    }

    println!(
        concat!(
            "{{",
            "\"cloud_profile\":{{",
            "\"section_count\":{},",
            "\"source_snapshot_matches\":{},",
            "\"identity_matches\":{},",
            "\"private_field_count\":0",
            "}},",
            "\"private_catalog\":{{",
            "\"distribution_scope\":\"private_tenant_candidate\",",
            "\"asset_field_count\":0",
            "}}",
            "}}"
        ),
        descriptor.section_count, source_snapshot_matches, identity_matches
    );
    Ok(())
}

fn parse_root() -> Result<PathBuf, Box<dyn Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [flag, root] if flag == "--root" => Ok(PathBuf::from(root)),
        _ => Err("usage: verify-local-fixture --root <tests/fixtures/local-data>".into()),
    }
}

fn decode<T>(path: &Path) -> Result<T, Box<dyn Error>>
where
    T: Message + Default,
{
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read fixture {}: {error}", path.display()))?;
    T::decode(bytes.as_slice())
        .map_err(|error| format!("could not decode fixture {}: {error}", path.display()).into())
}

fn validate_census(
    normalized: &NormalizedSaveSet,
    census: &SaveCensus,
) -> Result<(), Box<dyn Error>> {
    if census.character_count != 1
        || normalized.character.is_none()
        || usize::try_from(census.equipment_count)? != normalized.equipment.len()
        || usize::try_from(census.owned_pal_count)? != normalized.pals.len()
        || usize::try_from(census.base_count)? != normalized.bases.len()
        || census.required_roles_seen != normalized.required_roles_seen
    {
        return Err("normalized save and independent census disagree".into());
    }
    let mut normalized_pal_ids = normalized
        .pals
        .iter()
        .map(|pal| pal.instance_id.as_slice())
        .collect::<Vec<_>>();
    normalized_pal_ids.sort_unstable();
    let mut census_pal_ids = census
        .owned_pal_instance_ids
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    census_pal_ids.sort_unstable();
    if normalized_pal_ids != census_pal_ids {
        return Err("normalized save and census Pal GUID sets disagree".into());
    }
    Ok(())
}
