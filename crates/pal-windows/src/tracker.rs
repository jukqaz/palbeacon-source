use std::fmt;

use pal_domain::{WindowSnapshot, WindowValidationError};

use crate::backend::{MonitorId, WindowBackend, WindowBackendError, WindowId, WindowObservation};

pub const PALWORLD_IMAGE_NAME: &str = "Palworld-Win64-Shipping.exe";

#[derive(Clone, PartialEq, Eq)]
pub struct TrackedWindow {
    id: WindowId,
    monitor_id: MonitorId,
    image_path: String,
    snapshot: WindowSnapshot,
}

impl fmt::Debug for TrackedWindow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrackedWindow")
            .field("id", &self.id)
            .field("monitor_id", &self.monitor_id)
            .field("image_path", &"<redacted>")
            .field("snapshot", &self.snapshot)
            .finish()
    }
}

impl TrackedWindow {
    pub const fn id(&self) -> WindowId {
        self.id
    }

    pub const fn monitor_id(&self) -> MonitorId {
        self.monitor_id
    }

    pub fn image_path(&self) -> &str {
        &self.image_path
    }

    pub const fn snapshot(&self) -> &WindowSnapshot {
        &self.snapshot
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    Attached(TrackedWindow),
    Changed {
        previous: TrackedWindow,
        current: TrackedWindow,
    },
    Detached {
        previous: TrackedWindow,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackerErrorCategory {
    InvalidSelectedSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackerError {
    Backend(WindowBackendError),
    Materialization {
        category: TrackerErrorCategory,
        field: &'static str,
    },
}

impl fmt::Display for TrackerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(error) => error.fmt(formatter),
            Self::Materialization { category, field } => {
                write!(
                    formatter,
                    "selected window materialization failed: {category:?} ({field})"
                )
            }
        }
    }
}

impl std::error::Error for TrackerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            Self::Materialization { .. } => None,
        }
    }
}

impl From<WindowBackendError> for TrackerError {
    fn from(error: WindowBackendError) -> Self {
        Self::Backend(error)
    }
}

pub struct GameWindowTracker<B> {
    backend: B,
    current: Option<TrackedWindow>,
}

impl<B: WindowBackend> GameWindowTracker<B> {
    pub const fn new(backend: B) -> Self {
        Self {
            backend,
            current: None,
        }
    }

    pub fn poll(&mut self) -> Result<Option<WindowEvent>, TrackerError> {
        if let Some(previous) = self.current.as_ref() {
            let inspected = self.backend.inspect(previous.id());
            if let Ok(Some(observation)) = inspected
                && observation.id() == previous.id()
                && observation.process_id() == previous.snapshot().process_id()
                && is_valid_current_candidate(&observation)
            {
                let current = materialize_current(&observation, previous)?;
                if current == *previous {
                    return Ok(None);
                }
                let previous = previous.clone();
                self.current = Some(current.clone());
                return Ok(Some(WindowEvent::Changed { previous, current }));
            }
        }

        let selected = self
            .backend
            .enumerate_top_level()?
            .into_iter()
            .filter(is_valid_candidate)
            .min_by(candidate_order);

        let Some(observation) = selected else {
            return Ok(self
                .current
                .take()
                .map(|previous| WindowEvent::Detached { previous }));
        };
        let current = materialize(&observation)?;

        let event = match self.current.take() {
            Some(previous)
                if previous.id() == current.id()
                    && previous.snapshot().process_id() == current.snapshot().process_id() =>
            {
                if previous == current {
                    None
                } else {
                    Some(WindowEvent::Changed {
                        previous,
                        current: current.clone(),
                    })
                }
            }
            _ => Some(WindowEvent::Attached(current.clone())),
        };
        self.current = Some(current);
        Ok(event)
    }
}

fn is_valid_candidate(observation: &WindowObservation) -> bool {
    observation.visible()
        && has_valid_identity(observation)
        && has_visible_client_geometry(observation)
}

fn is_valid_current_candidate(observation: &WindowObservation) -> bool {
    (observation.visible() || observation.minimized())
        && has_valid_identity(observation)
        && (observation.minimized() || has_visible_client_geometry(observation))
}

fn has_valid_identity(observation: &WindowObservation) -> bool {
    observation.inspectable()
        && observation.top_level()
        && observation
            .image_basename()
            .eq_ignore_ascii_case(PALWORLD_IMAGE_NAME)
        && observation.dpi() != 0
}

fn has_visible_client_geometry(observation: &WindowObservation) -> bool {
    let rect = observation.client_rect();
    rect.width() != 0 && rect.height() != 0
}

fn candidate_order(left: &WindowObservation, right: &WindowObservation) -> std::cmp::Ordering {
    right
        .foreground()
        .cmp(&left.foreground())
        .then_with(|| right.client_rect().area().cmp(&left.client_rect().area()))
        .then_with(|| left.id().cmp(&right.id()))
}

fn materialize(observation: &WindowObservation) -> Result<TrackedWindow, TrackerError> {
    let rect = observation.client_rect();
    materialize_with_rect(
        observation,
        rect.left(),
        rect.top(),
        rect.width(),
        rect.height(),
    )
}

fn materialize_current(
    observation: &WindowObservation,
    previous: &TrackedWindow,
) -> Result<TrackedWindow, TrackerError> {
    let rect = observation.client_rect();
    if observation.minimized() && (rect.width() == 0 || rect.height() == 0) {
        let snapshot = previous.snapshot();
        materialize_with_rect(
            observation,
            snapshot.client_left(),
            snapshot.client_top(),
            snapshot.client_width(),
            snapshot.client_height(),
        )
    } else {
        materialize(observation)
    }
}

fn materialize_with_rect(
    observation: &WindowObservation,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
) -> Result<TrackedWindow, TrackerError> {
    let snapshot = WindowSnapshot::new(
        observation.process_id(),
        left,
        top,
        width,
        height,
        observation.dpi(),
        observation.visible(),
        observation.foreground(),
        observation.minimized(),
    )
    .map_err(materialization_error)?;
    Ok(TrackedWindow {
        id: observation.id(),
        monitor_id: observation.monitor_id(),
        image_path: observation.image_path().to_owned(),
        snapshot,
    })
}

fn materialization_error(error: WindowValidationError) -> TrackerError {
    TrackerError::Materialization {
        category: TrackerErrorCategory::InvalidSelectedSnapshot,
        field: error.field(),
    }
}
