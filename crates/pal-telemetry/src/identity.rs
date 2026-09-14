use sha2::{Digest, Sha256};
use thiserror::Error;
use x509_parser::{certificate::X509Certificate, extensions::GeneralName, prelude::FromDer};

use crate::wire::validate_canonical_sha256;

#[derive(Clone)]
pub struct AllowlistEntry {
    fingerprint_sha256: String,
    identity_uri: String,
    world_alias: String,
    subject_id: [u8; 32],
    revoked: bool,
}

impl AllowlistEntry {
    pub fn new(
        fingerprint_sha256: impl Into<String>,
        identity_uri: impl Into<String>,
        world_alias: impl Into<String>,
        subject_id: impl AsRef<[u8]>,
    ) -> Result<Self, IdentityError> {
        let fingerprint_sha256 = fingerprint_sha256.into();
        let identity_uri = identity_uri.into();
        let world_alias = world_alias.into();
        let subject_id: [u8; 32] = subject_id
            .as_ref()
            .try_into()
            .map_err(|_| IdentityError::InvalidConfiguration)?;
        if validate_canonical_sha256(&fingerprint_sha256).is_err()
            || !identity_uri.is_ascii()
            || !identity_uri.starts_with("spiffe://")
            || world_alias.is_empty()
            || world_alias.len() > 64
            || !world_alias.is_ascii()
        {
            return Err(IdentityError::InvalidConfiguration);
        }
        Ok(Self {
            fingerprint_sha256,
            identity_uri,
            world_alias,
            subject_id,
            revoked: false,
        })
    }

    pub const fn revoked(mut self) -> Self {
        self.revoked = true;
        self
    }
}

#[derive(Clone, Default)]
pub struct ClientAllowlist {
    entries: Vec<AllowlistEntry>,
}

impl ClientAllowlist {
    pub fn new(entries: impl IntoIterator<Item = AllowlistEntry>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn authorize_world(
        &self,
        certificate_der: &[u8],
        world_alias: &str,
    ) -> Result<(), IdentityError> {
        let (fingerprint, uris) = certificate_identity(certificate_der)?;
        if self
            .entries
            .iter()
            .any(|entry| entry.revoked && entry.fingerprint_sha256 == fingerprint)
        {
            return Err(IdentityError::NotAllowlisted);
        }
        if self.entries.iter().any(|entry| {
            !entry.revoked
                && entry.fingerprint_sha256 == fingerprint
                && uris.iter().any(|uri| uri == &entry.identity_uri)
                && entry.world_alias == world_alias
        }) {
            Ok(())
        } else {
            Err(IdentityError::NotAllowlisted)
        }
    }

    pub fn authorize_pair(
        &self,
        certificate_der: &[u8],
        world_alias: &str,
        subject_id: &[u8],
    ) -> Result<(), IdentityError> {
        let (fingerprint, uris) = certificate_identity(certificate_der)?;
        if self
            .entries
            .iter()
            .any(|entry| entry.revoked && entry.fingerprint_sha256 == fingerprint)
        {
            return Err(IdentityError::NotAllowlisted);
        }
        if self.entries.iter().any(|entry| {
            !entry.revoked
                && entry.fingerprint_sha256 == fingerprint
                && uris.iter().any(|uri| uri == &entry.identity_uri)
                && entry.world_alias == world_alias
                && entry.subject_id.as_slice() == subject_id
        }) {
            Ok(())
        } else {
            Err(IdentityError::NotAllowlisted)
        }
    }
}

pub fn certificate_sha256(certificate_der: &[u8]) -> String {
    let digest = Sha256::digest(certificate_der);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn certificate_identity(certificate_der: &[u8]) -> Result<(String, Vec<String>), IdentityError> {
    let (_, certificate) = X509Certificate::from_der(certificate_der)
        .map_err(|_| IdentityError::InvalidCertificate)?;
    let extension = certificate
        .subject_alternative_name()
        .map_err(|_| IdentityError::InvalidCertificate)?
        .ok_or(IdentityError::InvalidCertificate)?;
    let uris = extension
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::URI(uri) => Some((*uri).to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if uris.is_empty() {
        return Err(IdentityError::InvalidCertificate);
    }
    Ok((certificate_sha256(certificate_der), uris))
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("invalid client identity configuration")]
    InvalidConfiguration,
    #[error("invalid authenticated client certificate")]
    InvalidCertificate,
    #[error("authenticated client is not allowlisted")]
    NotAllowlisted,
}
