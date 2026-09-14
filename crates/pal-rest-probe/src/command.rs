use std::path::PathBuf;

use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeCommand {
    Preflight,
    Load,
    Rotation,
    Movement,
    Evaluate { input_path: PathBuf },
}

pub fn parse_command<I, S>(args: I) -> Result<ProbeCommand, CommandError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let tail = args.collect::<Vec<_>>();
    if tail
        .iter()
        .any(|argument| argument == "--password" || argument.starts_with("--password="))
    {
        return Err(CommandError::PasswordArgumentForbidden);
    }
    let Some(subcommand) = tail.first().map(String::as_str) else {
        return Err(CommandError::MissingSubcommand);
    };
    match subcommand {
        "preflight" if tail.len() == 1 => Ok(ProbeCommand::Preflight),
        "load" if tail.len() == 1 => Ok(ProbeCommand::Load),
        "rotation" if tail.len() == 1 => Ok(ProbeCommand::Rotation),
        "movement" if tail.len() == 1 => Ok(ProbeCommand::Movement),
        "evaluate" => parse_evaluate(&tail),
        "preflight" | "load" | "rotation" | "movement" => Err(CommandError::UnexpectedArgument),
        _ => Err(CommandError::UnknownSubcommand),
    }
}

fn parse_evaluate(tail: &[String]) -> Result<ProbeCommand, CommandError> {
    if tail.len() == 3 && tail[1] == "--input" && !tail[2].is_empty() {
        Ok(ProbeCommand::Evaluate {
            input_path: PathBuf::from(&tail[2]),
        })
    } else if !tail.iter().any(|argument| argument == "--input") {
        Err(CommandError::MissingInput)
    } else {
        Err(CommandError::UnexpectedArgument)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CommandError {
    #[error("a probe subcommand is required")]
    MissingSubcommand,
    #[error("unknown probe subcommand")]
    UnknownSubcommand,
    #[error("unexpected probe argument")]
    UnexpectedArgument,
    #[error("evaluate requires --input")]
    MissingInput,
    #[error("password command-line arguments are forbidden")]
    PasswordArgumentForbidden,
}
