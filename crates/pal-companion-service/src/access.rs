use pal_data_types::PrincipalId;
use sha2::{Digest, Sha256};
use thiserror::Error;

const PRINCIPAL_DOMAIN: &[u8] = b"pal-companion/access-principal/v1\0";

/// Identity fields copied only from a cryptographically verified Access JWT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedAccessIdentity {
    pub subject: String,
    pub email: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessRole {
    Member,
    Operator,
}

pub struct PrincipalContext {
    principal_id: PrincipalId,
    email: String,
    role: AccessRole,
}

impl PrincipalContext {
    pub fn from_verified_identity(
        identity: VerifiedAccessIdentity,
        operator_email: &str,
    ) -> Result<Self, AuthorizationError> {
        let subject = identity.subject.trim();
        let email = canonical_email(&identity.email)?;
        if subject.is_empty() || subject.len() > 512 {
            return Err(AuthorizationError::InvalidSubject);
        }

        let mut digest = Sha256::new();
        digest.update(PRINCIPAL_DOMAIN);
        digest.update(subject.as_bytes());
        let digest: [u8; 32] = digest.finalize().into();
        let mut id = [0u8; 16];
        id.copy_from_slice(&digest[..16]);

        let role = if email == canonical_email(operator_email)? {
            AccessRole::Operator
        } else {
            AccessRole::Member
        };
        Ok(Self {
            principal_id: PrincipalId::from_bytes(id),
            email,
            role,
        })
    }

    pub const fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn principal_hex(&self) -> String {
        self.principal_id.to_string()
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub const fn role(&self) -> AccessRole {
        self.role
    }

    /// Returns the authenticated owner scope. There is intentionally no
    /// parameter that could be populated from a client-supplied user ID.
    pub const fn owner_scope(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn authorize(&self, capability: ProtectedCapability) -> Result<(), AuthorizationError> {
        match capability {
            ProtectedCapability::OwnProfile
            | ProtectedCapability::OwnPals
            | ProtectedCapability::OwnInventory
            | ProtectedCapability::OwnBreeding
            | ProtectedCapability::OwnAssistant => Ok(()),
            ProtectedCapability::OperatorHealth
            | ProtectedCapability::OperatorBindings
            | ProtectedCapability::OperatorSync
            | ProtectedCapability::OperatorAudit
                if self.role == AccessRole::Operator =>
            {
                Ok(())
            }
            ProtectedCapability::OperatorHealth
            | ProtectedCapability::OperatorBindings
            | ProtectedCapability::OperatorSync
            | ProtectedCapability::OperatorAudit => Err(AuthorizationError::OperatorRequired),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectedCapability {
    OwnProfile,
    OwnPals,
    OwnInventory,
    OwnBreeding,
    OwnAssistant,
    OperatorHealth,
    OperatorBindings,
    OperatorSync,
    OperatorAudit,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AuthorizationError {
    #[error("Access subject is missing or invalid")]
    InvalidSubject,
    #[error("Access email is missing or invalid")]
    InvalidEmail,
    #[error("operator capability required")]
    OperatorRequired,
}

fn canonical_email(value: &str) -> Result<String, AuthorizationError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 320
        || value.contains(char::is_whitespace)
        || !value.contains('@')
    {
        return Err(AuthorizationError::InvalidEmail);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(subject: &str, email: &str) -> VerifiedAccessIdentity {
        VerifiedAccessIdentity {
            subject: subject.to_owned(),
            email: email.to_owned(),
        }
    }

    #[test]
    fn principal_is_stable_and_email_case_does_not_change_identity() {
        let first = PrincipalContext::from_verified_identity(
            identity("access-subject", "USER@example.com"),
            "operator@example.com",
        )
        .unwrap();
        let second = PrincipalContext::from_verified_identity(
            identity("access-subject", "user@example.com"),
            "operator@example.com",
        )
        .unwrap();

        assert_eq!(
            first.principal_id().to_string(),
            second.principal_id().to_string()
        );
        assert_eq!(first.email(), "user@example.com");
    }

    #[test]
    fn different_access_subjects_never_share_an_owner_scope() {
        let first = PrincipalContext::from_verified_identity(
            identity("subject-a", "same@example.com"),
            "operator@example.com",
        )
        .unwrap();
        let second = PrincipalContext::from_verified_identity(
            identity("subject-b", "same@example.com"),
            "operator@example.com",
        )
        .unwrap();

        assert_ne!(
            first.owner_scope().to_string(),
            second.owner_scope().to_string()
        );
    }

    #[test]
    fn operator_role_does_not_change_owner_scope() {
        let operator = PrincipalContext::from_verified_identity(
            identity("operator-subject", "operator@example.com"),
            "OPERATOR@example.com",
        )
        .unwrap();

        assert_eq!(operator.role(), AccessRole::Operator);
        assert!(operator.authorize(ProtectedCapability::OwnPals).is_ok());
        assert!(
            operator
                .authorize(ProtectedCapability::OperatorBindings)
                .is_ok()
        );
        assert_eq!(
            operator.owner_scope().to_string(),
            operator.principal_id().to_string()
        );
    }

    #[test]
    fn member_cannot_use_operator_routes() {
        let member = PrincipalContext::from_verified_identity(
            identity("member-subject", "member@example.com"),
            "operator@example.com",
        )
        .unwrap();

        assert!(member.authorize(ProtectedCapability::OwnInventory).is_ok());
        assert_eq!(
            member.authorize(ProtectedCapability::OperatorHealth),
            Err(AuthorizationError::OperatorRequired)
        );
        assert_eq!(
            member.authorize(ProtectedCapability::OperatorAudit),
            Err(AuthorizationError::OperatorRequired)
        );
    }
}
