use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowValidationError {
    VisibleWidthZero,
    VisibleHeightZero,
    VisibleDpiZero,
}

impl WindowValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::VisibleWidthZero => "client_width",
            Self::VisibleHeightZero => "client_height",
            Self::VisibleDpiZero => "dpi",
        }
    }
}

impl fmt::Display for WindowValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} must be nonzero for a visible window",
            self.field()
        )
    }
}

impl Error for WindowValidationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSnapshot {
    process_id: u32,
    client_left: i32,
    client_top: i32,
    client_width: u32,
    client_height: u32,
    dpi: u32,
    visible: bool,
    active: bool,
    minimized: bool,
}

impl WindowSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        process_id: u32,
        client_left: i32,
        client_top: i32,
        client_width: u32,
        client_height: u32,
        dpi: u32,
        visible: bool,
        active: bool,
        minimized: bool,
    ) -> Result<Self, WindowValidationError> {
        let snapshot = Self {
            process_id,
            client_left,
            client_top,
            client_width,
            client_height,
            dpi,
            visible,
            active,
            minimized,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), WindowValidationError> {
        if !self.visible {
            return Ok(());
        }
        if self.client_width == 0 {
            return Err(WindowValidationError::VisibleWidthZero);
        }
        if self.client_height == 0 {
            return Err(WindowValidationError::VisibleHeightZero);
        }
        if self.dpi == 0 {
            return Err(WindowValidationError::VisibleDpiZero);
        }
        Ok(())
    }

    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    pub const fn client_left(&self) -> i32 {
        self.client_left
    }

    pub const fn client_top(&self) -> i32 {
        self.client_top
    }

    pub const fn client_width(&self) -> u32 {
        self.client_width
    }

    pub const fn client_height(&self) -> u32 {
        self.client_height
    }

    pub const fn dpi(&self) -> u32 {
        self.dpi
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }

    pub const fn active(&self) -> bool {
        self.active
    }

    pub const fn minimized(&self) -> bool {
        self.minimized
    }
}
