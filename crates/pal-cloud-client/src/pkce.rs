use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use secrecy::{ExposeSecret, SecretString};

use crate::AccessAuthError;

#[derive(Clone)]
pub struct RandomMaterial {
    bytes: [u8; 32],
}

impl RandomMaterial {
    pub fn generate() -> Result<Self, AccessAuthError> {
        let mut bytes = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| AccessAuthError::RandomUnavailable)?;
        Ok(Self { bytes })
    }

    #[cfg(test)]
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn base64url(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.bytes)
    }
}

impl std::fmt::Debug for RandomMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RandomMaterial(<redacted>)")
    }
}

pub struct PkceMaterial {
    verifier: SecretString,
    challenge: String,
}

impl PkceMaterial {
    pub fn generate() -> Result<Self, AccessAuthError> {
        Ok(Self::from_random(RandomMaterial::generate()?))
    }

    pub(crate) fn from_random(random: RandomMaterial) -> Self {
        let verifier = random.base64url();
        let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()).as_ref());
        Self {
            verifier: SecretString::from(verifier),
            challenge,
        }
    }

    pub fn challenge(&self) -> &str {
        &self.challenge
    }

    pub(crate) fn verifier(&self) -> &str {
        self.verifier.expose_secret()
    }
}

impl std::fmt::Debug for PkceMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PkceMaterial")
            .field("verifier", &"<redacted>")
            .field("challenge", &self.challenge)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s256_challenge_matches_verifier_and_debug_redacts_it() {
        let pkce = PkceMaterial::from_random(RandomMaterial::from_bytes([7; 32]));
        let expected = URL_SAFE_NO_PAD.encode(digest(&SHA256, pkce.verifier().as_bytes()).as_ref());
        assert_eq!(pkce.challenge(), expected);
        assert!(!format!("{pkce:?}").contains(pkce.verifier()));
    }

    #[test]
    fn independent_entropy_produces_independent_values() {
        let verifier = RandomMaterial::from_bytes([1; 32]).base64url();
        let state = RandomMaterial::from_bytes([2; 32]).base64url();
        let nonce = RandomMaterial::from_bytes([3; 32]).base64url();
        assert_ne!(verifier, state);
        assert_ne!(state, nonce);
        assert!(!verifier.contains('='));
    }
}
