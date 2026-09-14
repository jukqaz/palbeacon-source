//! Deterministic position-source selection for the Windows overlay.
//!
//! The local, exact-build read-only source owns the normal game path. The
//! official REST source is deliberately cold until local collection is
//! unavailable and the user has explicitly configured the fallback.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionSourceKind {
    LocalProcess,
    OfficialRest,
}

impl PositionSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalProcess => "local_process",
            Self::OfficialRest => "official_rest",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionSourceAvailability {
    Live,
    Waiting,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionSourceDecision {
    active: Option<PositionSourceKind>,
    poll_official_rest: bool,
}

impl PositionSourceDecision {
    pub const fn active(self) -> Option<PositionSourceKind> {
        self.active
    }

    pub const fn poll_official_rest(self) -> bool {
        self.poll_official_rest
    }

    pub const fn is_fallback(self) -> bool {
        matches!(self.active, Some(PositionSourceKind::OfficialRest))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionSourcePolicy {
    official_rest_configured: bool,
}

impl PositionSourcePolicy {
    pub const NAME: &'static str = "local_process_first";

    pub const fn local_only() -> Self {
        Self {
            official_rest_configured: false,
        }
    }

    pub const fn with_official_rest_fallback() -> Self {
        Self {
            official_rest_configured: true,
        }
    }

    pub const fn official_rest_configured(self) -> bool {
        self.official_rest_configured
    }

    /// Selects at most one active source.
    ///
    /// A waiting local source includes process discovery, title-screen state,
    /// and the short exact-build worker startup window. Those states must not
    /// cause a server round-trip or source flapping. REST is considered only
    /// after the local source becomes unavailable.
    pub const fn select(
        self,
        local: PositionSourceAvailability,
        official_rest: PositionSourceAvailability,
    ) -> PositionSourceDecision {
        match local {
            PositionSourceAvailability::Live => PositionSourceDecision {
                active: Some(PositionSourceKind::LocalProcess),
                poll_official_rest: false,
            },
            PositionSourceAvailability::Waiting => PositionSourceDecision {
                active: None,
                poll_official_rest: false,
            },
            PositionSourceAvailability::Unavailable if self.official_rest_configured => {
                PositionSourceDecision {
                    active: if matches!(official_rest, PositionSourceAvailability::Live) {
                        Some(PositionSourceKind::OfficialRest)
                    } else {
                        None
                    },
                    poll_official_rest: true,
                }
            }
            PositionSourceAvailability::Unavailable => PositionSourceDecision {
                active: None,
                poll_official_rest: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_live_wins_even_when_rest_is_live() {
        let decision = PositionSourcePolicy::with_official_rest_fallback().select(
            PositionSourceAvailability::Live,
            PositionSourceAvailability::Live,
        );
        assert_eq!(decision.active(), Some(PositionSourceKind::LocalProcess));
        assert!(!decision.poll_official_rest());
        assert!(!decision.is_fallback());
    }

    #[test]
    fn title_screen_wait_does_not_activate_or_poll_rest() {
        let decision = PositionSourcePolicy::with_official_rest_fallback().select(
            PositionSourceAvailability::Waiting,
            PositionSourceAvailability::Live,
        );
        assert_eq!(decision.active(), None);
        assert!(!decision.poll_official_rest());
    }

    #[test]
    fn configured_rest_is_used_only_after_local_becomes_unavailable() {
        let decision = PositionSourcePolicy::with_official_rest_fallback().select(
            PositionSourceAvailability::Unavailable,
            PositionSourceAvailability::Live,
        );
        assert_eq!(decision.active(), Some(PositionSourceKind::OfficialRest));
        assert!(decision.poll_official_rest());
        assert!(decision.is_fallback());
    }

    #[test]
    fn current_local_only_release_never_polls_rest() {
        let decision = PositionSourcePolicy::local_only().select(
            PositionSourceAvailability::Unavailable,
            PositionSourceAvailability::Live,
        );
        assert_eq!(decision.active(), None);
        assert!(!decision.poll_official_rest());
        assert!(!decision.is_fallback());
    }
}
