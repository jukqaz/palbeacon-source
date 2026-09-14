use std::error::Error;
use std::fmt;

const MIN_ZOOM: f32 = 0.50;
const MAX_ZOOM: f32 = 4.00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewValidationError {
    MiniMapCenterXNonFinite,
    MiniMapCenterYNonFinite,
    MiniMapZoomOutOfRange,
    ExpandedMapCenterXNonFinite,
    ExpandedMapCenterYNonFinite,
    ExpandedMapZoomOutOfRange,
}

impl ViewValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::MiniMapCenterXNonFinite => "mini_map_view.center_x",
            Self::MiniMapCenterYNonFinite => "mini_map_view.center_y",
            Self::MiniMapZoomOutOfRange => "mini_map_view.zoom",
            Self::ExpandedMapCenterXNonFinite => "expanded_map_view.center_x",
            Self::ExpandedMapCenterYNonFinite => "expanded_map_view.center_y",
            Self::ExpandedMapZoomOutOfRange => "expanded_map_view.zoom",
        }
    }
}

impl fmt::Display for ViewValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MiniMapCenterXNonFinite
            | Self::MiniMapCenterYNonFinite
            | Self::ExpandedMapCenterXNonFinite
            | Self::ExpandedMapCenterYNonFinite => {
                write!(formatter, "{} must be finite", self.field())
            }
            Self::MiniMapZoomOutOfRange | Self::ExpandedMapZoomOutOfRange => {
                write!(formatter, "{} must be within 0.50..=4.00", self.field())
            }
        }
    }
}

impl Error for ViewValidationError {}

#[derive(Clone, Debug, PartialEq)]
pub struct MiniMapView {
    center_x: f64,
    center_y: f64,
    zoom: f32,
}

impl MiniMapView {
    pub fn new(center_x: f64, center_y: f64, zoom: f32) -> Result<Self, ViewValidationError> {
        validate_center(center_x, ViewValidationError::MiniMapCenterXNonFinite)?;
        validate_center(center_y, ViewValidationError::MiniMapCenterYNonFinite)?;
        validate_zoom(zoom, ViewValidationError::MiniMapZoomOutOfRange)?;
        Ok(Self {
            center_x,
            center_y,
            zoom,
        })
    }

    pub const fn center_x(&self) -> f64 {
        self.center_x
    }

    pub const fn center_y(&self) -> f64 {
        self.center_y
    }

    pub const fn zoom(&self) -> f32 {
        self.zoom
    }
}

impl Default for MiniMapView {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0).expect("the default minimap view is valid")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedMapView {
    center_x: f64,
    center_y: f64,
    zoom: f32,
}

impl ExpandedMapView {
    pub fn new(center_x: f64, center_y: f64, zoom: f32) -> Result<Self, ViewValidationError> {
        validate_center(center_x, ViewValidationError::ExpandedMapCenterXNonFinite)?;
        validate_center(center_y, ViewValidationError::ExpandedMapCenterYNonFinite)?;
        validate_zoom(zoom, ViewValidationError::ExpandedMapZoomOutOfRange)?;
        Ok(Self {
            center_x,
            center_y,
            zoom,
        })
    }

    pub const fn center_x(&self) -> f64 {
        self.center_x
    }

    pub const fn center_y(&self) -> f64 {
        self.center_y
    }

    pub const fn zoom(&self) -> f32 {
        self.zoom
    }
}

impl Default for ExpandedMapView {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0).expect("the default expanded-map view is valid")
    }
}

fn validate_center(value: f64, error: ViewValidationError) -> Result<(), ViewValidationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(error)
    }
}

fn validate_zoom(value: f32, error: ViewValidationError) -> Result<(), ViewValidationError> {
    if (MIN_ZOOM..=MAX_ZOOM).contains(&value) {
        Ok(())
    } else {
        Err(error)
    }
}
