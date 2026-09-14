use std::{
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_core::Stream;
use pal_protocol::v2::{
    ClockProbeRequest, ClockProbeResponse, GetDescriptorRequest, GetDescriptorResponse,
    ResumeCursor, SubscribeRequest, SubscribeResponse,
    position_telemetry_service_server::{PositionTelemetryService, PositionTelemetryServiceServer},
};
use thiserror::Error;
use tonic::{
    Request, Response, Status,
    transport::server::{TcpConnectInfo, TlsConnectInfo},
};

use crate::{
    ClientAllowlist, LatestTelemetry, MAX_DECODED_MESSAGE_SIZE, ValidationError,
    validate_clock_probe_request, validate_descriptor, validate_descriptor_request,
    validate_envelope, validate_subscribe_request,
};

#[derive(Clone)]
pub struct PositionTelemetryEndpoint {
    descriptor: GetDescriptorResponse,
    coordinate_profile_sha256: String,
    latest: LatestTelemetry,
    allowlist: ClientAllowlist,
}

impl PositionTelemetryEndpoint {
    pub fn new(
        descriptor: GetDescriptorResponse,
        coordinate_profile_sha256: impl Into<String>,
        latest: LatestTelemetry,
        allowlist: ClientAllowlist,
    ) -> Result<Self, ServiceBuildError> {
        let coordinate_profile_sha256 = coordinate_profile_sha256.into();
        validate_descriptor(&descriptor, &coordinate_profile_sha256)?;
        Ok(Self {
            descriptor,
            coordinate_profile_sha256,
            latest,
            allowlist,
        })
    }

    pub fn into_server(self) -> PositionTelemetryServiceServer<Self> {
        PositionTelemetryServiceServer::new(self)
            .max_decoding_message_size(MAX_DECODED_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_DECODED_MESSAGE_SIZE)
    }

    fn certificate_der<T>(&self, request: &Request<T>) -> Result<Vec<u8>, Status> {
        let connect_info = request
            .extensions()
            .get::<TlsConnectInfo<TcpConnectInfo>>()
            .ok_or_else(permission_denied)?;
        let certificates = connect_info.peer_certs().ok_or_else(permission_denied)?;
        certificates
            .first()
            .map(|certificate| certificate.as_ref().to_vec())
            .ok_or_else(permission_denied)
    }
}

type SubscribeStream =
    Pin<Box<dyn Stream<Item = Result<SubscribeResponse, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl PositionTelemetryService for PositionTelemetryEndpoint {
    type SubscribeStream = SubscribeStream;

    async fn get_descriptor(
        &self,
        request: Request<GetDescriptorRequest>,
    ) -> Result<Response<GetDescriptorResponse>, Status> {
        let certificate = self.certificate_der(&request)?;
        self.allowlist
            .authorize_world(&certificate, &self.descriptor.world_alias)
            .map_err(|_| permission_denied())?;
        validate_descriptor_request(request.get_ref()).map_err(invalid_argument)?;
        Ok(Response::new(self.descriptor.clone()))
    }

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let certificate = self.certificate_der(&request)?;
        let request = request.into_inner();
        validate_subscribe_request(&request).map_err(invalid_argument)?;
        self.allowlist
            .authorize_pair(&certificate, &request.world_alias, &request.subject_id)
            .map_err(|_| permission_denied())?;
        if request.world_alias != self.descriptor.world_alias {
            return Err(permission_denied());
        }
        if !self.latest.is_bound_to(
            &request.world_alias,
            &request.subject_id,
            &self.coordinate_profile_sha256,
        ) {
            return Err(permission_denied());
        }

        let latest = self.latest.clone();
        let expected_profile = self.coordinate_profile_sha256.clone();
        let expected_world = request.world_alias.clone();
        let expected_subject = request.subject_id.clone();
        let mut resume = request.resume;
        let stream = async_stream::try_stream! {
            loop {
                let value = latest
                    .next_after(resume.clone())
                    .await
                    .map_err(|_| Status::unavailable("telemetry state unavailable"))?;
                validate_envelope(&value, &expected_profile).map_err(invalid_argument)?;
                if value.world_alias != expected_world || value.subject_id != expected_subject {
                    Err(Status::unavailable("telemetry state unavailable"))?;
                }
                resume = Some(ResumeCursor {
                    boot_id: value.boot_id.clone(),
                    sequence: value.sequence,
                });
                yield value;
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn clock_probe(
        &self,
        request: Request<ClockProbeRequest>,
    ) -> Result<Response<ClockProbeResponse>, Status> {
        let certificate = self.certificate_der(&request)?;
        self.allowlist
            .authorize_world(&certificate, &self.descriptor.world_alias)
            .map_err(|_| permission_denied())?;
        validate_clock_probe_request(request.get_ref()).map_err(invalid_argument)?;
        let request = request.into_inner();
        let receive = unix_millis()?;
        let send = unix_millis()?;
        Ok(Response::new(ClockProbeResponse {
            nonce: request.nonce,
            client_send_unix_ms: request.client_send_unix_ms,
            agent_receive_unix_ms: receive,
            agent_send_unix_ms: send.max(receive),
        }))
    }
}

fn unix_millis() -> Result<i64, Status> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Status::internal("clock unavailable"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| Status::internal("clock unavailable"))
}

fn permission_denied() -> Status {
    Status::permission_denied("authenticated client is not authorized")
}

fn invalid_argument(_: ValidationError) -> Status {
    Status::invalid_argument("telemetry validation failed")
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ServiceBuildError {
    #[error("telemetry service configuration is invalid")]
    InvalidConfiguration,
}

impl From<ValidationError> for ServiceBuildError {
    fn from(_: ValidationError) -> Self {
        Self::InvalidConfiguration
    }
}
