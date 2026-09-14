use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use image::ImageFormat;
use pal_domain::PoiFilters;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

const MAX_MAP_SIDE: u32 = 4_096;
const MAX_SURFACE_SIDE: u32 = 4_096;
const MAX_POI_ICON_SIDE: u32 = 512;
const MAX_PREVIEW_BMP_FILE_BYTES: u64 = 64 * 1_024 * 1_024;
const OUT_OF_MAP_BACKDROP: u32 = 0x0007_0d14;
// The player marker uses the same cyan navigation accent as the minimap bezel. A pale rear needle
// keeps its direction readable over both land and water without turning into a tall rocket.
const PLAYER_COLOR: u32 = 0x0034_d7e6;
const PLAYER_REAR_COLOR: u32 = 0x008f_e4e8;
const PLAYER_RING_COLOR: u32 = 0x00c9_f5f7;
const ICON_SHADOW_COLOR: u32 = 0x0007_1019;
const ICON_WHITE: u32 = 0x00f4_fbff;
const FAST_TRAVEL_DARK: u32 = 0x0000_5f78;
const FAST_TRAVEL_COLOR: u32 = 0x0000_d8ef;
// Palworld's map language keeps character portraits on a restrained neutral plate. Red remains a
// small danger accent instead of becoming a full thumbnail background.
const BOSS_PLATE_COLOR: u32 = 0x0014_2830;
const BOSS_RING_COLOR: u32 = 0x00d7_e8e5;
const BOSS_ACCENT_COLOR: u32 = 0x00d9_4052;
const WANTED_COLOR: u32 = 0x00d9_4052;
const WANTED_DARK: u32 = 0x0030_1820;
const DUNGEON_RING_COLOR: u32 = 0x0071_55c9;
const DUNGEON_ACCENT_COLOR: u32 = 0x00bc_a9ff;
const DUNGEON_MOUTH_COLOR: u32 = 0x0015_202b;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapPoiKind {
    FastTravel,
    Boss,
    Wanted,
    Dungeon,
    Supplemental,
    PalSpawn,
}

const SUPPLEMENTAL_LAYER_IDS: [&str; 43] = [
    "respawn",
    "tower",
    "sealed-realm",
    "biome-boss",
    "oil-rig",
    "map-unlock",
    "sky-warp",
    "dimensional-warp",
    "dimensional-distortion",
    "enemy-camp",
    "anti-air-turret",
    "pal-merchant",
    "merchant",
    "npc",
    "messenger-of-love",
    "effigy",
    "medal",
    "memo",
    "egg",
    "skill-fruit",
    "world-tree-fruit",
    "ancient-shrine",
    "treasure-map",
    "elemental-chest",
    "chest",
    "poi",
    "kinship-peach",
    "fishing-spot",
    "salvage-rank-1",
    "salvage-rank-2",
    "oil-field",
    "soralite",
    "chromite",
    "hexolite-quartz",
    "ancient-bark",
    "ancient-lava",
    "ore-copper",
    "ore-coal",
    "ore-quartz",
    "ore-sulfur",
    "ore-crystal",
    "healing-spring",
    "paloxite",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapPoiIcon {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    visible_bounds: IconVisibleBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IconVisibleBounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl IconVisibleBounds {
    fn from_argb_pixels(width: u32, height: u32, pixels: &[u32]) -> Self {
        let mut left = width;
        let mut top = height;
        let mut right = 0;
        let mut bottom = 0;
        let mut found = false;

        for (index, pixel) in pixels.iter().copied().enumerate() {
            if pixel >> 24 == 0 {
                continue;
            }
            let index = u32::try_from(index).expect("POI icon pixel index fits u32");
            let x = index % width;
            let y = index / width;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
            found = true;
        }

        if found {
            Self {
                left,
                top,
                right,
                bottom,
            }
        } else {
            Self {
                left: 0,
                top: 0,
                right: width.saturating_sub(1),
                bottom: height.saturating_sub(1),
            }
        }
    }

    const fn width(self) -> u32 {
        self.right - self.left + 1
    }

    const fn height(self) -> u32 {
        self.bottom - self.top + 1
    }
}

impl MapPoiIcon {
    pub fn decode_png(bytes: &[u8]) -> Option<Self> {
        Self::decode_with_format(bytes, ImageFormat::Png)
    }

    pub fn decode_webp(bytes: &[u8]) -> Option<Self> {
        Self::decode_with_format(bytes, ImageFormat::WebP)
    }

    fn decode_with_format(bytes: &[u8], format: ImageFormat) -> Option<Self> {
        let image = image::load_from_memory_with_format(bytes, format)
            .ok()?
            .to_rgba8();
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 || width > MAX_POI_ICON_SIDE || height > MAX_POI_ICON_SIDE {
            return None;
        }
        let pixels = image
            .pixels()
            .map(|pixel| {
                (u32::from(pixel[3]) << 24)
                    | (u32::from(pixel[0]) << 16)
                    | (u32::from(pixel[1]) << 8)
                    | u32::from(pixel[2])
            })
            .collect::<Vec<_>>();
        let visible_bounds = IconVisibleBounds::from_argb_pixels(width, height, &pixels);
        Some(Self {
            width,
            height,
            pixels,
            visible_bounds,
        })
    }

    fn sample_scaled_visible_fit(&self, x: u32, y: u32, target_side: u32) -> u32 {
        let target_side = target_side.max(1);
        let source_width = self.visible_bounds.width();
        let source_height = self.visible_bounds.height();
        let fit_scale = (f64::from(target_side) / f64::from(source_width))
            .min(f64::from(target_side) / f64::from(source_height));
        let fitted_width = (f64::from(source_width) * fit_scale)
            .round()
            .clamp(1.0, f64::from(target_side)) as u32;
        let fitted_height = (f64::from(source_height) * fit_scale)
            .round()
            .clamp(1.0, f64::from(target_side)) as u32;
        let offset_x = (target_side - fitted_width) / 2;
        let offset_y = (target_side - fitted_height) / 2;
        if x < offset_x
            || y < offset_y
            || x >= offset_x + fitted_width
            || y >= offset_y + fitted_height
        {
            return 0;
        }

        let source_x = f64::from(self.visible_bounds.left)
            + (f64::from(x - offset_x) + 0.5) * f64::from(source_width) / f64::from(fitted_width)
            - 0.5;
        let source_y = f64::from(self.visible_bounds.top)
            + (f64::from(y - offset_y) + 0.5) * f64::from(source_height) / f64::from(fitted_height)
            - 0.5;
        sample_argb_bilinear(self.width, self.height, &self.pixels, source_x, source_y)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapPoi {
    map_x: f64,
    map_y: f64,
    kind: MapPoiKind,
    icon: Option<Arc<MapPoiIcon>>,
    filter_id: Option<Arc<str>>,
    supplemental_filter_bit: u64,
    night_spawn: bool,
}

impl MapPoi {
    pub fn new(map_x: f64, map_y: f64, kind: MapPoiKind) -> Option<Self> {
        (map_x.is_finite() && map_y.is_finite()).then_some(Self {
            map_x,
            map_y,
            kind,
            icon: None,
            filter_id: None,
            supplemental_filter_bit: 0,
            night_spawn: false,
        })
    }

    pub fn with_icon(mut self, icon: Arc<MapPoiIcon>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_pal_spawn_filter(mut self, species_id: Arc<str>, night_spawn: bool) -> Self {
        debug_assert_eq!(self.kind, MapPoiKind::PalSpawn);
        self.filter_id = Some(species_id);
        self.night_spawn = night_spawn;
        self
    }

    pub fn with_supplemental_filter(mut self, layer_id: &str) -> Option<Self> {
        debug_assert_eq!(self.kind, MapPoiKind::Supplemental);
        self.supplemental_filter_bit = supplemental_layer_bit(layer_id)?;
        self.filter_id = Some(Arc::from(layer_id));
        Some(self)
    }

    pub const fn map_x(&self) -> f64 {
        self.map_x
    }

    pub const fn map_y(&self) -> f64 {
        self.map_y
    }

    pub const fn kind(&self) -> MapPoiKind {
        self.kind
    }

    pub fn icon(&self) -> Option<&MapPoiIcon> {
        self.icon.as_deref()
    }

    pub fn filter_id(&self) -> Option<&str> {
        self.filter_id.as_deref()
    }

    pub const fn is_night_spawn(&self) -> bool {
        self.night_spawn
    }

    pub const fn supplemental_filter_bit(&self) -> u64 {
        self.supplemental_filter_bit
    }
}

fn supplemental_layer_bit(layer_id: &str) -> Option<u64> {
    SUPPLEMENTAL_LAYER_IDS
        .iter()
        .position(|candidate| *candidate == layer_id)
        .map(|index| 1_u64 << index)
}

fn supplemental_filter_mask(filters: &PoiFilters) -> u64 {
    filters
        .enabled_layer_ids
        .iter()
        .filter_map(|layer_id| supplemental_layer_bit(layer_id))
        .fold(0_u64, |mask, bit| mask | bit)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct IconVisualPolicy {
    artwork_scale: f64,
    offset_x_fraction: f64,
    offset_y_fraction: f64,
}

impl IconVisualPolicy {
    const fn new(artwork_scale: f64, offset_x_fraction: f64, offset_y_fraction: f64) -> Self {
        Self {
            artwork_scale,
            offset_x_fraction,
            offset_y_fraction,
        }
    }
}

fn poi_icon_visual_policy(poi: &MapPoi) -> IconVisualPolicy {
    match poi.kind() {
        MapPoiKind::FastTravel => IconVisualPolicy::new(1.10, 0.005, 0.04),
        MapPoiKind::Dungeon => IconVisualPolicy::new(1.0, -0.008, 0.016),
        MapPoiKind::Wanted => IconVisualPolicy::new(0.94, -0.013, -0.031),
        MapPoiKind::Boss => IconVisualPolicy::new(0.94, 0.0, 0.0),
        MapPoiKind::PalSpawn => IconVisualPolicy::new(1.0, 0.0, 0.0),
        // Supplemental textures are decoded with their transparent margins
        // removed. Keep a shared optical footprint so eggs, resources and
        // collectibles no longer look smaller than navigation POIs.
        MapPoiKind::Supplemental => match poi.filter_id() {
            Some("effigy") => IconVisualPolicy::new(1.0, -0.027, -0.016),
            Some("ancient-lava") => IconVisualPolicy::new(1.0, -0.02, -0.02),
            Some("salvage-rank-1" | "salvage-rank-2") => IconVisualPolicy::new(1.0, -0.02, -0.012),
            Some("poi") => IconVisualPolicy::new(1.0, -0.047, -0.008),
            Some("fishing-spot") => IconVisualPolicy::new(1.0, 0.0, -0.039),
            Some("healing-spring") => IconVisualPolicy::new(1.0, 0.02, -0.012),
            Some("oil-field") => IconVisualPolicy::new(1.0, -0.027, -0.008),
            Some("soralite") => IconVisualPolicy::new(1.0, -0.004, -0.008),
            _ => IconVisualPolicy::new(1.0, 0.0, 0.0),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRasterError {
    DimensionsOutOfRange,
    PixelCountMismatch,
}

impl fmt::Display for MapRasterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DimensionsOutOfRange => "map raster dimensions are out of range",
            Self::PixelCountMismatch => "map raster pixel count does not match its dimensions",
        })
    }
}

impl Error for MapRasterError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmpDecodeError {
    Truncated,
    InvalidSignature,
    UnsupportedDibHeader,
    DimensionsOutOfRange,
    UnsupportedPlanes,
    UnsupportedBitDepth,
    UnsupportedCompression,
    PixelDataOutOfRange,
    Raster(MapRasterError),
}

impl fmt::Display for BmpDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Truncated => "BMP data is truncated",
            Self::InvalidSignature => "BMP signature is invalid",
            Self::UnsupportedDibHeader => "BMP DIB header is unsupported",
            Self::DimensionsOutOfRange => "BMP dimensions are out of range",
            Self::UnsupportedPlanes => "BMP plane count is unsupported",
            Self::UnsupportedBitDepth => "BMP bit depth is unsupported",
            Self::UnsupportedCompression => "compressed BMP data is unsupported",
            Self::PixelDataOutOfRange => "BMP pixel data lies outside the file",
            Self::Raster(_) => "decoded BMP raster is invalid",
        })
    }
}

impl Error for BmpDecodeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapFileError {
    Io,
    FileTooLarge,
    Decode(BmpDecodeError),
}

impl fmt::Display for MapFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Io => "map BMP could not be read",
            Self::FileTooLarge => "map BMP exceeds the 64 MiB preview budget",
            Self::Decode(_) => "map BMP could not be decoded",
        })
    }
}

impl Error for MapFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Io | Self::FileTooLarge => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRaster {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl MapRaster {
    pub fn new(width: u32, height: u32, pixels: Vec<u32>) -> Result<Self, MapRasterError> {
        validate_map_dimensions(width, height)?;
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or(MapRasterError::DimensionsOutOfRange)?;
        if pixels.len() != expected {
            return Err(MapRasterError::PixelCountMismatch);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn decode_bmp(bytes: &[u8]) -> Result<Self, BmpDecodeError> {
        if bytes.len() < 54 {
            return Err(BmpDecodeError::Truncated);
        }
        if &bytes[0..2] != b"BM" {
            return Err(BmpDecodeError::InvalidSignature);
        }
        let pixel_offset = read_u32(bytes, 10)? as usize;
        let dib_size = read_u32(bytes, 14)?;
        if dib_size < 40 {
            return Err(BmpDecodeError::UnsupportedDibHeader);
        }
        let width_signed = read_i32(bytes, 18)?;
        let height_signed = read_i32(bytes, 22)?;
        if width_signed <= 0 || height_signed == 0 || height_signed == i32::MIN {
            return Err(BmpDecodeError::DimensionsOutOfRange);
        }
        let width =
            u32::try_from(width_signed).map_err(|_| BmpDecodeError::DimensionsOutOfRange)?;
        let height = height_signed.unsigned_abs();
        validate_map_dimensions(width, height).map_err(BmpDecodeError::Raster)?;
        if read_u16(bytes, 26)? != 1 {
            return Err(BmpDecodeError::UnsupportedPlanes);
        }
        let bits_per_pixel = read_u16(bytes, 28)?;
        if !matches!(bits_per_pixel, 24 | 32) {
            return Err(BmpDecodeError::UnsupportedBitDepth);
        }
        if read_u32(bytes, 30)? != 0 {
            return Err(BmpDecodeError::UnsupportedCompression);
        }

        let bytes_per_pixel = usize::from(bits_per_pixel / 8);
        let width_usize =
            usize::try_from(width).map_err(|_| BmpDecodeError::DimensionsOutOfRange)?;
        let height_usize =
            usize::try_from(height).map_err(|_| BmpDecodeError::DimensionsOutOfRange)?;
        let unpadded_row = width_usize
            .checked_mul(bytes_per_pixel)
            .ok_or(BmpDecodeError::DimensionsOutOfRange)?;
        let row_stride = unpadded_row
            .checked_add(3)
            .map(|row| row & !3)
            .ok_or(BmpDecodeError::DimensionsOutOfRange)?;
        let pixel_bytes = row_stride
            .checked_mul(height_usize)
            .ok_or(BmpDecodeError::DimensionsOutOfRange)?;
        let pixel_end = pixel_offset
            .checked_add(pixel_bytes)
            .ok_or(BmpDecodeError::PixelDataOutOfRange)?;
        if pixel_offset < 14 + dib_size as usize || pixel_end > bytes.len() {
            return Err(BmpDecodeError::PixelDataOutOfRange);
        }

        let top_down = height_signed < 0;
        let mut pixels = vec![0_u32; width_usize * height_usize];
        for destination_y in 0..height_usize {
            let source_y = if top_down {
                destination_y
            } else {
                height_usize - destination_y - 1
            };
            let source_row = pixel_offset + source_y * row_stride;
            let destination_row = destination_y * width_usize;
            for x in 0..width_usize {
                let source = source_row + x * bytes_per_pixel;
                let blue = u32::from(bytes[source]);
                let green = u32::from(bytes[source + 1]);
                let red = u32::from(bytes[source + 2]);
                pixels[destination_row + x] = (red << 16) | (green << 8) | blue;
            }
        }
        Self::new(width, height, pixels).map_err(BmpDecodeError::Raster)
    }

    pub fn load_bmp_file(path: impl AsRef<Path>) -> Result<Self, MapFileError> {
        let file = File::open(path).map_err(|_| MapFileError::Io)?;
        let metadata = file.metadata().map_err(|_| MapFileError::Io)?;
        if metadata.len() > MAX_PREVIEW_BMP_FILE_BYTES {
            return Err(MapFileError::FileTooLarge);
        }

        let capacity = usize::try_from(metadata.len()).map_err(|_| MapFileError::FileTooLarge)?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(MAX_PREVIEW_BMP_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| MapFileError::Io)?;
        if bytes.len() as u64 > MAX_PREVIEW_BMP_FILE_BYTES {
            return Err(MapFileError::FileTooLarge);
        }
        Self::decode_bmp(&bytes).map_err(MapFileError::Decode)
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = usize::try_from(y).ok()? * usize::try_from(self.width).ok()?
            + usize::try_from(x).ok()?;
        self.pixels.get(index).copied()
    }
}

fn validate_map_dimensions(width: u32, height: u32) -> Result<(), MapRasterError> {
    if width == 0 || height == 0 || width > MAX_MAP_SIDE || height > MAX_MAP_SIDE {
        return Err(MapRasterError::DimensionsOutOfRange);
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, BmpDecodeError> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(BmpDecodeError::Truncated)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, BmpDecodeError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(BmpDecodeError::Truncated)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, BmpDecodeError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(BmpDecodeError::Truncated)?;
    Ok(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedMapPoint {
    x: f64,
    y: f64,
}

impl NormalizedMapPoint {
    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldToImageTransform {
    min_world_x: f64,
    min_world_y: f64,
    max_world_x: f64,
    max_world_y: f64,
    x_from_world_x: f64,
    x_from_world_y: f64,
    x_offset: f64,
    y_from_world_x: f64,
    y_from_world_y: f64,
    y_offset: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldToImageTransformError {
    NonFiniteBounds,
    DegenerateBounds,
    InvalidMapDimensions,
    NonFiniteMatrix,
    SingularMatrix,
}

impl fmt::Display for WorldToImageTransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFiniteBounds => "world bounds contain a non-finite number",
            Self::DegenerateBounds => "world bounds must have positive spans",
            Self::InvalidMapDimensions => "map dimensions must be nonzero",
            Self::NonFiniteMatrix => "world-to-map matrix contains a non-finite number",
            Self::SingularMatrix => "world-to-map matrix must be nonsingular",
        })
    }
}

impl Error for WorldToImageTransformError {}

impl WorldToImageTransform {
    const RELATIVE_DETERMINANT_EPSILON: f64 = 1.0e-12;

    /// Builds the normalized Palworld map projection for axis-aligned world bounds.
    ///
    /// Palworld world `Y` increases toward image-right while world `X` increases
    /// toward image-up. The returned transform maps the north-west world corner
    /// to `(0, 0)` and the south-east corner to `(1, 1)`.
    pub fn from_world_bounds(
        min_world_x: f64,
        min_world_y: f64,
        max_world_x: f64,
        max_world_y: f64,
    ) -> Result<Self, WorldToImageTransformError> {
        if ![min_world_x, min_world_y, max_world_x, max_world_y]
            .into_iter()
            .all(f64::is_finite)
        {
            return Err(WorldToImageTransformError::NonFiniteBounds);
        }
        let world_x_span = max_world_x - min_world_x;
        let world_y_span = max_world_y - min_world_y;
        if world_x_span <= 0.0 || world_y_span <= 0.0 {
            return Err(WorldToImageTransformError::DegenerateBounds);
        }

        Ok(Self {
            min_world_x,
            min_world_y,
            max_world_x,
            max_world_y,
            x_from_world_x: 0.0,
            x_from_world_y: 1.0 / world_y_span,
            x_offset: -min_world_y / world_y_span,
            y_from_world_x: -1.0 / world_x_span,
            y_from_world_y: 0.0,
            y_offset: max_world_x / world_x_span,
        })
    }

    /// Builds a normalized renderer projection from an already validated map-pixel affine matrix.
    ///
    /// The caller is responsible for authenticating the matrix and its calibration evidence.
    /// This constructor only validates the numeric boundary and converts map pixels to normalized
    /// coordinates used by the rasterizer.
    pub fn from_affine_map_pixels(
        min_world_x: f64,
        min_world_y: f64,
        max_world_x: f64,
        max_world_y: f64,
        map_width_px: u32,
        map_height_px: u32,
        matrix: [[f64; 3]; 2],
    ) -> Result<Self, WorldToImageTransformError> {
        if ![min_world_x, min_world_y, max_world_x, max_world_y]
            .into_iter()
            .all(f64::is_finite)
        {
            return Err(WorldToImageTransformError::NonFiniteBounds);
        }
        if min_world_x >= max_world_x || min_world_y >= max_world_y {
            return Err(WorldToImageTransformError::DegenerateBounds);
        }
        if map_width_px == 0 || map_height_px == 0 {
            return Err(WorldToImageTransformError::InvalidMapDimensions);
        }
        if !matrix.iter().flatten().copied().all(f64::is_finite) {
            return Err(WorldToImageTransformError::NonFiniteMatrix);
        }

        let linear_scale = matrix[0][0]
            .abs()
            .max(matrix[0][1].abs())
            .max(matrix[1][0].abs())
            .max(matrix[1][1].abs());
        if linear_scale == 0.0 {
            return Err(WorldToImageTransformError::SingularMatrix);
        }
        let normalized_determinant = (matrix[0][0] / linear_scale) * (matrix[1][1] / linear_scale)
            - (matrix[0][1] / linear_scale) * (matrix[1][0] / linear_scale);
        if !normalized_determinant.is_finite()
            || normalized_determinant.abs() <= Self::RELATIVE_DETERMINANT_EPSILON
        {
            return Err(WorldToImageTransformError::SingularMatrix);
        }

        let map_width = f64::from(map_width_px);
        let map_height = f64::from(map_height_px);
        Ok(Self {
            min_world_x,
            min_world_y,
            max_world_x,
            max_world_y,
            x_from_world_x: matrix[0][0] / map_width,
            x_from_world_y: matrix[0][1] / map_width,
            x_offset: matrix[0][2] / map_width,
            y_from_world_x: matrix[1][0] / map_height,
            y_from_world_y: matrix[1][1] / map_height,
            y_offset: matrix[1][2] / map_height,
        })
    }

    pub fn project(self, world_x: f64, world_y: f64) -> NormalizedMapPoint {
        NormalizedMapPoint {
            x: self
                .x_from_world_x
                .mul_add(world_x, self.x_from_world_y.mul_add(world_y, self.x_offset)),
            y: self
                .y_from_world_x
                .mul_add(world_x, self.y_from_world_y.mul_add(world_y, self.y_offset)),
        }
    }

    /// Projects only coordinates owned by these exact axis-aligned world bounds.
    ///
    /// A finite coordinate outside the selected region is not a valid edge pixel. In
    /// particular, a MainMap surface must not silently render World Tree or another region at
    /// an unrelated point on the bitmap.
    pub fn project_within_bounds(self, world_x: f64, world_y: f64) -> Option<NormalizedMapPoint> {
        if !world_x.is_finite() || !world_y.is_finite() {
            return None;
        }
        ((self.min_world_x..=self.max_world_x).contains(&world_x)
            && (self.min_world_y..=self.max_world_y).contains(&world_y))
        .then(|| self.project(world_x, world_y))
    }
}

/// Returns the normalized transform derived from the authoritative MainMap
/// `FirstRegion` bounds for Palworld build 24181527.
pub fn authoritative_main_map_world_to_image() -> WorldToImageTransform {
    WorldToImageTransform::from_world_bounds(-1_099_400.0, -724_400.0, 349_400.0, 724_400.0)
        .expect("the embedded authoritative MainMap bounds are valid")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapViewError {
    NonFinite,
    ScaleOutOfRange,
    InvalidDimensions,
}

impl fmt::Display for MapViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFinite => "map view contains a non-finite number",
            Self::ScaleOutOfRange => "map view scale is out of range",
            Self::InvalidDimensions => "map or viewport dimensions must be nonzero",
        })
    }
}

impl Error for MapViewError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    center_x: f64,
    center_y: f64,
    player_map_x: Option<f64>,
    player_map_y: Option<f64>,
    highlight_map_x: Option<f64>,
    highlight_map_y: Option<f64>,
    source_pixels_per_screen_pixel: f64,
    map_rotation_degrees: f32,
    player_rotation_degrees: f32,
}

impl MapView {
    pub fn browse(
        center_x: f64,
        center_y: f64,
        source_pixels_per_screen_pixel: f64,
    ) -> Result<Self, MapViewError> {
        let mut view = Self::new(center_x, center_y, source_pixels_per_screen_pixel, 0.0)?;
        view.player_map_x = None;
        view.player_map_y = None;
        Ok(view)
    }

    pub fn new(
        center_x: f64,
        center_y: f64,
        source_pixels_per_screen_pixel: f64,
        map_rotation_degrees: f32,
    ) -> Result<Self, MapViewError> {
        Self::with_rotations(
            center_x,
            center_y,
            source_pixels_per_screen_pixel,
            map_rotation_degrees,
            0.0,
        )
    }

    pub fn with_rotations(
        center_x: f64,
        center_y: f64,
        source_pixels_per_screen_pixel: f64,
        map_rotation_degrees: f32,
        player_rotation_degrees: f32,
    ) -> Result<Self, MapViewError> {
        if !center_x.is_finite()
            || !center_y.is_finite()
            || !source_pixels_per_screen_pixel.is_finite()
            || !map_rotation_degrees.is_finite()
            || !player_rotation_degrees.is_finite()
        {
            return Err(MapViewError::NonFinite);
        }
        if !(0.01..=4_096.0).contains(&source_pixels_per_screen_pixel) {
            return Err(MapViewError::ScaleOutOfRange);
        }
        Ok(Self {
            center_x,
            center_y,
            player_map_x: Some(center_x),
            player_map_y: Some(center_y),
            highlight_map_x: None,
            highlight_map_y: None,
            source_pixels_per_screen_pixel,
            map_rotation_degrees,
            player_rotation_degrees,
        })
    }

    pub const fn center_x(self) -> f64 {
        self.center_x
    }

    pub const fn center_y(self) -> f64 {
        self.center_y
    }

    pub const fn source_pixels_per_screen_pixel(self) -> f64 {
        self.source_pixels_per_screen_pixel
    }

    pub const fn map_rotation_degrees(self) -> f32 {
        self.map_rotation_degrees
    }

    pub const fn player_rotation_degrees(self) -> f32 {
        self.player_rotation_degrees
    }

    pub fn player_map_pose(self) -> Option<(f64, f64, f32)> {
        Some((
            self.player_map_x?,
            self.player_map_y?,
            self.player_rotation_degrees,
        ))
    }

    pub fn with_player_map_pose(
        mut self,
        map_x: f64,
        map_y: f64,
        rotation_degrees: f32,
    ) -> Result<Self, MapViewError> {
        if !map_x.is_finite() || !map_y.is_finite() || !rotation_degrees.is_finite() {
            return Err(MapViewError::NonFinite);
        }
        self.player_map_x = Some(map_x);
        self.player_map_y = Some(map_y);
        self.player_rotation_degrees = rotation_degrees;
        Ok(self)
    }

    pub fn without_player_marker(mut self) -> Self {
        self.player_map_x = None;
        self.player_map_y = None;
        self
    }

    /// Moves the map as if its raster were dragged by the supplied pixel delta.
    ///
    /// A positive X delta means the map image follows the pointer to the right, so the source
    /// center moves left. The inverse rotation matches `rasterize_with_pois`, and the result is
    /// clamped so a rotated viewport cannot be dragged beyond the map's renderable bounds.
    pub fn panned_by_raster_delta(
        mut self,
        raster_delta_x: f64,
        raster_delta_y: f64,
        viewport_width: u32,
        viewport_height: u32,
        map_width: u32,
        map_height: u32,
    ) -> Result<Self, MapViewError> {
        if !raster_delta_x.is_finite() || !raster_delta_y.is_finite() {
            return Err(MapViewError::NonFinite);
        }
        if viewport_width == 0 || viewport_height == 0 || map_width == 0 || map_height == 0 {
            return Err(MapViewError::InvalidDimensions);
        }

        let radians = f64::from(self.map_rotation_degrees).to_radians();
        let cosine = radians.cos();
        let sine = radians.sin();
        let source_delta_x = cosine.mul_add(raster_delta_x, sine * raster_delta_y)
            * self.source_pixels_per_screen_pixel;
        let source_delta_y = (-sine).mul_add(raster_delta_x, cosine * raster_delta_y)
            * self.source_pixels_per_screen_pixel;
        self.center_x -= source_delta_x;
        self.center_y -= source_delta_y;

        let half_width = f64::from(viewport_width) * 0.5;
        let half_height = f64::from(viewport_height) * 0.5;
        let source_half_extent_x = (cosine.abs() * half_width + sine.abs() * half_height)
            * self.source_pixels_per_screen_pixel;
        let source_half_extent_y = (sine.abs() * half_width + cosine.abs() * half_height)
            * self.source_pixels_per_screen_pixel;
        self.center_x = clamp_map_center(self.center_x, source_half_extent_x, f64::from(map_width));
        self.center_y =
            clamp_map_center(self.center_y, source_half_extent_y, f64::from(map_height));
        Ok(self)
    }

    /// Applies a tracking-frame scale/rotation update without changing a retained browse center.
    pub fn with_raster_transform(
        mut self,
        source_pixels_per_screen_pixel: f64,
        map_rotation_degrees: f32,
    ) -> Result<Self, MapViewError> {
        if !source_pixels_per_screen_pixel.is_finite() || !map_rotation_degrees.is_finite() {
            return Err(MapViewError::NonFinite);
        }
        if !(0.01..=4_096.0).contains(&source_pixels_per_screen_pixel) {
            return Err(MapViewError::ScaleOutOfRange);
        }
        self.source_pixels_per_screen_pixel = source_pixels_per_screen_pixel;
        self.map_rotation_degrees = map_rotation_degrees;
        Ok(self)
    }

    pub fn with_search_focus(mut self, map_x: f64, map_y: f64) -> Result<Self, MapViewError> {
        if !map_x.is_finite() || !map_y.is_finite() {
            return Err(MapViewError::NonFinite);
        }
        self.center_x = map_x;
        self.center_y = map_y;
        self.highlight_map_x = Some(map_x);
        self.highlight_map_y = Some(map_y);
        Ok(self)
    }

    pub fn search_focus(
        map_x: f64,
        map_y: f64,
        source_pixels_per_screen_pixel: f64,
    ) -> Result<Self, MapViewError> {
        let mut view = Self::new(map_x, map_y, source_pixels_per_screen_pixel, 0.0)?;
        view.player_map_x = None;
        view.player_map_y = None;
        view.highlight_map_x = Some(map_x);
        view.highlight_map_y = Some(map_y);
        Ok(view)
    }
}

fn clamp_map_center(center: f64, half_extent: f64, map_extent: f64) -> f64 {
    if half_extent * 2.0 >= map_extent {
        map_extent * 0.5
    } else {
        center.clamp(half_extent, map_extent - half_extent)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuMapSurfaceError {
    DimensionsOutOfRange,
}

impl fmt::Display for CpuMapSurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CPU map surface dimensions are out of range")
    }
}

impl Error for CpuMapSurfaceError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActualMapSurfacePerformanceCounters {
    pub rasterized_frames: u64,
    pub last_raster_cpu_100ns: u64,
    pub raster_cpu_100ns_total: u64,
    pub raster_cpu_timing_samples: u64,
    pub raster_cpu_timing_failures: u64,
}

#[derive(Debug)]
pub struct CpuMapSurface {
    size: crate::PhysicalSize,
    pixels: Vec<u32>,
    visible_poi_centers: Vec<(i32, i32)>,
    poi_occupancy: Vec<usize>,
    performance_counters: ActualMapSurfacePerformanceCounters,
}

impl CpuMapSurface {
    pub fn new(size: impl Into<crate::PhysicalSize>) -> Result<Self, CpuMapSurfaceError> {
        let size = size.into();
        let pixel_count = surface_pixel_count(size)?;
        Ok(Self {
            size,
            pixels: vec![0; pixel_count],
            visible_poi_centers: Vec::with_capacity(192),
            poi_occupancy: Vec::with_capacity(512),
            performance_counters: ActualMapSurfacePerformanceCounters::default(),
        })
    }

    pub fn rasterize(&mut self, map: &MapRaster, view: MapView) {
        self.rasterize_with_pois(
            map,
            view,
            &[],
            &PoiFilters {
                fast_travel: false,
                boss: false,
                wanted: false,
                dungeon: false,
                ..PoiFilters::default()
            },
        );
    }

    pub fn rasterize_with_pois(
        &mut self,
        map: &MapRaster,
        view: MapView,
        pois: &[MapPoi],
        filters: &PoiFilters,
    ) {
        let cpu_before = current_thread_cpu_100ns();
        let center_x = f64::from(self.size.width) * 0.5;
        let center_y = f64::from(self.size.height) * 0.5;
        let inverse_rotation = -f64::from(view.map_rotation_degrees).to_radians();
        let (sine, cosine) = inverse_rotation.sin_cos();
        for y in 0..self.size.height {
            for x in 0..self.size.width {
                let local_x = f64::from(x) + 0.5 - center_x;
                let local_y = f64::from(y) + 0.5 - center_y;
                let rotated_x = cosine.mul_add(local_x, -sine * local_y);
                let rotated_y = sine.mul_add(local_x, cosine * local_y);
                let source_x = view.center_x + rotated_x * view.source_pixels_per_screen_pixel;
                let source_y = view.center_y + rotated_y * view.source_pixels_per_screen_pixel;
                let color = sample_bilinear(map, source_x, source_y).unwrap_or(OUT_OF_MAP_BACKDROP);
                self.set_pixel(x as i32, y as i32, color);
            }
        }
        self.draw_pois(view, pois, filters);
        if self.size.width >= 32 && self.size.height >= 32 {
            if let (Some(map_x), Some(map_y)) = (view.player_map_x, view.player_map_y)
                && let Some((player_x, player_y)) = self.project_map_point(view, map_x, map_y)
                && player_x >= 0
                && player_y >= 0
                && player_x < i32::try_from(self.size.width).unwrap_or(i32::MAX)
                && player_y < i32::try_from(self.size.height).unwrap_or(i32::MAX)
            {
                self.draw_player_arrow(player_x, player_y, view.player_rotation_degrees);
            }
            if let (Some(map_x), Some(map_y)) = (view.highlight_map_x, view.highlight_map_y)
                && let Some((screen_x, screen_y)) = self.project_map_point(view, map_x, map_y)
            {
                self.draw_search_highlight(screen_x, screen_y);
            }
        }
        let cpu_after = current_thread_cpu_100ns();
        self.performance_counters.rasterized_frames = self
            .performance_counters
            .rasterized_frames
            .saturating_add(1);
        if let (Some(before), Some(after)) = (cpu_before, cpu_after) {
            let cpu_100ns = after.saturating_sub(before);
            self.performance_counters.last_raster_cpu_100ns = cpu_100ns;
            self.performance_counters.raster_cpu_100ns_total = self
                .performance_counters
                .raster_cpu_100ns_total
                .saturating_add(cpu_100ns);
            self.performance_counters.raster_cpu_timing_samples = self
                .performance_counters
                .raster_cpu_timing_samples
                .saturating_add(1);
        } else {
            self.performance_counters.raster_cpu_timing_failures = self
                .performance_counters
                .raster_cpu_timing_failures
                .saturating_add(1);
        }
    }

    pub const fn performance_counters(&self) -> ActualMapSurfacePerformanceCounters {
        self.performance_counters
    }

    pub const fn size(&self) -> crate::PhysicalSize {
        self.size
    }

    pub fn resize(&mut self, size: crate::PhysicalSize) -> Result<bool, CpuMapSurfaceError> {
        if size == self.size {
            return Ok(false);
        }
        let pixel_count = surface_pixel_count(size)?;
        self.pixels.resize(pixel_count, 0);
        self.size = size;
        Ok(true)
    }

    pub const fn diameter(&self) -> u32 {
        self.size.width
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn pixel_capacity(&self) -> usize {
        self.pixels.capacity()
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.size.width || y >= self.size.height {
            return None;
        }
        let index = usize::try_from(y).ok()? * usize::try_from(self.size.width).ok()?
            + usize::try_from(x).ok()?;
        self.pixels.get(index).copied()
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
        if x >= self.size.width || y >= self.size.height {
            return;
        }
        let index = usize::try_from(y).expect("bounded surface coordinate fits usize")
            * usize::try_from(self.size.width).expect("bounded surface width fits usize")
            + usize::try_from(x).expect("bounded surface coordinate fits usize");
        self.pixels[index] = color;
    }

    fn blend_pixel(&mut self, x: i32, y: i32, source: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let Ok(x) = u32::try_from(x) else {
            return;
        };
        let Ok(y) = u32::try_from(y) else {
            return;
        };
        if x >= self.size.width || y >= self.size.height {
            return;
        }
        let index = usize::try_from(y).expect("bounded surface coordinate fits usize")
            * usize::try_from(self.size.width).expect("bounded surface width fits usize")
            + usize::try_from(x).expect("bounded surface coordinate fits usize");
        let alpha = (source >> 24) & 0xff;
        if alpha == 0 {
            return;
        }
        if alpha == 0xff {
            self.pixels[index] = source & 0x00ff_ffff;
            return;
        }
        let destination = self.pixels[index];
        let inverse = 0xff - alpha;
        let red = (((source >> 16) & 0xff) * alpha + ((destination >> 16) & 0xff) * inverse + 0x7f)
            / 0xff;
        let green =
            (((source >> 8) & 0xff) * alpha + ((destination >> 8) & 0xff) * inverse + 0x7f) / 0xff;
        let blue = ((source & 0xff) * alpha + (destination & 0xff) * inverse + 0x7f) / 0xff;
        self.pixels[index] = (red << 16) | (green << 8) | blue;
    }

    fn draw_player_arrow(&mut self, center_x: i32, center_y: i32, rotation_degrees: f32) {
        let radians = f64::from(rotation_degrees).to_radians();
        let (sine, cosine) = radians.sin_cos();
        let arrow_scale =
            (f64::from(self.size.width.min(self.size.height)) / 320.0).clamp(1.0, 1.25);
        let puck_radius = scaled_px(13, arrow_scale);
        let puck_inner_radius = scaled_px(11, arrow_scale);
        for dy in -puck_radius..=puck_radius {
            for dx in -puck_radius..=puck_radius {
                let distance = dx * dx + dy * dy;
                if distance <= puck_radius * puck_radius {
                    self.blend_pixel(center_x + dx, center_y + dy, 0xb807_1019);
                }
                if distance <= puck_radius * puck_radius
                    && distance >= puck_inner_radius * puck_inner_radius
                {
                    self.blend_pixel(center_x + dx, center_y + dy, 0xc068_b7c0);
                }
            }
        }

        let outline_tip = f64::from(scaled_px(19, arrow_scale));
        let outline_half_width = f64::from(scaled_px(10, arrow_scale));
        let outline_shoulder_y = f64::from(scaled_px(7, arrow_scale));
        let outline_tail = f64::from(scaled_px(11, arrow_scale));
        let fill_tip = f64::from(scaled_px(16, arrow_scale));
        let fill_half_width = f64::from(scaled_px(7, arrow_scale));
        let fill_shoulder_y = f64::from(scaled_px(6, arrow_scale));
        let fill_tail = f64::from(scaled_px(8, arrow_scale));
        let raster_extent = scaled_px(21, arrow_scale);

        for screen_dy in -raster_extent..=raster_extent {
            for screen_dx in -raster_extent..=raster_extent {
                // Inverse-rotate the screen sample into the marker's local north-facing space.
                // Sampling in destination space keeps diagonal rotations solid rather than leaving holes.
                let local_x = cosine * f64::from(screen_dx) + sine * f64::from(screen_dy);
                let local_y = -sine * f64::from(screen_dx) + cosine * f64::from(screen_dy);

                let in_outline = point_in_compass_kite(
                    local_x,
                    local_y,
                    outline_tip,
                    outline_half_width,
                    outline_shoulder_y,
                    outline_tail,
                );
                if !in_outline {
                    continue;
                }

                let mut color = ICON_SHADOW_COLOR;
                if point_in_compass_kite(
                    local_x,
                    local_y,
                    fill_tip,
                    fill_half_width,
                    fill_shoulder_y,
                    fill_tail,
                ) {
                    color = ICON_WHITE;
                    let color_inset = f64::from(scaled_px(2, arrow_scale));
                    let forward_base_y = f64::from(scaled_px(2, arrow_scale));
                    if local_y <= forward_base_y
                        && point_in_triangle(
                            local_x,
                            local_y,
                            (0.0, -fill_tip + color_inset),
                            (fill_half_width - color_inset, fill_shoulder_y - color_inset),
                            (
                                -fill_half_width + color_inset,
                                fill_shoulder_y - color_inset,
                            ),
                        )
                    {
                        color = PLAYER_COLOR;
                    } else if local_y >= forward_base_y {
                        color = PLAYER_REAR_COLOR;
                    }
                }
                self.set_pixel(center_x + screen_dx, center_y + screen_dy, color);
            }
        }

        let center_radius = scaled_px(2, arrow_scale);
        for dy in -center_radius..=center_radius {
            for dx in -center_radius..=center_radius {
                if dx * dx + dy * dy <= center_radius * center_radius {
                    self.set_pixel(center_x + dx, center_y + dy, PLAYER_RING_COLOR);
                }
            }
        }
    }

    fn project_map_point(&self, view: MapView, map_x: f64, map_y: f64) -> Option<(i32, i32)> {
        let rotation = f64::from(view.map_rotation_degrees).to_radians();
        let (sine, cosine) = rotation.sin_cos();
        let source_x = map_x - view.center_x;
        let source_y = map_y - view.center_y;
        let screen_x = f64::from(self.size.width) * 0.5
            + (cosine.mul_add(source_x, -sine * source_y) / view.source_pixels_per_screen_pixel);
        let screen_y = f64::from(self.size.height) * 0.5
            + (sine.mul_add(source_x, cosine * source_y) / view.source_pixels_per_screen_pixel);
        if !screen_x.is_finite() || !screen_y.is_finite() {
            return None;
        }
        Some((screen_x.floor() as i32, screen_y.floor() as i32))
    }

    fn draw_search_highlight(&mut self, center_x: i32, center_y: i32) {
        let outer_radius = scaled_px(
            22,
            (f64::from(self.size.width.min(self.size.height)) / 320.0).clamp(1.0, 1.35),
        );
        let inner_radius = outer_radius.saturating_sub(3);
        for dy in -outer_radius..=outer_radius {
            for dx in -outer_radius..=outer_radius {
                let distance = dx * dx + dy * dy;
                if distance <= outer_radius * outer_radius
                    && distance >= inner_radius * inner_radius
                {
                    self.blend_pixel(center_x + dx, center_y + dy, 0xe0f2_b84b);
                }
            }
        }
        for offset in -4..=4 {
            self.blend_pixel(center_x + offset, center_y, 0xf8ff_f4d2);
            self.blend_pixel(center_x, center_y + offset, 0xf8ff_f4d2);
        }
    }

    fn draw_pois(&mut self, view: MapView, pois: &[MapPoi], filters: &PoiFilters) {
        let compact_minimap = self.size.width == self.size.height;
        let center_x = f64::from(self.size.width) * 0.5;
        let center_y = f64::from(self.size.height) * 0.5;
        let rotation = f64::from(view.map_rotation_degrees).to_radians();
        let (sine, cosine) = rotation.sin_cos();
        let scale = view.source_pixels_per_screen_pixel;
        let marker_scale = self.poi_marker_scale();
        let zoom_density = (scale / 1.25).clamp(1.0, 1.45);
        let minimum_separation = (24.0 * marker_scale * zoom_density).round() as i32;
        let minimum_separation_squared = minimum_separation * minimum_separation;
        let edge_margin = scaled_px(12, marker_scale);
        let supplemental_filter_mask = supplemental_filter_mask(filters);
        self.visible_poi_centers.clear();
        let occupancy_width = usize::try_from(
            (i32::try_from(self.size.width).expect("bounded overlay width") + 2 * edge_margin)
                .div_euclid(minimum_separation)
                + 3,
        )
        .expect("positive occupancy width");
        let occupancy_height = usize::try_from(
            (i32::try_from(self.size.height).expect("bounded overlay height") + 2 * edge_margin)
                .div_euclid(minimum_separation)
                + 3,
        )
        .expect("positive occupancy height");
        self.poi_occupancy
            .resize(occupancy_width * occupancy_height, usize::MAX);
        self.poi_occupancy.fill(usize::MAX);

        // Higher-value, more distinctive markers reserve screen space first. This keeps a zoomed
        // out map legible instead of stacking hundreds of symbols into a broken-looking blob.
        for kind in [
            MapPoiKind::Boss,
            MapPoiKind::Wanted,
            MapPoiKind::FastTravel,
            MapPoiKind::Dungeon,
            MapPoiKind::PalSpawn,
            MapPoiKind::Supplemental,
        ] {
            let enabled = match kind {
                MapPoiKind::FastTravel => filters.fast_travel,
                MapPoiKind::Boss => filters.boss,
                MapPoiKind::Wanted => filters.wanted,
                MapPoiKind::Dungeon => filters.dungeon,
                MapPoiKind::PalSpawn => !compact_minimap && !filters.selected_pal_ids.is_empty(),
                // The compact circular HUD follows only the exact in-app map filters. Reviewed
                // supplemental layers remain available on the expanded map without leaking into
                // the player's glanceable minimap.
                MapPoiKind::Supplemental => !compact_minimap && supplemental_filter_mask != 0,
            };
            if !enabled {
                continue;
            }
            for poi in pois.iter().filter(|poi| poi.kind == kind) {
                if kind == MapPoiKind::PalSpawn
                    && (!pal_spawn_filter_enabled(poi, filters)
                        || (filters.night_only && !poi.is_night_spawn()))
                {
                    continue;
                }
                if kind == MapPoiKind::Supplemental
                    && poi.supplemental_filter_bit() & supplemental_filter_mask == 0
                {
                    continue;
                }
                let source_x = poi.map_x - view.center_x;
                let source_y = poi.map_y - view.center_y;
                let screen_x = center_x + (cosine.mul_add(source_x, -sine * source_y) / scale);
                let screen_y = center_y + (sine.mul_add(source_x, cosine * source_y) / scale);
                if screen_x < -f64::from(edge_margin)
                    || screen_y < -f64::from(edge_margin)
                    || screen_x > f64::from(self.size.width) + f64::from(edge_margin)
                    || screen_y > f64::from(self.size.height) + f64::from(edge_margin)
                {
                    continue;
                }
                let (screen_x, screen_y) =
                    clamp_minimap_marker_center(self.size, screen_x, screen_y, edge_margin);
                let screen_x = screen_x.round() as i32;
                let screen_y = screen_y.round() as i32;
                let occupancy_x =
                    usize::try_from((screen_x + edge_margin).div_euclid(minimum_separation) + 1)
                        .expect("culled POI occupancy x is non-negative");
                let occupancy_y =
                    usize::try_from((screen_y + edge_margin).div_euclid(minimum_separation) + 1)
                        .expect("culled POI occupancy y is non-negative");
                let mut overlaps = false;
                for neighbor_y in
                    occupancy_y.saturating_sub(1)..=(occupancy_y + 1).min(occupancy_height - 1)
                {
                    for neighbor_x in
                        occupancy_x.saturating_sub(1)..=(occupancy_x + 1).min(occupancy_width - 1)
                    {
                        let owner = self.poi_occupancy[neighbor_y * occupancy_width + neighbor_x];
                        if owner == usize::MAX {
                            continue;
                        }
                        let (x, y) = self.visible_poi_centers[owner];
                        let dx = x - screen_x;
                        let dy = y - screen_y;
                        if dx * dx + dy * dy < minimum_separation_squared {
                            overlaps = true;
                            break;
                        }
                    }
                    if overlaps {
                        break;
                    }
                }
                if overlaps {
                    continue;
                }
                let owner = self.visible_poi_centers.len();
                self.visible_poi_centers.push((screen_x, screen_y));
                self.poi_occupancy[occupancy_y * occupancy_width + occupancy_x] = owner;
                self.draw_poi_marker(screen_x, screen_y, poi, marker_scale);
            }
        }
    }

    fn poi_marker_scale(&self) -> f64 {
        let short_side = self.size.width.min(self.size.height);
        match short_side {
            0..=319 => 0.90,
            320..=479 => 1.00,
            480..=719 => 1.27,
            720..=1_079 => 1.55,
            _ => 1.64,
        }
    }

    fn draw_poi_marker(&mut self, center_x: i32, center_y: i32, poi: &MapPoi, scale: f64) {
        match poi.kind {
            MapPoiKind::FastTravel => {
                self.draw_compact_icon_backplate(center_x, center_y, scale);
                if !self.draw_game_map_icon(
                    center_x,
                    center_y,
                    poi.icon(),
                    scale,
                    poi_icon_visual_policy(poi),
                ) {
                    self.draw_fast_travel_icon(center_x, center_y, scale);
                }
            }
            MapPoiKind::Boss => self.draw_boss_icon(center_x, center_y, poi.icon(), scale),
            MapPoiKind::Wanted => {
                self.draw_emphasis_frame(center_x, center_y, scale, WANTED_COLOR);
                if !self.draw_game_map_icon(
                    center_x,
                    center_y,
                    poi.icon(),
                    scale,
                    poi_icon_visual_policy(poi),
                ) {
                    self.draw_wanted_icon(center_x, center_y, scale);
                }
            }
            MapPoiKind::Dungeon => {
                self.draw_compact_icon_backplate(center_x, center_y, scale);
                if !self.draw_game_map_icon(
                    center_x,
                    center_y,
                    poi.icon(),
                    scale,
                    poi_icon_visual_policy(poi),
                ) {
                    self.draw_dungeon_icon(center_x, center_y, scale);
                }
            }
            MapPoiKind::Supplemental => {
                self.draw_compact_icon_backplate(center_x, center_y, scale * 0.9);
                if !self.draw_game_map_icon(
                    center_x,
                    center_y,
                    poi.icon(),
                    scale,
                    poi_icon_visual_policy(poi),
                ) {
                    self.draw_unverified_map_marker(center_x, center_y, scale);
                }
            }
            MapPoiKind::PalSpawn => {
                self.draw_pal_spawn_icon(center_x, center_y, poi.icon(), scale);
            }
        }
    }

    fn draw_game_map_icon(
        &mut self,
        center_x: i32,
        center_y: i32,
        icon: Option<&MapPoiIcon>,
        scale: f64,
        policy: IconVisualPolicy,
    ) -> bool {
        let Some(icon) = icon else {
            return false;
        };
        let marker_side = f64::from(scaled_px(22, scale));
        let center_x = center_x + (marker_side * policy.offset_x_fraction).round() as i32;
        let center_y = center_y + (marker_side * policy.offset_y_fraction).round() as i32;
        let target_side = u32::try_from(scaled_px(22, scale * policy.artwork_scale))
            .expect("positive icon size")
            | 1;
        let half = i32::try_from(target_side / 2).expect("small icon half fits i32");
        for dy in -half..=half {
            for dx in -half..=half {
                let source = icon.sample_scaled_visible_fit(
                    u32::try_from(dx + half).expect("map icon x is non-negative"),
                    u32::try_from(dy + half).expect("map icon y is non-negative"),
                    target_side,
                );
                if source >> 24 != 0 {
                    self.blend_pixel(center_x + dx + 1, center_y + dy + 1, 0x7007_1019);
                    self.blend_pixel(center_x + dx, center_y + dy, source);
                }
            }
        }
        true
    }

    fn draw_pal_spawn_icon(
        &mut self,
        center_x: i32,
        center_y: i32,
        icon: Option<&MapPoiIcon>,
        scale: f64,
    ) {
        let Some(icon) = icon else {
            return;
        };
        let outer_radius = scaled_px(11, scale);
        for dy in -outer_radius..=outer_radius {
            for dx in -outer_radius..=outer_radius {
                if dx * dx + dy * dy <= outer_radius * outer_radius {
                    self.set_pixel(center_x + dx + 1, center_y + dy + 2, ICON_SHADOW_COLOR);
                    self.set_pixel(center_x + dx, center_y + dy, ICON_WHITE);
                }
            }
        }
        let target_side = u32::try_from(scaled_px(21, scale)).expect("positive icon size") | 1;
        let half = i32::try_from(target_side / 2).expect("small icon half fits i32");
        for dy in -half..=half {
            for dx in -half..=half {
                if dx * dx + dy * dy > half * half {
                    continue;
                }
                let source = icon.sample_scaled_visible_fit(
                    u32::try_from(dx + half).expect("Pal icon x is non-negative"),
                    u32::try_from(dy + half).expect("Pal icon y is non-negative"),
                    target_side,
                );
                self.blend_pixel(center_x + dx, center_y + dy, source);
            }
        }
    }

    fn draw_fast_travel_icon(&mut self, center_x: i32, center_y: i32, scale: f64) {
        let half_width = scaled_px(7, scale);
        let half_height = scaled_px(9, scale);
        for dy in -half_height..=half_height {
            for dx in -half_width..=half_width {
                let source_x = (f64::from(dx) / scale).abs();
                let source_y = (f64::from(dy) / scale).abs();
                let metric = source_x * 10.0 + source_y * 7.0;
                let color = if metric <= 47.0 {
                    FAST_TRAVEL_COLOR
                } else if metric <= 61.0 {
                    FAST_TRAVEL_DARK
                } else {
                    continue;
                };
                self.set_pixel(center_x + dx + 1, center_y + dy + 1, ICON_SHADOW_COLOR);
                self.set_pixel(center_x + dx, center_y + dy, color);
            }
        }

        // The in-game marker reads primarily as a compact cyan waypoint diamond. Keep the
        // silhouette clean instead of inventing a mast/crown glyph inside it.
        let inner_half_width = scaled_px(2, scale);
        let inner_half_height = scaled_px(3, scale);
        for dy in -inner_half_height..=inner_half_height {
            for dx in -inner_half_width..=inner_half_width {
                let metric = dx.abs() * 3 + dy.abs() * 2;
                if metric <= scaled_px(6, scale) {
                    self.set_pixel(center_x + dx, center_y + dy, FAST_TRAVEL_DARK);
                }
            }
        }
    }

    fn draw_compact_icon_backplate(&mut self, center_x: i32, center_y: i32, scale: f64) {
        let radius = scaled_px(11, scale);
        let inner_radius = (radius - scaled_px(1, scale)).max(1);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let distance = dx * dx + dy * dy;
                if distance <= radius * radius {
                    self.blend_pixel(center_x + dx, center_y + dy, 0x9407_141d);
                }
                if distance <= radius * radius && distance >= inner_radius * inner_radius {
                    self.blend_pixel(center_x + dx, center_y + dy, 0x7029_5065);
                }
            }
        }
    }

    fn draw_unverified_map_marker(&mut self, center_x: i32, center_y: i32, scale: f64) {
        // This state is intentionally neutral: it communicates that the position is valid while
        // avoiding a fabricated resource or character silhouette when artwork is unavailable.
        let radius = scaled_px(3, scale);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    self.blend_pixel(center_x + dx, center_y + dy, 0xd0a8_c2c8);
                }
            }
        }
    }

    fn draw_emphasis_frame(&mut self, center_x: i32, center_y: i32, scale: f64, accent: u32) {
        let outer_radius = scaled_px(12, scale);
        let white_radius = scaled_px(11, scale);
        let accent_radius = scaled_px(10, scale);
        for dy in -outer_radius..=outer_radius {
            for dx in -outer_radius..=outer_radius {
                let distance = dx * dx + dy * dy;
                if distance <= outer_radius * outer_radius {
                    self.set_pixel(center_x + dx + 1, center_y + dy + 2, ICON_SHADOW_COLOR);
                }
                if distance <= white_radius * white_radius {
                    self.set_pixel(center_x + dx, center_y + dy, ICON_WHITE);
                }
                if distance <= accent_radius * accent_radius {
                    self.set_pixel(center_x + dx, center_y + dy, accent);
                }
            }
        }
    }

    fn draw_dungeon_icon(&mut self, center_x: i32, center_y: i32, scale: f64) {
        // Palworld's dungeon marker is a cave/portal silhouette, not a decorated circular badge.
        let arch_radius = scaled_px(7, scale);
        let arch_inner = scaled_px(4, scale);
        let floor_y = scaled_px(6, scale);
        for dy in -arch_radius..=floor_y {
            for dx in -arch_radius..=arch_radius {
                let distance = dx * dx + dy.min(0) * dy.min(0);
                let on_arch = dy <= 0
                    && distance <= arch_radius * arch_radius
                    && distance >= arch_inner * arch_inner;
                let on_side = dy > 0 && dx.abs() >= arch_inner && dx.abs() <= arch_radius;
                if on_arch || on_side {
                    self.set_pixel(center_x + dx + 1, center_y + dy + 1, ICON_SHADOW_COLOR);
                    self.set_pixel(center_x + dx, center_y + dy, DUNGEON_ACCENT_COLOR);
                } else if dy >= -arch_inner && dy <= floor_y && dx.abs() < arch_inner {
                    self.set_pixel(center_x + dx, center_y + dy, DUNGEON_MOUTH_COLOR);
                }
            }
        }
        for dx in -arch_radius..=arch_radius {
            self.set_pixel(center_x + dx + 1, center_y + floor_y + 1, ICON_SHADOW_COLOR);
            self.set_pixel(center_x + dx, center_y + floor_y, DUNGEON_RING_COLOR);
        }
    }

    fn draw_boss_icon(
        &mut self,
        center_x: i32,
        center_y: i32,
        icon: Option<&MapPoiIcon>,
        scale: f64,
    ) {
        self.draw_boss_frame(center_x, center_y, scale);

        // A missing portrait deliberately leaves the neutral boss plate visible. This is clearer
        // than substituting a different character and still preserves the small danger accent.
        if let Some(icon) = icon {
            let target_side = u32::try_from(scaled_px(21, scale)).expect("positive icon size") | 1;
            let half = i32::try_from(target_side / 2).expect("small icon half fits i32");
            for dy in -half..=half {
                for dx in -half..=half {
                    if dx * dx + dy * dy > half * half {
                        continue;
                    }
                    let source = icon.sample_scaled_visible_fit(
                        u32::try_from(dx + half).expect("boss icon x is non-negative"),
                        u32::try_from(dy + half).expect("boss icon y is non-negative"),
                        target_side,
                    );
                    self.blend_pixel(center_x + dx, center_y + dy, source);
                }
            }
        }
    }

    fn draw_boss_frame(&mut self, center_x: i32, center_y: i32, scale: f64) {
        let outer_radius = scaled_px(13, scale);
        let ring_radius = scaled_px(12, scale);
        let plate_radius = scaled_px(10, scale);
        for dy in -outer_radius..=outer_radius {
            for dx in -outer_radius..=outer_radius {
                let distance = dx * dx + dy * dy;
                if distance <= outer_radius * outer_radius {
                    self.set_pixel(center_x + dx + 1, center_y + dy + 2, ICON_SHADOW_COLOR);
                }
                if distance <= ring_radius * ring_radius {
                    self.set_pixel(center_x + dx, center_y + dy, BOSS_RING_COLOR);
                }
                if distance <= plate_radius * plate_radius {
                    self.set_pixel(center_x + dx, center_y + dy, BOSS_PLATE_COLOR);
                }
            }
        }

        let accent_y = center_y - ring_radius;
        let accent_half_width = scaled_px(3, scale);
        let accent_height = scaled_px(2, scale);
        for dy in 0..=accent_height {
            let inset = dy.min(accent_half_width);
            for dx in -(accent_half_width - inset)..=(accent_half_width - inset) {
                self.set_pixel(center_x + dx, accent_y + dy, BOSS_ACCENT_COLOR);
            }
        }
    }

    fn draw_wanted_icon(&mut self, center_x: i32, center_y: i32, scale: f64) {
        // Keep wanted targets distinct from Pal bosses without inventing a mascot. A compact
        // red-and-white reticle matches the game's danger-marker language and stays readable at
        // mini-map scale.
        let radius = scaled_px(8, scale);
        let inner_radius = scaled_px(5, scale);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let distance_squared = dx * dx + dy * dy;
                if distance_squared <= radius * radius {
                    let color = if distance_squared <= inner_radius * inner_radius {
                        WANTED_DARK
                    } else {
                        WANTED_COLOR
                    };
                    self.set_pixel(center_x + dx, center_y + dy, color);
                }
            }
        }

        let arm = scaled_px(4, scale);
        let thickness = scaled_px(1, scale);
        for offset in -arm..=arm {
            for cross in -thickness..=thickness {
                self.set_pixel(center_x + offset, center_y + cross, ICON_WHITE);
                self.set_pixel(center_x + cross, center_y + offset, ICON_WHITE);
            }
        }
        let center_radius = scaled_px(1, scale);
        for dy in -center_radius..=center_radius {
            for dx in -center_radius..=center_radius {
                self.set_pixel(center_x + dx, center_y + dy, WANTED_COLOR);
            }
        }
    }
}

fn pal_spawn_filter_enabled(poi: &MapPoi, filters: &PoiFilters) -> bool {
    let Some(species) = poi.filter_id() else {
        return false;
    };
    filters.selected_pal_ids.iter().any(|selected| {
        species.eq_ignore_ascii_case(selected)
            || (species.len() > selected.len()
                && species.as_bytes()[species.len() - selected.len() - 1] == b'_'
                && species[species.len() - selected.len()..].eq_ignore_ascii_case(selected))
    })
}

pub type ActualMapSurface = CpuMapSurface;
pub type ActualMapSurfaceError = CpuMapSurfaceError;

fn surface_pixel_count(size: crate::PhysicalSize) -> Result<usize, CpuMapSurfaceError> {
    if size.width == 0
        || size.height == 0
        || size.width > MAX_SURFACE_SIDE
        || size.height > MAX_SURFACE_SIDE
    {
        return Err(CpuMapSurfaceError::DimensionsOutOfRange);
    }
    usize::try_from(size.width)
        .ok()
        .and_then(|width| {
            usize::try_from(size.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(CpuMapSurfaceError::DimensionsOutOfRange)
}

fn current_thread_cpu_100ns() -> Option<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the pseudo-handle refers to the current thread and all FILETIME pointers are writable.
    if unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    Some(filetime_100ns(kernel).saturating_add(filetime_100ns(user)))
}

fn filetime_100ns(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn sample_bilinear(map: &MapRaster, x: f64, y: f64) -> Option<u32> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    if x < -0.5 || y < -0.5 || x > f64::from(map.width) - 0.5 || y > f64::from(map.height) - 0.5 {
        return None;
    }
    let x = x.clamp(0.0, f64::from(map.width - 1));
    let y = y.clamp(0.0, f64::from(map.height - 1));
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(map.width - 1);
    let y1 = (y0 + 1).min(map.height - 1);
    Some(interpolate_packed(
        map.pixel(x0, y0).expect("bounded map sample"),
        map.pixel(x1, y0).expect("bounded map sample"),
        map.pixel(x0, y1).expect("bounded map sample"),
        map.pixel(x1, y1).expect("bounded map sample"),
        x - f64::from(x0),
        y - f64::from(y0),
    ))
}

fn sample_argb_bilinear(width: u32, height: u32, pixels: &[u32], x: f64, y: f64) -> u32 {
    let x = x.clamp(0.0, f64::from(width - 1));
    let y = y.clamp(0.0, f64::from(height - 1));
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let pixel = |sample_x: u32, sample_y: u32| {
        let index = usize::try_from(sample_y).expect("bounded icon y fits usize")
            * usize::try_from(width).expect("bounded icon width fits usize")
            + usize::try_from(sample_x).expect("bounded icon x fits usize");
        pixels[index]
    };
    interpolate_argb_premultiplied(
        pixel(x0, y0),
        pixel(x1, y0),
        pixel(x0, y1),
        pixel(x1, y1),
        x - f64::from(x0),
        y - f64::from(y0),
    )
}

fn interpolate_argb_premultiplied(
    top_left: u32,
    top_right: u32,
    bottom_left: u32,
    bottom_right: u32,
    x_weight: f64,
    y_weight: f64,
) -> u32 {
    let samples = [
        (top_left, (1.0 - x_weight) * (1.0 - y_weight)),
        (top_right, x_weight * (1.0 - y_weight)),
        (bottom_left, (1.0 - x_weight) * y_weight),
        (bottom_right, x_weight * y_weight),
    ];
    let alpha = samples
        .iter()
        .map(|(pixel, weight)| f64::from((pixel >> 24) & 0xff) * weight)
        .sum::<f64>();
    if alpha <= f64::EPSILON {
        return 0;
    }
    let channel = |shift: u32| {
        let premultiplied = samples
            .iter()
            .map(|(pixel, weight)| {
                let sample_alpha = f64::from((pixel >> 24) & 0xff) / 255.0;
                f64::from((pixel >> shift) & 0xff) * sample_alpha * weight
            })
            .sum::<f64>();
        (premultiplied * 255.0 / alpha).round().clamp(0.0, 255.0) as u32
    };
    ((alpha.round().clamp(0.0, 255.0) as u32) << 24)
        | (channel(16) << 16)
        | (channel(8) << 8)
        | channel(0)
}

fn interpolate_packed(
    top_left: u32,
    top_right: u32,
    bottom_left: u32,
    bottom_right: u32,
    x_weight: f64,
    y_weight: f64,
) -> u32 {
    [24_u32, 16, 8, 0]
        .into_iter()
        .map(|shift| {
            let top = f64::from((top_left >> shift) & 0xff)
                + (f64::from((top_right >> shift) & 0xff) - f64::from((top_left >> shift) & 0xff))
                    * x_weight;
            let bottom = f64::from((bottom_left >> shift) & 0xff)
                + (f64::from((bottom_right >> shift) & 0xff)
                    - f64::from((bottom_left >> shift) & 0xff))
                    * x_weight;
            ((top + (bottom - top) * y_weight).round() as u32).min(0xff) << shift
        })
        .fold(0, |packed, channel| packed | channel)
}

fn point_in_compass_kite(
    x: f64,
    y: f64,
    tip: f64,
    half_width: f64,
    shoulder_y: f64,
    tail: f64,
) -> bool {
    point_in_triangle(x, y, (0.0, -tip), (half_width, shoulder_y), (0.0, tail))
        || point_in_triangle(x, y, (0.0, -tip), (0.0, tail), (-half_width, shoulder_y))
}

fn point_in_triangle(
    x: f64,
    y: f64,
    first: (f64, f64),
    second: (f64, f64),
    third: (f64, f64),
) -> bool {
    let edge = |start: (f64, f64), end: (f64, f64)| {
        (x - end.0).mul_add(start.1 - end.1, -(start.0 - end.0) * (y - end.1))
    };
    let first_edge = edge(first, second);
    let second_edge = edge(second, third);
    let third_edge = edge(third, first);
    let has_negative = first_edge < 0.0 || second_edge < 0.0 || third_edge < 0.0;
    let has_positive = first_edge > 0.0 || second_edge > 0.0 || third_edge > 0.0;
    !(has_negative && has_positive)
}

fn scaled_px(value: i32, scale: f64) -> i32 {
    (f64::from(value) * scale).round().max(1.0) as i32
}

fn clamp_minimap_marker_center(
    size: crate::PhysicalSize,
    screen_x: f64,
    screen_y: f64,
    marker_radius: i32,
) -> (f64, f64) {
    if size.width != size.height {
        return (screen_x, screen_y);
    }
    let center = f64::from(size.width) * 0.5;
    let bezel_inset = (f64::from(size.width) / 36.0).round().max(2.0);
    let safe_radius = (center - f64::from(marker_radius.max(1)) - bezel_inset).max(1.0);
    let dx = screen_x - center;
    let dy = screen_y - center;
    let distance_squared = dx.mul_add(dx, dy * dy);
    if distance_squared <= safe_radius * safe_radius || distance_squared <= f64::EPSILON {
        return (screen_x, screen_y);
    }
    let scale = safe_radius / distance_squared.sqrt();
    (center + dx * scale, center + dy * scale)
}
