use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};

use pal_domain::{
    HotkeyAction, HotkeyBindings, HotkeyChord, HotkeyModifiers, HotkeyValidationReason,
};

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;
pub const MOD_NOREPEAT: u32 = 0x4000;

const ACTION_HOTKEY_ID_START: i32 = 0x5100;
pub const TRANSIENT_HOTKEY_ID_START: i32 = 0x5200;
pub const TRANSIENT_HOTKEY_ID_END: i32 = 0x52FF;

const CANONICAL_ACTIONS: [HotkeyAction; 4] = [
    HotkeyAction::OverlayVisibility,
    HotkeyAction::RotationToggle,
    HotkeyAction::TemporaryInteraction,
    HotkeyAction::InteractionLock,
];

pub const fn action_registration_id(action: HotkeyAction) -> i32 {
    ACTION_HOTKEY_ID_START + action_index(action) as i32
}

pub const fn action_from_registration_id(id: i32) -> Option<HotkeyAction> {
    let index = id - ACTION_HOTKEY_ID_START;
    if index < 0 || index >= CANONICAL_ACTIONS.len() as i32 {
        None
    } else {
        Some(CANONICAL_ACTIONS[index as usize])
    }
}

const fn action_index(action: HotkeyAction) -> usize {
    match action {
        HotkeyAction::OverlayVisibility => 0,
        HotkeyAction::RotationToggle => 1,
        HotkeyAction::TemporaryInteraction => 2,
        HotkeyAction::InteractionLock => 3,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyBackendOperation {
    Register,
    Unregister,
}

impl HotkeyBackendOperation {
    const fn label(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Unregister => "unregister",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyBackendErrorCategory {
    Collision,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyBackendError {
    operation: HotkeyBackendOperation,
    category: HotkeyBackendErrorCategory,
    code: u32,
}

impl HotkeyBackendError {
    pub const fn new(
        operation: HotkeyBackendOperation,
        category: HotkeyBackendErrorCategory,
        code: u32,
    ) -> Self {
        Self {
            operation,
            category,
            code,
        }
    }

    pub const fn operation(self) -> HotkeyBackendOperation {
        self.operation
    }

    pub const fn category(self) -> HotkeyBackendErrorCategory {
        self.category
    }

    pub const fn code(self) -> u32 {
        self.code
    }
}

impl fmt::Display for HotkeyBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "hotkey backend {} failed ({:?}, code {})",
            self.operation.label(),
            self.category,
            self.code
        )
    }
}

impl std::error::Error for HotkeyBackendError {}

pub trait HotkeyBackend {
    fn register(
        &mut self,
        id: i32,
        modifiers: u32,
        virtual_key: u32,
    ) -> Result<(), HotkeyBackendError>;

    fn unregister(&mut self, id: i32) -> Result<(), HotkeyBackendError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyDisableReason {
    OsCollision,
    DuplicateChord { conflicting_action: HotkeyAction },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisabledHotkey {
    pub action: HotkeyAction,
    pub reason: HotkeyDisableReason,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HotkeyReport {
    pub active: Vec<HotkeyAction>,
    pub disabled: Vec<DisabledHotkey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyRegistryError {
    InvalidBinding {
        action: HotkeyAction,
        reason: HotkeyValidationReason,
    },
    FatalRegister {
        action: HotkeyAction,
        source: HotkeyBackendError,
    },
    FatalUnregister {
        action: HotkeyAction,
        source: HotkeyBackendError,
    },
    Poisoned,
}

impl fmt::Display for HotkeyRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBinding { action, reason } => {
                write!(formatter, "invalid hotkey for {action:?}: {reason:?}")
            }
            Self::FatalRegister { action, source } => {
                write!(formatter, "fatal hotkey register for {action:?}: {source}")
            }
            Self::FatalUnregister { action, source } => {
                write!(
                    formatter,
                    "fatal hotkey unregister for {action:?}: {source}"
                )
            }
            Self::Poisoned => write!(formatter, "hotkey registry is poisoned"),
        }
    }
}

impl std::error::Error for HotkeyRegistryError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ActiveRegistration {
    chord: HotkeyChord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    None,
    Register(HotkeyChord),
    Duplicate {
        chord: HotkeyChord,
        conflicting_action: HotkeyAction,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    None,
    Active,
    Disabled(HotkeyDisableReason),
}

/// A registry is statically confined to the Core message-pump thread that created it.
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<pal_windows::HotkeyRegistry<pal_windows::FakeHotkeyBackend>>();
/// ```
///
/// ```compile_fail
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<pal_windows::HotkeyRegistry<pal_windows::FakeHotkeyBackend>>();
/// ```
pub struct HotkeyRegistry<B: HotkeyBackend> {
    backend: B,
    initialized: bool,
    targets: [Target; 4],
    outcomes: [Outcome; 4],
    active: [Option<ActiveRegistration>; 4],
    unregister_attempted: [bool; 4],
    report: Option<HotkeyReport>,
    poisoned: bool,
    owner_thread: PhantomData<Rc<()>>,
}

impl<B: HotkeyBackend> HotkeyRegistry<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            initialized: false,
            targets: [Target::None; 4],
            outcomes: [Outcome::None; 4],
            active: [None; 4],
            unregister_attempted: [false; 4],
            report: None,
            poisoned: false,
            owner_thread: PhantomData,
        }
    }

    pub fn replace(
        &mut self,
        bindings: HotkeyBindings,
    ) -> Result<&HotkeyReport, HotkeyRegistryError> {
        if self.poisoned {
            return Err(HotkeyRegistryError::Poisoned);
        }
        validate_bindings(bindings)?;
        let targets = resolve_targets(bindings);

        if self.initialized && targets == self.targets {
            return Ok(self
                .report
                .as_ref()
                .expect("initialized healthy registry has an effective report"));
        }

        for (index, action) in CANONICAL_ACTIONS.into_iter().enumerate() {
            if targets[index] == self.targets[index] {
                continue;
            }
            if self.active[index].is_some() {
                self.unregister_attempted[index] = true;
                if let Err(source) = self.backend.unregister(action_registration_id(action)) {
                    self.poisoned = true;
                    self.report = None;
                    return Err(HotkeyRegistryError::FatalUnregister { action, source });
                }
                self.active[index] = None;
            }
        }

        for (index, action) in CANONICAL_ACTIONS.into_iter().enumerate() {
            if targets[index] == self.targets[index] {
                continue;
            }
            self.outcomes[index] = match targets[index] {
                Target::None => Outcome::None,
                Target::Duplicate {
                    conflicting_action, ..
                } => Outcome::Disabled(HotkeyDisableReason::DuplicateChord { conflicting_action }),
                Target::Register(chord) => {
                    match self.backend.register(
                        action_registration_id(action),
                        modifiers_to_native(chord.modifiers),
                        u32::from(chord.virtual_key),
                    ) {
                        Ok(()) => {
                            self.active[index] = Some(ActiveRegistration { chord });
                            self.unregister_attempted[index] = false;
                            Outcome::Active
                        }
                        Err(source)
                            if source.category() == HotkeyBackendErrorCategory::Collision =>
                        {
                            self.active[index] = None;
                            Outcome::Disabled(HotkeyDisableReason::OsCollision)
                        }
                        Err(source) => {
                            self.poisoned = true;
                            self.report = None;
                            return Err(HotkeyRegistryError::FatalRegister { action, source });
                        }
                    }
                }
            };
        }

        self.targets = targets;
        self.initialized = true;
        self.report = Some(build_report(&self.outcomes));
        Ok(self
            .report
            .as_ref()
            .expect("healthy replacement just stored an effective report"))
    }

    pub fn effective_report(&self) -> Option<&HotkeyReport> {
        if self.poisoned {
            None
        } else {
            self.report.as_ref()
        }
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub fn claimed_virtual_keys(&self) -> Vec<u16> {
        self.active
            .iter()
            .flatten()
            .map(|registration| registration.chord.virtual_key)
            .collect()
    }
}

impl<B: HotkeyBackend> Drop for HotkeyRegistry<B> {
    fn drop(&mut self) {
        for (index, action) in CANONICAL_ACTIONS.into_iter().enumerate() {
            if self.active[index].take().is_some() && !self.unregister_attempted[index] {
                self.unregister_attempted[index] = true;
                let _ = self.backend.unregister(action_registration_id(action));
            }
        }
    }
}

fn validate_bindings(bindings: HotkeyBindings) -> Result<(), HotkeyRegistryError> {
    for (action, chord) in action_chords(bindings) {
        let Some(chord) = chord else {
            continue;
        };
        if chord.virtual_key == 0 {
            return Err(HotkeyRegistryError::InvalidBinding {
                action,
                reason: HotkeyValidationReason::ZeroVirtualKey,
            });
        }
        if chord.virtual_key == 0x4B && !has_modifier(chord.modifiers) {
            return Err(HotkeyRegistryError::InvalidBinding {
                action,
                reason: HotkeyValidationReason::UnmodifiedK,
            });
        }
        if !has_modifier(chord.modifiers) && matches!(chord.virtual_key, 0x30..=0x39 | 0x41..=0x5A)
        {
            return Err(HotkeyRegistryError::InvalidBinding {
                action,
                reason: HotkeyValidationReason::UnmodifiedGameKey,
            });
        }
    }
    Ok(())
}

fn resolve_targets(bindings: HotkeyBindings) -> [Target; 4] {
    let mut targets = [Target::None; 4];
    for (index, (action, chord)) in action_chords(bindings).into_iter().enumerate() {
        let Some(chord) = chord else {
            continue;
        };
        let duplicate = targets[..index]
            .iter()
            .enumerate()
            .find_map(|(prior_index, target)| match target {
                Target::Register(prior) | Target::Duplicate { chord: prior, .. }
                    if *prior == chord =>
                {
                    Some(CANONICAL_ACTIONS[prior_index])
                }
                _ => None,
            });
        targets[index] = duplicate.map_or(Target::Register(chord), |conflicting_action| {
            Target::Duplicate {
                chord,
                conflicting_action,
            }
        });
        debug_assert_eq!(action, CANONICAL_ACTIONS[index]);
    }
    targets
}

fn action_chords(bindings: HotkeyBindings) -> [(HotkeyAction, Option<HotkeyChord>); 4] {
    [
        (HotkeyAction::OverlayVisibility, bindings.overlay_visibility),
        (HotkeyAction::RotationToggle, bindings.rotation_toggle),
        (
            HotkeyAction::TemporaryInteraction,
            bindings.temporary_interaction,
        ),
        (HotkeyAction::InteractionLock, bindings.interaction_lock),
    ]
}

const fn has_modifier(modifiers: HotkeyModifiers) -> bool {
    modifiers.control || modifiers.alt || modifiers.shift || modifiers.windows
}

const fn modifiers_to_native(modifiers: HotkeyModifiers) -> u32 {
    let mut native = MOD_NOREPEAT;
    if modifiers.control {
        native |= MOD_CONTROL;
    }
    if modifiers.alt {
        native |= MOD_ALT;
    }
    if modifiers.shift {
        native |= MOD_SHIFT;
    }
    if modifiers.windows {
        native |= MOD_WIN;
    }
    native
}

fn build_report(outcomes: &[Outcome; 4]) -> HotkeyReport {
    let mut report = HotkeyReport::default();
    for (index, action) in CANONICAL_ACTIONS.into_iter().enumerate() {
        match outcomes[index] {
            Outcome::None => {}
            Outcome::Active => report.active.push(action),
            Outcome::Disabled(reason) => {
                report.disabled.push(DisabledHotkey { action, reason });
            }
        }
    }
    report
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyBackendCall {
    Register {
        id: i32,
        modifiers: u32,
        virtual_key: u32,
    },
    Unregister {
        id: i32,
    },
}

#[derive(Debug, Default)]
struct FakeHotkeyState {
    calls: Vec<HotkeyBackendCall>,
    collision_ids: Vec<i32>,
    unregister_failures: Vec<(i32, HotkeyBackendError)>,
}

#[derive(Clone, Debug, Default)]
pub struct FakeHotkeyBackend {
    state: Arc<Mutex<FakeHotkeyState>>,
}

impl FakeHotkeyBackend {
    pub fn calls(&self) -> Vec<HotkeyBackendCall> {
        self.state().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state().calls.clear();
    }

    pub fn collide_registration_id(&self, id: i32) {
        let mut state = self.state();
        if !state.collision_ids.contains(&id) {
            state.collision_ids.push(id);
        }
    }

    pub fn fail_unregister(&self, id: i32, error: HotkeyBackendError) {
        let mut state = self.state();
        state
            .unregister_failures
            .retain(|(existing, _)| *existing != id);
        state.unregister_failures.push((id, error));
    }

    fn state(&self) -> MutexGuard<'_, FakeHotkeyState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl HotkeyBackend for FakeHotkeyBackend {
    fn register(
        &mut self,
        id: i32,
        modifiers: u32,
        virtual_key: u32,
    ) -> Result<(), HotkeyBackendError> {
        let mut state = self.state();
        state.calls.push(HotkeyBackendCall::Register {
            id,
            modifiers,
            virtual_key,
        });
        if state.collision_ids.contains(&id) {
            return Err(HotkeyBackendError::new(
                HotkeyBackendOperation::Register,
                HotkeyBackendErrorCategory::Collision,
                1409,
            ));
        }
        Ok(())
    }

    fn unregister(&mut self, id: i32) -> Result<(), HotkeyBackendError> {
        let mut state = self.state();
        state.calls.push(HotkeyBackendCall::Unregister { id });
        state
            .unregister_failures
            .iter()
            .find_map(|(failed_id, error)| (*failed_id == id).then_some(*error))
            .map_or(Ok(()), Err)
    }
}
