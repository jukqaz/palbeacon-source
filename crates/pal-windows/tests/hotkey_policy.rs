use pal_domain::{
    HotkeyAction, HotkeyBindings, HotkeyChord, HotkeyModifiers, HotkeyValidationReason,
};
use pal_windows::{
    DisabledHotkey, FakeHotkeyBackend, HotkeyBackendCall, HotkeyBackendError,
    HotkeyBackendErrorCategory, HotkeyBackendOperation, HotkeyDisableReason, HotkeyRegistry,
    HotkeyRegistryError, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    action_from_registration_id, action_registration_id,
};

#[test]
fn action_registration_ids_round_trip_and_reject_unowned_messages() {
    for action in [
        HotkeyAction::OverlayVisibility,
        HotkeyAction::RotationToggle,
        HotkeyAction::TemporaryInteraction,
        HotkeyAction::InteractionLock,
    ] {
        assert_eq!(
            action_from_registration_id(action_registration_id(action)),
            Some(action)
        );
    }
    assert_eq!(action_from_registration_id(0), None);
    assert_eq!(action_from_registration_id(i32::MAX), None);
}

fn chord(virtual_key: u16) -> HotkeyChord {
    HotkeyChord {
        modifiers: HotkeyModifiers::default(),
        virtual_key,
    }
}

fn modified(virtual_key: u16, control: bool, alt: bool, shift: bool, windows: bool) -> HotkeyChord {
    HotkeyChord {
        modifiers: HotkeyModifiers {
            control,
            alt,
            shift,
            windows,
        },
        virtual_key,
    }
}

fn bindings(entries: &[(HotkeyAction, HotkeyChord)]) -> HotkeyBindings {
    let mut result = HotkeyBindings::default();
    for (action, chord) in entries {
        match action {
            HotkeyAction::OverlayVisibility => result.overlay_visibility = Some(*chord),
            HotkeyAction::RotationToggle => result.rotation_toggle = Some(*chord),
            HotkeyAction::TemporaryInteraction => result.temporary_interaction = Some(*chord),
            HotkeyAction::InteractionLock => result.interaction_lock = Some(*chord),
        }
    }
    result
}

#[test]
fn defaults_register_nothing_and_never_claim_k() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);

    let report = registry.replace(HotkeyBindings::default()).unwrap();

    assert!(report.active.is_empty());
    assert!(report.disabled.is_empty());
    assert!(probe.calls().is_empty());
    assert!(!registry.claimed_virtual_keys().contains(&0x4B));
}

#[test]
fn unmodified_k_is_rejected_before_os_calls_and_leaves_registry_unchanged() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    let initial = bindings(&[(HotkeyAction::OverlayVisibility, chord(0x70))]);
    let initial_report = registry.replace(initial).unwrap().clone();
    probe.clear_calls();

    let invalid = bindings(&[
        (HotkeyAction::OverlayVisibility, chord(0x71)),
        (HotkeyAction::RotationToggle, chord(0x4B)),
    ]);
    assert_eq!(
        registry.replace(invalid).unwrap_err(),
        HotkeyRegistryError::InvalidBinding {
            action: HotkeyAction::RotationToggle,
            reason: HotkeyValidationReason::UnmodifiedK,
        }
    );
    assert!(probe.calls().is_empty());
    assert_eq!(registry.effective_report(), Some(&initial_report));
    assert_eq!(registry.claimed_virtual_keys(), vec![0x70]);
}

#[test]
fn standalone_function_keys_f1_through_f24_register_without_modifiers() {
    for virtual_key in [0x70, 0x87] {
        let backend = FakeHotkeyBackend::default();
        let probe = backend.clone();
        let mut registry = HotkeyRegistry::new(backend);

        registry
            .replace(bindings(&[(
                HotkeyAction::OverlayVisibility,
                chord(virtual_key),
            )]))
            .unwrap();

        assert_eq!(
            probe.calls(),
            vec![HotkeyBackendCall::Register {
                id: action_registration_id(HotkeyAction::OverlayVisibility),
                modifiers: MOD_NOREPEAT,
                virtual_key: u32::from(virtual_key),
            }]
        );
    }
}

#[test]
fn modified_alphanumeric_key_registers_with_the_requested_combination() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);

    registry
        .replace(bindings(&[(
            HotkeyAction::RotationToggle,
            modified(0x41, false, true, true, false),
        )]))
        .unwrap();

    assert_eq!(
        probe.calls(),
        vec![HotkeyBackendCall::Register {
            id: action_registration_id(HotkeyAction::RotationToggle),
            modifiers: MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
            virtual_key: 0x41,
        }]
    );
}

#[test]
fn unmodified_alphanumeric_key_is_rejected_atomically() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    let initial = bindings(&[(HotkeyAction::OverlayVisibility, chord(0x70))]);
    let initial_report = registry.replace(initial).unwrap().clone();
    probe.clear_calls();

    let invalid = bindings(&[
        (HotkeyAction::OverlayVisibility, chord(0x71)),
        (HotkeyAction::RotationToggle, chord(0x41)),
    ]);
    assert_eq!(
        registry.replace(invalid).unwrap_err(),
        HotkeyRegistryError::InvalidBinding {
            action: HotkeyAction::RotationToggle,
            reason: HotkeyValidationReason::UnmodifiedGameKey,
        }
    );
    assert!(probe.calls().is_empty());
    assert_eq!(registry.effective_report(), Some(&initial_report));
}

#[test]
fn zero_virtual_key_is_rejected_atomically() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    let initial = bindings(&[(HotkeyAction::RotationToggle, chord(0x71))]);
    let initial_report = registry.replace(initial).unwrap().clone();
    probe.clear_calls();

    let invalid = bindings(&[
        (HotkeyAction::OverlayVisibility, chord(0x72)),
        (HotkeyAction::TemporaryInteraction, chord(0)),
    ]);
    assert_eq!(
        registry.replace(invalid).unwrap_err(),
        HotkeyRegistryError::InvalidBinding {
            action: HotkeyAction::TemporaryInteraction,
            reason: HotkeyValidationReason::ZeroVirtualKey,
        }
    );
    assert!(probe.calls().is_empty());
    assert_eq!(registry.effective_report(), Some(&initial_report));
}

#[test]
fn each_modifier_maps_exactly_and_mod_norepeat_is_always_included() {
    let cases = [
        (HotkeyModifiers::default(), MOD_NOREPEAT),
        (
            HotkeyModifiers {
                control: true,
                ..HotkeyModifiers::default()
            },
            MOD_CONTROL | MOD_NOREPEAT,
        ),
        (
            HotkeyModifiers {
                alt: true,
                ..HotkeyModifiers::default()
            },
            MOD_ALT | MOD_NOREPEAT,
        ),
        (
            HotkeyModifiers {
                shift: true,
                ..HotkeyModifiers::default()
            },
            MOD_SHIFT | MOD_NOREPEAT,
        ),
        (
            HotkeyModifiers {
                windows: true,
                ..HotkeyModifiers::default()
            },
            MOD_WIN | MOD_NOREPEAT,
        ),
        (
            HotkeyModifiers {
                control: true,
                alt: true,
                shift: true,
                windows: true,
            },
            MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_WIN | MOD_NOREPEAT,
        ),
    ];

    for (modifiers, expected) in cases {
        let backend = FakeHotkeyBackend::default();
        let probe = backend.clone();
        let mut registry = HotkeyRegistry::new(backend);
        registry
            .replace(bindings(&[(
                HotkeyAction::OverlayVisibility,
                HotkeyChord {
                    modifiers,
                    virtual_key: 0x70,
                },
            )]))
            .unwrap();
        assert_eq!(
            probe.calls(),
            vec![HotkeyBackendCall::Register {
                id: action_registration_id(HotkeyAction::OverlayVisibility),
                modifiers: expected,
                virtual_key: 0x70,
            }]
        );
    }
}

#[test]
fn one_os_collision_disables_only_that_action() {
    let backend = FakeHotkeyBackend::default();
    backend.collide_registration_id(action_registration_id(HotkeyAction::RotationToggle));
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);

    let report = registry
        .replace(bindings(&[
            (HotkeyAction::OverlayVisibility, chord(0x70)),
            (HotkeyAction::RotationToggle, chord(0x71)),
            (HotkeyAction::TemporaryInteraction, chord(0x72)),
        ]))
        .unwrap();

    assert_eq!(
        report.active,
        vec![
            HotkeyAction::OverlayVisibility,
            HotkeyAction::TemporaryInteraction
        ]
    );
    assert_eq!(
        report.disabled,
        vec![DisabledHotkey {
            action: HotkeyAction::RotationToggle,
            reason: HotkeyDisableReason::OsCollision,
        }]
    );
    assert_eq!(probe.calls().len(), 3);
}

#[test]
fn duplicate_chord_disables_only_the_later_canonical_action() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    let same = modified(0x70, true, false, false, false);

    let report = registry
        .replace(bindings(&[
            (HotkeyAction::OverlayVisibility, same),
            (HotkeyAction::RotationToggle, same),
        ]))
        .unwrap();

    assert_eq!(report.active, vec![HotkeyAction::OverlayVisibility]);
    assert_eq!(
        report.disabled,
        vec![DisabledHotkey {
            action: HotkeyAction::RotationToggle,
            reason: HotkeyDisableReason::DuplicateChord {
                conflicting_action: HotkeyAction::OverlayVisibility,
            },
        }]
    );
    assert_eq!(probe.calls().len(), 1);
}

#[test]
fn removing_an_action_unregisters_it_exactly_once() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    registry
        .replace(bindings(&[(HotkeyAction::OverlayVisibility, chord(0x70))]))
        .unwrap();
    probe.clear_calls();

    registry.replace(HotkeyBindings::default()).unwrap();

    assert_eq!(
        probe.calls(),
        vec![HotkeyBackendCall::Unregister {
            id: action_registration_id(HotkeyAction::OverlayVisibility)
        }]
    );
}

#[test]
fn unchanged_binding_set_performs_zero_os_calls() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    let requested = bindings(&[
        (HotkeyAction::OverlayVisibility, chord(0x70)),
        (HotkeyAction::RotationToggle, chord(0x71)),
    ]);
    let first = registry.replace(requested).unwrap().clone();
    probe.clear_calls();

    let second = registry.replace(requested).unwrap();

    assert_eq!(second, &first);
    assert!(probe.calls().is_empty());
}

#[test]
fn changed_binding_only_unregisters_and_registers_its_stable_id() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    registry
        .replace(bindings(&[
            (HotkeyAction::OverlayVisibility, chord(0x70)),
            (HotkeyAction::RotationToggle, chord(0x71)),
        ]))
        .unwrap();
    probe.clear_calls();

    registry
        .replace(bindings(&[
            (HotkeyAction::OverlayVisibility, chord(0x72)),
            (HotkeyAction::RotationToggle, chord(0x71)),
        ]))
        .unwrap();

    let id = action_registration_id(HotkeyAction::OverlayVisibility);
    assert_eq!(
        probe.calls(),
        vec![
            HotkeyBackendCall::Unregister { id },
            HotkeyBackendCall::Register {
                id,
                modifiers: MOD_NOREPEAT,
                virtual_key: 0x72,
            },
        ]
    );
}

#[test]
fn changed_action_collision_does_not_retain_or_restore_old_binding() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    registry
        .replace(bindings(&[(HotkeyAction::OverlayVisibility, chord(0x70))]))
        .unwrap();
    probe.clear_calls();
    probe.collide_registration_id(action_registration_id(HotkeyAction::OverlayVisibility));

    let report = registry
        .replace(bindings(&[(HotkeyAction::OverlayVisibility, chord(0x72))]))
        .unwrap();

    assert!(report.active.is_empty());
    assert_eq!(
        report.disabled,
        vec![DisabledHotkey {
            action: HotkeyAction::OverlayVisibility,
            reason: HotkeyDisableReason::OsCollision,
        }]
    );
    assert!(registry.claimed_virtual_keys().is_empty());
    assert_eq!(
        probe.calls(),
        vec![
            HotkeyBackendCall::Unregister {
                id: action_registration_id(HotkeyAction::OverlayVisibility),
            },
            HotkeyBackendCall::Register {
                id: action_registration_id(HotkeyAction::OverlayVisibility),
                modifiers: MOD_NOREPEAT,
                virtual_key: 0x72,
            },
        ]
    );
}

#[test]
fn stable_registration_ids_persist_across_replacement() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    registry
        .replace(bindings(&[
            (HotkeyAction::RotationToggle, chord(0x70)),
            (HotkeyAction::InteractionLock, chord(0x71)),
        ]))
        .unwrap();
    probe.clear_calls();
    registry
        .replace(bindings(&[
            (HotkeyAction::RotationToggle, chord(0x72)),
            (HotkeyAction::InteractionLock, chord(0x73)),
        ]))
        .unwrap();

    let calls = probe.calls();
    for action in [HotkeyAction::RotationToggle, HotkeyAction::InteractionLock] {
        let id = action_registration_id(action);
        assert!(calls.contains(&HotkeyBackendCall::Unregister { id }));
        assert!(calls.iter().any(
            |call| matches!(call, HotkeyBackendCall::Register { id: actual, .. } if *actual == id)
        ));
    }
}

#[test]
fn drop_unregisters_each_active_id_exactly_once() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    {
        let mut registry = HotkeyRegistry::new(backend);
        registry
            .replace(bindings(&[
                (HotkeyAction::OverlayVisibility, chord(0x70)),
                (HotkeyAction::RotationToggle, chord(0x71)),
            ]))
            .unwrap();
        probe.clear_calls();
    }

    assert_eq!(
        probe.calls(),
        vec![
            HotkeyBackendCall::Unregister {
                id: action_registration_id(HotkeyAction::OverlayVisibility),
            },
            HotkeyBackendCall::Unregister {
                id: action_registration_id(HotkeyAction::RotationToggle),
            },
        ]
    );
}

#[test]
fn unregister_failure_is_fatal_poisons_registry_and_withholds_success_report() {
    let backend = FakeHotkeyBackend::default();
    let probe = backend.clone();
    let mut registry = HotkeyRegistry::new(backend);
    registry
        .replace(bindings(&[
            (HotkeyAction::OverlayVisibility, chord(0x70)),
            (HotkeyAction::RotationToggle, chord(0x71)),
        ]))
        .unwrap();
    probe.clear_calls();
    probe.fail_unregister(
        action_registration_id(HotkeyAction::OverlayVisibility),
        HotkeyBackendError::new(
            HotkeyBackendOperation::Unregister,
            HotkeyBackendErrorCategory::System,
            5,
        ),
    );

    let error = registry.replace(HotkeyBindings::default()).unwrap_err();
    assert!(matches!(
        error,
        HotkeyRegistryError::FatalUnregister {
            action: HotkeyAction::OverlayVisibility,
            ..
        }
    ));
    assert!(registry.is_poisoned());
    assert_eq!(registry.effective_report(), None);
    let calls_after_failure = probe.calls();
    assert_eq!(
        registry.replace(HotkeyBindings::default()).unwrap_err(),
        HotkeyRegistryError::Poisoned
    );
    assert_eq!(probe.calls(), calls_after_failure);

    drop(registry);
    let calls = probe.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| {
                **call
                    == HotkeyBackendCall::Unregister {
                        id: action_registration_id(HotkeyAction::OverlayVisibility),
                    }
            })
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| {
                **call
                    == HotkeyBackendCall::Unregister {
                        id: action_registration_id(HotkeyAction::RotationToggle),
                    }
            })
            .count(),
        1
    );
}

#[test]
fn active_and_disabled_report_order_is_canonical() {
    let backend = FakeHotkeyBackend::default();
    backend.collide_registration_id(action_registration_id(HotkeyAction::InteractionLock));
    let mut registry = HotkeyRegistry::new(backend);
    let duplicate = chord(0x70);

    let report = registry
        .replace(bindings(&[
            (HotkeyAction::InteractionLock, chord(0x73)),
            (HotkeyAction::TemporaryInteraction, chord(0x72)),
            (HotkeyAction::RotationToggle, duplicate),
            (HotkeyAction::OverlayVisibility, duplicate),
        ]))
        .unwrap();

    assert_eq!(
        report.active,
        vec![
            HotkeyAction::OverlayVisibility,
            HotkeyAction::TemporaryInteraction,
        ]
    );
    assert_eq!(
        report.disabled,
        vec![
            DisabledHotkey {
                action: HotkeyAction::RotationToggle,
                reason: HotkeyDisableReason::DuplicateChord {
                    conflicting_action: HotkeyAction::OverlayVisibility,
                },
            },
            DisabledHotkey {
                action: HotkeyAction::InteractionLock,
                reason: HotkeyDisableReason::OsCollision,
            },
        ]
    );
}
