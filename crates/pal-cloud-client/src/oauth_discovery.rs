use reqwest::{
    StatusCode, Url,
    header::{HeaderMap, WWW_AUTHENTICATE},
};

use crate::{AccessAuthError, ManagedOAuthPolicy};

const MAX_CHALLENGE_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryChallenge {
    pub protected_resource_metadata_url: Url,
}

impl DiscoveryChallenge {
    pub fn parse(
        status: StatusCode,
        headers: &HeaderMap,
        policy: &ManagedOAuthPolicy,
    ) -> Result<Self, AccessAuthError> {
        if status != StatusCode::UNAUTHORIZED {
            return Err(AccessAuthError::InvalidMetadata);
        }
        let values = headers
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|value| value.to_str().map_err(|_| AccessAuthError::InvalidMetadata))
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() != 1 || values[0].len() > MAX_CHALLENGE_BYTES {
            return Err(AccessAuthError::InvalidMetadata);
        }
        let value = values[0].trim();
        let Some(parameters) = value.strip_prefix("Bearer ") else {
            return Err(AccessAuthError::InvalidMetadata);
        };
        if parameters.is_empty() || contains_unquoted_challenge(parameters) {
            return Err(AccessAuthError::InvalidMetadata);
        }
        let parameters = parse_parameters(parameters)?;
        let metadata = parameters
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("resource_metadata"))
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>();
        if metadata.len() != 1 {
            return Err(AccessAuthError::InvalidMetadata);
        }
        let protected_resource_metadata_url =
            Url::parse(metadata[0]).map_err(|_| AccessAuthError::InvalidMetadata)?;
        if protected_resource_metadata_url != policy.protected_resource_metadata_url {
            return Err(AccessAuthError::InvalidMetadata);
        }
        Ok(Self {
            protected_resource_metadata_url,
        })
    }
}

fn parse_parameters(value: &str) -> Result<Vec<(String, String)>, AccessAuthError> {
    let mut result: Vec<(String, String)> = Vec::new();
    for part in split_quoted(value)? {
        let Some((name, value)) = part.split_once('=') else {
            return Err(AccessAuthError::InvalidMetadata);
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || value.len() < 2
            || !value.starts_with('"')
            || !value.ends_with('"')
        {
            return Err(AccessAuthError::InvalidMetadata);
        }
        let decoded = decode_quoted(&value[1..value.len() - 1])?;
        if result
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            return Err(AccessAuthError::InvalidMetadata);
        }
        result.push((name.to_owned(), decoded));
    }
    Ok(result)
}

fn split_quoted(value: &str) -> Result<Vec<&str>, AccessAuthError> {
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    let mut result = Vec::new();
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ',' if !quoted => {
                let part = value[start..index].trim();
                if part.is_empty() {
                    return Err(AccessAuthError::InvalidMetadata);
                }
                result.push(part);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return Err(AccessAuthError::InvalidMetadata);
    }
    let final_part = value[start..].trim();
    if final_part.is_empty() {
        return Err(AccessAuthError::InvalidMetadata);
    }
    result.push(final_part);
    Ok(result)
}

fn decode_quoted(value: &str) -> Result<String, AccessAuthError> {
    let mut output = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' || character.is_control() {
            return Err(AccessAuthError::InvalidMetadata);
        } else {
            output.push(character);
        }
    }
    if escaped {
        return Err(AccessAuthError::InvalidMetadata);
    }
    Ok(output)
}

fn contains_unquoted_challenge(value: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            _ => {}
        }
    }
    // A malformed quote is rejected by the parser. This helper exists to keep
    // the exact-one-header rule explicit without trying to accept a second
    // challenge encoded into the same field.
    !quoted && value.contains(", Bearer ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use reqwest::header::HeaderValue;

    use super::*;

    fn policy() -> ManagedOAuthPolicy {
        ManagedOAuthPolicy::new(
            Url::parse("https://pal.example.com/").unwrap(),
            BTreeSet::from(["https://team.cloudflareaccess.com/".to_owned()]),
        )
        .unwrap()
    }

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(WWW_AUTHENTICATE, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn requires_one_exact_rfc9728_bearer_challenge() {
        let policy = policy();
        let challenge = DiscoveryChallenge::parse(
            StatusCode::UNAUTHORIZED,
            &headers(concat!(
                "Bearer realm=\"pal\", ",
                "resource_metadata=\"https://pal.example.com/",
                ".well-known/cloudflare-access-protected-resource/\""
            )),
            &policy,
        )
        .unwrap();
        assert_eq!(
            challenge.protected_resource_metadata_url,
            policy.protected_resource_metadata_url
        );
    }

    #[test]
    fn rejects_wrong_status_scheme_url_or_duplicate_parameter() {
        let policy = policy();
        let exact = headers(
            "Bearer resource_metadata=\"https://pal.example.com/.well-known/cloudflare-access-protected-resource/\"",
        );
        assert_eq!(
            DiscoveryChallenge::parse(StatusCode::OK, &exact, &policy),
            Err(AccessAuthError::InvalidMetadata)
        );
        for challenge in [
            "Basic realm=\"pal\"",
            "Bearer resource_metadata=\"http://pal.example.com/.well-known/cloudflare-access-protected-resource/\"",
            "Bearer resource_metadata=\"https://evil.example/.well-known/cloudflare-access-protected-resource/\"",
            concat!(
                "Bearer resource_metadata=\"https://pal.example.com/.well-known/cloudflare-access-protected-resource/\",",
                " resource_metadata=\"https://pal.example.com/.well-known/cloudflare-access-protected-resource/\""
            ),
            concat!(
                "Bearer resource_metadata=\"https://pal.example.com/.well-known/cloudflare-access-protected-resource/\",",
                " Bearer realm=\"two\""
            ),
        ] {
            assert_eq!(
                DiscoveryChallenge::parse(StatusCode::UNAUTHORIZED, &headers(challenge), &policy),
                Err(AccessAuthError::InvalidMetadata),
                "{challenge}"
            );
        }
    }

    #[test]
    fn rejects_multiple_header_lines_and_malformed_quotes() {
        let policy = policy();
        let mut multiple = headers(
            "Bearer resource_metadata=\"https://pal.example.com/.well-known/cloudflare-access-protected-resource/\"",
        );
        multiple.append(
            WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"other\""),
        );
        assert_eq!(
            DiscoveryChallenge::parse(StatusCode::UNAUTHORIZED, &multiple, &policy),
            Err(AccessAuthError::InvalidMetadata)
        );
        assert_eq!(
            DiscoveryChallenge::parse(
                StatusCode::UNAUTHORIZED,
                &headers("Bearer resource_metadata=\"unterminated"),
                &policy
            ),
            Err(AccessAuthError::InvalidMetadata)
        );
    }
}
