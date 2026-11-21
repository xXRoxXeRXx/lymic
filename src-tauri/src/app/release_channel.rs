//! The release channel this binary was built for: a compile-time label (`github` or `store`).
//!
//! It says which distribution the build is meant for, not what the process runs as; the Virtual
//! Library also needs real Package Identity (see `package_identity`).

pub(crate) const DEFAULT_RELEASE_CHANNEL: &str = "github";

pub(crate) fn resolve_release_channel(channel: Option<&str>) -> &'static str {
    match channel {
        Some("store") => "store",
        _ => DEFAULT_RELEASE_CHANNEL,
    }
}

pub(crate) fn effective_release_channel() -> &'static str {
    resolve_release_channel(option_env!("LYMIC_RELEASE_CHANNEL"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_github_release_channel_when_unset() {
        assert_eq!(resolve_release_channel(None), "github");
        assert_eq!(resolve_release_channel(Some("unknown")), "github");
        assert_eq!(resolve_release_channel(Some("github")), "github");
        assert_eq!(resolve_release_channel(Some("store")), "store");
    }
}
