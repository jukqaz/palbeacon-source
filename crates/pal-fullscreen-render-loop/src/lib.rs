use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use hudhook::{
    BeforeWndProc, ImguiRenderLoop, MessageFilter, RenderContext,
    imgui::{
        Condition, Context, FontConfig, FontGlyphRanges, FontSource, Image, MouseButton,
        StyleColor, StyleVar, TextureId, WindowFlags,
    },
    windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::VK_ESCAPE,
            WindowsAndMessaging::{CURSOR_SHOWING, CURSORINFO, GetCursorInfo, WM_KEYUP},
        },
    },
};
use pal_fullscreen_bridge::{
    ConsumerAction, ConsumerBackend, FrameDisplayMode, FrameInputMode, FramePoiFilters,
    FrameReader, FrameSearchEntry, FrameSearchKind, FullscreenFrame,
};
use pal_render::OVERLAY_VISUAL_V1;

const BRIDGE_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(8);
const WRITER_HEARTBEAT_TIMEOUT_MS: u64 = 2_000;
const CURSOR_HIDE_GRACE: Duration = Duration::from_millis(100);
const MODE_ACTION_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const ENTER_INTERACTION_INTENT_TIMEOUT: Duration = Duration::from_millis(750);
const INTERACTION_ACK_TIMEOUT: Duration = Duration::from_millis(1_500);
const OVERLAY_ORIGIN: [f32; 2] = [18.0, 18.0];
const PANEL_WIDTH: f32 = OVERLAY_VISUAL_V1.expanded_panel_width_dip as f32;
const PANEL_INSET: f32 = OVERLAY_VISUAL_V1.expanded_panel_inset_dip as f32;
const SEARCH_ROW_HEIGHT: f32 = OVERLAY_VISUAL_V1.expanded_search_row_height_dip as f32;
const SEARCH_MAX_RESULTS: usize = 3;
const FILTER_BUTTON_SIZE: [f32; 2] = [
    OVERLAY_VISUAL_V1.filter_button_width_dip as f32,
    OVERLAY_VISUAL_V1.filter_button_height_dip as f32,
];
const FILTER_GAP: f32 = 8.0;
const FILTER_COLUMNS: usize = 2;
const DOCK_MAX_WIDTH: f32 = 620.0;
const DOCK_MIN_WIDTH: f32 = 420.0;
const DOCK_HEIGHT: f32 = 44.0;
const DOCK_PADDING: f32 = 6.0;
const DOCK_GAP: f32 = 6.0;
const DOCK_CONTROL_HEIGHT: f32 = 32.0;
const FILTER_TOGGLE_WIDTH: f32 = 58.0;
const ZOOM_BUTTON_WIDTH: f32 = 34.0;
const ROTATION_BUTTON_WIDTH: f32 = 48.0;
const RECENTER_BUTTON_WIDTH: f32 = 96.0;
const NO_SEARCH_SELECTION: u32 = u32::MAX;

pub struct FullscreenRenderLoop {
    alive: Arc<AtomicBool>,
    hook_ready: Arc<AtomicBool>,
    writer_alive: Arc<AtomicBool>,
    pending_frame: Arc<Mutex<Option<FullscreenFrame>>>,
    pending_catalog: Arc<Mutex<Option<Vec<FrameSearchEntry>>>>,
    pending_mode_action: Arc<AtomicU32>,
    pending_action: Arc<AtomicU32>,
    pending_search_selection: Arc<AtomicU32>,
    pending_pan: Arc<Mutex<[f32; 2]>>,
    pending_recenter: Arc<AtomicBool>,
    textures: HashMap<(u32, u32), TextureId>,
    current: Option<PresentedFrame>,
    search_catalog: Vec<FrameSearchEntry>,
    search_query: String,
    filter_open: bool,
    viewport_size: [f32; 2],
    cursor_sync: CursorInteractionSync,
    interaction_authorization: Mutex<InteractionAuthorization>,
    pending_enter_intent: Mutex<Option<Instant>>,
    escape_lock_pending: AtomicBool,
    map_drag_active: bool,
    last_drag_mouse_position: Option<[f32; 2]>,
}

struct PresentedFrame {
    texture: TextureId,
    raster_width: u32,
    raster_height: u32,
    display_width: u32,
    display_height: u32,
    opacity: f32,
    logical_visible: bool,
    display_mode: FrameDisplayMode,
    input_mode: FrameInputMode,
    poi_filters: FramePoiFilters,
}

#[derive(Clone, Copy, Debug)]
struct PendingModeTransition {
    target: FrameInputMode,
    sent_at: Instant,
}

#[derive(Debug, Default)]
struct CursorInteractionSync {
    hidden_since: Option<Instant>,
    pending: Option<PendingModeTransition>,
    enter_intent_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum InteractionAuthorizationPhase {
    #[default]
    Idle,
    AwaitingInteractiveAck {
        requested_at: Instant,
    },
    Authorized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InteractionAuthorizationDecision {
    NoCapture,
    Capture,
    RequestLock,
}

#[derive(Debug, Default)]
struct InteractionAuthorization {
    phase: InteractionAuthorizationPhase,
    unauthorized_lock_sent_at: Option<Instant>,
}

struct BridgePendingState {
    writer_alive: Arc<AtomicBool>,
    frame: Arc<Mutex<Option<FullscreenFrame>>>,
    catalog: Arc<Mutex<Option<Vec<FrameSearchEntry>>>>,
    mode_action: Arc<AtomicU32>,
    action: Arc<AtomicU32>,
    search_selection: Arc<AtomicU32>,
    pan: Arc<Mutex<[f32; 2]>>,
    recenter: Arc<AtomicBool>,
}

impl FullscreenRenderLoop {
    pub fn new(backend: ConsumerBackend) -> Self {
        let alive = Arc::new(AtomicBool::new(true));
        let hook_ready = Arc::new(AtomicBool::new(false));
        let writer_alive = Arc::new(AtomicBool::new(false));
        let pending_frame = Arc::new(Mutex::new(None));
        let pending_catalog = Arc::new(Mutex::new(None));
        let pending_mode_action = Arc::new(AtomicU32::new(0));
        let pending_action = Arc::new(AtomicU32::new(0));
        let pending_search_selection = Arc::new(AtomicU32::new(NO_SEARCH_SELECTION));
        let pending_pan = Arc::new(Mutex::new([0.0, 0.0]));
        let pending_recenter = Arc::new(AtomicBool::new(false));
        spawn_bridge_reader(
            backend,
            Arc::clone(&alive),
            Arc::clone(&hook_ready),
            BridgePendingState {
                writer_alive: Arc::clone(&writer_alive),
                frame: Arc::clone(&pending_frame),
                catalog: Arc::clone(&pending_catalog),
                mode_action: Arc::clone(&pending_mode_action),
                action: Arc::clone(&pending_action),
                search_selection: Arc::clone(&pending_search_selection),
                pan: Arc::clone(&pending_pan),
                recenter: Arc::clone(&pending_recenter),
            },
        );
        Self {
            alive,
            hook_ready,
            writer_alive,
            pending_frame,
            pending_catalog,
            pending_mode_action,
            pending_action,
            pending_search_selection,
            pending_pan,
            pending_recenter,
            textures: HashMap::new(),
            current: None,
            search_catalog: Vec::new(),
            search_query: String::new(),
            filter_open: false,
            viewport_size: [0.0, 0.0],
            cursor_sync: CursorInteractionSync::default(),
            interaction_authorization: Mutex::new(InteractionAuthorization::default()),
            pending_enter_intent: Mutex::new(None),
            escape_lock_pending: AtomicBool::new(false),
            map_drag_active: false,
            last_drag_mouse_position: None,
        }
    }

    fn reset_cursor_interaction(&mut self) {
        self.cursor_sync.reset();
        self.interaction_authorization
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        *self
            .pending_enter_intent
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        self.escape_lock_pending.store(false, Ordering::Release);
        self.pending_mode_action.store(0, Ordering::Release);
    }

    fn interaction_authorized(&self) -> bool {
        self.interaction_authorization
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_authorized()
    }
}

impl ImguiRenderLoop for FullscreenRenderLoop {
    fn initialize<'a>(
        &'a mut self,
        context: &mut Context,
        _render_context: &'a mut dyn RenderContext,
    ) {
        context.set_ini_filename(None);
        context.io_mut().config_windows_move_from_title_bar_only = true;
        context.fonts().add_font(&[FontSource::TtfData {
            data: include_bytes!("../../../assets/palbeacon/fonts/PretendardVariable.ttf"),
            size_pixels: 14.0,
            config: Some(FontConfig {
                glyph_ranges: FontGlyphRanges::korean(),
                rasterizer_multiply: 1.1,
                ..FontConfig::default()
            }),
        }]);
        self.hook_ready.store(true, Ordering::Release);
    }

    fn before_render<'a>(
        &'a mut self,
        context: &mut Context,
        render_context: &'a mut dyn RenderContext,
    ) {
        if !self.writer_alive.load(Ordering::Acquire) {
            self.current = None;
            self.search_catalog.clear();
            self.filter_open = false;
            self.map_drag_active = false;
            self.last_drag_mouse_position = None;
            self.reset_cursor_interaction();
            return;
        }
        let newest = self
            .pending_frame
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(frame) = newest {
            let key = (frame.width, frame.height);
            let texture = if let Some(texture) = self.textures.get(&key).copied() {
                if render_context
                    .replace_texture(texture, &frame.rgba, frame.width, frame.height)
                    .is_err()
                {
                    return;
                }
                texture
            } else {
                let Ok(texture) =
                    render_context.load_texture(&frame.rgba, frame.width, frame.height)
                else {
                    return;
                };
                self.textures.insert(key, texture);
                texture
            };
            self.current = Some(PresentedFrame {
                texture,
                raster_width: frame.width,
                raster_height: frame.height,
                display_width: frame.display_width,
                display_height: frame.display_height,
                opacity: frame.opacity,
                logical_visible: frame.logical_visible,
                display_mode: frame.display_mode,
                input_mode: frame.input_mode,
                poi_filters: frame.poi_filters,
            });
        }
        if let Some(catalog) = self
            .pending_catalog
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            self.search_catalog = catalog;
        }

        self.viewport_size = context.io().display_size;
        if let Some((expanded, input_mode)) = self.current.as_ref().map(|frame| {
            (
                frame.logical_visible && frame.display_mode == FrameDisplayMode::ExpandedMap,
                frame.input_mode,
            )
        }) {
            let now = Instant::now();
            let cursor_visible = game_cursor_visible();
            if let Some(cursor_visible) = cursor_visible {
                let pending_enter_intent = self
                    .pending_enter_intent
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                if let Some(intent_at) = pending_enter_intent {
                    self.cursor_sync.note_enter_intent(intent_at);
                }
                let escape_lock_pending = self.escape_lock_pending.load(Ordering::Acquire);
                if let Some(action) = self.cursor_sync.update(
                    now,
                    expanded,
                    input_mode,
                    cursor_visible,
                    escape_lock_pending,
                ) {
                    let mut authorization = self
                        .interaction_authorization
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    match action {
                        ConsumerAction::EnterInteractive => authorization.note_enter_requested(now),
                        ConsumerAction::Lock => authorization.clear(),
                        _ => {}
                    }
                    self.pending_mode_action
                        .store(action as u32, Ordering::Release);
                } else if !self.cursor_sync.has_pending_transition() {
                    self.pending_mode_action.store(0, Ordering::Release);
                }
                if escape_lock_pending && escape_lock_acknowledged(input_mode, cursor_visible) {
                    self.escape_lock_pending.store(false, Ordering::Release);
                }
            }
            if !expanded {
                self.reset_cursor_interaction();
            } else {
                let decision = self
                    .interaction_authorization
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .update(now, true, expanded, input_mode, cursor_visible);
                if decision == InteractionAuthorizationDecision::RequestLock {
                    self.pending_mode_action
                        .store(ConsumerAction::Lock as u32, Ordering::Release);
                }
            }
        } else {
            self.reset_cursor_interaction();
        }

        let io = context.io_mut();
        // Interaction is entered only after Palworld processes Escape and exposes its own cursor.
        // Reusing that cursor avoids a duplicated software pointer and keeps game/menu behavior
        // authoritative.
        io.mouse_draw_cursor = false;
    }

    fn render(&mut self, ui: &mut hudhook::imgui::Ui) {
        let Some(frame) = self.current.as_ref() else {
            return;
        };
        if !frame.logical_visible {
            return;
        }

        let map_size = frame_display_size(frame);
        let overlay_size = expanded_overlay_size(frame);
        let overlay_origin = overlay_origin(frame, self.viewport_size);
        let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
        let interactive = is_authorized_interactive_frame(frame, self.interaction_authorized());
        let mut flags = WindowFlags::NO_DECORATION
            | WindowFlags::NO_BACKGROUND
            | WindowFlags::NO_NAV
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
            | WindowFlags::NO_MOVE
            | WindowFlags::NO_RESIZE
            | WindowFlags::NO_SCROLLBAR
            | WindowFlags::NO_SCROLL_WITH_MOUSE;
        if !interactive {
            flags |= WindowFlags::NO_INPUTS;
        }
        ui.window("##pal-companion-fullscreen-overlay")
            .position(overlay_origin, Condition::Always)
            .size(overlay_size, Condition::Always)
            .flags(flags)
            .build(|| {
                ui.set_cursor_pos([0.0, 0.0]);
                Image::new(frame.texture, map_size)
                    .tint_col([1.0, 1.0, 1.0, frame.opacity])
                    .build(ui);
                if frame.display_mode == FrameDisplayMode::ExpandedMap {
                    if interactive {
                        draw_bottom_dock(
                            ui,
                            frame,
                            &self.search_catalog,
                            &mut self.search_query,
                            &mut self.filter_open,
                            DockPendingActions {
                                action: &self.pending_action,
                                selection: &self.pending_search_selection,
                                recenter: &self.pending_recenter,
                            },
                        );
                        update_map_drag(
                            ui,
                            frame,
                            self.filter_open,
                            !self.search_query.trim().is_empty(),
                            &mut self.map_drag_active,
                            &mut self.last_drag_mouse_position,
                            &self.pending_pan,
                        );
                    } else {
                        self.filter_open = false;
                        self.map_drag_active = false;
                        self.last_drag_mouse_position = None;
                        draw_interaction_hint(ui, frame);
                    }
                }
            });
        draw_bezel(ui, frame, overlay_origin);
    }

    fn message_filter(&self, _io: &hudhook::imgui::Io) -> MessageFilter {
        if !self.writer_alive.load(Ordering::Acquire) {
            return MessageFilter::empty();
        }
        message_filter_for_frame(self.current.as_ref(), self.interaction_authorized())
    }

    fn before_wnd_proc(
        &self,
        _hwnd: HWND,
        umsg: u32,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> BeforeWndProc {
        // A locked map never enters interaction directly from the key event. Palworld receives
        // Escape first; the cursor synchronizer enables interaction only while this explicit
        // intent remains recent and the game cursor is observably visible. Requiring both signals
        // prevents the always-visible main-menu cursor from capturing Palworld input.
        if self.writer_alive.load(Ordering::Acquire)
            && umsg == WM_KEYUP
            && wparam.0 == usize::from(VK_ESCAPE.0)
            && let Some(frame) = self.current.as_ref()
            && frame.logical_visible
            && frame.display_mode == FrameDisplayMode::ExpandedMap
        {
            if frame.input_mode == FrameInputMode::Interactive {
                self.interaction_authorization
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clear();
                *self
                    .pending_enter_intent
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = None;
                self.escape_lock_pending.store(true, Ordering::Release);
                self.pending_mode_action
                    .store(ConsumerAction::Lock as u32, Ordering::Release);
                // Palworld must still receive Escape. Break only skips ImGui's Escape handling;
                // keyboard messages are deliberately not part of the propagation filter above.
                BeforeWndProc::Break
            } else {
                if !self.escape_lock_pending.load(Ordering::Acquire) {
                    *self
                        .pending_enter_intent
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
                }
                BeforeWndProc::Continue
            }
        } else {
            BeforeWndProc::Continue
        }
    }
}

struct DockPendingActions<'a> {
    action: &'a AtomicU32,
    selection: &'a AtomicU32,
    recenter: &'a AtomicBool,
}

fn draw_bottom_dock(
    ui: &hudhook::imgui::Ui,
    frame: &PresentedFrame,
    catalog: &[FrameSearchEntry],
    query: &mut String,
    filter_open: &mut bool,
    pending: DockPendingActions<'_>,
) {
    let dock = bottom_dock_layout(frame);
    draw_floating_surface(ui, dock.left, dock.top, dock.width, dock.height, 8.0);

    let _frame_rounding = ui.push_style_var(StyleVar::FrameRounding(8.0));
    let _frame_padding = ui.push_style_var(StyleVar::FramePadding([10.0, 7.0]));
    let _item_spacing = ui.push_style_var(StyleVar::ItemSpacing([DOCK_GAP, 0.0]));
    let _text = ui.push_style_color(
        StyleColor::Text,
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_text.rgba()),
    );
    let _frame_background = ui.push_style_color(
        StyleColor::FrameBg,
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_backdrop.rgba()),
    );
    let _frame_hovered = ui.push_style_color(
        StyleColor::FrameBgHovered,
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_rail.rgba()),
    );
    let _frame_active = ui.push_style_color(
        StyleColor::FrameBgActive,
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_rail.rgba()),
    );

    let fixed_width = FILTER_TOGGLE_WIDTH
        + ZOOM_BUTTON_WIDTH * 2.0
        + ROTATION_BUTTON_WIDTH
        + RECENTER_BUTTON_WIDTH
        + DOCK_GAP * 5.0;
    let search_width = (dock.width - DOCK_PADDING * 2.0 - fixed_width).max(96.0);
    let control_top = dock.top + DOCK_PADDING;
    let mut x = dock.left + DOCK_PADDING;

    ui.set_cursor_pos([x, control_top]);
    ui.set_next_item_width(search_width);
    ui.input_text("##map_search", query)
        .hint("장소 검색")
        .build();
    x += search_width + DOCK_GAP;

    ui.set_cursor_pos([x, control_top]);
    if dock_button(
        ui,
        "필터##toggle_filters",
        [FILTER_TOGGLE_WIDTH, DOCK_CONTROL_HEIGHT],
        *filter_open,
    ) {
        *filter_open = !*filter_open;
    }
    x += FILTER_TOGGLE_WIDTH + DOCK_GAP;

    for (label, action, width) in [
        ("−##zoom_out", ConsumerAction::ZoomOut, ZOOM_BUTTON_WIDTH),
        ("+##zoom_in", ConsumerAction::ZoomIn, ZOOM_BUTTON_WIDTH),
        (
            "방향##rotate",
            ConsumerAction::ToggleRotation,
            ROTATION_BUTTON_WIDTH,
        ),
    ] {
        ui.set_cursor_pos([x, control_top]);
        if dock_button(ui, label, [width, DOCK_CONTROL_HEIGHT], false) {
            pending.action.store(action as u32, Ordering::Release);
        }
        x += width + DOCK_GAP;
    }

    ui.set_cursor_pos([x, control_top]);
    if dock_button(
        ui,
        "내 위치##recenter",
        [RECENTER_BUTTON_WIDTH, DOCK_CONTROL_HEIGHT],
        false,
    ) {
        pending.recenter.store(true, Ordering::Release);
    }

    if *filter_open {
        draw_filter_popover(ui, frame, pending.action, dock);
    } else if !query.trim().is_empty() {
        draw_search_results_popover(ui, catalog, query, pending.selection, dock);
    }
    draw_mode_guidance(ui, frame, "F9 작은 지도  ·  Esc 게임으로");
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DockLayout {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

fn bottom_dock_layout(frame: &PresentedFrame) -> DockLayout {
    let display_size = frame_display_size(frame);
    let available = (display_size[0] - PANEL_INSET * 2.0).max(1.0);
    let width = available
        .min(DOCK_MAX_WIDTH)
        .max(DOCK_MIN_WIDTH.min(available));
    DockLayout {
        left: (display_size[0] - width) * 0.5,
        top: display_size[1] - PANEL_INSET - DOCK_HEIGHT,
        width,
        height: DOCK_HEIGHT,
    }
}

fn draw_floating_surface(
    ui: &hudhook::imgui::Ui,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    rounding: f32,
) {
    let origin = ui.window_pos();
    let minimum = [origin[0] + left, origin[1] + top];
    let maximum = [minimum[0] + width, minimum[1] + height];
    let mut background = normalized_color(OVERLAY_VISUAL_V1.palette.neutral_backdrop.rgba());
    background[3] = 0.94;
    let draw_list = ui.get_window_draw_list();
    draw_list
        .add_rect(minimum, maximum, background)
        .rounding(rounding)
        .filled(true)
        .build();
    draw_list
        .add_rect(
            minimum,
            maximum,
            normalized_color(OVERLAY_VISUAL_V1.palette.neutral_border.rgba()),
        )
        .rounding(rounding)
        .thickness(1.0)
        .build();
}

fn dock_button(ui: &hudhook::imgui::Ui, label: &str, size: [f32; 2], selected: bool) -> bool {
    let base = if selected {
        OVERLAY_VISUAL_V1.palette.minimap_bezel_body.rgba()
    } else {
        OVERLAY_VISUAL_V1.palette.neutral_rail.rgba()
    };
    let border = if selected {
        OVERLAY_VISUAL_V1.palette.cyan_live.rgba()
    } else {
        OVERLAY_VISUAL_V1.palette.neutral_border.rgba()
    };
    let _rounding = ui.push_style_var(StyleVar::FrameRounding(7.0));
    let _border_size = ui.push_style_var(StyleVar::FrameBorderSize(1.0));
    let _button = ui.push_style_color(StyleColor::Button, normalized_color(base));
    let _hovered = ui.push_style_color(
        StyleColor::ButtonHovered,
        normalized_color(OVERLAY_VISUAL_V1.palette.minimap_bezel_body.rgba()),
    );
    let _active = ui.push_style_color(
        StyleColor::ButtonActive,
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_border.rgba()),
    );
    let _border = ui.push_style_color(StyleColor::Border, normalized_color(border));
    let _text = ui.push_style_color(
        StyleColor::Text,
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_text.rgba()),
    );
    ui.button_with_size(label, size)
}

fn draw_search_results_popover(
    ui: &hudhook::imgui::Ui,
    catalog: &[FrameSearchEntry],
    query: &str,
    pending_selection: &AtomicU32,
    dock: DockLayout,
) {
    let matches = search_matches(catalog, query, SEARCH_MAX_RESULTS);
    let rows = matches.len().max(1);
    let height = 18.0 + SEARCH_ROW_HEIGHT * rows as f32;
    let top = dock.top - DOCK_GAP - height;
    let width = PANEL_WIDTH.min(dock.width);
    draw_floating_surface(ui, dock.left, top, width, height, 8.0);
    if matches.is_empty() {
        ui.set_cursor_pos([dock.left + 12.0, top + 14.0]);
        ui.text_disabled("검색 결과가 없습니다");
        return;
    }
    for (row, index) in matches.into_iter().enumerate() {
        let entry = &catalog[index];
        ui.set_cursor_pos([dock.left + 8.0, top + 8.0 + SEARCH_ROW_HEIGHT * row as f32]);
        let label = format!(
            "{}  {}##search_result_{index}",
            search_kind_label(entry.kind),
            entry.title
        );
        if ui
            .selectable_config(&label)
            .size([width - 16.0, 26.0])
            .build()
            && let Ok(index) = u32::try_from(index)
        {
            pending_selection.store(index, Ordering::Release);
        }
        if !entry.subtitle.is_empty() {
            ui.set_cursor_pos([
                dock.left + 38.0,
                top + 34.0 + SEARCH_ROW_HEIGHT * row as f32,
            ]);
            ui.text_colored(
                normalized_color(OVERLAY_VISUAL_V1.palette.neutral_muted.rgba()),
                &entry.subtitle,
            );
        }
    }
}

fn draw_interaction_hint(ui: &hudhook::imgui::Ui, frame: &PresentedFrame) {
    draw_mode_guidance(ui, frame, "Esc 지도 조작");
}

fn draw_mode_guidance(ui: &hudhook::imgui::Ui, frame: &PresentedFrame, text: &str) {
    let display_size = frame_display_size(frame);
    let text_size = ui.calc_text_size(text);
    let width = text_size[0] + 20.0;
    let height = 28.0;
    let left = display_size[0] - PANEL_INSET - width;
    let dock = bottom_dock_layout(frame);
    let side_gap = (display_size[0] - dock.width) * 0.5;
    let top = if frame.input_mode == FrameInputMode::Interactive && width + PANEL_INSET > side_gap {
        dock.top - DOCK_GAP - height
    } else {
        display_size[1] - PANEL_INSET - height
    };
    draw_floating_surface(ui, left, top, width, height, 7.0);
    ui.set_cursor_pos([left + 10.0, top + 6.0]);
    ui.text_colored(
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_muted.rgba()),
        text,
    );
}

fn update_map_drag(
    ui: &hudhook::imgui::Ui,
    frame: &PresentedFrame,
    filter_open: bool,
    search_popover_open: bool,
    map_drag_active: &mut bool,
    last_mouse_position: &mut Option<[f32; 2]>,
    pending_pan: &Mutex<[f32; 2]>,
) {
    let mouse_position = ui.io().mouse_pos;
    let window_origin = ui.window_pos();
    let local_position = [
        mouse_position[0] - window_origin[0],
        mouse_position[1] - window_origin[1],
    ];
    if ui.is_mouse_clicked(MouseButton::Left) {
        *map_drag_active =
            map_drag_starts_at(frame, local_position, filter_open, search_popover_open);
        *last_mouse_position = (*map_drag_active).then_some(mouse_position);
    }
    if !ui.is_mouse_down(MouseButton::Left) {
        *map_drag_active = false;
        *last_mouse_position = None;
        return;
    }
    if !*map_drag_active {
        return;
    }

    let Some(previous) = last_mouse_position.replace(mouse_position) else {
        return;
    };
    let logical_delta = [
        mouse_position[0] - previous[0],
        mouse_position[1] - previous[1],
    ];
    let raster_delta = logical_drag_to_raster(frame, logical_delta);
    if raster_delta == [0.0, 0.0] || !raster_delta.into_iter().all(f32::is_finite) {
        return;
    }
    let mut pending = pending_pan
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    pending[0] += raster_delta[0];
    pending[1] += raster_delta[1];
}

fn logical_drag_to_raster(frame: &PresentedFrame, logical_delta: [f32; 2]) -> [f32; 2] {
    [
        logical_delta[0] * frame.raster_width as f32 / frame.display_width as f32,
        logical_delta[1] * frame.raster_height as f32 / frame.display_height as f32,
    ]
}

fn map_drag_starts_at(
    frame: &PresentedFrame,
    point: [f32; 2],
    filter_open: bool,
    search_popover_open: bool,
) -> bool {
    if !point.into_iter().all(f32::is_finite) {
        return false;
    }
    let display = frame_display_size(frame);
    if point[0] < 0.0 || point[1] < 0.0 || point[0] >= display[0] || point[1] >= display[1] {
        return false;
    }
    let dock = bottom_dock_layout(frame);
    if rect_contains(dock.left, dock.top, dock.width, dock.height, point) {
        return false;
    }
    if filter_open {
        let rows = 4.0;
        let height = 70.0 + FILTER_BUTTON_SIZE[1] * rows + FILTER_GAP * (rows - 1.0);
        return !rect_contains(
            dock.left,
            dock.top - DOCK_GAP - height,
            PANEL_WIDTH.min(dock.width),
            height,
            point,
        );
    }
    if search_popover_open {
        // Reserve the maximum three-row result surface. A shorter result list merely leaves a
        // small non-draggable safety margin above the search field and prevents focus clicks from
        // becoming map drags while results update asynchronously.
        let height = 18.0 + SEARCH_ROW_HEIGHT * SEARCH_MAX_RESULTS as f32;
        return !rect_contains(
            dock.left,
            dock.top - DOCK_GAP - height,
            PANEL_WIDTH.min(dock.width),
            height,
            point,
        );
    }
    true
}

fn rect_contains(left: f32, top: f32, width: f32, height: f32, point: [f32; 2]) -> bool {
    point[0] >= left && point[0] < left + width && point[1] >= top && point[1] < top + height
}

fn search_matches(catalog: &[FrameSearchEntry], query: &str, limit: usize) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut ranked = catalog
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let title = entry.title.to_lowercase();
            let subtitle = entry.subtitle.to_lowercase();
            let rank = if title == query {
                0
            } else if title.starts_with(&query) {
                1
            } else if title.contains(&query) {
                2
            } else if subtitle.contains(&query) {
                3
            } else {
                return None;
            };
            Some((rank, title, index))
        })
        .collect::<Vec<_>>();
    ranked.sort_unstable_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, index)| index)
        .collect()
}

const fn search_kind_label(kind: FrameSearchKind) -> &'static str {
    match kind {
        FrameSearchKind::Pal => "팰",
        FrameSearchKind::Poi => "장소",
        FrameSearchKind::Resource => "자원",
    }
}

fn is_interactive_frame(frame: &PresentedFrame) -> bool {
    frame.logical_visible
        && frame.display_mode == FrameDisplayMode::ExpandedMap
        && frame.input_mode == FrameInputMode::Interactive
}

fn is_authorized_interactive_frame(frame: &PresentedFrame, authorized: bool) -> bool {
    authorized && is_interactive_frame(frame)
}

fn message_filter_for_frame(
    frame: Option<&PresentedFrame>,
    interaction_authorized: bool,
) -> MessageFilter {
    if frame.is_some_and(|frame| is_authorized_interactive_frame(frame, interaction_authorized)) {
        // Search input is owned by the overlay. Only Escape remains available to Palworld so its
        // menu/cursor state stays authoritative; mouse input is isolated as well so dock clicks
        // cannot click through into the game menu.
        MessageFilter::InputKeyboardExceptEscape
            | MessageFilter::InputMouse
            | MessageFilter::InputRaw
    } else {
        MessageFilter::empty()
    }
}

fn expanded_overlay_size(frame: &PresentedFrame) -> [f32; 2] {
    frame_display_size(frame)
}

fn frame_display_size(frame: &PresentedFrame) -> [f32; 2] {
    [frame.display_width as f32, frame.display_height as f32]
}

fn overlay_origin(frame: &PresentedFrame, viewport_size: [f32; 2]) -> [f32; 2] {
    if frame.display_mode != FrameDisplayMode::ExpandedMap
        || viewport_size[0] <= 0.0
        || viewport_size[1] <= 0.0
    {
        return OVERLAY_ORIGIN;
    }
    let display_size = frame_display_size(frame);
    [
        ((viewport_size[0] - display_size[0]) * 0.5).max(0.0),
        ((viewport_size[1] - display_size[1]) * 0.5).max(0.0),
    ]
}

fn game_cursor_visible() -> Option<bool> {
    let mut cursor = CURSORINFO {
        cbSize: u32::try_from(std::mem::size_of::<CURSORINFO>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `cursor` points to initialized writable storage and advertises its exact ABI size.
    unsafe { GetCursorInfo(&mut cursor) }.ok()?;
    Some(cursor.flags.0 & CURSOR_SHOWING.0 != 0)
}

impl InteractionAuthorization {
    fn note_enter_requested(&mut self, now: Instant) {
        self.phase = InteractionAuthorizationPhase::AwaitingInteractiveAck { requested_at: now };
        self.unauthorized_lock_sent_at = None;
    }

    fn clear(&mut self) {
        self.phase = InteractionAuthorizationPhase::Idle;
        self.unauthorized_lock_sent_at = None;
    }

    const fn is_authorized(&self) -> bool {
        matches!(self.phase, InteractionAuthorizationPhase::Authorized)
    }

    fn update(
        &mut self,
        now: Instant,
        connected: bool,
        expanded_visible: bool,
        input_mode: FrameInputMode,
        cursor_visible: Option<bool>,
    ) -> InteractionAuthorizationDecision {
        if !connected || !expanded_visible {
            self.clear();
            return InteractionAuthorizationDecision::NoCapture;
        }

        if let InteractionAuthorizationPhase::AwaitingInteractiveAck { requested_at } = self.phase
            && now.checked_duration_since(requested_at).unwrap_or_default()
                > INTERACTION_ACK_TIMEOUT
        {
            self.phase = InteractionAuthorizationPhase::Idle;
        }

        match input_mode {
            FrameInputMode::Locked => {
                self.unauthorized_lock_sent_at = None;
                if self.phase == InteractionAuthorizationPhase::Authorized {
                    self.phase = InteractionAuthorizationPhase::Idle;
                }
                InteractionAuthorizationDecision::NoCapture
            }
            FrameInputMode::Interactive => match self.phase {
                InteractionAuthorizationPhase::Authorized => {
                    self.unauthorized_lock_sent_at = None;
                    InteractionAuthorizationDecision::Capture
                }
                InteractionAuthorizationPhase::AwaitingInteractiveAck { .. }
                    if cursor_visible == Some(true) =>
                {
                    self.phase = InteractionAuthorizationPhase::Authorized;
                    self.unauthorized_lock_sent_at = None;
                    InteractionAuthorizationDecision::Capture
                }
                InteractionAuthorizationPhase::AwaitingInteractiveAck { .. } => {
                    InteractionAuthorizationDecision::NoCapture
                }
                InteractionAuthorizationPhase::Idle => {
                    let should_request_lock =
                        self.unauthorized_lock_sent_at.is_none_or(|sent_at| {
                            now.checked_duration_since(sent_at).unwrap_or_default()
                                >= MODE_ACTION_RETRY_INTERVAL
                        });
                    if should_request_lock {
                        self.unauthorized_lock_sent_at = Some(now);
                        InteractionAuthorizationDecision::RequestLock
                    } else {
                        InteractionAuthorizationDecision::NoCapture
                    }
                }
            },
        }
    }
}

impl CursorInteractionSync {
    fn note_enter_intent(&mut self, at: Instant) {
        self.enter_intent_at = Some(at);
    }

    fn update(
        &mut self,
        now: Instant,
        expanded: bool,
        input_mode: FrameInputMode,
        cursor_visible: bool,
        escape_lock_pending: bool,
    ) -> Option<ConsumerAction> {
        if !expanded {
            self.reset();
            return None;
        }

        if self
            .pending
            .is_some_and(|pending| pending.target == input_mode)
        {
            self.pending = None;
        }

        if escape_lock_pending {
            self.hidden_since = None;
            self.enter_intent_at = None;
            return self.request_transition(
                now,
                input_mode,
                FrameInputMode::Locked,
                ConsumerAction::Lock,
            );
        }

        match input_mode {
            FrameInputMode::Locked => {
                self.hidden_since = None;
                let retrying_enter = self
                    .pending
                    .is_some_and(|pending| pending.target == FrameInputMode::Interactive);
                let recent_enter_intent = self.enter_intent_is_recent(now);
                if !cursor_visible {
                    // Never keep an entry transition alive after the authoritative game cursor
                    // disappears. A new Escape intent is required if it becomes visible later.
                    if retrying_enter {
                        self.pending = None;
                    }
                    return None;
                }
                if !retrying_enter && !recent_enter_intent {
                    return None;
                }
                if !retrying_enter {
                    self.enter_intent_at = None;
                }
                self.request_transition(
                    now,
                    input_mode,
                    FrameInputMode::Interactive,
                    ConsumerAction::EnterInteractive,
                )
            }
            FrameInputMode::Interactive => {
                self.enter_intent_at = None;
                if cursor_visible {
                    self.hidden_since = None;
                    self.pending = None;
                    return None;
                }
                let hidden_since = *self.hidden_since.get_or_insert(now);
                if now.checked_duration_since(hidden_since).unwrap_or_default() < CURSOR_HIDE_GRACE
                {
                    self.pending = None;
                    return None;
                }
                self.request_transition(
                    now,
                    input_mode,
                    FrameInputMode::Locked,
                    ConsumerAction::Lock,
                )
            }
        }
    }

    fn request_transition(
        &mut self,
        now: Instant,
        input_mode: FrameInputMode,
        target: FrameInputMode,
        action: ConsumerAction,
    ) -> Option<ConsumerAction> {
        if input_mode == target {
            self.pending = None;
            return None;
        }
        let should_emit = self.pending.is_none_or(|pending| {
            pending.target != target
                || now
                    .checked_duration_since(pending.sent_at)
                    .unwrap_or_default()
                    >= MODE_ACTION_RETRY_INTERVAL
        });
        if !should_emit {
            return None;
        }
        self.pending = Some(PendingModeTransition {
            target,
            sent_at: now,
        });
        Some(action)
    }

    const fn has_pending_transition(&self) -> bool {
        self.pending.is_some()
    }

    fn enter_intent_is_recent(&mut self, now: Instant) -> bool {
        let Some(intent_at) = self.enter_intent_at else {
            return false;
        };
        if now.checked_duration_since(intent_at).unwrap_or_default()
            <= ENTER_INTERACTION_INTENT_TIMEOUT
        {
            true
        } else {
            self.enter_intent_at = None;
            false
        }
    }

    fn reset(&mut self) {
        self.hidden_since = None;
        self.pending = None;
        self.enter_intent_at = None;
    }
}

fn escape_lock_acknowledged(input_mode: FrameInputMode, cursor_visible: bool) -> bool {
    input_mode == FrameInputMode::Locked && !cursor_visible
}

fn draw_filter_popover(
    ui: &hudhook::imgui::Ui,
    frame: &PresentedFrame,
    pending_action: &AtomicU32,
    dock: DockLayout,
) {
    let rows = 4.0;
    let width = PANEL_WIDTH.min(dock.width);
    let height = 70.0 + FILTER_BUTTON_SIZE[1] * rows + FILTER_GAP * (rows - 1.0);
    let left = dock.left;
    let top = dock.top - DOCK_GAP - height;
    draw_floating_surface(ui, left, top, width, height, 8.0);
    ui.set_cursor_pos([left + 12.0, top + 10.0]);
    ui.text_colored(
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_text.rgba()),
        "필터",
    );
    let selected_count = filter_buttons(frame.poi_filters)
        .iter()
        .filter(|(_, _, selected)| *selected)
        .count();
    ui.set_cursor_pos([left + 58.0, top + 10.0]);
    ui.text_colored(
        normalized_color(OVERLAY_VISUAL_V1.palette.neutral_muted.rgba()),
        format!("선택 {selected_count}개"),
    );
    let _rounding = ui.push_style_var(StyleVar::FrameRounding(9.0));
    let _border_size = ui.push_style_var(StyleVar::FrameBorderSize(1.0));
    let _text_alignment = ui.push_style_var(StyleVar::ButtonTextAlign([0.5, 0.5]));
    for (index, (label, action, selected)) in
        filter_buttons(frame.poi_filters).into_iter().enumerate()
    {
        let column = (index % FILTER_COLUMNS) as f32;
        let row = (index / FILTER_COLUMNS) as f32;
        ui.set_cursor_pos([
            left + 8.0 + column * (FILTER_BUTTON_SIZE[0] + FILTER_GAP),
            top + 58.0 + row * (FILTER_BUTTON_SIZE[1] + FILTER_GAP),
        ]);
        let button_color = normalized_color(if selected {
            OVERLAY_VISUAL_V1.palette.minimap_bezel_body.rgba()
        } else {
            OVERLAY_VISUAL_V1.palette.neutral_backdrop.rgba()
        });
        let hovered_color = normalized_color(OVERLAY_VISUAL_V1.palette.neutral_rail.rgba());
        let mut active_color = normalized_color(OVERLAY_VISUAL_V1.palette.cyan_live.rgba());
        active_color[3] = 0.72;
        let border_color = normalized_color(if selected {
            OVERLAY_VISUAL_V1.palette.cyan_live.rgba()
        } else {
            OVERLAY_VISUAL_V1.palette.neutral_border.rgba()
        });
        let text_color = normalized_color(if selected {
            OVERLAY_VISUAL_V1.palette.neutral_text.rgba()
        } else {
            OVERLAY_VISUAL_V1.palette.neutral_muted.rgba()
        });
        let _button = ui.push_style_color(StyleColor::Button, button_color);
        let _hovered = ui.push_style_color(StyleColor::ButtonHovered, hovered_color);
        let _active = ui.push_style_color(StyleColor::ButtonActive, active_color);
        let _border = ui.push_style_color(StyleColor::Border, border_color);
        let _text = ui.push_style_color(StyleColor::Text, text_color);
        let clicked = ui.button_with_size(label, FILTER_BUTTON_SIZE);
        if clicked {
            pending_action.store(action as u32, Ordering::Release);
        }
    }
}

fn filter_buttons(filters: FramePoiFilters) -> [(&'static str, ConsumerAction, bool); 8] {
    [
        (
            pal_render::FilterAction::FastTravel.imgui_label_ko(),
            ConsumerAction::ToggleFastTravel,
            filters.fast_travel,
        ),
        (
            pal_render::FilterAction::Boss.imgui_label_ko(),
            ConsumerAction::ToggleBoss,
            filters.boss,
        ),
        (
            pal_render::FilterAction::Wanted.imgui_label_ko(),
            ConsumerAction::ToggleWanted,
            filters.wanted,
        ),
        (
            pal_render::FilterAction::Dungeon.imgui_label_ko(),
            ConsumerAction::ToggleDungeon,
            filters.dungeon,
        ),
        (
            pal_render::FilterAction::Tower.imgui_label_ko(),
            ConsumerAction::ToggleTower,
            filters.tower,
        ),
        (
            pal_render::FilterAction::Egg.imgui_label_ko(),
            ConsumerAction::ToggleEgg,
            filters.egg,
        ),
        (
            pal_render::FilterAction::Resources.imgui_label_ko(),
            ConsumerAction::ToggleResources,
            filters.resources,
        ),
        (
            pal_render::FilterAction::Salvage.imgui_label_ko(),
            ConsumerAction::ToggleSalvage,
            filters.salvage,
        ),
    ]
}

fn draw_bezel(ui: &hudhook::imgui::Ui, frame: &PresentedFrame, origin: [f32; 2]) {
    let draw_list = ui.get_foreground_draw_list();
    let display_size = frame_display_size(frame);
    let outer = normalized_color(OVERLAY_VISUAL_V1.palette.minimap_bezel_outer.rgba());
    let body = normalized_color(OVERLAY_VISUAL_V1.palette.minimap_bezel_body.rgba());
    let highlight = normalized_color(OVERLAY_VISUAL_V1.palette.minimap_bezel_highlight.rgba());
    match frame.display_mode {
        FrameDisplayMode::MiniMap => {
            let center = [
                origin[0] + display_size[0] * 0.5,
                origin[1] + display_size[1] * 0.5,
            ];
            let radius = display_size[0].min(display_size[1]) * 0.5 - 1.0;
            draw_list
                .add_circle(center, radius, outer)
                .num_segments(96)
                .thickness(7.0)
                .build();
            draw_list
                .add_circle(center, radius - 3.0, body)
                .num_segments(96)
                .thickness(3.0)
                .build();
            draw_list
                .add_circle(center, radius - 5.0, highlight)
                .num_segments(96)
                .thickness(1.0)
                .build();
        }
        FrameDisplayMode::ExpandedMap => {
            let overlay_size = expanded_overlay_size(frame);
            let maximum = [origin[0] + overlay_size[0], origin[1] + overlay_size[1]];
            draw_list
                .add_rect(origin, maximum, outer)
                .rounding(10.0)
                .thickness(1.0)
                .build();
        }
    }
}

fn normalized_color(color: [u8; 4]) -> [f32; 4] {
    color.map(|channel| f32::from(channel) / 255.0)
}

impl Drop for FullscreenRenderLoop {
    fn drop(&mut self) {
        self.reset_cursor_interaction();
        self.alive.store(false, Ordering::Release);
        self.hook_ready.store(false, Ordering::Release);
    }
}

fn spawn_bridge_reader(
    backend: ConsumerBackend,
    alive: Arc<AtomicBool>,
    hook_ready: Arc<AtomicBool>,
    pending: BridgePendingState,
) {
    thread::Builder::new()
        .name(format!("pal-fullscreen-{backend:?}-bridge"))
        .spawn(move || {
            bridge_reader_loop(backend, alive, hook_ready, pending);
        })
        .expect("fullscreen bridge reader thread");
}

fn bridge_reader_loop(
    backend: ConsumerBackend,
    alive: Arc<AtomicBool>,
    hook_ready: Arc<AtomicBool>,
    pending: BridgePendingState,
) {
    let BridgePendingState {
        writer_alive,
        frame: pending_frame,
        catalog: pending_catalog,
        mode_action: pending_mode_action,
        action: pending_action,
        search_selection: pending_search_selection,
        pan: pending_pan,
        recenter: pending_recenter,
    } = pending;
    let mut reader = None;
    let mut last_sequence = 0;
    let mut last_search_sequence = 0;
    while alive.load(Ordering::Acquire) {
        if reader.is_none() {
            match FrameReader::open_default() {
                Ok(candidate) => reader = Some(candidate),
                Err(_) => {
                    thread::sleep(BRIDGE_RETRY_INTERVAL);
                    continue;
                }
            }
        }
        if !hook_ready.load(Ordering::Acquire) {
            thread::sleep(BRIDGE_RETRY_INTERVAL);
            continue;
        }
        let active_reader = reader.as_ref().expect("reader initialized");
        if !active_reader.writer_is_alive(WRITER_HEARTBEAT_TIMEOUT_MS) {
            writer_alive.store(false, Ordering::Release);
            active_reader.mark_consumer_stopped();
            last_sequence = 0;
            last_search_sequence = 0;
            pending_frame
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            pending_catalog
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            pending_mode_action.store(0, Ordering::Release);
            pending_action.store(0, Ordering::Release);
            pending_search_selection.store(NO_SEARCH_SELECTION, Ordering::Release);
            *pending_pan
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = [0.0, 0.0];
            pending_recenter.store(false, Ordering::Release);
            thread::sleep(FRAME_POLL_INTERVAL);
            continue;
        }
        writer_alive.store(true, Ordering::Release);
        if let Some(action) = take_pending_consumer_action(&pending_mode_action, &pending_action) {
            active_reader.submit_action(action);
        }
        let pan_delta = {
            let mut pending = pending_pan
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *pending)
        };
        if pan_delta != [0.0, 0.0] {
            let _ = active_reader.submit_pan_delta(pan_delta[0], pan_delta[1]);
        }
        if pending_recenter.swap(false, Ordering::AcqRel) {
            active_reader.submit_recenter();
        }
        let selection = pending_search_selection.swap(NO_SEARCH_SELECTION, Ordering::AcqRel);
        if selection != NO_SEARCH_SELECTION {
            let _ = active_reader.submit_search_selection(selection as usize);
        }
        if let Some((sequence, catalog)) = active_reader.read_search_catalog(last_search_sequence) {
            last_search_sequence = sequence;
            *pending_catalog
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(catalog);
        }
        if let Some(frame) = active_reader.read_latest(last_sequence) {
            last_sequence = frame.sequence;
            *pending_frame
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(frame);
        }
        if last_sequence != 0 {
            active_reader.mark_consumer_alive(backend);
        }
        thread::sleep(FRAME_POLL_INTERVAL);
    }
    if let Some(reader) = reader {
        reader.mark_consumer_stopped();
    }
    writer_alive.store(false, Ordering::Release);
}

fn take_pending_consumer_action(
    pending_mode_action: &AtomicU32,
    pending_action: &AtomicU32,
) -> Option<ConsumerAction> {
    ConsumerAction::from_raw(pending_mode_action.swap(0, Ordering::AcqRel))
        .or_else(|| ConsumerAction::from_raw(pending_action.swap(0, Ordering::AcqRel)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presented_frame(
        display_mode: FrameDisplayMode,
        input_mode: FrameInputMode,
    ) -> PresentedFrame {
        PresentedFrame {
            texture: TextureId::new(1),
            raster_width: 504,
            raster_height: 360,
            display_width: 504,
            display_height: 360,
            opacity: 1.0,
            logical_visible: true,
            display_mode,
            input_mode,
            poi_filters: FramePoiFilters::default(),
        }
    }

    #[test]
    fn render_loop_starts_without_a_writer() {
        let mut render_loop = FullscreenRenderLoop::new(ConsumerBackend::DirectX12);
        assert!(render_loop.alive.load(Ordering::Acquire));
        assert!(!render_loop.hook_ready.load(Ordering::Acquire));
        assert!(!render_loop.writer_alive.load(Ordering::Acquire));
        assert!(!render_loop.filter_open);
        assert!(!render_loop.map_drag_active);
        render_loop.current = Some(presented_frame(
            FrameDisplayMode::ExpandedMap,
            FrameInputMode::Interactive,
        ));
        assert!(
            render_loop.before_wnd_proc(
                HWND::default(),
                WM_KEYUP,
                WPARAM(usize::from(VK_ESCAPE.0)),
                LPARAM::default(),
            ) == BeforeWndProc::Continue
        );
        assert_eq!(render_loop.pending_mode_action.load(Ordering::Acquire), 0);
    }

    #[test]
    fn leaving_expanded_map_clears_escape_latch_and_local_mode_action() {
        let mut render_loop = FullscreenRenderLoop::new(ConsumerBackend::DirectX12);
        let now = Instant::now();
        render_loop
            .escape_lock_pending
            .store(true, Ordering::Release);
        render_loop
            .pending_mode_action
            .store(ConsumerAction::Lock as u32, Ordering::Release);
        let _ = render_loop
            .cursor_sync
            .update(now, true, FrameInputMode::Interactive, true, true);
        render_loop.cursor_sync.note_enter_intent(now);
        render_loop
            .interaction_authorization
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .note_enter_requested(now);
        *render_loop
            .pending_enter_intent
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(now);

        render_loop.reset_cursor_interaction();

        assert!(!render_loop.escape_lock_pending.load(Ordering::Acquire));
        assert_eq!(render_loop.pending_mode_action.load(Ordering::Acquire), 0);
        assert!(!render_loop.cursor_sync.has_pending_transition());
        assert!(!render_loop.cursor_sync.enter_intent_is_recent(now));
        assert!(!render_loop.interaction_authorized());
        assert!(
            render_loop
                .pending_enter_intent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
    }

    #[test]
    fn expanded_filter_buttons_cover_both_rows_in_stable_order() {
        let buttons = filter_buttons(FramePoiFilters::default());
        assert_eq!(buttons.len(), 8);
        assert_eq!(buttons[0].1, ConsumerAction::ToggleFastTravel);
        assert_eq!(buttons[3].1, ConsumerAction::ToggleDungeon);
        assert_eq!(buttons[4].1, ConsumerAction::ToggleTower);
        assert_eq!(buttons[7].1, ConsumerAction::ToggleSalvage);
        assert!(buttons[4].2);
        assert!(!buttons[7].2);
    }

    #[test]
    fn interactive_fullscreen_map_keeps_escape_available_to_the_game() {
        let mut render_loop = FullscreenRenderLoop::new(ConsumerBackend::DirectX12);
        render_loop.current = Some(presented_frame(
            FrameDisplayMode::ExpandedMap,
            FrameInputMode::Interactive,
        ));

        let filter = message_filter_for_frame(render_loop.current.as_ref(), true);

        assert!(filter.contains(MessageFilter::InputMouse));
        assert!(filter.contains(MessageFilter::InputRaw));
        assert!(filter.contains(MessageFilter::InputKeyboardExceptEscape));
        assert!(!filter.contains(MessageFilter::InputKeyboard));
    }

    #[test]
    fn locked_expanded_map_does_not_capture_main_menu_input() {
        let mut render_loop = FullscreenRenderLoop::new(ConsumerBackend::DirectX12);
        render_loop.current = Some(presented_frame(
            FrameDisplayMode::ExpandedMap,
            FrameInputMode::Locked,
        ));

        let filter = message_filter_for_frame(render_loop.current.as_ref(), false);

        assert!(filter.is_empty());
    }

    #[test]
    fn persisted_interactive_first_frame_requests_lock_without_capturing_input() {
        let mut authorization = InteractionAuthorization::default();
        let now = Instant::now();

        assert_eq!(
            authorization.update(now, true, true, FrameInputMode::Interactive, Some(true),),
            InteractionAuthorizationDecision::RequestLock
        );
        assert!(!authorization.is_authorized());
        let frame = presented_frame(FrameDisplayMode::ExpandedMap, FrameInputMode::Interactive);
        assert!(message_filter_for_frame(Some(&frame), false).is_empty());
        assert_eq!(
            authorization.update(
                now + MODE_ACTION_RETRY_INTERVAL - Duration::from_millis(1),
                true,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::NoCapture
        );
        assert_eq!(
            authorization.update(
                now + MODE_ACTION_RETRY_INTERVAL,
                true,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::RequestLock
        );
    }

    #[test]
    fn explicit_enter_request_authorizes_only_visible_interactive_ack() {
        let mut authorization = InteractionAuthorization::default();
        let now = Instant::now();
        authorization.note_enter_requested(now);

        assert_eq!(
            authorization.update(now, true, true, FrameInputMode::Locked, Some(true),),
            InteractionAuthorizationDecision::NoCapture
        );
        assert_eq!(
            authorization.update(
                now + Duration::from_millis(1),
                true,
                true,
                FrameInputMode::Interactive,
                Some(false),
            ),
            InteractionAuthorizationDecision::NoCapture
        );
        assert!(!authorization.is_authorized());
        assert_eq!(
            authorization.update(
                now + Duration::from_millis(2),
                true,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::Capture
        );
        assert!(authorization.is_authorized());
    }

    #[test]
    fn interactive_ack_after_authorization_timeout_is_rejected_and_locked() {
        let mut authorization = InteractionAuthorization::default();
        let now = Instant::now();
        authorization.note_enter_requested(now);

        assert_eq!(
            authorization.update(
                now + INTERACTION_ACK_TIMEOUT + Duration::from_millis(1),
                true,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::RequestLock
        );
        assert!(!authorization.is_authorized());
    }

    #[test]
    fn hidden_exit_disconnect_and_restart_clear_interaction_authorization() {
        let now = Instant::now();
        let mut authorization = InteractionAuthorization::default();
        authorization.note_enter_requested(now);
        assert_eq!(
            authorization.update(
                now + Duration::from_millis(1),
                true,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::Capture
        );

        assert_eq!(
            authorization.update(
                now + Duration::from_millis(2),
                true,
                false,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::NoCapture
        );
        assert!(!authorization.is_authorized());

        authorization.note_enter_requested(now + Duration::from_millis(3));
        let _ = authorization.update(
            now + Duration::from_millis(4),
            true,
            true,
            FrameInputMode::Interactive,
            Some(true),
        );
        assert_eq!(
            authorization.update(
                now + Duration::from_millis(5),
                false,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::NoCapture
        );
        assert!(!authorization.is_authorized());

        let mut restarted = InteractionAuthorization::default();
        assert_eq!(
            restarted.update(
                now + Duration::from_millis(6),
                true,
                true,
                FrameInputMode::Interactive,
                Some(true),
            ),
            InteractionAuthorizationDecision::RequestLock
        );
        assert!(!restarted.is_authorized());
    }

    #[test]
    fn locked_escape_records_entry_intent_without_entering_before_cursor_observation() {
        let mut render_loop = FullscreenRenderLoop::new(ConsumerBackend::DirectX12);
        render_loop.writer_alive.store(true, Ordering::Release);
        render_loop.current = Some(presented_frame(
            FrameDisplayMode::ExpandedMap,
            FrameInputMode::Locked,
        ));

        assert!(
            render_loop.before_wnd_proc(
                HWND::default(),
                WM_KEYUP,
                WPARAM(usize::from(VK_ESCAPE.0)),
                LPARAM::default(),
            ) == BeforeWndProc::Continue
        );
        assert_eq!(
            ConsumerAction::from_raw(render_loop.pending_mode_action.swap(0, Ordering::AcqRel),),
            None
        );
        assert!(
            render_loop
                .pending_enter_intent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_some()
        );

        render_loop.current.as_mut().expect("frame").input_mode = FrameInputMode::Interactive;
        assert!(
            render_loop.before_wnd_proc(
                HWND::default(),
                WM_KEYUP,
                WPARAM(usize::from(VK_ESCAPE.0)),
                LPARAM::default(),
            ) == BeforeWndProc::Break
        );
        assert_eq!(
            ConsumerAction::from_raw(render_loop.pending_mode_action.swap(0, Ordering::AcqRel),),
            Some(ConsumerAction::Lock)
        );
        assert!(render_loop.escape_lock_pending.load(Ordering::Acquire));
    }

    #[test]
    fn expanded_compact_hud_uses_the_selected_geometry() {
        let frame = presented_frame(FrameDisplayMode::ExpandedMap, FrameInputMode::Interactive);

        assert_eq!(expanded_overlay_size(&frame), [504.0, 360.0]);
        let dock = bottom_dock_layout(&frame);

        assert_eq!(dock.left, 14.0);
        assert_eq!(dock.top, 302.0);
        assert_eq!(dock.width, 476.0);
        assert_eq!(dock.height, 44.0);
        assert!(dock.left + dock.width <= expanded_overlay_size(&frame)[0] - PANEL_INSET);
    }

    #[test]
    fn expanded_texture_uses_logical_display_geometry_and_centers_in_the_game_viewport() {
        let mut frame = presented_frame(FrameDisplayMode::ExpandedMap, FrameInputMode::Interactive);
        frame.display_width = 1_600;
        frame.display_height = 900;

        assert_eq!(frame_display_size(&frame), [1_600.0, 900.0]);
        assert_eq!(expanded_overlay_size(&frame), [1_600.0, 900.0]);
        assert_eq!(overlay_origin(&frame, [1_920.0, 1_080.0]), [160.0, 90.0]);
    }

    #[test]
    fn logical_drag_is_scaled_back_to_the_bounded_transfer_raster() {
        let mut frame = presented_frame(FrameDisplayMode::ExpandedMap, FrameInputMode::Interactive);
        frame.raster_width = 1_024;
        frame.raster_height = 576;
        frame.display_width = 1_920;
        frame.display_height = 1_080;

        let delta = logical_drag_to_raster(&frame, [192.0, -108.0]);

        assert!((delta[0] - 102.4).abs() < 0.001);
        assert!((delta[1] + 57.6).abs() < 0.001);
    }

    #[test]
    fn map_drag_never_starts_on_the_dock_or_search_and_filter_popovers() {
        let frame = presented_frame(FrameDisplayMode::ExpandedMap, FrameInputMode::Interactive);
        let dock = bottom_dock_layout(&frame);

        assert!(map_drag_starts_at(&frame, [250.0, 150.0], false, false));
        assert!(!map_drag_starts_at(
            &frame,
            [dock.left + 10.0, dock.top + 10.0],
            false,
            false,
        ));
        assert!(!map_drag_starts_at(
            &frame,
            [dock.left + 10.0, dock.top - 20.0],
            false,
            true,
        ));
        assert!(!map_drag_starts_at(
            &frame,
            [dock.left + 10.0, dock.top - 20.0],
            true,
            false,
        ));
    }

    #[test]
    fn visible_main_menu_cursor_does_not_enter_without_escape_intent() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();

        assert_eq!(
            sync.update(now, true, FrameInputMode::Locked, true, false),
            None
        );
        assert!(!sync.has_pending_transition());
    }

    #[test]
    fn explicit_escape_intent_with_visible_cursor_enters_and_retries_until_acknowledged() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();
        sync.note_enter_intent(now);

        assert_eq!(
            sync.update(now, true, FrameInputMode::Locked, true, false),
            Some(ConsumerAction::EnterInteractive)
        );
        assert_eq!(
            sync.update(
                now + MODE_ACTION_RETRY_INTERVAL - Duration::from_millis(1),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            None
        );
        assert_eq!(
            sync.update(
                now + MODE_ACTION_RETRY_INTERVAL,
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            Some(ConsumerAction::EnterInteractive)
        );
        assert_eq!(
            sync.update(
                now + MODE_ACTION_RETRY_INTERVAL + Duration::from_millis(1),
                true,
                FrameInputMode::Interactive,
                true,
                false,
            ),
            None
        );
        assert!(!sync.has_pending_transition());
    }

    #[test]
    fn explicit_escape_intent_waits_for_visible_game_cursor() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();
        sync.note_enter_intent(now);

        assert_eq!(
            sync.update(now, true, FrameInputMode::Locked, false, false),
            None
        );
        assert_eq!(
            sync.update(
                now + Duration::from_millis(1),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            Some(ConsumerAction::EnterInteractive)
        );
    }

    #[test]
    fn expired_escape_intent_does_not_enter_with_visible_cursor() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();
        sync.note_enter_intent(now);

        assert_eq!(
            sync.update(
                now + ENTER_INTERACTION_INTENT_TIMEOUT + Duration::from_millis(1),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            None
        );
        assert!(!sync.has_pending_transition());
        assert!(!sync.enter_intent_is_recent(
            now + ENTER_INTERACTION_INTENT_TIMEOUT + Duration::from_millis(1)
        ));
    }

    #[test]
    fn transient_hidden_cursor_does_not_lock_interaction() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();

        assert_eq!(
            sync.update(now, true, FrameInputMode::Interactive, false, false),
            None
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE - Duration::from_millis(1),
                true,
                FrameInputMode::Interactive,
                false,
                false,
            ),
            None
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE,
                true,
                FrameInputMode::Interactive,
                true,
                false,
            ),
            None
        );
        assert!(!sync.has_pending_transition());
    }

    #[test]
    fn stable_hidden_cursor_locks_after_grace_period() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();

        assert_eq!(
            sync.update(now, true, FrameInputMode::Interactive, false, false),
            None
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE,
                true,
                FrameInputMode::Interactive,
                false,
                false,
            ),
            Some(ConsumerAction::Lock)
        );
    }

    #[test]
    fn visible_recovery_after_lock_does_not_reenter_without_new_escape() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();

        assert_eq!(
            sync.update(now, true, FrameInputMode::Interactive, false, false),
            None
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE,
                true,
                FrameInputMode::Interactive,
                false,
                false,
            ),
            Some(ConsumerAction::Lock)
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE + Duration::from_millis(1),
                true,
                FrameInputMode::Interactive,
                true,
                false,
            ),
            None
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE + Duration::from_millis(2),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            None
        );
        assert!(!sync.has_pending_transition());
    }

    #[test]
    fn explicit_escape_lock_suppresses_reentry_until_locked_and_hidden() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();

        assert_eq!(
            sync.update(now, true, FrameInputMode::Interactive, true, true),
            Some(ConsumerAction::Lock)
        );
        assert_eq!(
            sync.update(
                now + Duration::from_millis(1),
                true,
                FrameInputMode::Locked,
                true,
                true,
            ),
            None
        );
        assert!(!escape_lock_acknowledged(FrameInputMode::Locked, true));
        assert!(escape_lock_acknowledged(FrameInputMode::Locked, false));
        assert_eq!(
            sync.update(
                now + Duration::from_millis(2),
                true,
                FrameInputMode::Locked,
                false,
                true,
            ),
            None
        );
        assert_eq!(
            sync.update(
                now + Duration::from_millis(3),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            None
        );
        sync.note_enter_intent(now + Duration::from_millis(4));
        assert_eq!(
            sync.update(
                now + Duration::from_millis(4),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            Some(ConsumerAction::EnterInteractive)
        );
    }

    #[test]
    fn leaving_expanded_map_resets_cursor_transition_timers() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();

        assert_eq!(
            sync.update(now, true, FrameInputMode::Interactive, false, false),
            None
        );
        assert_eq!(
            sync.update(
                now + Duration::from_millis(50),
                false,
                FrameInputMode::Interactive,
                false,
                false,
            ),
            None
        );
        assert_eq!(
            sync.update(
                now + CURSOR_HIDE_GRACE,
                true,
                FrameInputMode::Interactive,
                false,
                false,
            ),
            None
        );
    }

    #[test]
    fn leaving_expanded_map_discards_unconsumed_escape_intent() {
        let mut sync = CursorInteractionSync::default();
        let now = Instant::now();
        sync.note_enter_intent(now);

        assert_eq!(
            sync.update(now, false, FrameInputMode::Locked, true, false),
            None
        );
        assert_eq!(
            sync.update(
                now + Duration::from_millis(1),
                true,
                FrameInputMode::Locked,
                true,
                false,
            ),
            None
        );
        assert!(!sync.has_pending_transition());
    }

    #[test]
    fn mode_actions_are_drained_before_ui_actions_without_discarding_the_ui_action() {
        let pending_mode_action = AtomicU32::new(ConsumerAction::Lock as u32);
        let pending_action = AtomicU32::new(ConsumerAction::ToggleBoss as u32);

        assert_eq!(
            take_pending_consumer_action(&pending_mode_action, &pending_action),
            Some(ConsumerAction::Lock)
        );
        assert_eq!(
            take_pending_consumer_action(&pending_mode_action, &pending_action),
            Some(ConsumerAction::ToggleBoss)
        );
        assert_eq!(
            take_pending_consumer_action(&pending_mode_action, &pending_action),
            None
        );
    }

    #[test]
    fn unified_search_ranks_exact_pal_names_before_subtitle_matches() {
        let catalog = vec![
            FrameSearchEntry {
                kind: FrameSearchKind::Resource,
                title: "석탄".to_owned(),
                subtitle: "자원 위치".to_owned(),
            },
            FrameSearchEntry {
                kind: FrameSearchKind::Pal,
                title: "페스키".to_owned(),
                subtitle: "팰 출현 지역 · SkyDragon".to_owned(),
            },
            FrameSearchEntry {
                kind: FrameSearchKind::Poi,
                title: "페스키의 봉인 영역".to_owned(),
                subtitle: "필드 보스".to_owned(),
            },
        ];

        assert_eq!(search_matches(&catalog, "페스키", 7), vec![1, 2]);
        assert_eq!(search_matches(&catalog, "SkyDragon", 7), vec![1]);
        assert!(search_matches(&catalog, "없음", 7).is_empty());
    }

    #[test]
    fn search_drawer_keeps_only_three_ranked_results_visible() {
        let catalog = (0..5)
            .map(|index| FrameSearchEntry {
                kind: FrameSearchKind::Resource,
                title: format!("석탄 광맥 {index}"),
                subtitle: "자원".to_owned(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            search_matches(&catalog, "석탄", SEARCH_MAX_RESULTS),
            vec![0, 1, 2]
        );
    }
}
