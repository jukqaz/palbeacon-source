use pal_wire::v2::{ClientRole, HotkeyAction, HotkeyBindings, HotkeyChord};
use prost::Message;

#[test]
fn retired_action_and_role_numbers_are_not_reassigned() {
    assert!(HotkeyAction::try_from(2).is_err());
    assert_eq!(HotkeyAction::RotationToggle as i32, 5);
    assert!(ClientRole::try_from(1).is_err());
    assert_eq!(ClientRole::ManagementUi as i32, 3);
}

#[test]
fn old_expansion_binding_is_not_decoded_as_rotation() {
    let chord = HotkeyChord::default().encode_to_vec();
    let mut old_tag_two = vec![0x12, chord.len() as u8];
    old_tag_two.extend_from_slice(&chord);
    let decoded = HotkeyBindings::decode(old_tag_two.as_slice()).unwrap();
    assert!(decoded.rotation_toggle.is_none());

    let current = HotkeyBindings {
        rotation_toggle: Some(HotkeyChord::default()),
        ..HotkeyBindings::default()
    };
    assert_eq!(current.encode_to_vec(), vec![0x2a, 0]);
    assert_eq!(
        HotkeyBindings::decode(current.encode_to_vec().as_slice()).unwrap(),
        current
    );
}
