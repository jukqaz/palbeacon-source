#![cfg(feature = "test-harness")]

use std::time::Duration;

use pal_domain::PoiFilters;
use pal_map_pack_store::PoiKind;
use pal_overlay_win::synthetic_preview::{
    BOSS_COLOR, DUNGEON_COLOR, FAST_TRAVEL_COLOR, PLAYER_COLOR, PREVIEW_DIAMETER,
    SYNTHETIC_HEADING_DEGREES, SyntheticCliError, SyntheticPoiSymbol, SyntheticPreviewFrame,
    SyntheticPreviewOptions, SyntheticSurface,
};
use pal_render::RenderCommand;

#[test]
fn cli_selects_synthetic_preview_with_a_bounded_duration() {
    let options = SyntheticPreviewOptions::parse(["--synthetic-map", "--duration-seconds", "7"])
        .expect("valid preview options");

    assert!(options.synthetic_map());
    assert_eq!(options.duration(), Duration::from_secs(7));
    assert_eq!(
        SyntheticPreviewOptions::parse(["--duration-seconds", "0"]),
        Err(SyntheticCliError::DurationOutOfRange)
    );
    assert_eq!(
        SyntheticPreviewOptions::parse(["--unknown"]),
        Err(SyntheticCliError::UnknownArgument)
    );
}

#[test]
fn cli_selects_a_local_real_map_and_rejects_conflicting_map_modes() {
    assert_eq!(
        SyntheticPreviewOptions::parse([
            "--real-map-bmp",
            "C:\\maps\\palworld.bmp",
            "--duration-seconds",
            "7",
        ]),
        Err(SyntheticCliError::RealMapRequiresExplicitTestLandmark)
    );

    let options = SyntheticPreviewOptions::parse([
        "--real-map-bmp",
        "C:\\maps\\palworld.bmp",
        "--allow-fixed-test-landmark",
        "--duration-seconds",
        "7",
    ])
    .expect("valid real-map preview options");

    assert_eq!(
        options.real_map_bmp().map(|path| path.to_string_lossy()),
        Some("C:\\maps\\palworld.bmp".into())
    );
    assert_eq!(
        SyntheticPreviewOptions::parse([
            "--synthetic-map",
            "--real-map-bmp",
            "C:\\maps\\palworld.bmp",
        ]),
        Err(SyntheticCliError::ConflictingMapModes)
    );
    assert_eq!(
        SyntheticPreviewOptions::parse(["--real-map-bmp"]),
        Err(SyntheticCliError::MissingMapPath)
    );
}

#[test]
fn replay_cli_requires_a_real_map_and_accepts_an_explicit_rotation_mode() {
    assert_eq!(
        SyntheticPreviewOptions::parse(["--position-replay", "route.json"]),
        Err(SyntheticCliError::ReplayRequiresRealMap)
    );
    assert_eq!(
        SyntheticPreviewOptions::parse([
            "--real-map-bmp",
            "map.bmp",
            "--position-replay",
            "route.json",
        ]),
        Err(SyntheticCliError::ReplayRequiresDevelopmentOptIn)
    );

    let options = SyntheticPreviewOptions::parse([
        "--real-map-bmp",
        "map.bmp",
        "--position-replay",
        "route.json",
        "--allow-development-replay",
        "--rotation-mode",
        "heading-up",
    ])
    .expect("valid replay preview options");

    assert_eq!(
        options.position_replay().map(|path| path.to_string_lossy()),
        Some("route.json".into())
    );
    assert_eq!(options.rotation_mode(), pal_domain::RotationMode::HeadingUp);
    assert_eq!(
        SyntheticPreviewOptions::parse([
            "--real-map-bmp",
            "map.bmp",
            "--rotation-mode",
            "sideways",
        ]),
        Err(SyntheticCliError::RotationModeInvalid)
    );
}

#[test]
fn fixture_snapshot_drives_heading_rotation_and_exact_poi_symbols() {
    let frame = SyntheticPreviewFrame::from_fixture(PoiFilters::default())
        .expect("validated synthetic fixture");

    assert_eq!(frame.map_rotation_degrees(), -SYNTHETIC_HEADING_DEGREES);
    assert_eq!(
        frame
            .commands()
            .iter()
            .filter(|command| matches!(command, RenderCommand::MapTransform(_)))
            .count(),
        1
    );
    assert_eq!(
        frame
            .commands()
            .iter()
            .filter(|command| matches!(command, RenderCommand::PlayerArrow(_)))
            .count(),
        1
    );
    assert_eq!(
        frame
            .projected_pois()
            .iter()
            .map(|poi| (poi.kind(), poi.symbol()))
            .collect::<Vec<_>>(),
        vec![
            (PoiKind::FastTravel, SyntheticPoiSymbol::FastTravelDiamond),
            (PoiKind::Boss, SyntheticPoiSymbol::BossRing),
            (PoiKind::Dungeon, SyntheticPoiSymbol::DungeonGate),
        ]
    );

    let boss = frame
        .projected_pois()
        .iter()
        .find(|poi| poi.kind() == PoiKind::Boss)
        .unwrap();
    assert!((boss.x() - 210.0).abs() < 0.01);
    assert!((boss.y() - 210.0).abs() < 0.01);

    let fast = frame
        .projected_pois()
        .iter()
        .find(|poi| poi.kind() == PoiKind::FastTravel)
        .unwrap();
    assert!((fast.x() - 210.198).abs() < 0.01);
    assert!((fast.y() - 210.028).abs() < 0.01);
}

#[test]
fn fixture_filters_control_exact_projected_pois() {
    let cases = [
        (
            PoiFilters {
                fast_travel: false,
                boss: false,
                wanted: false,
                dungeon: false,
                ..PoiFilters::default()
            },
            vec![],
        ),
        (
            PoiFilters {
                fast_travel: true,
                boss: false,
                wanted: false,
                dungeon: false,
                ..PoiFilters::default()
            },
            vec![PoiKind::FastTravel],
        ),
        (
            PoiFilters {
                fast_travel: false,
                boss: true,
                wanted: false,
                dungeon: false,
                ..PoiFilters::default()
            },
            vec![PoiKind::Boss],
        ),
        (
            PoiFilters {
                fast_travel: false,
                boss: false,
                wanted: false,
                dungeon: true,
                ..PoiFilters::default()
            },
            vec![PoiKind::Dungeon],
        ),
    ];

    for (filters, expected) in cases {
        let frame = SyntheticPreviewFrame::from_fixture(filters).unwrap();
        let actual = frame
            .projected_pois()
            .iter()
            .map(|poi| poi.kind())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}

#[test]
fn raster_fills_rectangular_terrain_and_draws_all_required_symbols() {
    let frame = SyntheticPreviewFrame::from_fixture(PoiFilters::default()).unwrap();
    let mut surface = SyntheticSurface::new(PREVIEW_DIAMETER).unwrap();

    surface.rasterize(&frame);

    assert_ne!(surface.pixel(0, 0), Some(0));
    assert_ne!(
        surface.pixel(PREVIEW_DIAMETER - 1, PREVIEW_DIAMETER - 1),
        Some(0)
    );
    assert_ne!(
        surface.pixel(PREVIEW_DIAMETER / 2, PREVIEW_DIAMETER / 2),
        Some(0)
    );
    for color in [FAST_TRAVEL_COLOR, BOSS_COLOR, DUNGEON_COLOR, PLAYER_COLOR] {
        assert!(
            surface.pixels().contains(&color),
            "required symbol color {color:#08x} is missing"
        );
    }
    assert!(surface.watermark_drawn());
}
