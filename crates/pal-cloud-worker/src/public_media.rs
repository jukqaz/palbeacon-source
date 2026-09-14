//! Strict mapping between public media routes and private R2 object keys.

use thiserror::Error;

pub const ROUTE_PREFIX: &str = "/media/v2/";
const OBJECT_PREFIX: &str = "public/v1/";

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PublicMediaPathError {
    #[error("public media route is invalid")]
    InvalidRoute,
}

pub fn content_type_from_object_key(object_key: &str) -> Option<&'static str> {
    match object_key.rsplit_once('.').map(|(_, extension)| extension) {
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        _ => None,
    }
}

pub fn object_key_from_path(path: &str) -> Result<String, PublicMediaPathError> {
    let suffix = path
        .strip_prefix(ROUTE_PREFIX)
        .ok_or(PublicMediaPathError::InvalidRoute)?;
    let mut segments = suffix.split('/');
    let hash = segments
        .next()
        .filter(|value| value.len() == 64 && value.bytes().all(is_lower_hex))
        .ok_or(PublicMediaPathError::InvalidRoute)?;
    let file_name = segments
        .next()
        .filter(|value| is_safe_file_name(value))
        .ok_or(PublicMediaPathError::InvalidRoute)?;
    if segments.next().is_some() {
        return Err(PublicMediaPathError::InvalidRoute);
    }
    Ok(format!("{OBJECT_PREFIX}{hash}/{file_name}"))
}

fn is_lower_hex(value: u8) -> bool {
    value.is_ascii_digit() || (b'a'..=b'f').contains(&value)
}

fn is_safe_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && matches!(
            value.rsplit_once('.').map(|(_, extension)| extension),
            Some("png" | "webp" | "jpg" | "jpeg")
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "9a8422b91a69b78b84e6d2ee7422a8047c055595b57bcf59a20404a2833f5e91";

    #[test]
    fn maps_content_addressed_media_route_to_private_r2_key() {
        let path = format!("{ROUTE_PREFIX}{HASH}/app-mark-square-v2.png");
        assert_eq!(
            object_key_from_path(&path).unwrap(),
            format!("{OBJECT_PREFIX}{HASH}/app-mark-square-v2.png")
        );
    }

    #[test]
    fn rejects_traversal_extra_segments_and_non_hash_keys() {
        for path in [
            "/media/v2/short/file.png",
            "/media/v2/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/file.png",
            "/media/v2/../../file.png",
            "/media/v2/hash/file.svg",
            "/media/v2/hash/file.png/extra",
        ] {
            assert_eq!(
                object_key_from_path(path),
                Err(PublicMediaPathError::InvalidRoute)
            );
        }
    }

    #[test]
    fn maps_only_allowed_image_extensions_to_content_types() {
        assert_eq!(
            content_type_from_object_key("public/v1/hash/image.png"),
            Some("image/png")
        );
        assert_eq!(
            content_type_from_object_key("public/v1/hash/image.webp"),
            Some("image/webp")
        );
        assert_eq!(
            content_type_from_object_key("public/v1/hash/image.jpeg"),
            Some("image/jpeg")
        );
        assert_eq!(
            content_type_from_object_key("public/v1/hash/image.svg"),
            None
        );
    }
}
