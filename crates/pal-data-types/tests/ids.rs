use pal_data_types::*;

#[test]
fn textual_ids_are_bounded_and_canonical() {
    assert!(SpeciesId::parse("Quivern").is_ok());
    assert_eq!(SpeciesId::parse(""), Err(IdError::Empty));
    assert_eq!(SpeciesId::parse(" Quivern"), Err(IdError::NonCanonical));
    assert_eq!(SpeciesId::parse("Quivern "), Err(IdError::NonCanonical));
    assert_eq!(SpeciesId::parse("Quivérn"), Err(IdError::NonCanonical));
    assert_eq!(
        SpeciesId::parse("Quivern/Alpha"),
        Err(IdError::NonCanonical)
    );
    assert_eq!(SpeciesId::parse("a".repeat(129)), Err(IdError::TooLong));

    let exact_limit = "a".repeat(128);
    assert_eq!(
        SpeciesId::parse(exact_limit.clone()).unwrap().as_str(),
        exact_limit
    );
}

#[test]
fn every_textual_id_uses_the_same_canonical_grammar() {
    macro_rules! assert_text_id_grammar {
        ($($id:ident),+ $(,)?) => {
            $(
                assert!($id::parse("alpha_42-beta.gamma:delta").is_ok(), stringify!($id));
                assert_eq!($id::parse("bad value"), Err(IdError::NonCanonical), stringify!($id));
            )+
        };
    }

    assert_text_id_grammar!(
        GameBuildId,
        WorldId,
        SpeciesId,
        PassiveId,
        SkillId,
        ItemId,
        RecipeId,
        LocationId,
        TechnologyId,
        BuildingId,
        WorkTypeId,
        MapLayerId,
        PoiId,
        FarmMethodId,
        DataSourceId,
        MechanicsModelId,
        SolverVersion,
    );
}

#[test]
fn binary_ids_require_their_exact_widths_and_round_trip_bytes() {
    macro_rules! assert_16_byte_id {
        ($($id:ident),+ $(,)?) => {
            $(
                let value = $id::from_bytes([7; 16]);
                assert_eq!(value.as_bytes(), &[7; 16]);
                assert_eq!($id::from_slice(&[7; 16]).unwrap().as_bytes(), &[7; 16]);
                assert!($id::from_bytes([7; 16]) == value);
                assert!(matches!(
                    $id::from_slice(&[7; 15]),
                    Err(IdError::InvalidLength { expected: 16, actual: 15 })
                ));
            )+
        };
    }
    macro_rules! assert_32_byte_id {
        ($($id:ident),+ $(,)?) => {
            $(
                let value = $id::from_bytes([8; 32]);
                assert_eq!(value.as_bytes(), &[8; 32]);
                assert_eq!($id::from_slice(&[8; 32]).unwrap().as_bytes(), &[8; 32]);
                assert!($id::from_bytes([8; 32]) == value);
                assert!(matches!(
                    $id::from_slice(&[8; 31]),
                    Err(IdError::InvalidLength { expected: 32, actual: 31 })
                ));
            )+
        };
    }

    assert_16_byte_id!(
        PalInstanceId,
        BaseId,
        SourceInstallId,
        ClientInstanceId,
        GameSessionId,
        RequestId,
        ApprovalId,
        PrincipalId,
    );
    assert_32_byte_id!(
        OwnerSubjectId,
        DatasetManifestId,
        PersonalSnapshotId,
        ProjectionId,
        AnalysisId,
        ArtifactId,
        ReleaseManifestId,
        EvidenceRef,
    );
}

#[test]
fn binary_ids_display_as_fixed_width_lowercase_hex() {
    assert_eq!(
        PalInstanceId::from_bytes([0xAB; 16]).to_string(),
        "ab".repeat(16)
    );
    assert_eq!(
        OwnerSubjectId::from_bytes([0xCD; 32]).to_string(),
        "cd".repeat(32)
    );
}

#[test]
fn owner_subject_is_the_only_save_owner_identifier_name() {
    for source in [
        include_str!("../src/ids.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/build.rs"),
    ] {
        assert!(!source.contains("SaveSubjectId"));
    }
}

#[test]
fn binary_identifier_api_does_not_add_unapproved_traits() {
    let source = include_str!("../src/ids.rs");
    let binary_section = source
        .split_once("macro_rules! binary_id")
        .expect("binary ID macro")
        .1;

    for forbidden in ["Clone", "Copy", "Debug", "Hash"] {
        assert!(
            !binary_section.contains(forbidden),
            "unapproved binary ID trait: {forbidden}"
        );
    }
}

#[test]
fn data_types_manifest_has_no_serialization_transport_or_storage_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden in [
        "prost", "tonic", "tokio", "windows", "rusqlite", "reqwest", "serde",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}
