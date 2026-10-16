use std::collections::HashMap;

use sha2::{Digest, Sha256};

const MAX_FILENAME_UTF16_CODE_UNITS: usize = 255;

/// Allocates Windows-safe names for one projected directory.
pub(crate) struct FilenameResolver {
    names_by_asset_id: HashMap<String, String>,
    asset_ids_by_name: HashMap<String, String>,
}

impl FilenameResolver {
    pub(crate) fn new() -> Self {
        Self {
            names_by_asset_id: HashMap::new(),
            asset_ids_by_name: HashMap::new(),
        }
    }

    /// Returns a stable, legal filename for an asset in this directory.
    pub(crate) fn resolve(&mut self, source_name: &str, asset_id: &str) -> String {
        if let Some(name) = self.names_by_asset_id.get(asset_id) {
            return name.clone();
        }

        let sanitized = sanitize_filename(source_name);
        let name = if self.is_available(&sanitized, asset_id) {
            sanitized
        } else {
            let suffix = format!("~{}", short_asset_id(asset_id));
            let candidate = with_suffix(&sanitized, &suffix);
            if self.is_available(&candidate, asset_id) {
                candidate
            } else {
                with_suffix(
                    &sanitized,
                    &format!("{suffix}-{}", stable_id_hash(asset_id)),
                )
            }
        };

        self.asset_ids_by_name
            .insert(windows_name_key(&name), asset_id.to_owned());
        self.names_by_asset_id
            .insert(asset_id.to_owned(), name.clone());
        name
    }

    fn is_available(&self, name: &str, asset_id: &str) -> bool {
        self.asset_ids_by_name
            .get(&windows_name_key(name))
            .is_none_or(|existing_asset_id| existing_asset_id == asset_id)
    }
}

fn sanitize_filename(source_name: &str) -> String {
    let name = source_name
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    ':' | '*' | '?' | '"' | '<' | '>' | '|' | '/' | '\\'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let name = name.trim_end_matches([' ', '.']);
    let (base, extension) = split_extension(name);
    let base = if is_reserved_device_name(base) {
        format!("_{base}")
    } else {
        base.to_owned()
    };
    let name = format!("{base}{extension}");

    if name.is_empty() {
        "_".to_owned()
    } else {
        truncate_filename(&name, "")
    }
}

fn with_suffix(name: &str, suffix: &str) -> String {
    truncate_filename(name, suffix)
}

fn truncate_filename(name: &str, suffix: &str) -> String {
    let (base, extension) = split_extension(name);
    let suffix = truncate_to_utf16(suffix, MAX_FILENAME_UTF16_CODE_UNITS);
    let extension_limit = MAX_FILENAME_UTF16_CODE_UNITS.saturating_sub(utf16_units(suffix));
    let extension = truncate_to_utf16(extension, extension_limit);
    let base_limit =
        MAX_FILENAME_UTF16_CODE_UNITS.saturating_sub(utf16_units(suffix) + utf16_units(extension));
    format!(
        "{}{}{}",
        truncate_to_utf16(base, base_limit),
        suffix,
        extension
    )
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 => name.split_at(index),
        _ => (name, ""),
    }
}

fn is_reserved_device_name(base: &str) -> bool {
    let name = base.to_ascii_uppercase();
    matches!(name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || name
            .strip_prefix("COM")
            .or_else(|| name.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn short_asset_id(asset_id: &str) -> String {
    let short_id = asset_id
        .chars()
        .take(8)
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    ':' | '*' | '?' | '"' | '<' | '>' | '|' | '/' | '\\'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if short_id.is_empty() {
        "asset".to_owned()
    } else {
        short_id
    }
}

fn stable_id_hash(asset_id: &str) -> String {
    let hash = Sha256::digest(asset_id.as_bytes());
    hash[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}

fn truncate_to_utf16(value: &str, maximum_units: usize) -> &str {
    let mut units = 0;
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let character_units = character.len_utf16();
        if units + character_units > maximum_units {
            break;
        }
        units += character_units;
        end = index + character.len_utf8();
    }
    &value[..end]
}

fn utf16_units(value: &str) -> usize {
    value.encode_utf16().count()
}

fn windows_name_key(name: &str) -> String {
    name.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::FilenameResolver;

    #[test]
    fn replaces_windows_illegal_and_control_characters() {
        let mut resolver = FilenameResolver::new();

        assert_eq!(
            resolver.resolve("summer:trip?\u{0007}.jpg", "asset-a"),
            "summer_trip__.jpg"
        );
    }

    #[test]
    fn makes_reserved_device_names_safe_even_with_extensions() {
        let mut resolver = FilenameResolver::new();

        assert_eq!(resolver.resolve("con.txt", "asset-a"), "_con.txt");
        assert_eq!(resolver.resolve("Lpt9", "asset-b"), "_Lpt9");
        assert_eq!(resolver.resolve("normal.jpg", "asset-c"), "normal.jpg");
    }

    #[test]
    fn removes_trailing_dots_and_spaces_and_keeps_a_non_empty_name() {
        let mut resolver = FilenameResolver::new();

        assert_eq!(resolver.resolve("holiday.jpg.  ", "asset-a"), "holiday.jpg");
        assert_eq!(resolver.resolve("...   ", "asset-b"), "_");
    }

    #[test]
    fn preserves_unicode_while_replacing_only_illegal_characters() {
        let mut resolver = FilenameResolver::new();

        assert_eq!(
            resolver.resolve("\u{590F}:\u{1F600}?\u{7167}\u{7247}.jpg", "asset-a"),
            "\u{590F}_\u{1F600}_\u{7167}\u{7247}.jpg"
        );
    }

    #[test]
    fn limits_names_to_255_utf16_code_units_without_damaging_the_extension() {
        let mut resolver = FilenameResolver::new();
        let source_name = format!("{}.jpg", "\u{1F600}".repeat(200));

        let name = resolver.resolve(&source_name, "asset-a");

        assert_eq!(name, format!("{}.jpg", "\u{1F600}".repeat(125)));
        assert!(name.encode_utf16().count() <= 255);
    }

    #[test]
    fn gives_different_assets_with_the_same_sanitized_name_a_stable_suffix() {
        let mut resolver = FilenameResolver::new();

        assert_eq!(resolver.resolve("a:b.jpg", "first-id"), "a_b.jpg");
        assert_eq!(resolver.resolve("a?b.jpg", "second-id"), "a_b~second-i.jpg");
        assert_eq!(
            resolver.resolve("other-name.jpg", "second-id"),
            "a_b~second-i.jpg"
        );
    }

    #[test]
    fn resolves_case_insensitive_windows_name_collisions_within_a_directory() {
        let mut resolver = FilenameResolver::new();

        assert_eq!(resolver.resolve("Photo.JPG", "first-id"), "Photo.JPG");
        assert_eq!(
            resolver.resolve("photo.jpg", "asset-987654"),
            "photo~asset-98.jpg"
        );
    }

    #[test]
    fn keeps_asset_ids_with_the_same_short_prefix_distinct() {
        let mut resolver = FilenameResolver::new();

        let first = resolver.resolve("same.jpg", "same-id-1");
        let second = resolver.resolve("same.jpg", "same-id-2");

        assert_eq!(first, "same.jpg");
        assert_ne!(second, first);
        assert!(second.starts_with("same~same-id-"));
        assert!(second.ends_with(".jpg"));
    }

    #[test]
    fn retains_the_collision_suffix_and_extension_when_shortening_a_long_name() {
        let mut resolver = FilenameResolver::new();
        let source_name = format!("{}.jpg", "a".repeat(300));

        resolver.resolve(&source_name, "first-id");
        let name = resolver.resolve(&source_name, "second-id");

        assert_eq!(name, format!("{}~second-i.jpg", "a".repeat(242)));
        assert_eq!(name.encode_utf16().count(), 255);
    }
}
