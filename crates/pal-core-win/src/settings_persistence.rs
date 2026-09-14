use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use pal_overlay_control::{
    ControlSettings, OverlayControlDocument, read_or_create_control, write_control,
};
use thiserror::Error;

use crate::control_session::SharedSettingsStore;

const PERSISTENCE_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Error)]
pub enum SettingsPersistenceError {
    #[error("settings persistence worker could not be created")]
    Spawn(#[source] io::Error),
    #[error("settings persistence worker panicked")]
    WorkerPanicked,
}

pub struct SettingsPersistenceRuntime {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl SettingsPersistenceRuntime {
    pub fn start(
        store: SharedSettingsStore,
        path: PathBuf,
    ) -> Result<Self, SettingsPersistenceError> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let initial_store_version = store.snapshot().version();
        let worker = thread::Builder::new()
            .name("pal-core-settings-persistence".to_owned())
            .spawn(move || {
                persist_changes(store, path, initial_store_version, worker_stop);
            })
            .map_err(SettingsPersistenceError::Spawn)?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub fn shutdown(mut self) -> Result<(), SettingsPersistenceError> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<(), SettingsPersistenceError> {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|_| SettingsPersistenceError::WorkerPanicked)
    }
}

impl Drop for SettingsPersistenceRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

fn persist_changes(
    store: SharedSettingsStore,
    path: PathBuf,
    mut persisted_store_version: u64,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        thread::sleep(PERSISTENCE_POLL_INTERVAL);
        let snapshot = store.snapshot();
        if snapshot.version() == persisted_store_version {
            continue;
        }
        let result = (|| {
            let current = read_or_create_control(&path)?;
            let document = OverlayControlDocument::next(
                ControlSettings::from_domain(snapshot.settings()),
                current.version,
            )?;
            write_control(&path, &document)
        })();
        match result {
            Ok(()) => persisted_store_version = snapshot.version(),
            Err(error) => {
                eprintln!("pal-core: overlay settings could not be persisted; retrying: {error}")
            }
        }
    }
}
