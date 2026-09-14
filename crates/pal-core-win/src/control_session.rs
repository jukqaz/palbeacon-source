use std::sync::{Arc, Mutex, MutexGuard};

use pal_domain::OverlaySettings;
use pal_protocol::{
    FrameCodec, FrameError, PROTOCOL_VERSION, decode_overlay_action, encode_display_mode,
    encode_input_mode, encode_overlay_settings,
    v2::{
        ClientRole, LocalEnvelope, OverlayCommandResult, OverlayCommandStatus, OverlaySettingField,
        ProtocolError, ProtocolErrorCode, SettingsAck, SettingsApplyStatus, SettingsSnapshot,
        local_envelope::Payload,
    },
};
use pal_windows_ipc::handshake::{HelloGate, HelloRejection, SessionPolicy};
use thiserror::Error;

use crate::settings_store::{
    ActionResult, PatchRejection, PatchResult, SettingsStore, SettingsStoreError, StoreSnapshot,
};

#[derive(Clone)]
pub struct SharedSettingsStore {
    inner: Arc<Mutex<SettingsStore>>,
}

impl SharedSettingsStore {
    fn new(store: SettingsStore) -> Self {
        Self {
            inner: Arc::new(Mutex::new(store)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, SettingsStore> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> StoreSnapshot {
        self.lock().snapshot()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("framed response encoding failed")]
    FrameEncode(#[from] FrameError),
    #[error("response message ID overflow")]
    ResponseIdOverflow,
    #[error("disconnect safety transition failed")]
    Disconnect(PatchRejection),
}

pub struct ControlSession {
    gate: HelloGate,
    store: SharedSettingsStore,
    next_response_id: u64,
}

impl ControlSession {
    pub fn new(settings: OverlaySettings) -> Result<Self, SettingsStoreError> {
        Ok(Self::from_shared(SharedSettingsStore::new(
            SettingsStore::new(settings)?,
        )))
    }

    pub fn from_shared(store: SharedSettingsStore) -> Self {
        Self {
            gate: HelloGate::new(SessionPolicy::any_local_client()),
            store,
            next_response_id: 1,
        }
    }

    pub fn shared_store(&self) -> SharedSettingsStore {
        self.store.clone()
    }

    pub fn settings_snapshot(&self) -> StoreSnapshot {
        self.store.snapshot()
    }

    pub fn disconnect(&mut self) -> Result<StoreSnapshot, SessionError> {
        if !self.gate.is_established() {
            return Ok(self.store.snapshot());
        }
        self.store
            .lock()
            .force_locked()
            .map_err(SessionError::Disconnect)
    }

    pub fn handle_envelope(
        &mut self,
        envelope: LocalEnvelope,
    ) -> Result<Option<Vec<u8>>, SessionError> {
        if !self.gate.is_established() {
            return match self.gate.evaluate(&envelope) {
                Ok(_) => Ok(None),
                Err(rejection) => self.protocol_error(&envelope, rejection),
            };
        }
        if let Err(rejection) = self.gate.validate_established(&envelope) {
            return self.protocol_error(&envelope, rejection);
        }

        if let Some(Payload::SettingsQuery(query)) = envelope.payload.as_ref() {
            let snapshot = self.store.snapshot();
            let response = SettingsSnapshot {
                trace_id: query.trace_id.clone(),
                settings_version: snapshot.version(),
                settings: Some(
                    encode_overlay_settings(snapshot.settings())
                        .expect("the store contains validated domain settings"),
                ),
            };
            return self
                .response(
                    &envelope,
                    Payload::SettingsSnapshot(response),
                    self.established_connection_id(),
                )
                .map(Some);
        }

        if let Some(Payload::OverlayCommand(command)) = envelope.payload.as_ref() {
            let raw_command = match pal_protocol::v2::OverlayCommandKind::try_from(command.command)
            {
                Ok(command) => command,
                Err(_) => {
                    return self.protocol_error_values(
                        &envelope,
                        ProtocolErrorCode::ValidationFailed,
                        "overlay_command.command",
                    );
                }
            };
            let action = match decode_overlay_action(raw_command) {
                Ok(action) => action,
                Err(_) => {
                    return self.protocol_error_values(
                        &envelope,
                        ProtocolErrorCode::ValidationFailed,
                        "overlay_command.command",
                    );
                }
            };
            let result = self
                .store
                .lock()
                .apply_action(command.expected_settings_version, action);
            let (status, error_code, snapshot) = match result {
                ActionResult::Applied(snapshot) => (
                    OverlayCommandStatus::Applied,
                    ProtocolErrorCode::None,
                    snapshot,
                ),
                ActionResult::NoOp(snapshot) => (
                    OverlayCommandStatus::NoOp,
                    ProtocolErrorCode::None,
                    snapshot,
                ),
                ActionResult::VersionConflict(snapshot) => (
                    OverlayCommandStatus::Rejected,
                    ProtocolErrorCode::VersionConflict,
                    snapshot,
                ),
                ActionResult::Rejected { current, .. } => (
                    OverlayCommandStatus::Rejected,
                    ProtocolErrorCode::InvalidSettings,
                    current,
                ),
            };
            let settings = snapshot.settings();
            let response = OverlayCommandResult {
                trace_id: command.trace_id.clone(),
                status: status as i32,
                visible: settings.enabled,
                input_mode: encode_input_mode(settings.input_mode) as i32,
                display_mode: encode_display_mode(settings.display_mode) as i32,
                settings_version: snapshot.version(),
                error_code: error_code as i32,
                effective_settings: Some(
                    encode_overlay_settings(settings)
                        .expect("the store contains validated domain settings"),
                ),
            };
            return self
                .response(
                    &envelope,
                    Payload::OverlayCommandResult(response),
                    self.established_connection_id(),
                )
                .map(Some);
        }

        let Some(Payload::SettingsPatch(patch)) = envelope.payload.as_ref() else {
            return self.protocol_error_values(
                &envelope,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            );
        };
        if patch.trace_id.len() != 16 {
            return self.protocol_error_values(
                &envelope,
                ProtocolErrorCode::InvalidTraceId,
                "settings_patch.trace_id",
            );
        }
        if self.gate.accepted_role() == Some(ClientRole::Overlay)
            && patch.changed_fields.as_slice() != [OverlaySettingField::PoiFilters as i32]
        {
            return self.protocol_error_values(
                &envelope,
                ProtocolErrorCode::UnexpectedPayload,
                "settings_patch.changed_fields",
            );
        }

        let result = {
            let mut store = self.store.lock();
            match patch.candidate.clone() {
                Some(candidate) => store.apply_patch(
                    patch.expected_settings_version,
                    candidate,
                    &patch.changed_fields,
                ),
                None => store.reject_missing_candidate(),
            }
        };
        let (status, snapshot) = match result {
            PatchResult::Applied(snapshot) => (SettingsApplyStatus::Applied, snapshot),
            PatchResult::VersionConflict(snapshot) => {
                (SettingsApplyStatus::VersionConflict, snapshot)
            }
            PatchResult::Rejected { current, .. } => {
                (SettingsApplyStatus::ValidationRejected, current)
            }
        };
        let effective_settings = encode_overlay_settings(snapshot.settings())
            .expect("the store contains validated domain settings");
        let ack = SettingsAck {
            trace_id: patch.trace_id.clone(),
            status: status as i32,
            applied_version: snapshot.version(),
            effective_settings: Some(effective_settings),
            hotkey_conflicts: Vec::new(),
        };
        self.response(
            &envelope,
            Payload::SettingsAck(ack),
            self.established_connection_id(),
        )
        .map(Some)
    }

    fn protocol_error(
        &mut self,
        request: &LocalEnvelope,
        rejection: HelloRejection,
    ) -> Result<Option<Vec<u8>>, SessionError> {
        self.protocol_error_values(request, rejection.code(), rejection.field())
    }

    fn protocol_error_values(
        &mut self,
        request: &LocalEnvelope,
        code: ProtocolErrorCode,
        field: &'static str,
    ) -> Result<Option<Vec<u8>>, SessionError> {
        let payload = Payload::Error(ProtocolError {
            code: code as i32,
            field: field.to_owned(),
            offending_value: String::new(),
        });
        self.response(request, payload, self.established_connection_id())
            .map(Some)
    }

    fn response(
        &mut self,
        request: &LocalEnvelope,
        payload: Payload,
        connection_id: Vec<u8>,
    ) -> Result<Vec<u8>, SessionError> {
        let message_id = self.next_response_id;
        self.next_response_id = self
            .next_response_id
            .checked_add(1)
            .ok_or(SessionError::ResponseIdOverflow)?;
        let response = LocalEnvelope {
            protocol_version: PROTOCOL_VERSION,
            connection_id,
            message_id,
            reply_to_message_id: Some(request.message_id),
            payload: Some(payload),
        };
        FrameCodec::encode(&response).map_err(SessionError::FrameEncode)
    }

    fn established_connection_id(&self) -> Vec<u8> {
        self.gate
            .connection_id()
            .map_or_else(|| vec![0; 16], |value| value.to_vec())
    }
}
