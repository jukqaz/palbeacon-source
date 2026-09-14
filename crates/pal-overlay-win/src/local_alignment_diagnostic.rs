#[cfg(feature = "development-local-alignment-diagnostic")]
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    time::Duration,
};

use pal_domain::Freshness;
#[cfg(feature = "development-local-alignment-diagnostic")]
use pal_protected_file::{read_protected_file, read_regular_file_no_follow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(feature = "development-local-alignment-diagnostic")]
use crate::actual_map_preview::MapRaster;

const OBSERVATION_SCHEMA: &str = "pal_companion.local_alignment_observation.v1";
const OBSERVATION_CLAIM: &str = "independent_native_marker_observation_not_gate_b";
const SUCCESS_SCHEMA: &str = "pal_companion.local_alignment_smoke.v1";
const DEVELOPMENT_CLAIM: &str = "development_smoke_only_not_gate_b";
const KNOWN_BUILD_ID: u64 = 24_181_527;
const KNOWN_MAP_SHA256: &str = "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b";
const MAP_WIDTH_PX: u32 = 2_048;
const MAP_HEIGHT_PX: u32 = 2_048;
const REQUIRED_SAMPLE_COUNT: usize = 10;
const MINIMUM_SAMPLE_WINDOW_MS: u64 = 900;
const NONCE_HEX_LENGTH: usize = 32;
const MAXIMUM_SPREAD_PX: f64 = 1.0;
const MAXIMUM_RESIDUAL_PX: f64 = 8.0;
#[cfg(feature = "development-local-alignment-diagnostic")]
const ALIGNMENT_MODE_FLAG: &str = "--development-local-alignment-diagnostic-json";
#[cfg(feature = "development-local-alignment-diagnostic")]
const MAP_FLAG: &str = "--real-map-bmp";
#[cfg(feature = "development-local-alignment-diagnostic")]
const OBSERVATION_FILE_FLAG: &str = "--observation-file";
#[cfg(feature = "development-local-alignment-diagnostic")]
const OBSERVATION_SHA_FLAG: &str = "--observation-sha256";
#[cfg(feature = "development-local-alignment-diagnostic")]
const TIMEOUT_FLAG: &str = "--timeout-seconds";
#[cfg(feature = "development-local-alignment-diagnostic")]
const MAXIMUM_OBSERVATION_BYTES: usize = 64 * 1_024;
#[cfg(feature = "development-local-alignment-diagnostic")]
const MAXIMUM_MAP_BYTES: usize = 64 * 1_024 * 1_024;
#[cfg(feature = "development-local-alignment-diagnostic")]
const MAXIMUM_TIMEOUT_SECONDS: u64 = 30;

#[derive(Clone, Debug, PartialEq)]
pub struct LocalAlignmentObservation {
    observed_marker_x_px: f64,
    observed_marker_y_px: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignmentSample {
    projected_x_px: f64,
    projected_y_px: f64,
    accepted_monotonic_offset_ms: u64,
    freshness: Freshness,
    has_yaw: bool,
}

impl AlignmentSample {
    pub const fn new(
        projected_x_px: f64,
        projected_y_px: f64,
        accepted_monotonic_offset_ms: u64,
        freshness: Freshness,
        has_yaw: bool,
    ) -> Self {
        Self {
            projected_x_px,
            projected_y_px,
            accepted_monotonic_offset_ms,
            freshness,
            has_yaw,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalAlignmentResult {
    sample_window_ms: u64,
    projected_spread_px: f64,
    residual_px: f64,
}

impl LocalAlignmentResult {
    pub const fn projected_spread_px(&self) -> f64 {
        self.projected_spread_px
    }

    pub const fn residual_px(&self) -> f64 {
        self.residual_px
    }

    pub const fn sample_window_ms(&self) -> u64 {
        self.sample_window_ms
    }

    pub const fn gate_b_approved(&self) -> bool {
        false
    }

    pub const fn claim(&self) -> &'static str {
        DEVELOPMENT_CLAIM
    }

    pub fn to_json_line(&self) -> String {
        let record = LocalAlignmentSuccessRecord {
            schema: SUCCESS_SCHEMA,
            claim: DEVELOPMENT_CLAIM,
            gate_b_approved: false,
            build_id: KNOWN_BUILD_ID,
            map_sha256: KNOWN_MAP_SHA256,
            sample_count: REQUIRED_SAMPLE_COUNT,
            sample_window_ms: self.sample_window_ms,
            projected_spread_px: self.projected_spread_px,
            residual_px: self.residual_px,
            within_development_smoke_threshold: true,
        };
        let mut line =
            serde_json::to_string(&record).expect("validated alignment result is serializable");
        line.push('\n');
        line
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LocalAlignmentDiagnosticError {
    #[error("invalid observation SHA-256")]
    InvalidObservationSha,
    #[error("observation SHA-256 mismatch")]
    ObservationShaMismatch,
    #[error("invalid observation document")]
    InvalidObservationDocument,
    #[error("invalid observation schema")]
    ObservationSchema,
    #[error("invalid observation claim")]
    ObservationClaim,
    #[error("invalid observation build")]
    ObservationBuild,
    #[error("invalid observation map hash")]
    ObservationMapSha,
    #[error("invalid observation map dimensions")]
    ObservationMapDimensions,
    #[error("invalid observation marker")]
    ObservationMarker,
    #[error("invalid observation nonce")]
    ObservationNonce,
    #[error("exactly ten samples are required")]
    SampleCount,
    #[error("sample window is too short")]
    SampleWindow,
    #[error("sample offsets are not strictly increasing")]
    SampleOrder,
    #[error("sample is not live")]
    SampleNotLive,
    #[error("sample is missing yaw")]
    SampleMissingYaw,
    #[error("sample coordinates are not finite")]
    SampleNotFinite,
    #[error("sample is outside the map")]
    SampleOutOfMap,
    #[error("projected spread exceeds the development smoke threshold")]
    SpreadThresholdExceeded,
    #[error("marker residual exceeds the development smoke threshold")]
    ResidualThresholdExceeded,
}

#[cfg(feature = "development-local-alignment-diagnostic")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalAlignmentDiagnosticCliError {
    #[error("the local alignment diagnostic mode flag is required")]
    MissingModeOptIn,
    #[error("--real-map-bmp requires a path")]
    MissingMapPath,
    #[error("--observation-file requires a path")]
    MissingObservationPath,
    #[error("--observation-sha256 requires a lowercase SHA-256")]
    MissingObservationSha,
    #[error("--observation-sha256 must be 64 lowercase hexadecimal characters")]
    InvalidObservationSha,
    #[error("--timeout-seconds requires a value")]
    MissingTimeout,
    #[error("--timeout-seconds must be an integer")]
    InvalidTimeout,
    #[error("--timeout-seconds must be between 1 and 30 seconds")]
    TimeoutOutOfRange,
    #[error("every local alignment diagnostic argument is required")]
    RequiredArgumentMissing,
    #[error("a local alignment diagnostic argument was provided more than once")]
    DuplicateArgument,
    #[error("unknown local alignment diagnostic argument")]
    UnknownArgument,
}

#[cfg(feature = "development-local-alignment-diagnostic")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAlignmentDiagnosticCommand {
    map_path: PathBuf,
    observation_path: PathBuf,
    observation_sha256: String,
    timeout: Duration,
}

#[cfg(feature = "development-local-alignment-diagnostic")]
impl LocalAlignmentDiagnosticCommand {
    pub fn parse<I, S>(arguments: I) -> Result<Self, LocalAlignmentDiagnosticCliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut mode_opt_in = false;
        let mut map_path = None;
        let mut observation_path = None;
        let mut observation_sha256 = None;
        let mut timeout = None;
        let mut arguments = arguments.into_iter().map(Into::into);

        while let Some(argument) = arguments.next() {
            if argument == OsStr::new(ALIGNMENT_MODE_FLAG) {
                set_cli_once(&mut mode_opt_in)?;
            } else if argument == OsStr::new(MAP_FLAG) {
                set_path_argument(
                    &mut map_path,
                    arguments.next(),
                    LocalAlignmentDiagnosticCliError::MissingMapPath,
                )?;
            } else if argument == OsStr::new(OBSERVATION_FILE_FLAG) {
                set_path_argument(
                    &mut observation_path,
                    arguments.next(),
                    LocalAlignmentDiagnosticCliError::MissingObservationPath,
                )?;
            } else if argument == OsStr::new(OBSERVATION_SHA_FLAG) {
                if observation_sha256.is_some() {
                    return Err(LocalAlignmentDiagnosticCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(LocalAlignmentDiagnosticCliError::MissingObservationSha)?;
                let value = raw
                    .to_str()
                    .ok_or(LocalAlignmentDiagnosticCliError::InvalidObservationSha)?;
                if !is_lowercase_sha256(value) {
                    return Err(LocalAlignmentDiagnosticCliError::InvalidObservationSha);
                }
                observation_sha256 = Some(value.to_owned());
            } else if argument == OsStr::new(TIMEOUT_FLAG) {
                if timeout.is_some() {
                    return Err(LocalAlignmentDiagnosticCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(LocalAlignmentDiagnosticCliError::MissingTimeout)?;
                if raw.to_string_lossy().starts_with("--") {
                    return Err(LocalAlignmentDiagnosticCliError::MissingTimeout);
                }
                let seconds = raw
                    .to_str()
                    .ok_or(LocalAlignmentDiagnosticCliError::InvalidTimeout)?
                    .parse::<u64>()
                    .map_err(|_| LocalAlignmentDiagnosticCliError::InvalidTimeout)?;
                if !(1..=MAXIMUM_TIMEOUT_SECONDS).contains(&seconds) {
                    return Err(LocalAlignmentDiagnosticCliError::TimeoutOutOfRange);
                }
                timeout = Some(Duration::from_secs(seconds));
            } else {
                return Err(LocalAlignmentDiagnosticCliError::UnknownArgument);
            }
        }

        if !mode_opt_in {
            return Err(LocalAlignmentDiagnosticCliError::MissingModeOptIn);
        }
        Ok(Self {
            map_path: map_path.ok_or(LocalAlignmentDiagnosticCliError::RequiredArgumentMissing)?,
            observation_path: observation_path
                .ok_or(LocalAlignmentDiagnosticCliError::RequiredArgumentMissing)?,
            observation_sha256: observation_sha256
                .ok_or(LocalAlignmentDiagnosticCliError::RequiredArgumentMissing)?,
            timeout: timeout.ok_or(LocalAlignmentDiagnosticCliError::RequiredArgumentMissing)?,
        })
    }

    pub fn map_path(&self) -> &Path {
        &self.map_path
    }

    pub fn observation_path(&self) -> &Path {
        &self.observation_path
    }

    pub fn observation_sha256(&self) -> &str {
        &self.observation_sha256
    }

    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn set_cli_once(value: &mut bool) -> Result<(), LocalAlignmentDiagnosticCliError> {
    if *value {
        return Err(LocalAlignmentDiagnosticCliError::DuplicateArgument);
    }
    *value = true;
    Ok(())
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn set_path_argument(
    target: &mut Option<PathBuf>,
    raw: Option<OsString>,
    missing: LocalAlignmentDiagnosticCliError,
) -> Result<(), LocalAlignmentDiagnosticCliError> {
    if target.is_some() {
        return Err(LocalAlignmentDiagnosticCliError::DuplicateArgument);
    }
    let raw = raw.ok_or(missing)?;
    if raw.is_empty() || raw.to_string_lossy().starts_with("--") {
        return Err(missing);
    }
    *target = Some(PathBuf::from(raw));
    Ok(())
}

#[cfg(feature = "development-local-alignment-diagnostic")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAlignmentPreflightPolicy {
    build_id: u64,
    map_sha256: String,
    map_width_px: u32,
    map_height_px: u32,
}

#[cfg(feature = "development-local-alignment-diagnostic")]
impl LocalAlignmentPreflightPolicy {
    pub fn exact_build_24181527() -> Self {
        Self {
            build_id: KNOWN_BUILD_ID,
            map_sha256: KNOWN_MAP_SHA256.to_owned(),
            map_width_px: MAP_WIDTH_PX,
            map_height_px: MAP_HEIGHT_PX,
        }
    }

    #[doc(hidden)]
    pub fn new_for_test(
        build_id: u64,
        map_sha256: String,
        map_width_px: u32,
        map_height_px: u32,
    ) -> Result<Self, LocalAlignmentDiagnosticError> {
        if build_id == 0
            || !is_lowercase_sha256(&map_sha256)
            || map_width_px == 0
            || map_height_px == 0
        {
            return Err(LocalAlignmentDiagnosticError::InvalidObservationDocument);
        }
        Ok(Self {
            build_id,
            map_sha256,
            map_width_px,
            map_height_px,
        })
    }

    pub fn map_sha256(&self) -> &str {
        &self.map_sha256
    }
}

#[cfg(feature = "development-local-alignment-diagnostic")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalAlignmentPreflightError {
    #[error("the protected observation file is unavailable")]
    ObservationFile,
    #[error("the protected observation is invalid")]
    ObservationInvalid,
    #[error("the regular map file is unavailable")]
    MapFile,
    #[error("the map hash does not match")]
    MapIntegrity,
    #[error("the map BMP is invalid")]
    MapFormat,
    #[error("the map dimensions do not match")]
    MapDimensions,
}

#[cfg(feature = "development-local-alignment-diagnostic")]
pub struct LocalAlignmentPreflight {
    observation: LocalAlignmentObservation,
    map: MapRaster,
}

#[cfg(feature = "development-local-alignment-diagnostic")]
impl LocalAlignmentPreflight {
    pub const fn game_build_id(&self) -> u64 {
        KNOWN_BUILD_ID
    }

    pub const fn observation(&self) -> &LocalAlignmentObservation {
        &self.observation
    }

    pub const fn map(&self) -> &MapRaster {
        &self.map
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationDocument {
    schema: String,
    claim: String,
    game_build_id: u64,
    map_sha256: String,
    map_width_px: u32,
    map_height_px: u32,
    observed_marker_x_px: f32,
    observed_marker_y_px: f32,
    nonce: String,
}

#[derive(Serialize)]
struct LocalAlignmentSuccessRecord {
    schema: &'static str,
    claim: &'static str,
    gate_b_approved: bool,
    build_id: u64,
    map_sha256: &'static str,
    sample_count: usize,
    sample_window_ms: u64,
    projected_spread_px: f64,
    residual_px: f64,
    within_development_smoke_threshold: bool,
}

pub fn parse_local_alignment_observation(
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<LocalAlignmentObservation, LocalAlignmentDiagnosticError> {
    parse_local_alignment_observation_with_policy(
        bytes,
        expected_sha256,
        KNOWN_BUILD_ID,
        KNOWN_MAP_SHA256,
        MAP_WIDTH_PX,
        MAP_HEIGHT_PX,
    )
}

fn parse_local_alignment_observation_with_policy(
    bytes: &[u8],
    expected_sha256: &str,
    expected_build_id: u64,
    expected_map_sha256: &str,
    expected_map_width_px: u32,
    expected_map_height_px: u32,
) -> Result<LocalAlignmentObservation, LocalAlignmentDiagnosticError> {
    if !is_lowercase_sha256(expected_sha256) {
        return Err(LocalAlignmentDiagnosticError::InvalidObservationSha);
    }

    if format!("{:x}", Sha256::digest(bytes)) != expected_sha256 {
        return Err(LocalAlignmentDiagnosticError::ObservationShaMismatch);
    }

    let document: ObservationDocument = serde_json::from_slice(bytes)
        .map_err(|_| LocalAlignmentDiagnosticError::InvalidObservationDocument)?;

    if document.schema != OBSERVATION_SCHEMA {
        return Err(LocalAlignmentDiagnosticError::ObservationSchema);
    }
    if document.claim != OBSERVATION_CLAIM {
        return Err(LocalAlignmentDiagnosticError::ObservationClaim);
    }
    if document.game_build_id != expected_build_id {
        return Err(LocalAlignmentDiagnosticError::ObservationBuild);
    }
    if document.map_sha256 != expected_map_sha256 {
        return Err(LocalAlignmentDiagnosticError::ObservationMapSha);
    }
    if document.map_width_px != expected_map_width_px
        || document.map_height_px != expected_map_height_px
    {
        return Err(LocalAlignmentDiagnosticError::ObservationMapDimensions);
    }
    if !is_map_coordinate(
        f64::from(document.observed_marker_x_px),
        f64::from(document.observed_marker_y_px),
    ) {
        return Err(LocalAlignmentDiagnosticError::ObservationMarker);
    }
    if document.nonce.len() != NONCE_HEX_LENGTH
        || !document
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(LocalAlignmentDiagnosticError::ObservationNonce);
    }
    if canonical_observation_bytes(&document) != bytes {
        return Err(LocalAlignmentDiagnosticError::InvalidObservationDocument);
    }

    Ok(LocalAlignmentObservation {
        observed_marker_x_px: f64::from(document.observed_marker_x_px),
        observed_marker_y_px: f64::from(document.observed_marker_y_px),
    })
}

fn canonical_observation_bytes(document: &ObservationDocument) -> Vec<u8> {
    let marker_x = canonical_marker_number(document.observed_marker_x_px);
    let marker_y = canonical_marker_number(document.observed_marker_y_px);
    format!(
        concat!(
            "{{",
            "\"schema\":\"{schema}\",",
            "\"claim\":\"{claim}\",",
            "\"game_build_id\":{game_build_id},",
            "\"map_sha256\":\"{map_sha256}\",",
            "\"map_width_px\":{map_width_px},",
            "\"map_height_px\":{map_height_px},",
            "\"observed_marker_x_px\":{marker_x},",
            "\"observed_marker_y_px\":{marker_y},",
            "\"nonce\":\"{nonce}\"",
            "}}"
        ),
        schema = document.schema,
        claim = document.claim,
        game_build_id = document.game_build_id,
        map_sha256 = document.map_sha256,
        map_width_px = document.map_width_px,
        map_height_px = document.map_height_px,
        marker_x = marker_x,
        marker_y = marker_y,
        nonce = document.nonce,
    )
    .into_bytes()
}

fn canonical_marker_number(value: f32) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let mut encoded =
        serde_json::to_string(&value).expect("a validated finite f32 marker is serializable");
    if encoded.ends_with(".0") {
        encoded.truncate(encoded.len() - 2);
    }
    if let Some(exponent_offset) = encoded.find('e') {
        let exponent = encoded[exponent_offset + 1..]
            .parse::<i32>()
            .expect("serde_json emitted a valid f32 exponent");
        encoded = format!(
            "{}E{exponent:+03}",
            &encoded[..exponent_offset],
            exponent = exponent
        );
    }
    encoded
}

#[cfg(feature = "development-local-alignment-diagnostic")]
pub fn preflight_local_alignment_then<T>(
    command: &LocalAlignmentDiagnosticCommand,
    source_factory: impl FnOnce(LocalAlignmentPreflight) -> T,
) -> Result<T, LocalAlignmentPreflightError> {
    preflight_local_alignment_then_with_policy(
        command,
        &LocalAlignmentPreflightPolicy::exact_build_24181527(),
        source_factory,
    )
}

#[cfg(feature = "development-local-alignment-diagnostic")]
#[doc(hidden)]
pub fn preflight_local_alignment_then_with_policy<T>(
    command: &LocalAlignmentDiagnosticCommand,
    policy: &LocalAlignmentPreflightPolicy,
    source_factory: impl FnOnce(LocalAlignmentPreflight) -> T,
) -> Result<T, LocalAlignmentPreflightError> {
    preflight_local_alignment_with(
        command,
        policy,
        read_protected_file,
        read_regular_file_no_follow,
        source_factory,
    )
}

#[cfg(feature = "development-local-alignment-diagnostic")]
pub fn preflight_local_alignment_with<T, ObservationReadError, MapReadError>(
    command: &LocalAlignmentDiagnosticCommand,
    policy: &LocalAlignmentPreflightPolicy,
    read_observation: impl FnOnce(&Path, usize) -> Result<Vec<u8>, ObservationReadError>,
    read_map: impl FnOnce(&Path, usize) -> Result<Vec<u8>, MapReadError>,
    source_factory: impl FnOnce(LocalAlignmentPreflight) -> T,
) -> Result<T, LocalAlignmentPreflightError> {
    let observation_bytes = read_observation(command.observation_path(), MAXIMUM_OBSERVATION_BYTES)
        .map_err(|_| LocalAlignmentPreflightError::ObservationFile)?;
    let observation = parse_local_alignment_observation_with_policy(
        &observation_bytes,
        command.observation_sha256(),
        policy.build_id,
        &policy.map_sha256,
        policy.map_width_px,
        policy.map_height_px,
    )
    .map_err(|_| LocalAlignmentPreflightError::ObservationInvalid)?;

    let map_bytes = read_map(command.map_path(), MAXIMUM_MAP_BYTES)
        .map_err(|_| LocalAlignmentPreflightError::MapFile)?;
    if sha256_hex(&map_bytes) != policy.map_sha256 {
        return Err(LocalAlignmentPreflightError::MapIntegrity);
    }
    let map =
        MapRaster::decode_bmp(&map_bytes).map_err(|_| LocalAlignmentPreflightError::MapFormat)?;
    if map.width() != policy.map_width_px || map.height() != policy.map_height_px {
        return Err(LocalAlignmentPreflightError::MapDimensions);
    }

    Ok(source_factory(LocalAlignmentPreflight { observation, map }))
}

pub fn evaluate_local_alignment(
    observation: &LocalAlignmentObservation,
    samples: &[AlignmentSample],
) -> Result<LocalAlignmentResult, LocalAlignmentDiagnosticError> {
    if samples.len() != REQUIRED_SAMPLE_COUNT {
        return Err(LocalAlignmentDiagnosticError::SampleCount);
    }
    if samples
        .windows(2)
        .any(|pair| pair[0].accepted_monotonic_offset_ms >= pair[1].accepted_monotonic_offset_ms)
    {
        return Err(LocalAlignmentDiagnosticError::SampleOrder);
    }

    let mut projected_x = [0.0; REQUIRED_SAMPLE_COUNT];
    let mut projected_y = [0.0; REQUIRED_SAMPLE_COUNT];
    for (index, sample) in samples.iter().enumerate() {
        if sample.freshness != Freshness::Live {
            return Err(LocalAlignmentDiagnosticError::SampleNotLive);
        }
        if !sample.has_yaw {
            return Err(LocalAlignmentDiagnosticError::SampleMissingYaw);
        }
        if !sample.projected_x_px.is_finite() || !sample.projected_y_px.is_finite() {
            return Err(LocalAlignmentDiagnosticError::SampleNotFinite);
        }
        if !is_map_coordinate(sample.projected_x_px, sample.projected_y_px) {
            return Err(LocalAlignmentDiagnosticError::SampleOutOfMap);
        }
        projected_x[index] = sample.projected_x_px;
        projected_y[index] = sample.projected_y_px;
    }

    let sample_window_ms = samples[REQUIRED_SAMPLE_COUNT - 1]
        .accepted_monotonic_offset_ms
        .checked_sub(samples[0].accepted_monotonic_offset_ms)
        .filter(|window| *window >= MINIMUM_SAMPLE_WINDOW_MS)
        .ok_or(LocalAlignmentDiagnosticError::SampleWindow)?;

    projected_x.sort_by(f64::total_cmp);
    projected_y.sort_by(f64::total_cmp);
    let median_x = (projected_x[4] + projected_x[5]) / 2.0;
    let median_y = (projected_y[4] + projected_y[5]) / 2.0;

    let projected_spread_px = samples
        .iter()
        .map(|sample| (sample.projected_x_px - median_x).hypot(sample.projected_y_px - median_y))
        .fold(0.0, f64::max);
    if projected_spread_px > MAXIMUM_SPREAD_PX {
        return Err(LocalAlignmentDiagnosticError::SpreadThresholdExceeded);
    }

    let residual_px = (median_x - observation.observed_marker_x_px)
        .hypot(median_y - observation.observed_marker_y_px);
    if residual_px > MAXIMUM_RESIDUAL_PX {
        return Err(LocalAlignmentDiagnosticError::ResidualThresholdExceeded);
    }

    Ok(LocalAlignmentResult {
        sample_window_ms,
        projected_spread_px,
        residual_px,
    })
}

fn is_map_coordinate(x: f64, y: f64) -> bool {
    x.is_finite()
        && y.is_finite()
        && (0.0..f64::from(MAP_WIDTH_PX)).contains(&x)
        && (0.0..f64::from(MAP_HEIGHT_PX)).contains(&y)
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(feature = "development-local-alignment-diagnostic")]
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
