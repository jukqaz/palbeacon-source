//! Transport-independent game-build compatibility policy.
//!
//! Public reference data never depends on a visitor's installed game build.
//! This policy is evaluated only for personal projections and other private
//! evidence that can be mapped incorrectly across incompatible game updates.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildPolicyDecision<'a> {
    Compatible { dataset_version: &'a str },
    ReviewRequired,
    Blocked,
    DatasetMismatch { expected_dataset_version: &'a str },
}

pub fn normalize_game_build_id(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix("steam:").unwrap_or(value);
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some(value.to_owned())
}

pub fn evaluate_build_policy<'a>(
    compatibility_state: Option<&str>,
    policy_dataset_version: Option<&'a str>,
    requested_dataset_version: &str,
) -> BuildPolicyDecision<'a> {
    match compatibility_state {
        Some("compatible") => match policy_dataset_version {
            Some(dataset_version) if dataset_version == requested_dataset_version => {
                BuildPolicyDecision::Compatible { dataset_version }
            }
            Some(dataset_version) => BuildPolicyDecision::DatasetMismatch {
                expected_dataset_version: dataset_version,
            },
            None => BuildPolicyDecision::ReviewRequired,
        },
        Some("blocked") => BuildPolicyDecision::Blocked,
        Some("observed") | None | Some(_) => BuildPolicyDecision::ReviewRequired,
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildPolicyDecision, evaluate_build_policy, normalize_game_build_id};

    #[test]
    fn normalizes_plain_and_steam_build_ids() {
        assert_eq!(
            normalize_game_build_id("24181527").as_deref(),
            Some("24181527")
        );
        assert_eq!(
            normalize_game_build_id("steam:24181527").as_deref(),
            Some("24181527")
        );
        assert_eq!(normalize_game_build_id("steam:"), None);
        assert_eq!(normalize_game_build_id("2418/1527"), None);
    }

    #[test]
    fn accepts_only_the_remotely_mapped_dataset() {
        assert_eq!(
            evaluate_build_policy(Some("compatible"), Some("data-v1"), "data-v1"),
            BuildPolicyDecision::Compatible {
                dataset_version: "data-v1"
            }
        );
        assert_eq!(
            evaluate_build_policy(Some("compatible"), Some("data-v1"), "data-v2"),
            BuildPolicyDecision::DatasetMismatch {
                expected_dataset_version: "data-v1"
            }
        );
    }

    #[test]
    fn unknown_builds_wait_for_review_without_blocking_public_data() {
        assert_eq!(
            evaluate_build_policy(None, None, "data-v1"),
            BuildPolicyDecision::ReviewRequired
        );
        assert_eq!(
            evaluate_build_policy(Some("observed"), None, "data-v1"),
            BuildPolicyDecision::ReviewRequired
        );
        assert_eq!(
            evaluate_build_policy(Some("blocked"), None, "data-v1"),
            BuildPolicyDecision::Blocked
        );
    }
}
