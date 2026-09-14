use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_PAGE_LIMIT: u32 = 50;
pub const MAX_PAGE_LIMIT: u32 = 100;
const CURSOR_VERSION: u8 = 1;
const MAX_ENCODED_CURSOR_BYTES: usize = 2_048;
const MAX_DECODED_CURSOR_BYTES: usize = 1_024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StableListCursor {
    version: u8,
    scope: String,
    dataset_version_filter: Option<String>,
    status_filter: Option<String>,
    sort_timestamp: String,
    row_dataset_version: String,
    row_key: String,
    row_ordinal: Option<i64>,
}

impl StableListCursor {
    pub fn new(
        scope: &str,
        dataset_version_filter: Option<&str>,
        status_filter: Option<&str>,
        sort_timestamp: &str,
        row_dataset_version: &str,
        row_key: &str,
        row_ordinal: Option<i64>,
    ) -> Result<Self, CursorError> {
        let cursor = Self {
            version: CURSOR_VERSION,
            scope: scope.to_owned(),
            dataset_version_filter: dataset_version_filter.map(ToOwned::to_owned),
            status_filter: status_filter.map(ToOwned::to_owned),
            sort_timestamp: sort_timestamp.to_owned(),
            row_dataset_version: row_dataset_version.to_owned(),
            row_key: row_key.to_owned(),
            row_ordinal,
        };
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn encode(&self) -> Result<String, CursorError> {
        self.validate()?;
        let json = serde_json::to_vec(self).map_err(|_| CursorError::Malformed)?;
        if json.len() > MAX_DECODED_CURSOR_BYTES {
            return Err(CursorError::TooLong);
        }
        Ok(URL_SAFE_NO_PAD.encode(json))
    }

    pub fn decode_for(
        encoded: &str,
        expected_scope: &str,
        expected_dataset_version_filter: Option<&str>,
        expected_status_filter: Option<&str>,
    ) -> Result<Self, CursorError> {
        if encoded.is_empty() || encoded.len() > MAX_ENCODED_CURSOR_BYTES {
            return Err(CursorError::TooLong);
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| CursorError::Malformed)?;
        if decoded.len() > MAX_DECODED_CURSOR_BYTES {
            return Err(CursorError::TooLong);
        }
        let cursor: Self = serde_json::from_slice(&decoded).map_err(|_| CursorError::Malformed)?;
        cursor.validate()?;
        if cursor.scope != expected_scope
            || cursor.dataset_version_filter.as_deref() != expected_dataset_version_filter
            || cursor.status_filter.as_deref() != expected_status_filter
        {
            return Err(CursorError::ContextMismatch);
        }
        Ok(cursor)
    }

    pub fn sort_timestamp(&self) -> &str {
        &self.sort_timestamp
    }

    pub fn row_dataset_version(&self) -> &str {
        &self.row_dataset_version
    }

    pub fn row_key(&self) -> &str {
        &self.row_key
    }

    pub const fn row_ordinal(&self) -> Option<i64> {
        self.row_ordinal
    }

    fn validate(&self) -> Result<(), CursorError> {
        if self.version != CURSOR_VERSION
            || !valid_token(&self.scope, 48)
            || self
                .dataset_version_filter
                .as_deref()
                .is_some_and(|value| !valid_token(value, 128))
            || self
                .status_filter
                .as_deref()
                .is_some_and(|value| !valid_token(value, 32))
            || !valid_timestamp(&self.sort_timestamp)
            || !valid_token(&self.row_dataset_version, 128)
            || !valid_token(&self.row_key, 256)
            || self.row_ordinal.is_some_and(|value| value < 0)
        {
            return Err(CursorError::Malformed);
        }
        Ok(())
    }
}

pub fn page_limit(requested: Option<u32>) -> Result<u32, CursorError> {
    let limit = requested.unwrap_or(DEFAULT_PAGE_LIMIT);
    if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
        return Err(CursorError::InvalidLimit);
    }
    Ok(limit)
}

fn valid_token(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn valid_timestamp(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.is_ascii()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b':' | b'.' | b'+' | b'T' | b'Z' | b' ')
        })
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CursorError {
    #[error("cursor is malformed")]
    Malformed,
    #[error("cursor is too long")]
    TooLong,
    #[error("cursor does not match the requested list or filters")]
    ContextMismatch,
    #[error("limit must be between 1 and 100")]
    InvalidLimit,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_cursor() -> StableListCursor {
        StableListCursor::new(
            "knowledge-errors",
            Some("dataset-v1"),
            Some("open"),
            "2026-07-30 12:34:56",
            "dataset-v1",
            "error:missing-edge",
            None,
        )
        .unwrap()
    }

    #[test]
    fn cursor_round_trip_preserves_filter_context_and_sort_key() {
        let cursor = fixture_cursor();
        let encoded = cursor.encode().unwrap();
        let decoded = StableListCursor::decode_for(
            &encoded,
            "knowledge-errors",
            Some("dataset-v1"),
            Some("open"),
        )
        .unwrap();
        assert_eq!(decoded, cursor);
        assert!(!encoded.contains(['{', '}', '"']));
    }

    #[test]
    fn cursor_rejects_tampering_wrong_scope_and_filter_reuse() {
        let encoded = fixture_cursor().encode().unwrap();
        assert_eq!(
            StableListCursor::decode_for(
                &encoded,
                "knowledge-manifests",
                Some("dataset-v1"),
                Some("open")
            ),
            Err(CursorError::ContextMismatch)
        );
        assert_eq!(
            StableListCursor::decode_for(
                &encoded,
                "knowledge-errors",
                Some("dataset-v2"),
                Some("open")
            ),
            Err(CursorError::ContextMismatch)
        );
        assert_eq!(
            StableListCursor::decode_for("***", "knowledge-errors", None, None),
            Err(CursorError::Malformed)
        );
    }

    #[test]
    fn page_limit_is_bounded() {
        assert_eq!(page_limit(None), Ok(50));
        assert_eq!(page_limit(Some(1)), Ok(1));
        assert_eq!(page_limit(Some(100)), Ok(100));
        assert_eq!(page_limit(Some(0)), Err(CursorError::InvalidLimit));
        assert_eq!(page_limit(Some(101)), Err(CursorError::InvalidLimit));
    }
}
