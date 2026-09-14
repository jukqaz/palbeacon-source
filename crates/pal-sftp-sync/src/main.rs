use std::process::ExitCode;

fn main() -> ExitCode {
    match pal_sftp_sync::sync_selected_once() {
        Ok(status) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".to_owned())
            );
            if matches!(
                status.state,
                pal_sftp_sync::SftpSyncState::Synced | pal_sftp_sync::SftpSyncState::UpToDate
            ) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("SFTP sync failed: {error}");
            ExitCode::FAILURE
        }
    }
}
