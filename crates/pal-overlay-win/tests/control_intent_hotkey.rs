use pal_domain::{HotkeyAction, InputMode, OverlayAction, OverlaySettings};
use pal_overlay_win::{ControlIntent, control_intent_for_hotkey};

fn expected(action: OverlayAction) -> ControlIntent {
    ControlIntent::Action {
        expected_settings_version: 41,
        action,
    }
}

#[test]
fn hotkeys_become_versioned_core_actions_from_current_effective_settings() {
    let mut settings = OverlaySettings::default();

    assert_eq!(
        control_intent_for_hotkey(HotkeyAction::OverlayVisibility, &settings, 41),
        expected(OverlayAction::ToggleVisibility)
    );
    assert_eq!(
        control_intent_for_hotkey(HotkeyAction::RotationToggle, &settings, 41),
        expected(OverlayAction::ToggleRotation)
    );
    assert_eq!(
        control_intent_for_hotkey(HotkeyAction::TemporaryInteraction, &settings, 41),
        expected(OverlayAction::EnterInteractive)
    );
    assert_eq!(
        control_intent_for_hotkey(HotkeyAction::InteractionLock, &settings, 41),
        expected(OverlayAction::Lock)
    );

    settings.input_mode = InputMode::PinnedInteractive;
    assert_eq!(
        control_intent_for_hotkey(HotkeyAction::RotationToggle, &settings, 41),
        expected(OverlayAction::ToggleRotation)
    );
    assert_eq!(
        control_intent_for_hotkey(HotkeyAction::TemporaryInteraction, &settings, 41),
        expected(OverlayAction::Lock)
    );
}
