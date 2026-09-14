use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use pal_domain::{
    CoreState, CoreStateParts, DisplayMode, ExpandedMapView, Freshness, MiniMapView,
    OverlaySettings, PoiFilters, PositionSample, RotationMode, SampleClock,
};
use pal_map_pack_store::{MapPackStore, PoiKind, WorldPoint};
use pal_render::{
    ActiveViewport, RenderCommand, RenderCommandBuffer, SnapshotBuilder, ViewportLayout,
    ViewportMetrics,
};

pub const PREVIEW_DIAMETER: u32 = 420;
pub const SYNTHETIC_HEADING_DEGREES: f32 = 37.0;
pub const FAST_TRAVEL_COLOR: u32 = 0x0000_d9ff;
pub const BOSS_COLOR: u32 = 0x00ff_4d4d;
pub const DUNGEON_COLOR: u32 = 0x00b3_5cff;
pub const PLAYER_COLOR: u32 = 0x00ff_ffff;

const BUILD_ID: &str = "24181527";
const TERRAIN_WATER: u32 = 0x0015_232a;
const TERRAIN_LOWLAND: u32 = 0x0026_352d;
const TERRAIN_UPLAND: u32 = 0x0037_4134;
const TERRAIN_RIDGE: u32 = 0x0047_4a3b;
const MAX_DURATION_SECONDS: u64 = 3_600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntheticCliError {
    MissingDuration,
    MissingMapPath,
    MissingReplayPath,
    MissingRotationMode,
    DurationInvalid,
    DurationOutOfRange,
    ConflictingMapModes,
    RealMapRequiresExplicitTestLandmark,
    ReplayRequiresRealMap,
    ReplayRequiresDevelopmentOptIn,
    RotationModeInvalid,
    UnknownArgument,
}

impl fmt::Display for SyntheticCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingDuration => "--duration-seconds requires a value",
            Self::MissingMapPath => "--real-map-bmp requires a path",
            Self::MissingReplayPath => "--position-replay requires a path",
            Self::MissingRotationMode => "--rotation-mode requires a value",
            Self::DurationInvalid => "--duration-seconds must be an integer",
            Self::DurationOutOfRange => "--duration-seconds must be between 1 and 3600 seconds",
            Self::ConflictingMapModes => {
                "--synthetic-map and --real-map-bmp cannot be used together"
            }
            Self::RealMapRequiresExplicitTestLandmark => {
                "a fixed real-map test landmark requires --allow-fixed-test-landmark"
            }
            Self::ReplayRequiresRealMap => "--position-replay requires --real-map-bmp",
            Self::ReplayRequiresDevelopmentOptIn => {
                "--position-replay is simulated data and requires --allow-development-replay"
            }
            Self::RotationModeInvalid => "--rotation-mode must be north-up or heading-up",
            Self::UnknownArgument => "unknown preview argument",
        };
        formatter.write_str(message)
    }
}

impl Error for SyntheticCliError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntheticPreviewOptions {
    synthetic_map: bool,
    real_map_bmp: Option<PathBuf>,
    position_replay: Option<PathBuf>,
    rotation_mode: RotationMode,
    duration: Option<Duration>,
}

impl SyntheticPreviewOptions {
    pub fn parse<I, S>(arguments: I) -> Result<Self, SyntheticCliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut synthetic_map = false;
        let mut real_map_bmp = None;
        let mut position_replay = None;
        let mut allow_fixed_test_landmark = false;
        let mut allow_development_replay = false;
        let mut rotation_mode = RotationMode::NorthUp;
        let mut duration = None;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_ref() {
                "--synthetic-map" => synthetic_map = true,
                "--real-map-bmp" => {
                    let raw = arguments.next().ok_or(SyntheticCliError::MissingMapPath)?;
                    real_map_bmp = Some(PathBuf::from(raw.as_ref()));
                }
                "--position-replay" => {
                    let raw = arguments
                        .next()
                        .ok_or(SyntheticCliError::MissingReplayPath)?;
                    position_replay = Some(PathBuf::from(raw.as_ref()));
                }
                "--allow-fixed-test-landmark" => allow_fixed_test_landmark = true,
                "--allow-development-replay" => allow_development_replay = true,
                "--rotation-mode" => {
                    let raw = arguments
                        .next()
                        .ok_or(SyntheticCliError::MissingRotationMode)?;
                    rotation_mode = match raw.as_ref() {
                        "north-up" => RotationMode::NorthUp,
                        "heading-up" => RotationMode::HeadingUp,
                        _ => return Err(SyntheticCliError::RotationModeInvalid),
                    };
                }
                "--duration-seconds" => {
                    let raw = arguments.next().ok_or(SyntheticCliError::MissingDuration)?;
                    let seconds = raw
                        .as_ref()
                        .parse::<u64>()
                        .map_err(|_| SyntheticCliError::DurationInvalid)?;
                    if !(1..=MAX_DURATION_SECONDS).contains(&seconds) {
                        return Err(SyntheticCliError::DurationOutOfRange);
                    }
                    duration = Some(Duration::from_secs(seconds));
                }
                _ => return Err(SyntheticCliError::UnknownArgument),
            }
        }
        if synthetic_map && real_map_bmp.is_some() {
            return Err(SyntheticCliError::ConflictingMapModes);
        }
        if position_replay.is_some() && real_map_bmp.is_none() {
            return Err(SyntheticCliError::ReplayRequiresRealMap);
        }
        if real_map_bmp.is_some() && position_replay.is_none() && !allow_fixed_test_landmark {
            return Err(SyntheticCliError::RealMapRequiresExplicitTestLandmark);
        }
        if position_replay.is_some() && !allow_development_replay {
            return Err(SyntheticCliError::ReplayRequiresDevelopmentOptIn);
        }
        Ok(Self {
            synthetic_map,
            real_map_bmp,
            position_replay,
            rotation_mode,
            duration,
        })
    }

    pub const fn synthetic_map(&self) -> bool {
        self.synthetic_map
    }

    pub fn real_map_bmp(&self) -> Option<&std::path::Path> {
        self.real_map_bmp.as_deref()
    }

    pub fn position_replay(&self) -> Option<&std::path::Path> {
        self.position_replay.as_deref()
    }

    pub const fn rotation_mode(&self) -> RotationMode {
        self.rotation_mode
    }

    pub fn duration(&self) -> Duration {
        self.duration.unwrap_or(Duration::MAX)
    }

    pub const fn optional_duration(&self) -> Option<Duration> {
        self.duration
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntheticPoiSymbol {
    FastTravelDiamond,
    BossRing,
    WantedReticle,
    DungeonGate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedSyntheticPoi {
    kind: PoiKind,
    symbol: SyntheticPoiSymbol,
    x: f64,
    y: f64,
}

impl ProjectedSyntheticPoi {
    pub const fn kind(self) -> PoiKind {
        self.kind
    }

    pub const fn symbol(self) -> SyntheticPoiSymbol {
        self.symbol
    }

    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }
}

#[derive(Debug)]
pub struct SyntheticPreviewFrame {
    commands: Vec<RenderCommand>,
    projected_pois: Vec<ProjectedSyntheticPoi>,
    map_rotation_degrees: f32,
}

impl SyntheticPreviewFrame {
    pub fn from_fixture(
        filters: PoiFilters,
    ) -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        let pack = MapPackStore::open(fixture_root(), BUILD_ID)?;
        let sample = PositionSample::new(
            "synthetic-world",
            b"synthetic-subject",
            b"synthetic-boot",
            1,
            1,
            400.0,
            -400.0,
            0.0,
            Some(SYNTHETIC_HEADING_DEGREES),
            SampleClock::received_with_age(0, 1_000),
        )?;
        let player_map = pack
            .select_region_pack(sample.x(), sample.y())?
            .transform()
            .project(WorldPoint::new(sample.x(), sample.y())?)?;
        let state = CoreState::from_parts(CoreStateParts {
            settings: OverlaySettings {
                enabled: true,
                display_mode: DisplayMode::ExpandedMap,
                rotation_mode: RotationMode::HeadingUp,
                poi_filters: filters,
                ..OverlaySettings::default()
            },
            settings_version: 1,
            window_snapshot: None,
            position_sample: Some(sample),
            freshness: Freshness::Live,
            heading_available: true,
            connected: true,
            visible: true,
            interpolate_position: true,
            mini_map_view: MiniMapView::new(player_map.x(), player_map.y(), 1.0)?,
            expanded_map_view: ExpandedMapView::new(player_map.x(), player_map.y(), 1.0)?,
        })?;
        let metrics = ViewportMetrics::new(PREVIEW_DIAMETER, PREVIEW_DIAMETER, 2.0)?;
        let snapshot =
            SnapshotBuilder::new(pack).build(&state, ViewportLayout::new(metrics, metrics))?;
        let mut command_buffer =
            RenderCommandBuffer::with_capacity(RenderCommandBuffer::required_capacity(&snapshot));
        command_buffer.build_from(&snapshot);
        let commands = command_buffer.commands().to_vec();
        let map_rotation_degrees = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::MapTransform(transform) => Some(transform.map_rotation_degrees()),
                _ => None,
            })
            .unwrap_or(0.0);
        let viewport = commands.iter().find_map(|command| match command {
            RenderCommand::MapTransform(transform) => Some(transform.viewport()),
            _ => None,
        });
        let mut projected_pois = Vec::new();
        if let Some(viewport) = viewport {
            for command in &commands {
                if let RenderCommand::Poi(poi) = command {
                    let (x, y) =
                        project_map_point(viewport, poi.map_position(), map_rotation_degrees);
                    projected_pois.push(ProjectedSyntheticPoi {
                        kind: poi.kind(),
                        symbol: symbol_for_kind(poi.kind()),
                        x,
                        y,
                    });
                }
            }
        }
        Ok(Self {
            commands,
            projected_pois,
            map_rotation_degrees,
        })
    }

    pub const fn map_rotation_degrees(&self) -> f32 {
        self.map_rotation_degrees
    }

    pub fn commands(&self) -> &[RenderCommand] {
        &self.commands
    }

    pub fn projected_pois(&self) -> &[ProjectedSyntheticPoi] {
        &self.projected_pois
    }
}

#[derive(Debug)]
pub struct SyntheticSurface {
    diameter: u32,
    pixels: Vec<u32>,
    watermark_drawn: bool,
}

impl SyntheticSurface {
    pub fn new(diameter: u32) -> Result<Self, SyntheticSurfaceError> {
        if diameter == 0 || diameter > 4_096 {
            return Err(SyntheticSurfaceError::DiameterOutOfRange);
        }
        let pixel_count = usize::try_from(diameter)
            .ok()
            .and_then(|side| side.checked_mul(side))
            .ok_or(SyntheticSurfaceError::DiameterOutOfRange)?;
        Ok(Self {
            diameter,
            pixels: vec![0; pixel_count],
            watermark_drawn: false,
        })
    }

    pub fn rasterize(&mut self, frame: &SyntheticPreviewFrame) {
        self.watermark_drawn = false;
        let center = f64::from(self.diameter) * 0.5;
        let rotation = -f64::from(frame.map_rotation_degrees()).to_radians();
        let (sine, cosine) = rotation.sin_cos();

        for y in 0..self.diameter {
            for x in 0..self.diameter {
                let local_x = f64::from(x) + 0.5 - center;
                let local_y = f64::from(y) + 0.5 - center;
                let terrain_x = cosine.mul_add(local_x, -sine * local_y) + 512.0;
                let terrain_y = sine.mul_add(local_x, cosine * local_y) + 512.0;
                self.set_pixel(x as i32, y as i32, terrain_color(terrain_x, terrain_y));
            }
        }

        for poi in frame.projected_pois() {
            let x = poi.x().round() as i32;
            let y = poi.y().round() as i32;
            match poi.symbol() {
                SyntheticPoiSymbol::FastTravelDiamond => {
                    self.draw_diamond(x, y, 7, FAST_TRAVEL_COLOR);
                }
                SyntheticPoiSymbol::BossRing => self.draw_ring(x, y, 9, BOSS_COLOR),
                SyntheticPoiSymbol::WantedReticle => self.draw_ring(x, y, 7, BOSS_COLOR),
                SyntheticPoiSymbol::DungeonGate => {
                    self.draw_dungeon_gate(x, y, DUNGEON_COLOR);
                }
            }
        }
        self.draw_player_arrow(
            i32::try_from(self.diameter / 2).unwrap_or(i32::MAX),
            i32::try_from(self.diameter / 2).unwrap_or(i32::MAX),
        );
        self.watermark_drawn = true;
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.diameter || y >= self.diameter {
            return None;
        }
        let index = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(self.diameter).ok()?)?
            .checked_add(usize::try_from(x).ok()?)?;
        self.pixels.get(index).copied()
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub const fn diameter(&self) -> u32 {
        self.diameter
    }

    pub const fn watermark_drawn(&self) -> bool {
        self.watermark_drawn
    }

    fn set_pixel(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let Ok(x) = u32::try_from(x) else {
            return;
        };
        let Ok(y) = u32::try_from(y) else {
            return;
        };
        if x >= self.diameter || y >= self.diameter {
            return;
        }
        let index = usize::try_from(y).expect("bounded preview coordinate fits usize")
            * usize::try_from(self.diameter).expect("bounded preview diameter fits usize")
            + usize::try_from(x).expect("bounded preview coordinate fits usize");
        self.pixels[index] = color;
    }

    fn draw_diamond(&mut self, center_x: i32, center_y: i32, radius: i32, color: u32) {
        for dy in -radius..=radius {
            let width = radius - dy.abs();
            for dx in -width..=width {
                self.set_pixel(center_x + dx, center_y + dy, color);
            }
        }
    }

    fn draw_ring(&mut self, center_x: i32, center_y: i32, radius: i32, color: u32) {
        let outer = radius * radius;
        let inner = (radius - 3) * (radius - 3);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let distance = dx * dx + dy * dy;
                if (inner..=outer).contains(&distance) {
                    self.set_pixel(center_x + dx, center_y + dy, color);
                }
            }
        }
    }

    fn draw_dungeon_gate(&mut self, center_x: i32, center_y: i32, color: u32) {
        for dy in -8_i32..=8 {
            for dx in -8_i32..=8 {
                if dy == -8 || dx == -8 || dx == 8 || (dy >= 3 && dx.abs() <= 2) {
                    self.set_pixel(center_x + dx, center_y + dy, color);
                }
            }
        }
    }

    fn draw_player_arrow(&mut self, center_x: i32, center_y: i32) {
        for dy in -13_i32..=11 {
            let width = if dy <= 0 { (dy + 13) / 3 } else { 2 };
            for dx in -width..=width {
                self.set_pixel(center_x + dx, center_y + dy, PLAYER_COLOR);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntheticSurfaceError {
    DiameterOutOfRange,
}

impl fmt::Display for SyntheticSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("synthetic preview diameter is out of range")
    }
}

impl Error for SyntheticSurfaceError {}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("map-pack-valid")
}

fn project_map_point(
    viewport: ActiveViewport,
    point: pal_map_pack_store::MapPoint,
    rotation_degrees: f32,
) -> (f64, f64) {
    let center = viewport.center();
    let scale = viewport.effective_map_pixels_per_screen_pixel();
    let local_x = (point.x() - center.x()) / scale;
    let local_y = (point.y() - center.y()) / scale;
    let radians = f64::from(rotation_degrees).to_radians();
    let (sine, cosine) = radians.sin_cos();
    let rotated_x = cosine.mul_add(local_x, -sine * local_y);
    let rotated_y = sine.mul_add(local_x, cosine * local_y);
    let metrics = viewport.metrics();
    (
        rotated_x + f64::from(metrics.output_width_px()) * 0.5,
        rotated_y + f64::from(metrics.output_height_px()) * 0.5,
    )
}

const fn symbol_for_kind(kind: PoiKind) -> SyntheticPoiSymbol {
    match kind {
        PoiKind::FastTravel => SyntheticPoiSymbol::FastTravelDiamond,
        PoiKind::Boss => SyntheticPoiSymbol::BossRing,
        PoiKind::Wanted => SyntheticPoiSymbol::WantedReticle,
        PoiKind::Dungeon => SyntheticPoiSymbol::DungeonGate,
    }
}

fn terrain_color(x: f64, y: f64) -> u32 {
    let broad = (x * 0.013).sin() + (y * 0.017).cos();
    let detail = ((x + y) * 0.031).sin() * 0.45 + ((x - y) * 0.023).cos() * 0.35;
    match broad + detail {
        value if value < -0.65 => TERRAIN_WATER,
        value if value < 0.2 => TERRAIN_LOWLAND,
        value if value < 1.0 => TERRAIN_UPLAND,
        _ => TERRAIN_RIDGE,
    }
}
