//! Generated, repository-wide game build capability contract.

#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/runtime_builds.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_build_and_live_position_capability_are_explicit() {
        assert_eq!(CURRENT_GAME_BUILD_ID, "24575825");
        assert_eq!(CURRENT_GAME_BUILD_ID_U64, 24_575_825);
        assert_eq!(CURRENT_STEAM_GAME_BUILD_ID, "steam:24575825");
        assert_eq!(WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID, "24575825");
        assert_eq!(WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID_U64, 24_575_825);
        const { assert!(CURRENT_BUILD_HAS_LIVE_POSITION_PROFILE) };
    }
}
