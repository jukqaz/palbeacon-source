#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PublicCachePolicy {
    success_cache_control: &'static str,
    dependency_unavailable_cache_control: Option<&'static str>,
    varies_by_query: bool,
}

impl PublicCachePolicy {
    pub(crate) const fn cache_control_for_status(self, status: u16) -> Option<&'static str> {
        match status {
            200 => Some(self.success_cache_control),
            503 => self.dependency_unavailable_cache_control,
            _ => None,
        }
    }

    pub(crate) const fn varies_by_query(self) -> bool {
        self.varies_by_query
    }
}

/// Returns a cache policy only for anonymous, immutable-by-dataset API reads.
///
/// Authentication, personal projections, operator routes and every mutation
/// deliberately remain outside this allowlist.
pub(crate) fn for_get_path(path: &str) -> Option<PublicCachePolicy> {
    let (success_cache_control, dependency_unavailable_cache_control, varies_by_query) = match path
    {
        "/api/v1/catalog/status" | "/api/v1/knowledge/status" => (
            "public, max-age=60, s-maxage=300",
            Some("public, max-age=0, s-maxage=30"),
            false,
        ),
        "/api/v1/wiki/search" => ("public, max-age=300, s-maxage=900", None, true),
        "/api/v1/wiki/read" => ("public, max-age=3600, s-maxage=21600", None, true),
        "/api/v1/knowledge/related" => ("public, max-age=1800, s-maxage=7200", None, true),
        _ => return None,
    };
    Some(PublicCachePolicy {
        success_cache_control,
        dependency_unavailable_cache_control,
        varies_by_query,
    })
}

pub(crate) fn if_none_match_matches(if_none_match: &str, current_etag: &str) -> bool {
    let current = current_etag
        .trim()
        .strip_prefix("W/")
        .unwrap_or(current_etag.trim());
    if_none_match.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == current
    })
}

#[cfg(test)]
mod tests {
    use super::{for_get_path, if_none_match_matches};

    #[test]
    fn caches_only_public_reference_reads() {
        for path in ["/api/v1/catalog/status", "/api/v1/knowledge/status"] {
            let policy = for_get_path(path).expect("status cache policy");
            assert_eq!(
                policy.cache_control_for_status(503),
                Some("public, max-age=0, s-maxage=30")
            );
            assert!(!policy.varies_by_query());
        }
        for path in [
            "/api/v1/wiki/search",
            "/api/v1/wiki/read",
            "/api/v1/knowledge/related",
        ] {
            assert!(
                for_get_path(path)
                    .expect("wiki cache policy")
                    .varies_by_query()
            );
        }
    }

    #[test]
    fn never_caches_personal_operator_or_unknown_routes() {
        assert!(for_get_path("/api/v1/me").is_none());
        assert!(for_get_path("/api/v1/me/pals").is_none());
        assert!(for_get_path("/api/v1/ops/health").is_none());
        assert!(for_get_path("/api/v1/unknown").is_none());
    }

    #[test]
    fn uses_shorter_search_than_document_ttl() {
        let search = for_get_path("/api/v1/wiki/search")
            .expect("search policy")
            .cache_control_for_status(200)
            .expect("search success policy");
        let document = for_get_path("/api/v1/wiki/read")
            .expect("document policy")
            .cache_control_for_status(200)
            .expect("document success policy");
        assert!(search.contains("s-maxage=900"));
        assert!(document.contains("s-maxage=21600"));
        assert!(!search.contains("stale-while-revalidate"));
        assert!(!document.contains("stale-while-revalidate"));
    }

    #[test]
    fn does_not_cache_high_cardinality_wiki_errors() {
        for path in [
            "/api/v1/wiki/search",
            "/api/v1/wiki/read",
            "/api/v1/knowledge/related",
        ] {
            assert_eq!(
                for_get_path(path)
                    .expect("wiki cache policy")
                    .cache_control_for_status(503),
                None
            );
        }
    }

    #[test]
    fn matches_standard_etag_lists_and_weak_validators() {
        assert!(if_none_match_matches(
            "\"old\", W/\"current\"",
            "\"current\""
        ));
        assert!(if_none_match_matches("*", "\"current\""));
        assert!(!if_none_match_matches("\"other\"", "\"current\""));
    }
}
