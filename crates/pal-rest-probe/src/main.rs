use std::{env, fs, process::ExitCode};

use pal_rest_probe::{
    GateAEvidence, ProbeCommand, encode_redacted_report, evaluate_unattested, parse_command,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), &'static str> {
    match parse_command(env::args()).map_err(|_| "invalid probe command")? {
        ProbeCommand::Evaluate { input_path } => {
            let bytes = fs::read(input_path).map_err(|_| "could not read aggregate evidence")?;
            let evidence = serde_json::from_slice::<GateAEvidence>(&bytes)
                .map_err(|_| "aggregate evidence is malformed")?;
            let report = evaluate_unattested(&evidence);
            let encoded = encode_redacted_report(&report, &[])
                .map_err(|_| "report failed privacy serialization")?;
            println!(
                "{}",
                String::from_utf8(encoded).map_err(|_| "report was not UTF-8")?
            );
            Ok(())
        }
        ProbeCommand::Preflight
        | ProbeCommand::Load
        | ProbeCommand::Rotation
        | ProbeCommand::Movement => Err("live probe command requires the pal-rest host adapter"),
    }
}
