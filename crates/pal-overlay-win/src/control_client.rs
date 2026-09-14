use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_domain::OverlaySettings;
use pal_protocol::{
    PROTOCOL_VERSION, decode_overlay_settings, encode_overlay_action, encode_overlay_settings,
    v2::{
        ClientHello, ClientRole, LocalEnvelope, OverlayCommand, OverlayCommandStatus,
        OverlaySettingField, ProtocolErrorCode, SettingsApplyStatus, SettingsPatch, SettingsQuery,
        local_envelope::Payload,
    },
};
use pal_windows_ipc::{CORE_PIPE_NAME, client::PipeClient};

use crate::ControlIntent;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const RECONNECT_INTERVAL: Duration = Duration::from_millis(250);
const IDLE_POLL: Duration = Duration::from_millis(50);
const COMMAND_CAPACITY: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub enum ControlClientEvent {
    Connected {
        settings_version: u64,
        settings: OverlaySettings,
    },
    SettingsApplied {
        settings_version: u64,
        settings: OverlaySettings,
    },
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlClientSubmit {
    Queued,
    QueueFull,
    Stopped,
}

pub struct OverlayControlClientRuntime {
    commands: SyncSender<ControlIntent>,
    events: Receiver<ControlClientEvent>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl OverlayControlClientRuntime {
    pub fn start() -> std::io::Result<Self> {
        Self::start_at(CORE_PIPE_NAME)
    }

    pub fn start_at(endpoint: &str) -> std::io::Result<Self> {
        let (command_sender, command_receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (event_sender, event_receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let endpoint = endpoint.to_owned();
        let worker = thread::Builder::new()
            .name("pal-overlay-control-client".to_owned())
            .spawn(move || {
                worker_loop(&endpoint, command_receiver, event_sender, worker_stop);
            })?;
        Ok(Self {
            commands: command_sender,
            events: event_receiver,
            stop,
            worker: Some(worker),
        })
    }

    pub fn submit(&self, intent: ControlIntent) -> ControlClientSubmit {
        match self.commands.try_send(intent) {
            Ok(()) => ControlClientSubmit::Queued,
            Err(TrySendError::Full(_)) => ControlClientSubmit::QueueFull,
            Err(TrySendError::Disconnected(_)) => ControlClientSubmit::Stopped,
        }
    }

    pub fn take_latest(&self) -> Option<ControlClientEvent> {
        let mut latest = None;
        loop {
            match self.events.try_recv() {
                Ok(event) => latest = Some(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return latest,
            }
        }
    }
}

impl Drop for OverlayControlClientRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    endpoint: &str,
    commands: Receiver<ControlIntent>,
    events: Sender<ControlClientEvent>,
    stop: Arc<AtomicBool>,
) {
    let mut disconnected_reported = false;
    while !stop.load(Ordering::Acquire) {
        let connection_id = unique_id();
        let hello = LocalEnvelope {
            protocol_version: PROTOCOL_VERSION,
            connection_id: connection_id.to_vec(),
            message_id: 1,
            reply_to_message_id: None,
            payload: Some(Payload::ClientHello(ClientHello {
                role: ClientRole::Overlay as i32,
                process_id: std::process::id(),
                minimum_protocol_version: PROTOCOL_VERSION,
                maximum_protocol_version: PROTOCOL_VERSION,
            })),
        };
        let Ok(mut client) = PipeClient::connect(endpoint, hello, CONNECT_TIMEOUT) else {
            if !disconnected_reported {
                send_latest(&events, ControlClientEvent::Disconnected);
                disconnected_reported = true;
            }
            thread::sleep(RECONNECT_INTERVAL);
            continue;
        };
        let mut next_message_id = 2_u64;
        let Some((mut settings_version, mut effective_settings)) =
            query_settings(&mut client, connection_id, &mut next_message_id)
        else {
            if !disconnected_reported {
                send_latest(&events, ControlClientEvent::Disconnected);
                disconnected_reported = true;
            }
            continue;
        };
        send_latest(
            &events,
            ControlClientEvent::Connected {
                settings_version,
                settings: effective_settings.clone(),
            },
        );

        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let intent = match commands.recv_timeout(IDLE_POLL) {
                Ok(intent) => intent,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            let requested_version = if intent.expected_settings_version() == 0 {
                settings_version
            } else {
                intent.expected_settings_version()
            };
            let Some(result) = send_intent(
                &mut client,
                connection_id,
                &mut next_message_id,
                requested_version,
                &effective_settings,
                &intent,
            ) else {
                send_latest(&events, ControlClientEvent::Disconnected);
                disconnected_reported = true;
                break;
            };
            settings_version = result.0;
            effective_settings = result.1.clone();
            send_latest(
                &events,
                ControlClientEvent::SettingsApplied {
                    settings_version: result.0,
                    settings: effective_settings.clone(),
                },
            );
            if result.2 {
                let Some(retry) = send_intent(
                    &mut client,
                    connection_id,
                    &mut next_message_id,
                    settings_version,
                    &effective_settings,
                    &intent,
                ) else {
                    send_latest(&events, ControlClientEvent::Disconnected);
                    disconnected_reported = true;
                    break;
                };
                settings_version = retry.0;
                effective_settings = retry.1.clone();
                send_latest(
                    &events,
                    ControlClientEvent::SettingsApplied {
                        settings_version: retry.0,
                        settings: effective_settings.clone(),
                    },
                );
            }
        }
    }
}

fn send_intent(
    client: &mut PipeClient,
    connection_id: [u8; 16],
    next_message_id: &mut u64,
    expected_settings_version: u64,
    effective_settings: &OverlaySettings,
    intent: &ControlIntent,
) -> Option<(u64, OverlaySettings, bool)> {
    match intent {
        ControlIntent::Action { action, .. } => {
            let (version, settings, error) = send_action(
                client,
                connection_id,
                next_message_id,
                expected_settings_version,
                *action,
            )?;
            Some((
                version,
                settings,
                error == ProtocolErrorCode::VersionConflict,
            ))
        }
        ControlIntent::ReplacePoiFilters { filters, .. } => {
            let mut candidate = effective_settings.clone();
            candidate.poi_filters = filters.clone();
            send_filter_patch(
                client,
                connection_id,
                next_message_id,
                expected_settings_version,
                candidate,
            )
        }
    }
}

fn query_settings(
    client: &mut PipeClient,
    connection_id: [u8; 16],
    next_message_id: &mut u64,
) -> Option<(u64, OverlaySettings)> {
    let message_id = take_message_id(next_message_id)?;
    let response = client
        .request(
            LocalEnvelope {
                protocol_version: PROTOCOL_VERSION,
                connection_id: connection_id.to_vec(),
                message_id,
                reply_to_message_id: None,
                payload: Some(Payload::SettingsQuery(SettingsQuery {
                    trace_id: trace_id(message_id),
                })),
            },
            REQUEST_TIMEOUT,
        )
        .ok()?;
    let Payload::SettingsSnapshot(snapshot) = response.payload? else {
        return None;
    };
    let settings = decode_overlay_settings(snapshot.settings?).ok()?;
    Some((snapshot.settings_version, settings))
}

fn send_action(
    client: &mut PipeClient,
    connection_id: [u8; 16],
    next_message_id: &mut u64,
    expected_settings_version: u64,
    action: pal_domain::OverlayAction,
) -> Option<(u64, OverlaySettings, ProtocolErrorCode)> {
    let message_id = take_message_id(next_message_id)?;
    let response = client
        .request(
            LocalEnvelope {
                protocol_version: PROTOCOL_VERSION,
                connection_id: connection_id.to_vec(),
                message_id,
                reply_to_message_id: None,
                payload: Some(Payload::OverlayCommand(OverlayCommand {
                    trace_id: trace_id(message_id),
                    command: encode_overlay_action(action) as i32,
                    expected_settings_version,
                })),
            },
            REQUEST_TIMEOUT,
        )
        .ok()?;
    let Payload::OverlayCommandResult(result) = response.payload? else {
        return None;
    };
    let status = OverlayCommandStatus::try_from(result.status).ok()?;
    if !matches!(
        status,
        OverlayCommandStatus::Applied | OverlayCommandStatus::NoOp | OverlayCommandStatus::Rejected
    ) {
        return None;
    }
    let error = ProtocolErrorCode::try_from(result.error_code).ok()?;
    let settings = decode_overlay_settings(result.effective_settings?).ok()?;
    Some((result.settings_version, settings, error))
}

fn send_filter_patch(
    client: &mut PipeClient,
    connection_id: [u8; 16],
    next_message_id: &mut u64,
    expected_settings_version: u64,
    candidate: OverlaySettings,
) -> Option<(u64, OverlaySettings, bool)> {
    let message_id = take_message_id(next_message_id)?;
    let response = client
        .request(
            LocalEnvelope {
                protocol_version: PROTOCOL_VERSION,
                connection_id: connection_id.to_vec(),
                message_id,
                reply_to_message_id: None,
                payload: Some(Payload::SettingsPatch(SettingsPatch {
                    trace_id: trace_id(message_id),
                    expected_settings_version,
                    candidate: Some(encode_overlay_settings(&candidate).ok()?),
                    changed_fields: vec![OverlaySettingField::PoiFilters as i32],
                })),
            },
            REQUEST_TIMEOUT,
        )
        .ok()?;
    let Payload::SettingsAck(ack) = response.payload? else {
        return None;
    };
    let status = SettingsApplyStatus::try_from(ack.status).ok()?;
    if !matches!(
        status,
        SettingsApplyStatus::Applied
            | SettingsApplyStatus::VersionConflict
            | SettingsApplyStatus::ValidationRejected
    ) {
        return None;
    }
    Some((
        ack.applied_version,
        decode_overlay_settings(ack.effective_settings?).ok()?,
        status == SettingsApplyStatus::VersionConflict,
    ))
}

fn take_message_id(next: &mut u64) -> Option<u64> {
    let value = *next;
    *next = next.checked_add(1)?;
    Some(value)
}

fn trace_id(message_id: u64) -> Vec<u8> {
    let mut trace = unique_id();
    trace[..8].copy_from_slice(&message_id.to_le_bytes());
    trace.to_vec()
}

fn unique_id() -> [u8; 16] {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let mut value = [0_u8; 16];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    value[..8].copy_from_slice(&now.to_le_bytes());
    value[8..].copy_from_slice(&COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    value
}

fn send_latest(sender: &Sender<ControlClientEvent>, event: ControlClientEvent) {
    let _ = sender.send(event);
}
