use std::fmt;

use pal_protocol::{
    PROTOCOL_VERSION,
    v2::{ClientRole, LocalEnvelope, ProtocolErrorCode, local_envelope::Payload},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloRejectionKind {
    ExpectedHello,
    SecondHello,
    InvalidRole,
    InvalidProcessId,
    InvalidConnectionId,
    InvalidMessageId,
    IncompatibleVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloRejection {
    kind: HelloRejectionKind,
    code: ProtocolErrorCode,
    field: &'static str,
}

impl HelloRejection {
    pub const fn kind(self) -> HelloRejectionKind {
        self.kind
    }

    pub const fn code(self) -> ProtocolErrorCode {
        self.code
    }

    pub const fn field(self) -> &'static str {
        self.field
    }

    pub const fn offending_value(self) -> &'static str {
        ""
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HelloAccepted {
    process_id: u32,
    connection_id: [u8; 16],
    role: ClientRole,
}

impl HelloAccepted {
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    pub const fn connection_id(&self) -> &[u8; 16] {
        &self.connection_id
    }

    pub const fn role(&self) -> ClientRole {
        self.role
    }
}

impl fmt::Debug for HelloAccepted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HelloAccepted")
            .field("process_id", &self.process_id)
            .field("role", &self.role)
            .field("connection_id", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllowedRole {
    ManagementUi,
    Overlay,
    AnyLocalClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionPolicy {
    role: AllowedRole,
}

impl SessionPolicy {
    pub const fn management_ui() -> Self {
        Self {
            role: AllowedRole::ManagementUi,
        }
    }

    pub const fn overlay() -> Self {
        Self {
            role: AllowedRole::Overlay,
        }
    }

    pub const fn any_local_client() -> Self {
        Self {
            role: AllowedRole::AnyLocalClient,
        }
    }

    pub fn permits(&self, payload: &Payload) -> bool {
        self.client_role()
            .is_some_and(|role| self.permits_for_role(role, payload))
    }

    fn permits_for_role(&self, role: ClientRole, payload: &Payload) -> bool {
        let effective_role = match self.role {
            AllowedRole::AnyLocalClient => role,
            AllowedRole::ManagementUi => ClientRole::ManagementUi,
            AllowedRole::Overlay => ClientRole::Overlay,
        };
        match effective_role {
            ClientRole::Unspecified => false,
            ClientRole::Overlay => matches!(
                payload,
                Payload::ClientHello(_)
                    | Payload::RenderApplied(_)
                    | Payload::OverlayCommand(_)
                    | Payload::SettingsPatch(_)
                    | Payload::SettingsQuery(_)
            ),
            ClientRole::ManagementUi => !matches!(
                payload,
                Payload::RenderSnapshot(_) | Payload::RenderApplied(_)
            ),
        }
    }

    const fn client_role(self) -> Option<ClientRole> {
        match self.role {
            AllowedRole::ManagementUi => Some(ClientRole::ManagementUi),
            AllowedRole::Overlay => Some(ClientRole::Overlay),
            AllowedRole::AnyLocalClient => None,
        }
    }
}

pub struct HelloGate {
    policy: SessionPolicy,
    accepted: Option<HelloAccepted>,
}

impl HelloGate {
    pub const fn new(policy: SessionPolicy) -> Self {
        Self {
            policy,
            accepted: None,
        }
    }

    pub const fn is_established(&self) -> bool {
        self.accepted.is_some()
    }

    pub fn connection_id(&self) -> Option<&[u8; 16]> {
        self.accepted.as_ref().map(HelloAccepted::connection_id)
    }

    pub fn accepted_role(&self) -> Option<ClientRole> {
        self.accepted.as_ref().map(HelloAccepted::role)
    }

    pub fn evaluate(&mut self, envelope: &LocalEnvelope) -> Result<HelloAccepted, HelloRejection> {
        if self.accepted.is_some() {
            return Err(rejection(
                HelloRejectionKind::SecondHello,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            ));
        }
        let Some(Payload::ClientHello(hello)) = envelope.payload.as_ref() else {
            return Err(rejection(
                HelloRejectionKind::ExpectedHello,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            ));
        };
        validate_common(envelope)?;
        let role = ClientRole::try_from(hello.role).ok();
        if role == Some(ClientRole::Unspecified)
            || role.is_none()
            || self
                .policy
                .client_role()
                .is_some_and(|required| role != Some(required))
        {
            return Err(rejection(
                HelloRejectionKind::InvalidRole,
                ProtocolErrorCode::InvalidRole,
                "client_hello.role",
            ));
        }
        if hello.process_id == 0 {
            return Err(rejection(
                HelloRejectionKind::InvalidProcessId,
                ProtocolErrorCode::ValidationFailed,
                "client_hello.process_id",
            ));
        }
        if hello.minimum_protocol_version > PROTOCOL_VERSION
            || hello.maximum_protocol_version < PROTOCOL_VERSION
            || hello.minimum_protocol_version > hello.maximum_protocol_version
        {
            return Err(rejection(
                HelloRejectionKind::IncompatibleVersion,
                ProtocolErrorCode::UnsupportedVersion,
                "client_hello.protocol_range",
            ));
        }

        let connection_id = envelope
            .connection_id
            .as_slice()
            .try_into()
            .expect("common validation requires exactly sixteen bytes");
        let accepted = HelloAccepted {
            process_id: hello.process_id,
            connection_id,
            role: role.expect("role was validated above"),
        };
        self.accepted = Some(accepted.clone());
        Ok(accepted)
    }

    pub fn validate_established(&self, envelope: &LocalEnvelope) -> Result<(), HelloRejection> {
        let Some(accepted) = self.accepted.as_ref() else {
            return Err(rejection(
                HelloRejectionKind::ExpectedHello,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            ));
        };
        if matches!(envelope.payload, Some(Payload::ClientHello(_))) {
            return Err(rejection(
                HelloRejectionKind::SecondHello,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            ));
        }
        validate_common(envelope)?;
        if envelope.connection_id.as_slice() != accepted.connection_id {
            return Err(rejection(
                HelloRejectionKind::InvalidConnectionId,
                ProtocolErrorCode::InvalidConnectionId,
                "connection_id",
            ));
        }
        let Some(payload) = envelope.payload.as_ref() else {
            return Err(rejection(
                HelloRejectionKind::ExpectedHello,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            ));
        };
        if !self.policy.permits_for_role(accepted.role(), payload) {
            return Err(rejection(
                HelloRejectionKind::ExpectedHello,
                ProtocolErrorCode::UnexpectedPayload,
                "payload",
            ));
        }
        Ok(())
    }
}

fn validate_common(envelope: &LocalEnvelope) -> Result<(), HelloRejection> {
    if envelope.connection_id.len() != 16 {
        return Err(rejection(
            HelloRejectionKind::InvalidConnectionId,
            ProtocolErrorCode::InvalidConnectionId,
            "connection_id",
        ));
    }
    if envelope.message_id == 0 {
        return Err(rejection(
            HelloRejectionKind::InvalidMessageId,
            ProtocolErrorCode::ValidationFailed,
            "message_id",
        ));
    }
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(rejection(
            HelloRejectionKind::IncompatibleVersion,
            ProtocolErrorCode::UnsupportedVersion,
            "protocol_version",
        ));
    }
    Ok(())
}

const fn rejection(
    kind: HelloRejectionKind,
    code: ProtocolErrorCode,
    field: &'static str,
) -> HelloRejection {
    HelloRejection { kind, code, field }
}
