use pal_rest_probe::{CommandError, ProbeCommand, parse_command};

#[test]
fn exposes_the_five_gate_a_subcommands() {
    for (name, expected) in [
        ("preflight", ProbeCommand::Preflight),
        ("load", ProbeCommand::Load),
        ("rotation", ProbeCommand::Rotation),
        ("movement", ProbeCommand::Movement),
    ] {
        assert_eq!(parse_command(["probe", name]), Ok(expected));
    }
    assert_eq!(
        parse_command(["probe", "evaluate", "--input", "report.json"]),
        Ok(ProbeCommand::Evaluate {
            input_path: "report.json".into(),
        })
    );
}

#[test]
fn password_arguments_are_forbidden() {
    assert_eq!(
        parse_command(["probe", "load", "--password", "secret"]),
        Err(CommandError::PasswordArgumentForbidden)
    );
}
