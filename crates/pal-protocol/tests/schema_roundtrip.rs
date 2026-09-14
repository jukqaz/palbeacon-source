use pal_domain::{
    DisplayMode as DomainDisplayMode, FpsProfile as DomainFpsProfile, Freshness as DomainFreshness,
    HotkeyBindings as DomainHotkeyBindings, HotkeyChord as DomainHotkeyChord,
    HotkeyModifiers as DomainHotkeyModifiers, InputMode as DomainInputMode,
    OverlaySettings as DomainOverlaySettings, PoiFilters as DomainPoiFilters, PositionSample,
    RotationMode as DomainRotationMode, SampleClock, WindowSnapshot as DomainWindowSnapshot,
};
use pal_protocol::{
    ConversionValue, DomainConversionError, FrameCodec, decode_freshness, decode_overlay_settings,
    decode_position_sample, decode_window_snapshot, encode_freshness, encode_overlay_settings,
    encode_position_sample, encode_window_snapshot,
    v2::{
        self, ClientHello, CompanionRequest, CompanionResponse, CoreStatusFrame, LocalEnvelope,
        OverlayCommand, OverlayCommandResult, PositionData, PositionFrame, ProtocolError,
        RenderApplied, RenderSnapshotFrame, SettingsAck, SettingsPatch, SettingsQuery,
        SettingsSnapshot, local_envelope,
    },
    validate_local_envelope,
};
use prost::Message;

fn trace_id(seed: u8) -> Vec<u8> {
    vec![seed; 16]
}

fn domain_settings() -> DomainOverlaySettings {
    DomainOverlaySettings {
        enabled: true,
        auto_show: false,
        rotation_mode: DomainRotationMode::HeadingUp,
        input_mode: DomainInputMode::PinnedInteractive,
        display_mode: DomainDisplayMode::ExpandedMap,
        opacity: 0.82,
        diameter_px: 480,
        zoom: 2.25,
        normalized_x: 0.75,
        normalized_y: 0.25,
        fps_profile: DomainFpsProfile::Sixty,
        poi_filters: DomainPoiFilters {
            fast_travel: true,
            boss: false,
            wanted: true,
            dungeon: true,
            enabled_layer_ids: vec!["tower".into(), "map-unlock".into()],
            selected_pal_ids: vec!["SkyDragon".into()],
            night_only: true,
        },
        hotkey_bindings: DomainHotkeyBindings {
            overlay_visibility: Some(DomainHotkeyChord {
                modifiers: DomainHotkeyModifiers {
                    control: true,
                    alt: false,
                    shift: true,
                    windows: false,
                },
                virtual_key: 0x50,
            }),
            rotation_toggle: None,
            temporary_interaction: None,
            interaction_lock: None,
        },
    }
}

fn domain_window() -> DomainWindowSnapshot {
    DomainWindowSnapshot::new(42, -10, 20, 1920, 1080, 144, true, true, false).unwrap()
}

fn domain_position(heading_degrees: Option<f32>) -> PositionSample {
    PositionSample::new(
        "private-world",
        [0x11; 16],
        [0x22; 16],
        3,
        7,
        10.0,
        -20.0,
        2.0,
        heading_degrees,
        SampleClock::received_with_age(10, 1_000),
    )
    .unwrap()
}

fn wire_position(heading_degrees: Option<f32>) -> PositionData {
    encode_position_sample(&domain_position(heading_degrees))
}

fn round_trip_envelope(payload: local_envelope::Payload) -> LocalEnvelope {
    let envelope = LocalEnvelope {
        protocol_version: pal_protocol::PROTOCOL_VERSION,
        connection_id: trace_id(0xA0),
        message_id: 99,
        reply_to_message_id: Some(98),
        payload: Some(payload),
    };
    validate_local_envelope(&envelope).unwrap();

    let frame = FrameCodec::encode(&envelope).unwrap();
    FrameCodec::decode::<LocalEnvelope>(&frame).unwrap()
}

#[test]
fn stable_common_values_round_trip_through_checked_domain_conversions() {
    let settings = domain_settings();
    let settings_wire = encode_overlay_settings(&settings).unwrap();
    assert_eq!(decode_overlay_settings(settings_wire).unwrap(), settings);

    let window = domain_window();
    let window_wire = encode_window_snapshot(&window);
    assert_eq!(decode_window_snapshot(window_wire).unwrap(), window);

    for heading in [Some(359.0), None] {
        let position = domain_position(heading);
        let position_wire = encode_position_sample(&position);
        assert_eq!(decode_position_sample(position_wire).unwrap(), position);
    }

    for freshness in [
        DomainFreshness::Live,
        DomainFreshness::Delayed,
        DomainFreshness::Stale,
        DomainFreshness::Offline,
    ] {
        let wire = encode_freshness(freshness);
        assert_eq!(decode_freshness(wire).unwrap(), freshness);
    }
}

#[test]
fn unspecified_and_unknown_enums_are_rejected_with_field_and_value() {
    let mut settings = encode_overlay_settings(&domain_settings()).unwrap();
    settings.rotation_mode = v2::RotationMode::Unspecified as i32;
    assert_eq!(
        decode_overlay_settings(settings).unwrap_err(),
        DomainConversionError::InvalidEnum {
            field: "rotation_mode",
            value: 0,
        }
    );

    let mut settings = encode_overlay_settings(&domain_settings()).unwrap();
    settings.fps_profile = 99;
    assert_eq!(
        decode_overlay_settings(settings).unwrap_err(),
        DomainConversionError::InvalidEnum {
            field: "fps_profile",
            value: 99,
        }
    );
}

#[test]
fn malformed_common_values_are_rejected_without_defaults() {
    let mut settings = encode_overlay_settings(&domain_settings()).unwrap();
    settings.opacity = f32::NAN;
    let error = decode_overlay_settings(settings).unwrap_err();
    assert_eq!(error.field(), "opacity");
    assert_eq!(error.value(), ConversionValue::F32Bits(f32::NAN.to_bits()));

    let mut settings = encode_overlay_settings(&domain_settings()).unwrap();
    settings
        .hotkey_bindings
        .as_mut()
        .unwrap()
        .overlay_visibility
        .as_mut()
        .unwrap()
        .virtual_key = u16::MAX as u32 + 1;
    assert_eq!(
        decode_overlay_settings(settings).unwrap_err(),
        DomainConversionError::OutOfRangeU32 {
            field: "hotkey_bindings.overlay_visibility.virtual_key",
            value: u16::MAX as u32 + 1,
        }
    );

    let invalid_window = v2::WindowSnapshot {
        process_id: 42,
        client_left: 0,
        client_top: 0,
        client_width: 0,
        client_height: 1080,
        dpi: 96,
        visible: true,
        active: true,
        minimized: false,
    };
    let error = decode_window_snapshot(invalid_window).unwrap_err();
    assert_eq!(error.field(), "client_width");
    assert_eq!(error.value(), ConversionValue::U32(0));

    let mut invalid_position = wire_position(Some(90.0));
    invalid_position.x = f64::INFINITY;
    let error = decode_position_sample(invalid_position).unwrap_err();
    assert_eq!(error.field(), "x");
    assert_eq!(
        error.value(),
        ConversionValue::F64Bits(f64::INFINITY.to_bits())
    );
}

#[test]
fn public_hotkey_wire_contract_has_the_approved_bindings() {
    let bindings = v2::HotkeyBindings {
        overlay_visibility: None,
        rotation_toggle: None,
        temporary_interaction: None,
        interaction_lock: None,
    };

    assert_eq!(bindings.encoded_len(), 0);
}

#[test]
fn invalid_domain_settings_are_not_serialized_to_wire() {
    let mut settings = domain_settings();
    settings.opacity = f32::NAN;

    let error = encode_overlay_settings(&settings).unwrap_err();
    assert_eq!(error.field(), "opacity");
    assert_eq!(error.value(), ConversionValue::F32Bits(f32::NAN.to_bits()));
}

#[test]
fn every_approved_payload_round_trips_inside_the_local_envelope() {
    let settings = encode_overlay_settings(&domain_settings()).unwrap();
    let position = wire_position(Some(359.0));
    let window = encode_window_snapshot(&domain_window());

    let payloads = vec![
        local_envelope::Payload::ClientHello(ClientHello {
            role: v2::ClientRole::ManagementUi as i32,
            process_id: 10,
            minimum_protocol_version: pal_protocol::PROTOCOL_VERSION,
            maximum_protocol_version: pal_protocol::PROTOCOL_VERSION,
        }),
        local_envelope::Payload::SettingsPatch(SettingsPatch {
            trace_id: trace_id(1),
            expected_settings_version: 7,
            candidate: Some(settings.clone()),
            changed_fields: vec![v2::OverlaySettingField::Opacity as i32],
        }),
        local_envelope::Payload::SettingsAck(SettingsAck {
            trace_id: trace_id(2),
            status: v2::SettingsApplyStatus::Applied as i32,
            applied_version: 8,
            effective_settings: Some(settings.clone()),
            hotkey_conflicts: Vec::new(),
        }),
        local_envelope::Payload::CoreStatus(CoreStatusFrame {
            startup_stage: v2::StartupStage::Live as i32,
            game_connected: true,
            window_connected: true,
            overlay_connected: true,
            position_connected: true,
            freshness: v2::Freshness::Live as i32,
            heading_available: true,
            position_source: v2::PositionSource::ServerAgent as i32,
            client_build_id: "24181527".into(),
            map_build_id: "24181527".into(),
            map_pack_hash: vec![3; 32],
            agent_server_fingerprint: vec![4; 32],
            agent_server_version: "1.0".into(),
            agent_gate_profile_hash: vec![5; 32],
            last_source_boot_id: vec![6; 16],
            last_source_sequence: 41,
            gate_error: None,
        }),
        local_envelope::Payload::Position(PositionFrame {
            trace_id: trace_id(3),
            position: Some(position.clone()),
            freshness: v2::Freshness::Live as i32,
        }),
        local_envelope::Payload::RenderSnapshot(RenderSnapshotFrame {
            trace_id: trace_id(4),
            source_boot_id: vec![6; 16],
            source_sequence: 41,
            settings_version: 8,
            window: Some(window),
            settings: Some(settings.clone()),
            position: Some(position),
            freshness: v2::Freshness::Live as i32,
        }),
        local_envelope::Payload::OverlayCommand(OverlayCommand {
            trace_id: trace_id(5),
            command: v2::OverlayCommandKind::OpenExpanded as i32,
            expected_settings_version: 8,
        }),
        local_envelope::Payload::OverlayCommandResult(OverlayCommandResult {
            trace_id: trace_id(6),
            status: v2::OverlayCommandStatus::Applied as i32,
            visible: true,
            input_mode: v2::InputMode::Locked as i32,
            display_mode: v2::DisplayMode::ExpandedMap as i32,
            settings_version: 8,
            error_code: v2::ProtocolErrorCode::None as i32,
            effective_settings: Some(settings.clone()),
        }),
        local_envelope::Payload::RenderApplied(RenderApplied {
            trace_id: trace_id(7),
            source_boot_id: vec![6; 16],
            sequence: 41,
            settings_version: 8,
            first_presented_local_qpc: 1_234_567,
        }),
        local_envelope::Payload::Error(ProtocolError {
            code: v2::ProtocolErrorCode::ValidationFailed as i32,
            field: "opacity".into(),
            offending_value: "NaN".into(),
        }),
        local_envelope::Payload::SettingsQuery(SettingsQuery {
            trace_id: trace_id(8),
        }),
        local_envelope::Payload::SettingsSnapshot(SettingsSnapshot {
            trace_id: trace_id(9),
            settings_version: 8,
            settings: Some(settings),
        }),
        local_envelope::Payload::MapViewIntent(v2::MapViewIntentV1 {
            trace_id: trace_id(10),
            expected_revision: 1,
            manifest_sha256: vec![0x42; 32],
            action: Some(v2::map_view_intent_v1::Action::RequestResync(
                v2::MapRequestResyncV1 {},
            )),
        }),
        local_envelope::Payload::MapViewUpdate(v2::MapViewStateUpdateV1 {
            trace_id: trace_id(11),
            status: v2::MapViewUpdateStatusV1::NoOp as i32,
            snapshot: Some(v2::MapViewStateSnapshotV1 {
                revision: 1,
                manifest_sha256: vec![0x42; 32],
                map_space: v2::MapSpaceV1::Palpagos as i32,
                enabled_layer_ids: vec!["boss".to_owned()],
                selected_poi_id: None,
                camera: Some(v2::MapCameraV1 {
                    center_x: 1_024.0,
                    center_y: 1_024.0,
                    zoom: 1.0,
                }),
            }),
            rejected_field: None,
        }),
    ];

    for payload in payloads {
        let expected = payload.clone();
        assert_eq!(round_trip_envelope(payload).payload, Some(expected));
    }
}

#[test]
fn position_frame_preserves_present_and_absent_heading_distinctly() {
    for heading in [Some(359.0), None] {
        let frame = PositionFrame {
            trace_id: trace_id(8),
            position: Some(wire_position(heading)),
            freshness: v2::Freshness::Live as i32,
        };
        let decoded = PositionFrame::decode(frame.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.position.unwrap().heading_degrees, heading);
    }
}

#[test]
fn envelope_and_trace_identifiers_must_be_exactly_sixteen_bytes() {
    let mut envelope =
        round_trip_envelope(local_envelope::Payload::OverlayCommand(OverlayCommand {
            trace_id: trace_id(9),
            command: v2::OverlayCommandKind::Show as i32,
            expected_settings_version: 1,
        }));
    envelope.connection_id.pop();
    assert_eq!(
        validate_local_envelope(&envelope).unwrap_err().field(),
        "connection_id"
    );

    envelope.connection_id = trace_id(10);
    if let Some(local_envelope::Payload::OverlayCommand(command)) = envelope.payload.as_mut() {
        command.trace_id.pop();
    }
    assert_eq!(
        validate_local_envelope(&envelope).unwrap_err().field(),
        "overlay_command.trace_id"
    );
}

#[test]
fn local_envelope_payload_tags_are_exactly_twenty_through_twenty_nine() {
    let payloads = [
        local_envelope::Payload::ClientHello(ClientHello::default()),
        local_envelope::Payload::SettingsPatch(SettingsPatch::default()),
        local_envelope::Payload::SettingsAck(SettingsAck::default()),
        local_envelope::Payload::CoreStatus(CoreStatusFrame::default()),
        local_envelope::Payload::Position(PositionFrame::default()),
        local_envelope::Payload::RenderSnapshot(RenderSnapshotFrame::default()),
        local_envelope::Payload::OverlayCommand(OverlayCommand::default()),
        local_envelope::Payload::OverlayCommandResult(OverlayCommandResult::default()),
        local_envelope::Payload::RenderApplied(RenderApplied::default()),
        local_envelope::Payload::Error(ProtocolError::default()),
    ];

    for (field_number, payload) in (20_u32..=29).zip(payloads) {
        let encoded = LocalEnvelope {
            payload: Some(payload),
            ..LocalEnvelope::default()
        }
        .encode_to_vec();
        assert_eq!(encoded, embedded_empty_message(field_number));
    }
}

#[test]
fn product_payload_tags_append_after_the_existing_contract() {
    let request = local_envelope::Payload::ProductRequest(CompanionRequest::default());
    let response = local_envelope::Payload::ProductResponse(CompanionResponse::default());

    assert_eq!(
        LocalEnvelope {
            payload: Some(request),
            ..LocalEnvelope::default()
        }
        .encode_to_vec(),
        embedded_empty_message(30)
    );
    assert_eq!(
        LocalEnvelope {
            payload: Some(response),
            ..LocalEnvelope::default()
        }
        .encode_to_vec(),
        embedded_empty_message(31)
    );
}

#[test]
fn settings_read_payload_tags_append_after_product_queries() {
    let query = local_envelope::Payload::SettingsQuery(SettingsQuery::default());
    let snapshot = local_envelope::Payload::SettingsSnapshot(SettingsSnapshot::default());

    assert_eq!(
        LocalEnvelope {
            payload: Some(query),
            ..LocalEnvelope::default()
        }
        .encode_to_vec(),
        embedded_empty_message(32)
    );
    assert_eq!(
        LocalEnvelope {
            payload: Some(snapshot),
            ..LocalEnvelope::default()
        }
        .encode_to_vec(),
        embedded_empty_message(33)
    );
}

#[test]
fn map_state_payload_tags_append_after_settings_reads() {
    let intent = local_envelope::Payload::MapViewIntent(v2::MapViewIntentV1::default());
    let update = local_envelope::Payload::MapViewUpdate(v2::MapViewStateUpdateV1::default());

    assert_eq!(
        LocalEnvelope {
            payload: Some(intent),
            ..LocalEnvelope::default()
        }
        .encode_to_vec(),
        embedded_empty_message(34)
    );
    assert_eq!(
        LocalEnvelope {
            payload: Some(update),
            ..LocalEnvelope::default()
        }
        .encode_to_vec(),
        embedded_empty_message(35)
    );
}

#[test]
fn companion_query_tags_cover_the_exact_nine_tool_allowlist() {
    let queries = [
        v2::companion_request::Query::ListOwnedPals(v2::ListOwnedPalsRequest::default()),
        v2::companion_request::Query::GetOwnedPal(v2::GetOwnedPalRequest::default()),
        v2::companion_request::Query::FindOwnedBreedingPlan(
            v2::FindOwnedBreedingPlanRequest::default(),
        ),
        v2::companion_request::Query::SearchCatalog(v2::SearchCatalogRequest::default()),
        v2::companion_request::Query::CompareFarmingMethods(
            v2::CompareFarmingMethodsRequest::default(),
        ),
        v2::companion_request::Query::AnalyzeBase(v2::AnalyzeBaseRequest::default()),
        v2::companion_request::Query::AnalyzeProgression(v2::AnalyzeProgressionRequest::default()),
        v2::companion_request::Query::CompareOwnedMounts(v2::CompareOwnedMountsRequest::default()),
        v2::companion_request::Query::EvaluateOwnedPal(v2::EvaluateOwnedPalRequest::default()),
    ];

    for (field_number, query) in (10_u32..=18).zip(queries) {
        assert_eq!(
            CompanionRequest {
                query: Some(query),
                ..CompanionRequest::default()
            }
            .encode_to_vec(),
            embedded_empty_message(field_number)
        );
    }
}

#[test]
fn save_ingest_and_census_contracts_round_trip_without_paths_or_raw_bytes() {
    let normalized = v2::NormalizedSaveSet {
        schema_major: 1,
        game_build_id: "steam:24181527".to_owned(),
        source_install_id: vec![0x11; 16],
        world_id: "fixture-world".to_owned(),
        owner_subject_id: vec![0x22; 32],
        required_roles_seen: vec![v2::SaveSourceRole::Level as i32],
        ..v2::NormalizedSaveSet::default()
    };
    let census = v2::SaveCensus {
        schema_major: 1,
        character_count: 1,
        owned_pal_count: 1,
        owned_pal_instance_ids: vec![vec![0x33; 16]],
        required_roles_seen: vec![v2::SaveSourceRole::Level as i32],
        ..v2::SaveCensus::default()
    };

    assert_eq!(
        v2::NormalizedSaveSet::decode(normalized.encode_to_vec().as_slice()).unwrap(),
        normalized
    );
    assert_eq!(
        v2::SaveCensus::decode(census.encode_to_vec().as_slice()).unwrap(),
        census
    );
}

#[test]
fn acquisition_contract_keeps_item_and_pal_targets_distinct() {
    let item = v2::AcquisitionTargetV1 {
        target: Some(v2::acquisition_target_v1::Target::ItemId(
            "PalOil".to_owned(),
        )),
    };
    let pal = v2::AcquisitionTargetV1 {
        target: Some(v2::acquisition_target_v1::Target::SpeciesId(
            "FixturePal".to_owned(),
        )),
    };

    assert!(matches!(
        item.target,
        Some(v2::acquisition_target_v1::Target::ItemId(_))
    ));
    assert!(matches!(
        pal.target,
        Some(v2::acquisition_target_v1::Target::SpeciesId(_))
    ));
}

#[test]
fn owned_pal_contract_retains_local_evaluation_and_location_fields() {
    let pal = v2::OwnedPal {
        learned_skill_ids: vec!["AirBlade".to_owned()],
        soul_hp: 1,
        soul_attack: 2,
        soul_defense: 3,
        soul_work_speed: 4,
        trust: 77,
        is_lucky: true,
        is_boss: true,
        is_alpha: true,
        is_awakened: true,
        is_mutated: true,
        slot_index: 5,
        base_id: Some(vec![0x44; 16]),
        ..v2::OwnedPal::default()
    };
    let decoded = v2::OwnedPal::decode(pal.encode_to_vec().as_slice()).unwrap();

    assert_eq!(decoded.learned_skill_ids, ["AirBlade"]);
    assert_eq!(decoded.trust, 77);
    assert_eq!(decoded.slot_index, 5);
    assert_eq!(decoded.base_id, Some(vec![0x44; 16]));
}

#[test]
fn product_payloads_round_trip_and_validate_trace_ids() {
    let mut request = CompanionRequest {
        trace_id: trace_id(10),
        ..CompanionRequest::default()
    };
    let mut response = CompanionResponse {
        trace_id: trace_id(11),
        ..CompanionResponse::default()
    };

    for payload in [
        local_envelope::Payload::ProductRequest(request.clone()),
        local_envelope::Payload::ProductResponse(response.clone()),
    ] {
        let expected = payload.clone();
        assert_eq!(round_trip_envelope(payload).payload, Some(expected));
    }

    request.trace_id.pop();
    let envelope = LocalEnvelope {
        connection_id: trace_id(12),
        payload: Some(local_envelope::Payload::ProductRequest(request)),
        ..LocalEnvelope::default()
    };
    let error = validate_local_envelope(&envelope).unwrap_err();
    assert_eq!(error.field(), "product_request.trace_id");

    response.trace_id.pop();
    let envelope = LocalEnvelope {
        connection_id: trace_id(13),
        payload: Some(local_envelope::Payload::ProductResponse(response)),
        ..LocalEnvelope::default()
    };
    let error = validate_local_envelope(&envelope).unwrap_err();
    assert_eq!(error.field(), "product_response.trace_id");
}

#[test]
fn tonic_telemetry_service_surface_remains_available() {
    #[allow(dead_code)]
    fn requires_service<T: v2::position_telemetry_service_server::PositionTelemetryService>() {
        let _ = std::marker::PhantomData::<T>;
    }

    let client = None::<
        v2::position_telemetry_service_client::PositionTelemetryServiceClient<
            tonic::transport::Channel,
        >,
    >;
    assert!(client.is_none());
}

fn embedded_empty_message(field_number: u32) -> Vec<u8> {
    let mut key = (field_number << 3) | 2;
    let mut result = Vec::new();
    while key >= 0x80 {
        result.push((key as u8) | 0x80);
        key >>= 7;
    }
    result.push(key as u8);
    result.push(0);
    result
}

#[test]
fn every_wire_enum_reserves_zero_for_unspecified() {
    assert_eq!(v2::RotationMode::Unspecified as i32, 0);
    assert_eq!(v2::InputMode::Unspecified as i32, 0);
    assert_eq!(v2::DisplayMode::Unspecified as i32, 0);
    assert_eq!(v2::FpsProfile::Unspecified as i32, 0);
    assert_eq!(v2::Freshness::Unspecified as i32, 0);
    assert_eq!(v2::ClientRole::Unspecified as i32, 0);
    assert_eq!(v2::OverlaySettingField::Unspecified as i32, 0);
    assert_eq!(v2::SettingsApplyStatus::Unspecified as i32, 0);
    assert_eq!(v2::HotkeyAction::Unspecified as i32, 0);
    assert_eq!(v2::StartupStage::Unspecified as i32, 0);
    assert_eq!(v2::PositionSource::Unspecified as i32, 0);
    assert_eq!(v2::GateErrorCode::Unspecified as i32, 0);
    assert_eq!(v2::OverlayCommandKind::Unspecified as i32, 0);
    assert_eq!(v2::OverlayCommandStatus::Unspecified as i32, 0);
    assert_eq!(v2::ProtocolErrorCode::Unspecified as i32, 0);
}

#[test]
fn command_hotkey_and_setting_field_enums_are_exhaustive_contracts() {
    assert_eq!(
        overlay_command_number(v2::OverlayCommandKind::ToggleSalvage),
        22
    );
    assert_eq!(hotkey_action_number(v2::HotkeyAction::InteractionLock), 4);
    assert_eq!(
        setting_field_number(v2::OverlaySettingField::HotkeyBindings),
        13
    );
}

fn overlay_command_number(value: v2::OverlayCommandKind) -> i32 {
    match value {
        v2::OverlayCommandKind::Unspecified => 0,
        v2::OverlayCommandKind::Start => 1,
        v2::OverlayCommandKind::Stop => 2,
        v2::OverlayCommandKind::Restart => 3,
        v2::OverlayCommandKind::Show => 4,
        v2::OverlayCommandKind::Hide => 5,
        v2::OverlayCommandKind::EnterInteractive => 6,
        v2::OverlayCommandKind::ExitInteractive => 7,
        v2::OverlayCommandKind::OpenExpanded => 8,
        v2::OverlayCommandKind::CloseExpanded => 9,
        v2::OverlayCommandKind::ZoomIn => 10,
        v2::OverlayCommandKind::ZoomOut => 11,
        v2::OverlayCommandKind::ToggleRotation => 12,
        v2::OverlayCommandKind::Lock => 13,
        v2::OverlayCommandKind::ToggleVisibility => 14,
        v2::OverlayCommandKind::ToggleFastTravel => 15,
        v2::OverlayCommandKind::ToggleBoss => 16,
        v2::OverlayCommandKind::ToggleWanted => 17,
        v2::OverlayCommandKind::ToggleDungeon => 18,
        v2::OverlayCommandKind::ToggleTower => 19,
        v2::OverlayCommandKind::ToggleEgg => 20,
        v2::OverlayCommandKind::ToggleResources => 21,
        v2::OverlayCommandKind::ToggleSalvage => 22,
    }
}

fn hotkey_action_number(value: v2::HotkeyAction) -> i32 {
    match value {
        v2::HotkeyAction::Unspecified => 0,
        v2::HotkeyAction::OverlayVisibility => 1,
        v2::HotkeyAction::RotationToggle => 2,
        v2::HotkeyAction::TemporaryInteraction => 3,
        v2::HotkeyAction::InteractionLock => 4,
    }
}

fn setting_field_number(value: v2::OverlaySettingField) -> i32 {
    match value {
        v2::OverlaySettingField::Unspecified => 0,
        v2::OverlaySettingField::Enabled => 1,
        v2::OverlaySettingField::AutoShow => 2,
        v2::OverlaySettingField::RotationMode => 3,
        v2::OverlaySettingField::InputMode => 4,
        v2::OverlaySettingField::DisplayMode => 5,
        v2::OverlaySettingField::Opacity => 6,
        v2::OverlaySettingField::DiameterPx => 7,
        v2::OverlaySettingField::Zoom => 8,
        v2::OverlaySettingField::NormalizedX => 9,
        v2::OverlaySettingField::NormalizedY => 10,
        v2::OverlaySettingField::FpsProfile => 11,
        v2::OverlaySettingField::PoiFilters => 12,
        v2::OverlaySettingField::HotkeyBindings => 13,
    }
}
