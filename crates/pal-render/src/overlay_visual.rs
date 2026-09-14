use pal_domain::{
    DisplayMode, EGG_LAYER_IDS, Freshness, InputMode, PoiFilters, RESOURCE_LAYER_IDS,
    SALVAGE_LAYER_IDS, TOWER_LAYER_IDS, layer_group_selected,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    rgba: [u8; 4],
}

impl Rgba8 {
    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            rgba: [red, green, blue, alpha],
        }
    }

    pub const fn rgb(self) -> [u8; 3] {
        [self.rgba[0], self.rgba[1], self.rgba[2]]
    }

    pub const fn rgba(self) -> [u8; 4] {
        self.rgba
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayPalette {
    pub neutral_backdrop: Rgba8,
    pub neutral_rail: Rgba8,
    pub neutral_border: Rgba8,
    pub minimap_bezel_shadow: Rgba8,
    pub minimap_bezel_outer: Rgba8,
    pub minimap_bezel_body: Rgba8,
    pub minimap_bezel_highlight: Rgba8,
    pub minimap_bezel_inner: Rgba8,
    pub minimap_tick_muted: Rgba8,
    pub neutral_text: Rgba8,
    pub neutral_muted: Rgba8,
    pub cyan_live: Rgba8,
    pub amber_warning: Rgba8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayVisualSpec {
    pub client_inset_dip: u32,
    pub mini_width_dip: u32,
    pub mini_height_dip: u32,
    pub expanded_width_dip: u32,
    pub expanded_height_dip: u32,
    pub expanded_rail_width_dip: u32,
    pub expanded_header_height_dip: u32,
    pub expanded_panel_width_dip: u32,
    pub expanded_panel_inset_dip: u32,
    pub expanded_search_row_height_dip: u32,
    pub corner_radius_dip: u32,
    pub border_dip: u32,
    pub minimap_bezel_body_dip: u32,
    pub minimap_bezel_inner_inset_dip: u32,
    pub minimap_tick_length_dip: u32,
    pub rail_button_dip: u32,
    pub rail_inset_dip: u32,
    pub filter_button_width_dip: u32,
    pub filter_button_height_dip: u32,
    pub filter_button_gap_dip: u32,
    pub filter_strip_inset_dip: u32,
    pub status_dot_dip: u32,
    pub palette: OverlayPalette,
}

pub const OVERLAY_VISUAL_V1: OverlayVisualSpec = OverlayVisualSpec {
    client_inset_dip: 16,
    mini_width_dip: 288,
    mini_height_dip: 288,
    expanded_width_dip: 1120,
    expanded_height_dip: 630,
    expanded_rail_width_dip: 56,
    expanded_header_height_dip: 0,
    expanded_panel_width_dip: 260,
    expanded_panel_inset_dip: 14,
    expanded_search_row_height_dip: 50,
    corner_radius_dip: 10,
    border_dip: 1,
    minimap_bezel_body_dip: 6,
    minimap_bezel_inner_inset_dip: 9,
    minimap_tick_length_dip: 8,
    rail_button_dip: 40,
    rail_inset_dip: 8,
    filter_button_width_dip: 114,
    filter_button_height_dip: 34,
    filter_button_gap_dip: 8,
    filter_strip_inset_dip: 12,
    status_dot_dip: 8,
    palette: OverlayPalette {
        neutral_backdrop: Rgba8::new(0x07, 0x0d, 0x14, 0xee),
        neutral_rail: Rgba8::new(0x0b, 0x17, 0x21, 0xf4),
        neutral_border: Rgba8::new(0x42, 0x6f, 0x7a, 0xe8),
        minimap_bezel_shadow: Rgba8::new(0x02, 0x07, 0x0b, 0xff),
        minimap_bezel_outer: Rgba8::new(0x0a, 0x17, 0x1e, 0xff),
        minimap_bezel_body: Rgba8::new(0x18, 0x30, 0x3a, 0xff),
        minimap_bezel_highlight: Rgba8::new(0x8f, 0xd6, 0xdc, 0xff),
        minimap_bezel_inner: Rgba8::new(0x2d, 0x55, 0x5f, 0xff),
        minimap_tick_muted: Rgba8::new(0x5a, 0x7d, 0x85, 0xff),
        neutral_text: Rgba8::new(0xee, 0xf4, 0xf7, 0xff),
        neutral_muted: Rgba8::new(0xa7, 0xb3, 0xbe, 0xff),
        cyan_live: Rgba8::new(0x34, 0xd7, 0xe6, 0xff),
        amber_warning: Rgba8::new(0xf2, 0xb8, 0x4b, 0xff),
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateBadge {
    Approved,
    NotApproved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Cyan,
    Amber,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailAction {
    Collapse,
    ZoomIn,
    ZoomOut,
    ToggleRotation,
    Lock,
}

impl RailAction {
    const fn index(self) -> usize {
        match self {
            Self::Collapse => 0,
            Self::ZoomIn => 1,
            Self::ZoomOut => 2,
            Self::ToggleRotation => 3,
            Self::Lock => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterAction {
    FastTravel,
    Boss,
    Wanted,
    Dungeon,
    Tower,
    Egg,
    Resources,
    Salvage,
}

impl FilterAction {
    const fn index(self) -> usize {
        match self {
            Self::FastTravel => 0,
            Self::Boss => 1,
            Self::Wanted => 2,
            Self::Dungeon => 3,
            Self::Tower => 4,
            Self::Egg => 5,
            Self::Resources => 6,
            Self::Salvage => 7,
        }
    }

    /// Full Korean name used by search results, details, and settings.
    pub const fn label_ko(self) -> &'static str {
        match self {
            Self::FastTravel => "참수리 상",
            Self::Boss => "필드 보스",
            Self::Wanted => "지명수배",
            Self::Dungeon => "던전",
            Self::Tower => "탑",
            Self::Egg => "팰의 알",
            Self::Resources => "자원",
            Self::Salvage => "인양",
        }
    }

    /// Short Korean name reserved for the overlay's constrained filter strip.
    pub const fn compact_label_ko(self) -> &'static str {
        match self {
            Self::FastTravel => "이동",
            Self::Boss => "보스",
            Self::Wanted => "현상",
            Self::Dungeon => "던전",
            Self::Tower => "탑",
            Self::Egg => "알",
            Self::Resources => "자원",
            Self::Salvage => "인양",
        }
    }

    /// ImGui label keeps a stable hidden identifier while exposing Korean UI.
    pub const fn imgui_label_ko(self) -> &'static str {
        match self {
            Self::FastTravel => "이동##filter_fast_travel",
            Self::Boss => "보스##filter_boss",
            Self::Wanted => "현상##filter_wanted",
            Self::Dungeon => "던전##filter_dungeon",
            Self::Tower => "탑##filter_tower",
            Self::Egg => "알##filter_egg",
            Self::Resources => "자원##filter_resources",
            Self::Salvage => "인양##filter_salvage",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailButtonState {
    Idle,
    Hovered,
    Pressed,
    Active,
}

pub const fn rail_button_tone(state: RailButtonState) -> StatusTone {
    match state {
        RailButtonState::Idle => StatusTone::Neutral,
        RailButtonState::Hovered | RailButtonState::Pressed | RailButtonState::Active => {
            StatusTone::Amber
        }
    }
}

pub const fn filter_button_tone(state: RailButtonState) -> StatusTone {
    match state {
        RailButtonState::Idle => StatusTone::Neutral,
        RailButtonState::Hovered | RailButtonState::Pressed => StatusTone::Amber,
        RailButtonState::Active => StatusTone::Cyan,
    }
}

const NO_RAIL: [RailAction; 0] = [];
const EXPANDED_RAIL: [RailAction; 5] = [
    RailAction::Collapse,
    RailAction::ZoomIn,
    RailAction::ZoomOut,
    RailAction::ToggleRotation,
    RailAction::Lock,
];
const NO_FILTERS: [FilterAction; 0] = [];
const EXPANDED_FILTERS: [FilterAction; 8] = [
    FilterAction::FastTravel,
    FilterAction::Boss,
    FilterAction::Wanted,
    FilterAction::Dungeon,
    FilterAction::Tower,
    FilterAction::Egg,
    FilterAction::Resources,
    FilterAction::Salvage,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChromePlan {
    status_tone: StatusTone,
    gate_badge: GateBadge,
    rail_actions: &'static [RailAction],
    rail_button_states: [RailButtonState; 5],
    filter_actions: &'static [FilterAction],
    filter_button_states: [RailButtonState; 8],
}

impl ChromePlan {
    pub const fn status_tone(self) -> StatusTone {
        self.status_tone
    }

    pub const fn gate_badge(self) -> GateBadge {
        self.gate_badge
    }

    pub const fn rail_actions(self) -> &'static [RailAction] {
        self.rail_actions
    }

    pub const fn rail_button_state(self, action: RailAction) -> RailButtonState {
        self.rail_button_states[action.index()]
    }

    pub const fn with_rail_button_state(
        mut self,
        action: RailAction,
        state: RailButtonState,
    ) -> Self {
        self.rail_button_states[action.index()] = state;
        self
    }

    pub const fn filter_actions(self) -> &'static [FilterAction] {
        self.filter_actions
    }

    pub const fn filter_button_state(self, action: FilterAction) -> RailButtonState {
        self.filter_button_states[action.index()]
    }

    pub const fn with_filter_button_state(
        mut self,
        action: FilterAction,
        state: RailButtonState,
    ) -> Self {
        self.filter_button_states[action.index()] = state;
        self
    }

    pub fn with_filter_selection(mut self, filters: &PoiFilters) -> Self {
        for (action, selected) in [
            (FilterAction::FastTravel, filters.fast_travel),
            (FilterAction::Boss, filters.boss),
            (FilterAction::Wanted, filters.wanted),
            (FilterAction::Dungeon, filters.dungeon),
            (
                FilterAction::Tower,
                layer_group_selected(&filters.enabled_layer_ids, &TOWER_LAYER_IDS),
            ),
            (
                FilterAction::Egg,
                layer_group_selected(&filters.enabled_layer_ids, &EGG_LAYER_IDS),
            ),
            (
                FilterAction::Resources,
                layer_group_selected(&filters.enabled_layer_ids, &RESOURCE_LAYER_IDS),
            ),
            (
                FilterAction::Salvage,
                layer_group_selected(&filters.enabled_layer_ids, &SALVAGE_LAYER_IDS),
            ),
        ] {
            self.filter_button_states[action.index()] = if selected {
                RailButtonState::Active
            } else {
                RailButtonState::Idle
            };
        }
        self
    }

    pub const fn has_filter_strip(self) -> bool {
        !self.filter_actions.is_empty()
    }

    pub const fn shows_cardinals(self) -> bool {
        !self.rail_actions.is_empty()
    }

    pub const fn shows_scale_bar(self) -> bool {
        !self.rail_actions.is_empty()
    }
}

pub const fn build_chrome_plan(
    display_mode: DisplayMode,
    _input_mode: InputMode,
    freshness: Freshness,
    gate_badge: GateBadge,
) -> ChromePlan {
    let status_tone =
        if matches!(freshness, Freshness::Live) && matches!(gate_badge, GateBadge::Approved) {
            StatusTone::Cyan
        } else {
            StatusTone::Amber
        };

    ChromePlan {
        status_tone,
        gate_badge,
        rail_actions: match display_mode {
            DisplayMode::MiniMap => &NO_RAIL,
            DisplayMode::ExpandedMap => &EXPANDED_RAIL,
        },
        rail_button_states: [RailButtonState::Idle; 5],
        filter_actions: match display_mode {
            DisplayMode::MiniMap => &NO_FILTERS,
            DisplayMode::ExpandedMap => &EXPANDED_FILTERS,
        },
        filter_button_states: [RailButtonState::Idle; 8],
    }
}
