//! How a placeholder remembers what it is.
//!
//! The Cloud Files platform hands a placeholder's *file identity* back on every callback for it.
//! Lymic stores the placeholder's path relative to the Virtual Library root there: UTF-8, `/`
//! separated, no leading separator. The root itself has no identity, which reads back as the empty
//! path. Every rule about what a path means stays in the projection (`virtual_library`); this only
//! carries the string.

/// The platform rejects identities longer than this (`CF_PLACEHOLDER_MAX_FILE_IDENTITY_LENGTH`).
pub(super) const MAX_IDENTITY_BYTES: usize = 4096;

/// The path of a child `name` of the directory at `parent` (`""` is the root).
pub(super) fn child_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// The identity bytes of the placeholder at `path`, or `None` when they would exceed what the
/// platform accepts.
pub(super) fn encode(path: &str) -> Option<Vec<u8>> {
    (path.len() <= MAX_IDENTITY_BYTES).then(|| path.as_bytes().to_vec())
}

/// The path a placeholder's identity names. An absent or empty identity is the root; bytes that
/// are not UTF-8 name nothing.
pub(super) fn decode(identity: &[u8]) -> Option<String> {
    std::str::from_utf8(identity).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_path_joins_with_a_forward_slash_and_the_root_has_no_prefix() {
        assert_eq!(child_path("", "All Photos"), "All Photos");
        assert_eq!(
            child_path("All Photos/2024", "03 March"),
            "All Photos/2024/03 March"
        );
    }

    #[test]
    fn an_identity_round_trips_names_with_spaces_and_non_ascii_characters() {
        let path = "Albums/Urlaub \u{2600}\u{fe0f} 2024/IMG_0001 (a1b2c3).jpg";

        assert_eq!(decode(&encode(path).unwrap()), Some(path.to_string()));
    }

    #[test]
    fn the_root_is_the_empty_identity() {
        assert_eq!(encode(""), Some(Vec::new()));
        assert_eq!(decode(&[]), Some(String::new()));
    }

    #[test]
    fn bytes_that_are_not_utf8_name_nothing() {
        assert_eq!(decode(&[0xff, 0xfe, 0x41]), None);
    }

    #[test]
    fn the_longest_projected_path_fits_the_platform_limit() {
        // Albums/<album>/<file>, each name at the NTFS maximum of 255 UTF-16 units. A BMP
        // character takes up to 3 UTF-8 bytes per unit, an astral one 4 bytes per 2 units.
        let name = "\u{20ac}".repeat(255);
        let path = format!("Albums/{name}/{name}");

        assert!(encode(&path).is_some(), "{} bytes", path.len());
    }

    #[test]
    fn an_identity_longer_than_the_platform_limit_is_refused() {
        assert_eq!(encode(&"a".repeat(MAX_IDENTITY_BYTES + 1)), None);
        assert!(encode(&"a".repeat(MAX_IDENTITY_BYTES)).is_some());
    }
}
