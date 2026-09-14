use std::{
    mem::{align_of, size_of},
    ptr,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use shared_memory::{Shmem, ShmemConf, ShmemError};
use thiserror::Error;

// v3 makes logical display geometry part of the producer/consumer contract and permits transfer
// rasters wider or taller than 1,024 pixels. A distinct mapping keeps an older injected v2 DLL
// from advertising a heartbeat for frames it cannot consume.
pub const DEFAULT_MAPPING_ID: &str = "/pal_companion_fullscreen_frame_v3";
/// The fixed shared mapping keeps the original 1,024 x 1,024 pixel budget, but the frame no
/// longer has an artificial 1,024-pixel limit on either individual axis. Wide and ultrawide
/// rasters are valid as long as their total pixel count fits this budget.
pub const MAX_FRAME_PIXELS: usize = 1_024 * 1_024;
// The exact-build catalogue combines named POIs with one regional entry per Pal species and
// supplemental layer. Keep this bounded but large enough for both the main world and World Tree
// without truncating results or rejecting a valid package at startup.
pub const MAX_SEARCH_ENTRIES: usize = 2_048;
const BYTES_PER_PIXEL: usize = 4;
const HEADER_REGION_BYTES: usize = 256;
const SEARCH_TITLE_BYTES: usize = 72;
const SEARCH_SUBTITLE_BYTES: usize = 72;
const MAGIC: u64 = u64::from_le_bytes(*b"PALFSV02");
const SCHEMA_VERSION: u32 = 3;
const MAX_PAYLOAD_BYTES: usize = MAX_FRAME_PIXELS * BYTES_PER_PIXEL;
const SEARCH_REGION_BYTES: usize = size_of::<SharedSearchEntry>() * MAX_SEARCH_ENTRIES;
const MAPPING_BYTES: usize = HEADER_REGION_BYTES + SEARCH_REGION_BYTES + MAX_PAYLOAD_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FrameDisplayMode {
    MiniMap = 0,
    ExpandedMap = 1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FrameInputMode {
    Locked = 0,
    Interactive = 1,
}

impl FrameInputMode {
    fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Locked),
            1 => Some(Self::Interactive),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramePoiFilters {
    pub fast_travel: bool,
    pub boss: bool,
    pub wanted: bool,
    pub dungeon: bool,
    pub tower: bool,
    pub egg: bool,
    pub resources: bool,
    pub salvage: bool,
}

impl Default for FramePoiFilters {
    fn default() -> Self {
        Self {
            fast_travel: true,
            boss: true,
            wanted: false,
            dungeon: true,
            tower: true,
            egg: false,
            resources: false,
            salvage: false,
        }
    }
}

impl FramePoiFilters {
    const FAST_TRAVEL: u32 = 1 << 0;
    const BOSS: u32 = 1 << 1;
    const WANTED: u32 = 1 << 2;
    const DUNGEON: u32 = 1 << 3;
    const TOWER: u32 = 1 << 4;
    const EGG: u32 = 1 << 5;
    const RESOURCES: u32 = 1 << 6;
    const SALVAGE: u32 = 1 << 7;

    const fn to_bits(self) -> u32 {
        (if self.fast_travel {
            Self::FAST_TRAVEL
        } else {
            0
        }) | (if self.boss { Self::BOSS } else { 0 })
            | (if self.wanted { Self::WANTED } else { 0 })
            | (if self.dungeon { Self::DUNGEON } else { 0 })
            | (if self.tower { Self::TOWER } else { 0 })
            | (if self.egg { Self::EGG } else { 0 })
            | (if self.resources { Self::RESOURCES } else { 0 })
            | (if self.salvage { Self::SALVAGE } else { 0 })
    }

    const fn from_bits(bits: u32) -> Self {
        Self {
            fast_travel: bits & Self::FAST_TRAVEL != 0,
            boss: bits & Self::BOSS != 0,
            wanted: bits & Self::WANTED != 0,
            dungeon: bits & Self::DUNGEON != 0,
            tower: bits & Self::TOWER != 0,
            egg: bits & Self::EGG != 0,
            resources: bits & Self::RESOURCES != 0,
            salvage: bits & Self::SALVAGE != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ConsumerAction {
    ToggleFastTravel = 1,
    ToggleBoss = 2,
    ToggleWanted = 3,
    ToggleDungeon = 4,
    ToggleTower = 5,
    ToggleEgg = 6,
    ToggleResources = 7,
    ToggleSalvage = 8,
    CloseExpanded = 9,
    ZoomIn = 10,
    ZoomOut = 11,
    ToggleRotation = 12,
    Lock = 13,
    EnterInteractive = 14,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FrameSearchKind {
    Pal = 1,
    Poi = 2,
    Resource = 3,
}

impl FrameSearchKind {
    const fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Pal),
            2 => Some(Self::Poi),
            3 => Some(Self::Resource),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameSearchEntry {
    pub kind: FrameSearchKind,
    pub title: String,
    pub subtitle: String,
}

impl ConsumerAction {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::ToggleFastTravel),
            2 => Some(Self::ToggleBoss),
            3 => Some(Self::ToggleWanted),
            4 => Some(Self::ToggleDungeon),
            5 => Some(Self::ToggleTower),
            6 => Some(Self::ToggleEgg),
            7 => Some(Self::ToggleResources),
            8 => Some(Self::ToggleSalvage),
            9 => Some(Self::CloseExpanded),
            10 => Some(Self::ZoomIn),
            11 => Some(Self::ZoomOut),
            12 => Some(Self::ToggleRotation),
            13 => Some(Self::Lock),
            14 => Some(Self::EnterInteractive),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameControls {
    pub input_mode: FrameInputMode,
    pub poi_filters: FramePoiFilters,
}

impl Default for FrameControls {
    fn default() -> Self {
        Self {
            input_mode: FrameInputMode::Locked,
            poi_filters: FramePoiFilters::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameMetadata {
    pub display_mode: FrameDisplayMode,
    pub controls: FrameControls,
    pub opacity: f32,
    pub logical_visible: bool,
    /// Logical on-screen width. The transferred raster can be smaller and is scaled by a capable
    /// consumer. Older consumers safely ignore this additive metadata and render at raster size.
    pub display_width: u32,
    /// Logical on-screen height. See `display_width`.
    pub display_height: u32,
}

impl FrameDisplayMode {
    fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::MiniMap),
            1 => Some(Self::ExpandedMap),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ConsumerBackend {
    Unknown = 0,
    DirectX11 = 11,
    DirectX12 = 12,
}

impl ConsumerBackend {
    pub fn from_raw(value: u32) -> Self {
        match value {
            11 => Self::DirectX11,
            12 => Self::DirectX12,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FullscreenFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub display_mode: FrameDisplayMode,
    pub input_mode: FrameInputMode,
    pub poi_filters: FramePoiFilters,
    pub opacity: f32,
    pub logical_visible: bool,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsumerStatus {
    pub backend: ConsumerBackend,
    pub heartbeat_age_ms: u64,
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("shared frame dimensions are invalid")]
    InvalidDimensions,
    #[error("shared frame pixel count does not match its dimensions")]
    PixelCountMismatch,
    #[error("shared search catalog is invalid")]
    InvalidSearchCatalog,
    #[error("shared frame mapping is incompatible")]
    IncompatibleMapping,
    #[error("shared memory operation failed: {0}")]
    SharedMemory(String),
}

#[repr(C, align(64))]
struct SharedHeader {
    magic: AtomicU64,
    schema_version: AtomicU32,
    capacity_bytes: AtomicU32,
    sequence: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
    payload_bytes: AtomicU32,
    display_mode: AtomicU32,
    opacity_bits: AtomicU32,
    logical_visible: AtomicU32,
    writer_heartbeat_ms: AtomicU64,
    consumer_heartbeat_ms: AtomicU64,
    consumer_backend: AtomicU32,
    poi_filter_bits: AtomicU32,
    input_mode: AtomicU32,
    consumer_action: AtomicU32,
    consumer_action_sequence: AtomicU32,
    search_sequence: AtomicU64,
    search_count: AtomicU32,
    consumer_search_selection: AtomicU32,
    consumer_search_sequence: AtomicU32,
    reserved: [AtomicU32; 11],
}

const DISPLAY_WIDTH_RESERVED_INDEX: usize = 0;
const DISPLAY_HEIGHT_RESERVED_INDEX: usize = 1;
// Navigation is additive inside the v3 reserved region, so the existing frame/action/search
// layout stays compatible. Pan uses one atomic packed pair to preserve X/Y coherence while the
// producer and injected consumer run at different frame rates.
const CONSUMER_PAN_RESERVED_INDEX: usize = 2;
const CONSUMER_RECENTER_SEQUENCE_RESERVED_INDEX: usize = 3;
const PAN_SUBPIXEL_UNITS: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConsumerPanDelta {
    pub raster_delta_x: f32,
    pub raster_delta_y: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SharedSearchEntry {
    kind: u32,
    title_length: u32,
    subtitle_length: u32,
    title: [u8; SEARCH_TITLE_BYTES],
    subtitle: [u8; SEARCH_SUBTITLE_BYTES],
}

const _: () = assert!(size_of::<SharedHeader>() <= HEADER_REGION_BYTES);
const _: () = assert!(HEADER_REGION_BYTES.is_multiple_of(align_of::<SharedHeader>()));
const _: () = assert!(SEARCH_REGION_BYTES.is_multiple_of(align_of::<u32>()));

pub struct FrameWriter {
    mapping: Shmem,
}

impl FrameWriter {
    pub fn create_or_open_default() -> Result<Self, BridgeError> {
        Self::create_or_open(DEFAULT_MAPPING_ID, true)
    }

    fn create_or_open(os_id: &str, persistent: bool) -> Result<Self, BridgeError> {
        let mut mapping = match ShmemConf::new().os_id(os_id).size(MAPPING_BYTES).create() {
            Ok(mapping) => mapping,
            Err(ShmemError::MappingIdExists) => ShmemConf::new()
                .os_id(os_id)
                .open()
                .map_err(shared_memory_error)?,
            Err(error) => return Err(shared_memory_error(error)),
        };
        if mapping.len() < MAPPING_BYTES {
            return Err(BridgeError::IncompatibleMapping);
        }
        if persistent {
            // The fixed bridge must survive a pal-overlay restart while an injected consumer still
            // holds the mapping. The small backing mapping is deliberately reused on the next run.
            mapping.set_owner(false);
        }
        let writer = Self { mapping };
        writer.initialize_header();
        Ok(writer)
    }

    pub fn publish_rgb_u32(
        &mut self,
        width: u32,
        height: u32,
        display_mode: FrameDisplayMode,
        opacity: f32,
        logical_visible: bool,
        pixels: &[u32],
    ) -> Result<u64, BridgeError> {
        self.publish_rgb_u32_with_controls(
            width,
            height,
            FrameMetadata {
                display_mode,
                controls: FrameControls::default(),
                opacity,
                logical_visible,
                display_width: width,
                display_height: height,
            },
            pixels,
        )
    }

    pub fn publish_rgb_u32_with_controls(
        &mut self,
        width: u32,
        height: u32,
        metadata: FrameMetadata,
        pixels: &[u32],
    ) -> Result<u64, BridgeError> {
        let pixel_count = checked_pixel_count(width, height)?;
        if metadata.display_width == 0 || metadata.display_height == 0 {
            return Err(BridgeError::InvalidDimensions);
        }
        if pixels.len() != pixel_count {
            return Err(BridgeError::PixelCountMismatch);
        }
        let payload_bytes = pixel_count
            .checked_mul(BYTES_PER_PIXEL)
            .ok_or(BridgeError::InvalidDimensions)?;
        let header = self.header();
        let odd_sequence = header.sequence.fetch_add(1, Ordering::AcqRel) | 1;
        header.width.store(width, Ordering::Relaxed);
        header.height.store(height, Ordering::Relaxed);
        header
            .payload_bytes
            .store(payload_bytes as u32, Ordering::Relaxed);
        header
            .display_mode
            .store(metadata.display_mode as u32, Ordering::Relaxed);
        header
            .input_mode
            .store(metadata.controls.input_mode as u32, Ordering::Relaxed);
        header
            .poi_filter_bits
            .store(metadata.controls.poi_filters.to_bits(), Ordering::Relaxed);
        header.opacity_bits.store(
            metadata.opacity.clamp(0.2, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        header
            .logical_visible
            .store(u32::from(metadata.logical_visible), Ordering::Relaxed);
        header.reserved[DISPLAY_WIDTH_RESERVED_INDEX]
            .store(metadata.display_width, Ordering::Relaxed);
        header.reserved[DISPLAY_HEIGHT_RESERVED_INDEX]
            .store(metadata.display_height, Ordering::Relaxed);
        header
            .writer_heartbeat_ms
            .store(unix_ms(), Ordering::Relaxed);

        // SAFETY: the mapping is at least MAPPING_BYTES, payload_bytes is bounded by the checked
        // dimensions, and the seqlock keeps readers from accepting a partially converted frame.
        let target = unsafe { std::slice::from_raw_parts_mut(self.payload_ptr(), payload_bytes) };
        for (index, (source, rgba)) in pixels
            .iter()
            .zip(target.chunks_exact_mut(BYTES_PER_PIXEL))
            .enumerate()
        {
            rgba[0] = (source >> 16) as u8;
            rgba[1] = (source >> 8) as u8;
            rgba[2] = *source as u8;
            rgba[3] = frame_alpha(index, width, height, metadata.display_mode);
        }
        let published = odd_sequence.wrapping_add(1) & !1;
        header.sequence.store(published, Ordering::Release);
        Ok(published)
    }

    pub fn take_consumer_action(&self, last_sequence: u32) -> Option<(u32, ConsumerAction)> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let sequence = header.consumer_action_sequence.load(Ordering::Acquire);
        if sequence == 0 || sequence == last_sequence {
            return None;
        }
        ConsumerAction::from_raw(header.consumer_action.load(Ordering::Relaxed))
            .map(|action| (sequence, action))
    }

    /// Drains the accumulated drag delta expressed in producer raster pixels.
    ///
    /// The injected renderer can publish several mouse samples between producer ticks. Draining
    /// an accumulated delta instead of a latest-only sample prevents slow producer frames from
    /// losing part of a drag gesture.
    pub fn take_consumer_pan_delta(&self) -> Option<ConsumerPanDelta> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        decode_pan_delta(header.reserved[CONSUMER_PAN_RESERVED_INDEX].swap(0, Ordering::AcqRel))
    }

    pub fn take_consumer_recenter(&self, last_sequence: u32) -> Option<u32> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let sequence =
            header.reserved[CONSUMER_RECENTER_SEQUENCE_RESERVED_INDEX].load(Ordering::Acquire);
        (sequence != 0 && sequence != last_sequence).then_some(sequence)
    }

    pub fn publish_search_catalog(&self, entries: &[FrameSearchEntry]) -> Result<u64, BridgeError> {
        if entries.len() > MAX_SEARCH_ENTRIES {
            return Err(BridgeError::InvalidSearchCatalog);
        }
        let header = self.header();
        let odd_sequence = header.search_sequence.fetch_add(1, Ordering::AcqRel) | 1;
        // SAFETY: the search region is reserved for exactly MAX_SEARCH_ENTRIES fixed records.
        let target = unsafe {
            std::slice::from_raw_parts_mut(
                self.search_ptr().cast::<SharedSearchEntry>(),
                MAX_SEARCH_ENTRIES,
            )
        };
        target.fill(SharedSearchEntry::empty());
        for (target, source) in target.iter_mut().zip(entries) {
            *target = SharedSearchEntry::from_public(source)?;
        }
        header
            .search_count
            .store(entries.len() as u32, Ordering::Relaxed);
        let published = odd_sequence.wrapping_add(1) & !1;
        header.search_sequence.store(published, Ordering::Release);
        Ok(published)
    }

    pub fn take_consumer_search_selection(&self, last_sequence: u32) -> Option<(u32, usize)> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let sequence = header.consumer_search_sequence.load(Ordering::Acquire);
        if sequence == 0 || sequence == last_sequence {
            return None;
        }
        let index = header.consumer_search_selection.load(Ordering::Relaxed) as usize;
        (index < header.search_count.load(Ordering::Acquire) as usize).then_some((sequence, index))
    }

    pub fn set_logical_visible(&self, visible: bool) {
        let header = self.header();
        let visible = u32::from(visible);
        if header.logical_visible.load(Ordering::Acquire) == visible {
            header
                .writer_heartbeat_ms
                .store(unix_ms(), Ordering::Release);
            return;
        }

        // Visibility belongs to the accepted frame snapshot. Publish it through the same seqlock
        // as pixel/metadata frames so an injected reader sees hide/show immediately without ever
        // accepting the old visibility under a new payload (or vice versa).
        let odd_sequence = header.sequence.fetch_add(1, Ordering::AcqRel) | 1;
        header.logical_visible.store(visible, Ordering::Relaxed);
        header
            .writer_heartbeat_ms
            .store(unix_ms(), Ordering::Relaxed);
        let published = odd_sequence.wrapping_add(1) & !1;
        header.sequence.store(published, Ordering::Release);
    }

    pub fn consumer_status(&self, maximum_age_ms: u64) -> Option<ConsumerStatus> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let heartbeat = header.consumer_heartbeat_ms.load(Ordering::Acquire);
        if heartbeat == 0 {
            return None;
        }
        let age = unix_ms().saturating_sub(heartbeat);
        (age <= maximum_age_ms).then(|| ConsumerStatus {
            backend: ConsumerBackend::from_raw(header.consumer_backend.load(Ordering::Acquire)),
            heartbeat_age_ms: age,
        })
    }

    fn initialize_header(&self) {
        let header = self.header();
        header.sequence.fetch_add(1, Ordering::AcqRel);
        header.magic.store(MAGIC, Ordering::Relaxed);
        header
            .schema_version
            .store(SCHEMA_VERSION, Ordering::Relaxed);
        header
            .capacity_bytes
            .store(MAX_PAYLOAD_BYTES as u32, Ordering::Relaxed);
        header.width.store(0, Ordering::Relaxed);
        header.height.store(0, Ordering::Relaxed);
        header.payload_bytes.store(0, Ordering::Relaxed);
        header
            .poi_filter_bits
            .store(FramePoiFilters::default().to_bits(), Ordering::Relaxed);
        header
            .input_mode
            .store(FrameInputMode::Locked as u32, Ordering::Relaxed);
        header.consumer_action.store(0, Ordering::Relaxed);
        header.consumer_action_sequence.store(0, Ordering::Relaxed);
        header.search_sequence.store(0, Ordering::Relaxed);
        header.search_count.store(0, Ordering::Relaxed);
        header
            .consumer_search_selection
            .store(u32::MAX, Ordering::Relaxed);
        header.consumer_search_sequence.store(0, Ordering::Relaxed);
        for reserved in &header.reserved {
            reserved.store(0, Ordering::Relaxed);
        }
        header.logical_visible.store(0, Ordering::Relaxed);
        header.writer_heartbeat_ms.store(0, Ordering::Relaxed);
        // A previous injected consumer may have disappeared without clearing its heartbeat.
        // Reset the lease whenever the authoritative writer starts; a healthy DLL reacquires it
        // after receiving the first complete frame.
        header.consumer_heartbeat_ms.store(0, Ordering::Relaxed);
        header
            .consumer_backend
            .store(ConsumerBackend::Unknown as u32, Ordering::Relaxed);
        let sequence = header.sequence.load(Ordering::Relaxed);
        header
            .sequence
            .store(sequence.wrapping_add(1) & !1, Ordering::Release);
    }

    fn header(&self) -> &SharedHeader {
        // SAFETY: shared_memory mappings are page aligned and remain valid for self's lifetime.
        unsafe { &*self.mapping.as_ptr().cast::<SharedHeader>() }
    }

    fn payload_ptr(&self) -> *mut u8 {
        // SAFETY: the header and search regions are within every validated mapping.
        unsafe {
            self.mapping
                .as_ptr()
                .add(HEADER_REGION_BYTES + SEARCH_REGION_BYTES)
        }
    }

    fn search_ptr(&self) -> *mut u8 {
        // SAFETY: HEADER_REGION_BYTES is within every validated mapping.
        unsafe { self.mapping.as_ptr().add(HEADER_REGION_BYTES) }
    }
}

pub struct FrameReader {
    mapping: Shmem,
}

impl FrameReader {
    pub fn open_default() -> Result<Self, BridgeError> {
        Self::open(DEFAULT_MAPPING_ID)
    }

    fn open(os_id: &str) -> Result<Self, BridgeError> {
        let mapping = ShmemConf::new()
            .os_id(os_id)
            .open()
            .map_err(shared_memory_error)?;
        if mapping.len() < MAPPING_BYTES {
            return Err(BridgeError::IncompatibleMapping);
        }
        let reader = Self { mapping };
        if !header_compatible(reader.header()) {
            return Err(BridgeError::IncompatibleMapping);
        }
        Ok(reader)
    }

    pub fn read_latest(&self, last_sequence: u64) -> Option<FullscreenFrame> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let before = header.sequence.load(Ordering::Acquire);
        if before == 0 || before & 1 != 0 || before == last_sequence {
            return None;
        }
        let width = header.width.load(Ordering::Relaxed);
        let height = header.height.load(Ordering::Relaxed);
        let display_width = header.reserved[DISPLAY_WIDTH_RESERVED_INDEX]
            .load(Ordering::Relaxed)
            .max(width);
        let display_height = header.reserved[DISPLAY_HEIGHT_RESERVED_INDEX]
            .load(Ordering::Relaxed)
            .max(height);
        let payload_bytes = header.payload_bytes.load(Ordering::Relaxed) as usize;
        let display_mode = FrameDisplayMode::from_raw(header.display_mode.load(Ordering::Relaxed))?;
        let input_mode = FrameInputMode::from_raw(header.input_mode.load(Ordering::Relaxed))?;
        let poi_filters =
            FramePoiFilters::from_bits(header.poi_filter_bits.load(Ordering::Relaxed));
        let opacity = f32::from_bits(header.opacity_bits.load(Ordering::Relaxed));
        let logical_visible = header.logical_visible.load(Ordering::Relaxed) != 0;
        let expected = checked_pixel_count(width, height)
            .ok()?
            .checked_mul(BYTES_PER_PIXEL)?;
        if payload_bytes != expected || payload_bytes > MAX_PAYLOAD_BYTES {
            return None;
        }
        let mut rgba = vec![0; payload_bytes];
        // SAFETY: both source and destination are valid for payload_bytes, and the seqlock below
        // rejects the copy if the writer changed the shared payload concurrently.
        unsafe {
            ptr::copy_nonoverlapping(self.payload_ptr(), rgba.as_mut_ptr(), payload_bytes);
        }
        let after = header.sequence.load(Ordering::Acquire);
        if before != after || after & 1 != 0 {
            return None;
        }
        Some(FullscreenFrame {
            sequence: after,
            width,
            height,
            display_width,
            display_height,
            display_mode,
            input_mode,
            poi_filters,
            opacity: opacity.clamp(0.2, 1.0),
            logical_visible,
            rgba,
        })
    }

    pub fn writer_heartbeat_age_ms(&self) -> Option<u64> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let heartbeat = header.writer_heartbeat_ms.load(Ordering::Acquire);
        if heartbeat == 0 {
            return None;
        }
        Some(unix_ms().saturating_sub(heartbeat))
    }

    pub fn writer_is_alive(&self, maximum_age_ms: u64) -> bool {
        self.writer_heartbeat_age_ms()
            .is_some_and(|age| age <= maximum_age_ms)
    }

    pub fn read_search_catalog(&self, last_sequence: u64) -> Option<(u64, Vec<FrameSearchEntry>)> {
        let header = self.header();
        if !header_compatible(header) {
            return None;
        }
        let before = header.search_sequence.load(Ordering::Acquire);
        if before == 0 || before & 1 != 0 || before == last_sequence {
            return None;
        }
        let count = header.search_count.load(Ordering::Relaxed) as usize;
        if count > MAX_SEARCH_ENTRIES {
            return None;
        }
        // SAFETY: the mapping owns a fixed search region with MAX_SEARCH_ENTRIES records.
        let source = unsafe {
            std::slice::from_raw_parts(self.search_ptr().cast::<SharedSearchEntry>(), count)
        };
        let entries = source
            .iter()
            .map(SharedSearchEntry::decode_public)
            .collect::<Option<Vec<_>>>()?;
        let after = header.search_sequence.load(Ordering::Acquire);
        if before != after || after & 1 != 0 {
            return None;
        }
        Some((after, entries))
    }

    pub fn mark_consumer_alive(&self, backend: ConsumerBackend) {
        let header = self.header();
        header
            .consumer_backend
            .store(backend as u32, Ordering::Relaxed);
        header
            .consumer_heartbeat_ms
            .store(unix_ms(), Ordering::Release);
    }

    pub fn mark_consumer_stopped(&self) {
        self.header()
            .consumer_heartbeat_ms
            .store(0, Ordering::Release);
    }

    pub fn submit_action(&self, action: ConsumerAction) -> u32 {
        let header = self.header();
        header
            .consumer_action
            .store(action as u32, Ordering::Relaxed);
        let mut sequence = header
            .consumer_action_sequence
            .fetch_add(1, Ordering::Release)
            .wrapping_add(1);
        if sequence == 0 {
            header.consumer_action_sequence.store(1, Ordering::Release);
            sequence = 1;
        }
        sequence
    }

    /// Accumulates a drag delta in producer raster pixels.
    ///
    /// Each axis is stored at 1/16-pixel precision in a signed 16-bit lane. Saturation is
    /// intentional: it bounds malformed input while retaining far more than one normal frame's
    /// cursor travel.
    pub fn submit_pan_delta(&self, raster_delta_x: f32, raster_delta_y: f32) -> bool {
        let Some(delta) = encode_pan_delta(raster_delta_x, raster_delta_y) else {
            return false;
        };
        let packed = &self.header().reserved[CONSUMER_PAN_RESERVED_INDEX];
        packed
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                Some(accumulate_packed_pan(current, delta))
            })
            .is_ok()
    }

    pub fn submit_recenter(&self) -> u32 {
        let sequence = &self.header().reserved[CONSUMER_RECENTER_SEQUENCE_RESERVED_INDEX];
        let mut next = sequence.fetch_add(1, Ordering::Release).wrapping_add(1);
        if next == 0 {
            sequence.store(1, Ordering::Release);
            next = 1;
        }
        next
    }

    pub fn submit_search_selection(&self, index: usize) -> Option<u32> {
        let index = u32::try_from(index).ok()?;
        let header = self.header();
        if index >= header.search_count.load(Ordering::Acquire) {
            return None;
        }
        header
            .consumer_search_selection
            .store(index, Ordering::Relaxed);
        let mut sequence = header
            .consumer_search_sequence
            .fetch_add(1, Ordering::Release)
            .wrapping_add(1);
        if sequence == 0 {
            header.consumer_search_sequence.store(1, Ordering::Release);
            sequence = 1;
        }
        Some(sequence)
    }

    fn header(&self) -> &SharedHeader {
        // SAFETY: shared_memory mappings are page aligned and remain valid for self's lifetime.
        unsafe { &*self.mapping.as_ptr().cast::<SharedHeader>() }
    }

    fn payload_ptr(&self) -> *const u8 {
        // SAFETY: the header and search regions are within every validated mapping.
        unsafe {
            self.mapping
                .as_ptr()
                .add(HEADER_REGION_BYTES + SEARCH_REGION_BYTES)
        }
    }

    fn search_ptr(&self) -> *const u8 {
        // SAFETY: HEADER_REGION_BYTES is within every validated mapping.
        unsafe { self.mapping.as_ptr().add(HEADER_REGION_BYTES) }
    }
}

impl SharedSearchEntry {
    const fn empty() -> Self {
        Self {
            kind: 0,
            title_length: 0,
            subtitle_length: 0,
            title: [0; SEARCH_TITLE_BYTES],
            subtitle: [0; SEARCH_SUBTITLE_BYTES],
        }
    }

    fn from_public(entry: &FrameSearchEntry) -> Result<Self, BridgeError> {
        let mut shared = Self::empty();
        shared.kind = entry.kind as u32;
        shared.title_length = copy_utf8_prefix(&entry.title, &mut shared.title) as u32;
        shared.subtitle_length = copy_utf8_prefix(&entry.subtitle, &mut shared.subtitle) as u32;
        if shared.title_length == 0 {
            return Err(BridgeError::InvalidSearchCatalog);
        }
        Ok(shared)
    }

    fn decode_public(&self) -> Option<FrameSearchEntry> {
        let title_length = self.title_length as usize;
        let subtitle_length = self.subtitle_length as usize;
        if title_length == 0
            || title_length > SEARCH_TITLE_BYTES
            || subtitle_length > SEARCH_SUBTITLE_BYTES
        {
            return None;
        }
        Some(FrameSearchEntry {
            kind: FrameSearchKind::from_raw(self.kind)?,
            title: std::str::from_utf8(&self.title[..title_length])
                .ok()?
                .to_owned(),
            subtitle: std::str::from_utf8(&self.subtitle[..subtitle_length])
                .ok()?
                .to_owned(),
        })
    }
}

fn copy_utf8_prefix(source: &str, target: &mut [u8]) -> usize {
    let mut length = source.len().min(target.len());
    while length > 0 && !source.is_char_boundary(length) {
        length -= 1;
    }
    target[..length].copy_from_slice(&source.as_bytes()[..length]);
    length
}

fn encode_pan_delta(raster_delta_x: f32, raster_delta_y: f32) -> Option<u32> {
    if !raster_delta_x.is_finite() || !raster_delta_y.is_finite() {
        return None;
    }
    let quantize = |value: f32| {
        (value * PAN_SUBPIXEL_UNITS)
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
    };
    let x = quantize(raster_delta_x);
    let y = quantize(raster_delta_y);
    if x == 0 && y == 0 {
        return None;
    }
    Some(pack_pan_lanes(x, y))
}

fn decode_pan_delta(packed: u32) -> Option<ConsumerPanDelta> {
    let (x, y) = unpack_pan_lanes(packed);
    if x == 0 && y == 0 {
        return None;
    }
    Some(ConsumerPanDelta {
        raster_delta_x: f32::from(x) / PAN_SUBPIXEL_UNITS,
        raster_delta_y: f32::from(y) / PAN_SUBPIXEL_UNITS,
    })
}

const fn pack_pan_lanes(x: i16, y: i16) -> u32 {
    x as u16 as u32 | ((y as u16 as u32) << 16)
}

const fn unpack_pan_lanes(packed: u32) -> (i16, i16) {
    (packed as u16 as i16, (packed >> 16) as u16 as i16)
}

fn accumulate_packed_pan(current: u32, delta: u32) -> u32 {
    let (current_x, current_y) = unpack_pan_lanes(current);
    let (delta_x, delta_y) = unpack_pan_lanes(delta);
    pack_pan_lanes(
        current_x.saturating_add(delta_x),
        current_y.saturating_add(delta_y),
    )
}

fn checked_pixel_count(width: u32, height: u32) -> Result<usize, BridgeError> {
    if width == 0 || height == 0 {
        return Err(BridgeError::InvalidDimensions);
    }
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(BridgeError::InvalidDimensions)?;
    if pixel_count > MAX_FRAME_PIXELS as u64 {
        return Err(BridgeError::InvalidDimensions);
    }
    usize::try_from(pixel_count).map_err(|_| BridgeError::InvalidDimensions)
}

fn frame_alpha(index: usize, width: u32, height: u32, display_mode: FrameDisplayMode) -> u8 {
    if display_mode == FrameDisplayMode::ExpandedMap {
        return 255;
    }
    let width_usize = width as usize;
    let x = (index % width_usize) as f64 + 0.5;
    let y = (index / width_usize) as f64 + 0.5;
    let center_x = f64::from(width) * 0.5;
    let center_y = f64::from(height) * 0.5;
    let radius = f64::from(width.min(height)) * 0.5 - 1.0;
    let edge = radius - (x - center_x).hypot(y - center_y);
    ((edge + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn header_compatible(header: &SharedHeader) -> bool {
    header.magic.load(Ordering::Acquire) == MAGIC
        && header.schema_version.load(Ordering::Acquire) == SCHEMA_VERSION
        && header.capacity_bytes.load(Ordering::Acquire) as usize == MAX_PAYLOAD_BYTES
}

fn shared_memory_error(error: ShmemError) -> BridgeError {
    BridgeError::SharedMemory(error.to_string())
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_display_geometry_uses_a_distinct_v3_mapping() {
        assert_eq!(SCHEMA_VERSION, 3);
        assert!(DEFAULT_MAPPING_ID.ends_with("_v3"));
    }
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_MAPPING: AtomicU64 = AtomicU64::new(1);

    fn mapping_id() -> String {
        format!(
            "/pal_companion_bridge_test_{}_{}",
            std::process::id(),
            NEXT_MAPPING.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn writer_and_reader_exchange_exact_rgba_and_metadata() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        let sequence = writer
            .publish_rgb_u32(
                2,
                1,
                FrameDisplayMode::ExpandedMap,
                0.73,
                true,
                &[0x0011_2233, 0x00aa_bbcc],
            )
            .expect("publish");
        let frame = reader.read_latest(0).expect("frame");
        assert_eq!(frame.sequence, sequence);
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.display_width, 2);
        assert_eq!(frame.display_height, 1);
        assert_eq!(frame.display_mode, FrameDisplayMode::ExpandedMap);
        assert!((frame.opacity - 0.73).abs() < f32::EPSILON);
        assert!(frame.logical_visible);
        assert_eq!(frame.rgba, [0x11, 0x22, 0x33, 0xff, 0xaa, 0xbb, 0xcc, 0xff]);
        assert!(reader.read_latest(sequence).is_none());
    }

    #[test]
    fn visibility_changes_publish_a_new_frame_snapshot_without_touching_payload() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        let initial_sequence = writer
            .publish_rgb_u32(
                2,
                1,
                FrameDisplayMode::ExpandedMap,
                0.8,
                true,
                &[0x0011_2233, 0x0044_5566],
            )
            .expect("publish");
        let initial = reader.read_latest(0).expect("initial frame");

        writer.set_logical_visible(false);
        let hidden = reader
            .read_latest(initial_sequence)
            .expect("visibility-only frame");
        assert_ne!(hidden.sequence, initial_sequence);
        assert!(!hidden.logical_visible);
        assert_eq!(hidden.width, initial.width);
        assert_eq!(hidden.height, initial.height);
        assert_eq!(hidden.rgba, initial.rgba);

        writer.set_logical_visible(true);
        let shown = reader
            .read_latest(hidden.sequence)
            .expect("shown visibility-only frame");
        assert!(shown.logical_visible);
        assert_eq!(shown.rgba, initial.rgba);
    }

    #[test]
    fn setting_the_same_visibility_is_sequence_idempotent() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        let sequence = writer
            .publish_rgb_u32(
                1,
                1,
                FrameDisplayMode::ExpandedMap,
                1.0,
                true,
                &[0x0011_2233],
            )
            .expect("publish");
        assert!(reader.read_latest(0).is_some());

        writer.set_logical_visible(true);

        assert!(reader.read_latest(sequence).is_none());
    }

    #[test]
    fn consumer_heartbeat_exposes_the_active_backend() {
        let id = mapping_id();
        let writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        assert_eq!(writer.consumer_status(1_000), None);
        reader.mark_consumer_alive(ConsumerBackend::DirectX12);
        assert_eq!(
            writer.consumer_status(1_000).map(|status| status.backend),
            Some(ConsumerBackend::DirectX12)
        );
        reader.mark_consumer_stopped();
        assert_eq!(writer.consumer_status(1_000), None);
    }

    #[test]
    fn reader_rejects_a_missing_or_expired_writer_lease() {
        let id = mapping_id();
        let writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        assert!(!reader.writer_is_alive(1_000));

        writer.set_logical_visible(false);
        assert!(reader.writer_is_alive(1_000));

        reader
            .header()
            .writer_heartbeat_ms
            .store(unix_ms().saturating_sub(1_001), Ordering::Release);
        assert!(!reader.writer_is_alive(1_000));
    }

    #[test]
    fn controls_and_consumer_actions_round_trip_without_touching_frame_pixels() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        let filters = FramePoiFilters {
            fast_travel: false,
            boss: true,
            wanted: false,
            dungeon: true,
            tower: true,
            egg: true,
            resources: true,
            salvage: false,
        };
        writer
            .publish_rgb_u32_with_controls(
                1,
                1,
                FrameMetadata {
                    display_mode: FrameDisplayMode::ExpandedMap,
                    controls: FrameControls {
                        input_mode: FrameInputMode::Interactive,
                        poi_filters: filters,
                    },
                    opacity: 1.0,
                    logical_visible: true,
                    display_width: 1_600,
                    display_height: 900,
                },
                &[0x0011_2233],
            )
            .expect("publish");

        let frame = reader.read_latest(0).expect("frame");
        assert_eq!(frame.input_mode, FrameInputMode::Interactive);
        assert_eq!(frame.poi_filters, filters);
        assert_eq!(frame.display_width, 1_600);
        assert_eq!(frame.display_height, 900);

        let sequence = reader.submit_action(ConsumerAction::ToggleResources);
        assert_eq!(
            writer.take_consumer_action(0),
            Some((sequence, ConsumerAction::ToggleResources))
        );
        assert_eq!(writer.take_consumer_action(sequence), None);

        let sequence = reader.submit_action(ConsumerAction::ZoomIn);
        assert_eq!(
            writer.take_consumer_action(0),
            Some((sequence, ConsumerAction::ZoomIn))
        );
    }

    #[test]
    fn pan_accumulates_between_producer_ticks_and_recenter_is_sequenced() {
        let id = mapping_id();
        let writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");

        assert!(reader.submit_pan_delta(10.25, -4.5));
        assert!(reader.submit_pan_delta(2.0, 1.25));
        let pan = writer.take_consumer_pan_delta().expect("accumulated pan");
        assert_eq!(pan.raster_delta_x, 12.25);
        assert_eq!(pan.raster_delta_y, -3.25);
        assert_eq!(writer.take_consumer_pan_delta(), None);

        let recenter = reader.submit_recenter();
        assert_eq!(writer.take_consumer_recenter(0), Some(recenter));
        assert_eq!(writer.take_consumer_recenter(recenter), None);
    }

    #[test]
    fn pan_rejects_non_finite_input_and_saturates_each_axis() {
        let id = mapping_id();
        let writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");

        assert!(!reader.submit_pan_delta(f32::NAN, 1.0));
        assert_eq!(writer.take_consumer_pan_delta(), None);

        assert!(reader.submit_pan_delta(10_000.0, -10_000.0));
        assert!(reader.submit_pan_delta(1.0, -1.0));
        let pan = writer.take_consumer_pan_delta().expect("saturated pan");
        assert_eq!(pan.raster_delta_x, f32::from(i16::MAX) / PAN_SUBPIXEL_UNITS);
        assert_eq!(pan.raster_delta_y, f32::from(i16::MIN) / PAN_SUBPIXEL_UNITS);
    }

    #[test]
    fn invalid_dimensions_and_pixel_counts_fail_closed() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        assert!(matches!(
            writer.publish_rgb_u32(0, 1, FrameDisplayMode::MiniMap, 1.0, true, &[]),
            Err(BridgeError::InvalidDimensions)
        ));
        assert!(matches!(
            writer.publish_rgb_u32(2, 2, FrameDisplayMode::ExpandedMap, 1.0, true, &[0; 3]),
            Err(BridgeError::PixelCountMismatch)
        ));
    }

    #[test]
    fn frame_capacity_is_total_pixels_not_an_independent_axis_cap() {
        assert_eq!(
            checked_pixel_count(2_048, 512).expect("wide boundary"),
            MAX_FRAME_PIXELS
        );
        assert_eq!(
            checked_pixel_count(4_096, 256).expect("ultrawide boundary"),
            MAX_FRAME_PIXELS
        );
        assert_eq!(
            checked_pixel_count(1_920, 546).expect("1080p-shaped raster"),
            1_048_320
        );

        assert!(matches!(
            checked_pixel_count(2_048, 513),
            Err(BridgeError::InvalidDimensions)
        ));
        assert!(matches!(
            checked_pixel_count(u32::MAX, u32::MAX),
            Err(BridgeError::InvalidDimensions)
        ));
    }

    #[test]
    fn logical_display_dimensions_must_be_nonzero() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let result = writer.publish_rgb_u32_with_controls(
            1,
            1,
            FrameMetadata {
                display_mode: FrameDisplayMode::ExpandedMap,
                controls: FrameControls::default(),
                opacity: 1.0,
                logical_visible: true,
                display_width: 0,
                display_height: 1,
            },
            &[0],
        );
        assert!(matches!(result, Err(BridgeError::InvalidDimensions)));
    }

    #[test]
    fn search_catalog_and_selection_round_trip_with_utf8_labels() {
        let id = mapping_id();
        let writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        let entries = vec![
            FrameSearchEntry {
                kind: FrameSearchKind::Pal,
                title: "페스키".to_owned(),
                subtitle: "팰 출현 지역 · SkyDragon".to_owned(),
            },
            FrameSearchEntry {
                kind: FrameSearchKind::Resource,
                title: "크로마이트".to_owned(),
                subtitle: "자원 위치".to_owned(),
            },
        ];
        let sequence = writer
            .publish_search_catalog(&entries)
            .expect("catalog publishes");

        assert_eq!(
            reader.read_search_catalog(0),
            Some((sequence, entries.clone()))
        );
        assert!(reader.read_search_catalog(sequence).is_none());
        let selection_sequence = reader.submit_search_selection(1).expect("valid selection");
        assert_eq!(
            writer.take_consumer_search_selection(0),
            Some((selection_sequence, 1))
        );
        assert!(reader.submit_search_selection(entries.len()).is_none());
    }

    #[test]
    fn minimap_alpha_masks_corners_but_keeps_center_opaque() {
        let id = mapping_id();
        let mut writer = FrameWriter::create_or_open(&id, false).expect("writer");
        let reader = FrameReader::open(&id).expect("reader");
        writer
            .publish_rgb_u32(
                5,
                5,
                FrameDisplayMode::MiniMap,
                1.0,
                true,
                &[0x0011_2233; 25],
            )
            .expect("publish");
        let frame = reader.read_latest(0).expect("frame");
        assert_eq!(frame.rgba[3], 0);
        assert_eq!(frame.rgba[(12 * 4) + 3], 255);
    }
}
