use pal_domain::{DisplayMode, InputMode};
use pal_windows::{TrackedWindow, WindowId};

use crate::{OverlayLayout, PhysicalRect, TopLeftOverlayLayouts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendReason {
    Minimized,
    Inactive,
    DisplayChange,
    Explicit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayLifecycle {
    Detached,
    HiddenAttached,
    Visible,
    Suspended(SuspendReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostUpdate {
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostPlan {
    layout: OverlayLayout,
    display_mode: DisplayMode,
    input_mode: InputMode,
    visible: bool,
}

impl HostPlan {
    pub const fn layout(self) -> OverlayLayout {
        self.layout
    }

    pub fn window_rect(self) -> PhysicalRect {
        self.layout.surface_shape.bounds()
    }

    pub const fn display_mode(self) -> DisplayMode {
        self.display_mode
    }

    pub const fn input_mode(self) -> InputMode {
        self.input_mode
    }

    pub const fn visible(self) -> bool {
        self.visible
    }

    pub fn transparent(self) -> bool {
        self.input_mode == InputMode::Locked
    }

    pub const fn topmost(self) -> bool {
        true
    }

    pub const fn no_activate(self) -> bool {
        true
    }
}

pub trait OverlayShellBackend {
    type Error;

    fn apply_plan(&mut self, plan: HostPlan) -> Result<(), Self::Error>;

    fn hide_fail_closed(&mut self) -> Result<(), Self::Error>;
}

pub struct OverlayHostController<B> {
    backend: B,
    layout: OverlayLayout,
    mini_layout: OverlayLayout,
    expanded_layout: OverlayLayout,
    display_mode: DisplayMode,
    input_mode: InputMode,
    lifecycle: OverlayLifecycle,
    requested_visible: bool,
    attached_window: Option<WindowId>,
    last_plan: Option<HostPlan>,
}

impl<B: OverlayShellBackend> OverlayHostController<B> {
    pub fn new(backend: B, layouts: TopLeftOverlayLayouts) -> Self {
        let mini_layout = layouts.mini();
        let expanded_layout = layouts.expanded();
        Self {
            backend,
            layout: mini_layout,
            mini_layout,
            expanded_layout,
            display_mode: DisplayMode::MiniMap,
            input_mode: InputMode::Locked,
            lifecycle: OverlayLifecycle::Detached,
            requested_visible: true,
            attached_window: None,
            last_plan: None,
        }
    }

    pub const fn lifecycle(&self) -> OverlayLifecycle {
        self.lifecycle
    }

    pub const fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub const fn display_mode(&self) -> DisplayMode {
        self.display_mode
    }

    pub fn is_visible(&self) -> bool {
        self.lifecycle == OverlayLifecycle::Visible
    }

    pub const fn backend(&self) -> &B {
        &self.backend
    }

    pub const fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn attach(&mut self, game: &TrackedWindow) -> Result<HostUpdate, B::Error> {
        let prior = self.visible_state();
        self.attached_window = Some(game.id());
        self.layout.host_rect = crate::PhysicalRect::new(
            game.snapshot().client_left(),
            game.snapshot().client_top(),
            game.snapshot().client_width(),
            game.snapshot().client_height(),
        );
        self.mini_layout.host_rect = self.layout.host_rect;
        self.expanded_layout.host_rect = self.layout.host_rect;
        self.layout = match self.display_mode {
            DisplayMode::MiniMap => self.mini_layout,
            DisplayMode::ExpandedMap => self.expanded_layout,
        };
        self.lifecycle = if game.snapshot().minimized() {
            self.input_mode = InputMode::Locked;
            OverlayLifecycle::Suspended(SuspendReason::Minimized)
        } else if !game.snapshot().active() || !game.snapshot().visible() {
            self.input_mode = InputMode::Locked;
            OverlayLifecycle::Suspended(SuspendReason::Inactive)
        } else if self.requested_visible {
            OverlayLifecycle::Visible
        } else {
            self.input_mode = InputMode::Locked;
            OverlayLifecycle::HiddenAttached
        };
        self.apply_current(prior != self.visible_state())
    }

    pub fn apply_layouts(
        &mut self,
        layouts: TopLeftOverlayLayouts,
    ) -> Result<HostUpdate, B::Error> {
        let mini_layout = layouts.mini();
        let expanded_layout = layouts.expanded();
        if self.mini_layout == mini_layout && self.expanded_layout == expanded_layout {
            return Ok(HostUpdate::Unchanged);
        }
        self.mini_layout = mini_layout;
        self.expanded_layout = expanded_layout;
        self.layout = match self.display_mode {
            DisplayMode::MiniMap => self.mini_layout,
            DisplayMode::ExpandedMap => self.expanded_layout,
        };
        self.apply_current(true)
    }

    pub fn set_display_mode(&mut self, mode: DisplayMode) -> Result<HostUpdate, B::Error> {
        if self.display_mode == mode {
            return Ok(HostUpdate::Unchanged);
        }
        self.display_mode = mode;
        self.layout = match mode {
            DisplayMode::MiniMap => self.mini_layout,
            DisplayMode::ExpandedMap => self.expanded_layout,
        };
        self.apply_current(true)
    }

    pub fn set_input_mode(&mut self, mode: InputMode) -> Result<HostUpdate, B::Error> {
        let actual = if self.lifecycle == OverlayLifecycle::Visible {
            mode
        } else {
            InputMode::Locked
        };
        if self.input_mode == actual {
            return Ok(HostUpdate::Unchanged);
        }
        self.input_mode = actual;
        self.apply_current(true)
    }

    pub fn set_requested_visible(&mut self, visible: bool) -> Result<HostUpdate, B::Error> {
        if self.requested_visible == visible {
            return Ok(HostUpdate::Unchanged);
        }
        self.requested_visible = visible;
        if self.attached_window.is_none() {
            return Ok(HostUpdate::Changed);
        }
        if matches!(self.lifecycle, OverlayLifecycle::Suspended(_)) {
            self.input_mode = InputMode::Locked;
            return self.apply_current(true);
        }
        if visible {
            self.lifecycle = OverlayLifecycle::Visible;
        } else {
            self.input_mode = InputMode::Locked;
            self.lifecycle = OverlayLifecycle::HiddenAttached;
        }
        self.apply_current(true)
    }

    pub fn suspend(&mut self, reason: SuspendReason) -> Result<(), B::Error> {
        if self.lifecycle == OverlayLifecycle::Detached
            || self.lifecycle == OverlayLifecycle::Suspended(reason)
        {
            return Ok(());
        }
        self.input_mode = InputMode::Locked;
        self.lifecycle = OverlayLifecycle::Suspended(reason);
        self.apply_current(true).map(|_| ())
    }

    pub fn detach(&mut self) -> Result<(), B::Error> {
        if self.lifecycle == OverlayLifecycle::Detached {
            return Ok(());
        }
        self.input_mode = InputMode::Locked;
        self.lifecycle = OverlayLifecycle::Detached;
        self.attached_window = None;
        self.apply_current(true).map(|_| ())
    }

    fn visible_state(&self) -> bool {
        self.is_visible()
    }

    fn current_plan(&self) -> HostPlan {
        HostPlan {
            layout: self.layout,
            display_mode: self.display_mode,
            input_mode: self.input_mode,
            visible: self.lifecycle == OverlayLifecycle::Visible,
        }
    }

    fn apply_current(&mut self, state_changed: bool) -> Result<HostUpdate, B::Error> {
        let plan = self.current_plan();
        if self.last_plan == Some(plan) {
            return Ok(if state_changed {
                HostUpdate::Changed
            } else {
                HostUpdate::Unchanged
            });
        }
        if let Err(error) = self.backend.apply_plan(plan) {
            let _ = self.backend.hide_fail_closed();
            self.input_mode = InputMode::Locked;
            self.lifecycle = if self.attached_window.is_some() {
                OverlayLifecycle::HiddenAttached
            } else {
                OverlayLifecycle::Detached
            };
            self.last_plan = None;
            return Err(error);
        }
        self.last_plan = Some(plan);
        Ok(HostUpdate::Changed)
    }
}
