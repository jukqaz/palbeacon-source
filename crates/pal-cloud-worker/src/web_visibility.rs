//! Public Web route visibility policy.

/// Returns `true` when a desktop-only route must not be served by the public Web app.
#[must_use]
pub fn is_private_web_path(path: &str) -> bool {
    matches!(path, "/settings" | "/settings/" | "/settings.html")
        || path == "/connection"
        || path.starts_with("/connection/")
}

#[cfg(test)]
mod tests {
    use super::is_private_web_path;

    #[test]
    fn hides_desktop_only_routes() {
        for path in [
            "/settings",
            "/settings/",
            "/settings.html",
            "/connection",
            "/connection/profile",
            "/connection/servers",
            "/connection/overlay",
        ] {
            assert!(is_private_web_path(path), "expected {path} to be private");
        }
    }

    #[test]
    fn keeps_public_routes_visible() {
        for path in [
            "/",
            "/search",
            "/pals",
            "/map",
            "/technology",
            "/wiki",
            "/connections",
        ] {
            assert!(!is_private_web_path(path), "expected {path} to be public");
        }
    }
}
