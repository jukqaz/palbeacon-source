use std::error::Error;
use std::fmt;

use pal_domain::{RotationMode, shortest_angle_lerp};
use pal_map_pack_store::{MapPoint, MapRect, WorldPoint};

const MAX_OUTPUT_DIMENSION_PX: u32 = 16_384;
const MIN_ZOOM: f32 = 0.50;
const MAX_ZOOM: f32 = 4.00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportValidationError {
    OutputWidthOutOfRange,
    OutputHeightOutOfRange,
    MapPixelsPerScreenPixelInvalid,
    ZoomOutOfRange,
    ScreenXNonFinite,
    ScreenYNonFinite,
    RotationNonFinite,
    PlayerZNonFinite,
    PlayerHeadingNonFinite,
    ProjectionNonFinite,
}

impl ViewportValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::OutputWidthOutOfRange => "output_width_px",
            Self::OutputHeightOutOfRange => "output_height_px",
            Self::MapPixelsPerScreenPixelInvalid => "map_pixels_per_screen_pixel_at_zoom_one",
            Self::ZoomOutOfRange => "zoom",
            Self::ScreenXNonFinite => "screen.x",
            Self::ScreenYNonFinite => "screen.y",
            Self::RotationNonFinite => "map_rotation_degrees",
            Self::PlayerZNonFinite => "player.z",
            Self::PlayerHeadingNonFinite => "player.heading_degrees",
            Self::ProjectionNonFinite => "projection",
        }
    }
}

impl fmt::Display for ViewportValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} is invalid", self.field())
    }
}

impl Error for ViewportValidationError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportMetrics {
    output_width_px: u32,
    output_height_px: u32,
    map_pixels_per_screen_pixel_at_zoom_one: f64,
}

impl ViewportMetrics {
    pub fn new(
        output_width_px: u32,
        output_height_px: u32,
        map_pixels_per_screen_pixel_at_zoom_one: f64,
    ) -> Result<Self, ViewportValidationError> {
        if !(1..=MAX_OUTPUT_DIMENSION_PX).contains(&output_width_px) {
            return Err(ViewportValidationError::OutputWidthOutOfRange);
        }
        if !(1..=MAX_OUTPUT_DIMENSION_PX).contains(&output_height_px) {
            return Err(ViewportValidationError::OutputHeightOutOfRange);
        }
        if !map_pixels_per_screen_pixel_at_zoom_one.is_finite()
            || map_pixels_per_screen_pixel_at_zoom_one <= 0.0
            || !(map_pixels_per_screen_pixel_at_zoom_one
                * f64::from(output_width_px + output_height_px))
            .is_finite()
        {
            return Err(ViewportValidationError::MapPixelsPerScreenPixelInvalid);
        }
        Ok(Self {
            output_width_px,
            output_height_px,
            map_pixels_per_screen_pixel_at_zoom_one,
        })
    }

    pub const fn output_width_px(self) -> u32 {
        self.output_width_px
    }

    pub const fn output_height_px(self) -> u32 {
        self.output_height_px
    }

    pub const fn map_pixels_per_screen_pixel_at_zoom_one(self) -> f64 {
        self.map_pixels_per_screen_pixel_at_zoom_one
    }

    pub fn effective_map_pixels_per_screen_pixel(self, zoom: f32) -> f64 {
        self.map_pixels_per_screen_pixel_at_zoom_one / f64::from(zoom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPoint {
    x: f64,
    y: f64,
}

impl ScreenPoint {
    pub fn new(x: f64, y: f64) -> Result<Self, ViewportValidationError> {
        if !x.is_finite() {
            return Err(ViewportValidationError::ScreenXNonFinite);
        }
        if !y.is_finite() {
            return Err(ViewportValidationError::ScreenYNonFinite);
        }
        Ok(Self { x, y })
    }

    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniMapViewport {
    center: MapPoint,
    zoom: f32,
    metrics: ViewportMetrics,
}

impl MiniMapViewport {
    pub fn new(
        center: MapPoint,
        zoom: f32,
        metrics: ViewportMetrics,
    ) -> Result<Self, ViewportValidationError> {
        validate_zoom(zoom)?;
        Ok(Self {
            center,
            zoom,
            metrics,
        })
    }

    pub const fn center(self) -> MapPoint {
        self.center
    }

    pub const fn zoom(self) -> f32 {
        self.zoom
    }

    pub const fn metrics(self) -> ViewportMetrics {
        self.metrics
    }

    pub fn effective_map_pixels_per_screen_pixel(self) -> f64 {
        self.metrics
            .effective_map_pixels_per_screen_pixel(self.zoom)
    }

    pub fn radius_map_pixels(self) -> f64 {
        f64::from(
            self.metrics
                .output_width_px
                .min(self.metrics.output_height_px),
        ) * 0.5
            * self.effective_map_pixels_per_screen_pixel()
    }

    pub fn broad_phase_bounds(self, map_width_px: u32, map_height_px: u32) -> Option<MapRect> {
        let radius = self.radius_map_pixels();
        intersect_map_bounds(
            saturating_add_f64(self.center.x(), -radius),
            saturating_add_f64(self.center.y(), -radius),
            saturating_add_f64(self.center.x(), radius),
            saturating_add_f64(self.center.y(), radius),
            map_width_px,
            map_height_px,
        )
    }

    pub fn contains_map_point(self, point: MapPoint) -> bool {
        let radius = self.radius_map_pixels();
        let epsilon = boundary_epsilon(self.center, radius);
        within_inclusive(
            (point.x() - self.center.x()).hypot(point.y() - self.center.y()),
            radius,
            epsilon,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpandedMapViewport {
    center: MapPoint,
    zoom: f32,
    metrics: ViewportMetrics,
}

impl ExpandedMapViewport {
    pub fn new(
        center: MapPoint,
        zoom: f32,
        metrics: ViewportMetrics,
    ) -> Result<Self, ViewportValidationError> {
        validate_zoom(zoom)?;
        Ok(Self {
            center,
            zoom,
            metrics,
        })
    }

    pub const fn center(self) -> MapPoint {
        self.center
    }

    pub const fn zoom(self) -> f32 {
        self.zoom
    }

    pub const fn metrics(self) -> ViewportMetrics {
        self.metrics
    }

    pub fn effective_map_pixels_per_screen_pixel(self) -> f64 {
        self.metrics
            .effective_map_pixels_per_screen_pixel(self.zoom)
    }

    pub fn map_to_screen(
        self,
        point: MapPoint,
        map_rotation_degrees: f32,
    ) -> Result<ScreenPoint, ViewportValidationError> {
        validate_rotation(map_rotation_degrees)?;
        let scale = self.effective_map_pixels_per_screen_pixel();
        let local_x = (point.x() - self.center.x()) / scale;
        let local_y = (point.y() - self.center.y()) / scale;
        let (rotated_x, rotated_y) = rotate(local_x, local_y, map_rotation_degrees);
        ScreenPoint::new(
            rotated_x + f64::from(self.metrics.output_width_px) * 0.5,
            rotated_y + f64::from(self.metrics.output_height_px) * 0.5,
        )
    }

    pub fn screen_to_map(
        self,
        point: ScreenPoint,
        map_rotation_degrees: f32,
    ) -> Result<MapPoint, ViewportValidationError> {
        validate_rotation(map_rotation_degrees)?;
        let screen_x = point.x() - f64::from(self.metrics.output_width_px) * 0.5;
        let screen_y = point.y() - f64::from(self.metrics.output_height_px) * 0.5;
        let (local_x, local_y) = rotate(screen_x, screen_y, -map_rotation_degrees);
        let scale = self.effective_map_pixels_per_screen_pixel();
        MapPoint::new(
            self.center.x() + local_x * scale,
            self.center.y() + local_y * scale,
        )
        .map_err(|_| ViewportValidationError::ProjectionNonFinite)
    }

    pub fn broad_phase_bounds(
        self,
        map_width_px: u32,
        map_height_px: u32,
        map_rotation_degrees: f32,
    ) -> Option<MapRect> {
        validate_rotation(map_rotation_degrees).ok()?;
        let half_width = f64::from(self.metrics.output_width_px) * 0.5;
        let half_height = f64::from(self.metrics.output_height_px) * 0.5;
        let corners = [
            (-half_width, -half_height),
            (half_width, -half_height),
            (half_width, half_height),
            (-half_width, half_height),
        ];
        let first = self.saturating_inverse_corner(corners[0], map_rotation_degrees);
        let mut min_x = first.0;
        let mut min_y = first.1;
        let mut max_x = first.0;
        let mut max_y = first.1;
        for corner in corners.into_iter().skip(1) {
            let point = self.saturating_inverse_corner(corner, map_rotation_degrees);
            min_x = min_x.min(point.0);
            min_y = min_y.min(point.1);
            max_x = max_x.max(point.0);
            max_y = max_y.max(point.1);
        }
        intersect_map_bounds(min_x, min_y, max_x, max_y, map_width_px, map_height_px)
    }

    fn saturating_inverse_corner(
        self,
        screen_offset: (f64, f64),
        map_rotation_degrees: f32,
    ) -> (f64, f64) {
        let (local_x, local_y) = rotate(screen_offset.0, screen_offset.1, -map_rotation_degrees);
        let scale = self.effective_map_pixels_per_screen_pixel();
        (
            saturating_add_f64(self.center.x(), saturating_mul_f64(local_x, scale)),
            saturating_add_f64(self.center.y(), saturating_mul_f64(local_y, scale)),
        )
    }

    pub fn contains_map_point(self, point: MapPoint, map_rotation_degrees: f32) -> bool {
        if validate_rotation(map_rotation_degrees).is_err() {
            return false;
        }
        let delta_x = point.x() - self.center.x();
        let delta_y = point.y() - self.center.y();
        if !delta_x.is_finite() || !delta_y.is_finite() {
            return false;
        }
        let (local_x, local_y) = rotate(delta_x, delta_y, map_rotation_degrees);
        if !local_x.is_finite() || !local_y.is_finite() {
            return false;
        }
        let scale = self.effective_map_pixels_per_screen_pixel();
        let half_width = f64::from(self.metrics.output_width_px) * 0.5 * scale;
        let half_height = f64::from(self.metrics.output_height_px) * 0.5 * scale;
        let epsilon = boundary_epsilon(self.center, half_width.max(half_height));
        within_inclusive(local_x.abs(), half_width, epsilon)
            && within_inclusive(local_y.abs(), half_height, epsilon)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActiveViewport {
    Mini(MiniMapViewport),
    Expanded(ExpandedMapViewport),
}

impl ActiveViewport {
    pub const fn center(self) -> MapPoint {
        match self {
            Self::Mini(viewport) => viewport.center(),
            Self::Expanded(viewport) => viewport.center(),
        }
    }

    pub const fn zoom(self) -> f32 {
        match self {
            Self::Mini(viewport) => viewport.zoom(),
            Self::Expanded(viewport) => viewport.zoom(),
        }
    }

    pub const fn metrics(self) -> ViewportMetrics {
        match self {
            Self::Mini(viewport) => viewport.metrics(),
            Self::Expanded(viewport) => viewport.metrics(),
        }
    }

    pub fn effective_map_pixels_per_screen_pixel(self) -> f64 {
        match self {
            Self::Mini(viewport) => viewport.effective_map_pixels_per_screen_pixel(),
            Self::Expanded(viewport) => viewport.effective_map_pixels_per_screen_pixel(),
        }
    }

    pub fn broad_phase_bounds(
        self,
        map_width_px: u32,
        map_height_px: u32,
        map_rotation_degrees: f32,
    ) -> Option<MapRect> {
        match self {
            Self::Mini(viewport) => viewport.broad_phase_bounds(map_width_px, map_height_px),
            Self::Expanded(viewport) => {
                viewport.broad_phase_bounds(map_width_px, map_height_px, map_rotation_degrees)
            }
        }
    }

    pub fn contains_map_point(self, point: MapPoint, map_rotation_degrees: f32) -> bool {
        match self {
            Self::Mini(viewport) => viewport.contains_map_point(point),
            Self::Expanded(viewport) => viewport.contains_map_point(point, map_rotation_degrees),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportLayout {
    mini_metrics: ViewportMetrics,
    expanded_metrics: ViewportMetrics,
}

impl ViewportLayout {
    pub const fn new(mini_metrics: ViewportMetrics, expanded_metrics: ViewportMetrics) -> Self {
        Self {
            mini_metrics,
            expanded_metrics,
        }
    }

    pub const fn mini_metrics(self) -> ViewportMetrics {
        self.mini_metrics
    }

    pub const fn expanded_metrics(self) -> ViewportMetrics {
        self.expanded_metrics
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadingStatus {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportPose {
    map_rotation_degrees: f32,
    player_rotation_degrees: f32,
    heading_status: HeadingStatus,
}

impl ViewportPose {
    pub const fn map_rotation_degrees(self) -> f32 {
        self.map_rotation_degrees
    }

    pub const fn player_rotation_degrees(self) -> f32 {
        self.player_rotation_degrees
    }

    pub const fn heading_status(self) -> HeadingStatus {
        self.heading_status
    }
}

pub fn compute_viewport_pose(mode: RotationMode, heading: Option<f32>) -> ViewportPose {
    let Some(heading) = heading.filter(|value| value.is_finite()) else {
        return ViewportPose {
            map_rotation_degrees: 0.0,
            player_rotation_degrees: 0.0,
            heading_status: HeadingStatus::Unavailable,
        };
    };
    let heading = heading.rem_euclid(360.0);
    match mode {
        RotationMode::NorthUp => ViewportPose {
            map_rotation_degrees: 0.0,
            player_rotation_degrees: heading,
            heading_status: HeadingStatus::Available,
        },
        RotationMode::HeadingUp => ViewportPose {
            map_rotation_degrees: -heading,
            player_rotation_degrees: 0.0,
            heading_status: HeadingStatus::Available,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPose {
    world: WorldPoint,
    map: MapPoint,
    z: f64,
    heading_degrees: Option<f32>,
}

impl PlayerPose {
    pub fn new(
        world: WorldPoint,
        map: MapPoint,
        z: f64,
        heading_degrees: Option<f32>,
    ) -> Result<Self, ViewportValidationError> {
        if !z.is_finite() {
            return Err(ViewportValidationError::PlayerZNonFinite);
        }
        if heading_degrees.is_some_and(|heading| !heading.is_finite()) {
            return Err(ViewportValidationError::PlayerHeadingNonFinite);
        }
        Ok(Self {
            world,
            map,
            z,
            heading_degrees: heading_degrees.map(|heading| heading.rem_euclid(360.0)),
        })
    }

    pub const fn world(self) -> WorldPoint {
        self.world
    }

    pub const fn map(self) -> MapPoint {
        self.map
    }

    pub const fn z(self) -> f64 {
        self.z
    }

    pub const fn heading_degrees(self) -> Option<f32> {
        self.heading_degrees
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpolationPolicy {
    Interpolate,
    Freeze,
}

pub fn interpolate_player_pose(
    from: PlayerPose,
    to: PlayerPose,
    t: f32,
    policy: InterpolationPolicy,
) -> PlayerPose {
    if policy == InterpolationPolicy::Freeze {
        return to;
    }
    let t = if t.is_nan() { 1.0 } else { t.clamp(0.0, 1.0) };
    let t64 = f64::from(t);
    let world = WorldPoint::new(
        lerp_f64(from.world.x(), to.world.x(), t64),
        lerp_f64(from.world.y(), to.world.y(), t64),
    )
    .expect("finite endpoint interpolation remains finite");
    let map = MapPoint::new(
        lerp_f64(from.map.x(), to.map.x(), t64),
        lerp_f64(from.map.y(), to.map.y(), t64),
    )
    .expect("finite endpoint interpolation remains finite");
    let z = lerp_f64(from.z, to.z, t64);
    let heading_degrees = match (from.heading_degrees, to.heading_degrees) {
        (Some(from), Some(to)) => Some(shortest_angle_lerp(from, to, t)),
        _ => to.heading_degrees,
    };
    PlayerPose {
        world,
        map,
        z,
        heading_degrees,
    }
}

fn validate_zoom(zoom: f32) -> Result<(), ViewportValidationError> {
    if (MIN_ZOOM..=MAX_ZOOM).contains(&zoom) {
        Ok(())
    } else {
        Err(ViewportValidationError::ZoomOutOfRange)
    }
}

fn validate_rotation(rotation: f32) -> Result<(), ViewportValidationError> {
    if rotation.is_finite() {
        Ok(())
    } else {
        Err(ViewportValidationError::RotationNonFinite)
    }
}

fn rotate(x: f64, y: f64, degrees: f32) -> (f64, f64) {
    let radians = f64::from(degrees).to_radians();
    let (sine, cosine) = radians.sin_cos();
    (cosine * x - sine * y, sine * x + cosine * y)
}

fn intersect_map_bounds(
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    map_width_px: u32,
    map_height_px: u32,
) -> Option<MapRect> {
    if map_width_px == 0 || map_height_px == 0 {
        return None;
    }
    let width = f64::from(map_width_px);
    let height = f64::from(map_height_px);
    if max_x < 0.0 || max_y < 0.0 || min_x > width || min_y > height {
        return None;
    }
    MapRect::new(
        min_x.max(0.0),
        min_y.max(0.0),
        max_x.min(width),
        max_y.min(height),
    )
    .ok()
}

fn boundary_epsilon(center: MapPoint, extent: f64) -> f64 {
    64.0 * f64::EPSILON
        * center
            .x()
            .abs()
            .max(center.y().abs())
            .max(extent.abs())
            .max(1.0)
}

fn within_inclusive(value: f64, bound: f64, epsilon: f64) -> bool {
    value <= bound || (value.is_finite() && value - bound <= epsilon)
}

fn saturating_add_f64(lhs: f64, rhs: f64) -> f64 {
    let result = lhs + rhs;
    if result.is_finite() {
        result
    } else if lhs.is_sign_negative() {
        -f64::MAX
    } else {
        f64::MAX
    }
}

fn saturating_mul_f64(lhs: f64, rhs: f64) -> f64 {
    let result = lhs * rhs;
    if result.is_finite() {
        result
    } else if lhs.is_sign_negative() ^ rhs.is_sign_negative() {
        -f64::MAX
    } else {
        f64::MAX
    }
}

fn lerp_f64(from: f64, to: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    if from.is_sign_positive() == to.is_sign_positive() {
        from + (to - from) * t
    } else {
        from * (1.0 - t) + to * t
    }
}
