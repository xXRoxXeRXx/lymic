//! NTFS-safe names: turns any Immich-provided text into one directory or file name that Windows
//! accepts, as a pure function so every platform enforces the same rules.
//!
//! The rules (Microsoft "Naming Files, Paths, and Namespaces"):
//! - `< > : " / \ | ? *` and control characters are not allowed, each becomes `_`;
//! - a name must not end in a dot or a space, and is trimmed of surrounding whitespace;
//! - the device names `CON PRN AUX NUL COM1-9 LPT1-9` are reserved, with or without an
//!   extension (`CON.txt`), and get a leading `_`;
//! - a name is at most 255 UTF-16 code units long;
//! - a name that ends up empty becomes `_`.

/// The longest name NTFS accepts, in UTF-16 code units.
pub(super) const MAX_NAME_UNITS: usize = 255;
/// The longest extension kept intact when a long name is shortened.
const MAX_KEPT_EXTENSION_UNITS: usize = 32;
const REPLACEMENT: char = '_';
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `name` as a name Windows accepts.
pub(super) fn sanitize(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| if is_illegal(c) { REPLACEMENT } else { c })
        .collect();
    let trimmed = trim_name(&replaced);
    let mut name = if units(trimmed) > MAX_NAME_UNITS {
        shorten(trimmed)
    } else {
        trimmed.to_string()
    };
    if name.is_empty() {
        return REPLACEMENT.to_string();
    }
    if is_reserved(&name) {
        name.insert(0, REPLACEMENT);
    }
    name
}

/// `text` cut to at most `max_units` UTF-16 code units, never inside a character.
pub(super) fn truncate_units(text: &str, max_units: usize) -> &str {
    let mut used = 0;
    for (index, c) in text.char_indices() {
        used += c.len_utf16();
        if used > max_units {
            return &text[..index];
        }
    }
    text
}

pub(super) fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

fn is_illegal(c: char) -> bool {
    c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

fn trim_name(name: &str) -> &str {
    name.trim().trim_end_matches(['.', ' '])
}

/// A name over the limit, cut in its stem so that a short extension survives.
fn shorten(name: &str) -> String {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension))
            if !stem.is_empty() && units(extension) < MAX_KEPT_EXTENSION_UNITS =>
        {
            (stem, Some(extension))
        }
        _ => (name, None),
    };
    let extension_units = extension.map_or(0, |extension| units(extension) + 1);
    let stem = trim_name(truncate_units(stem, MAX_NAME_UNITS - extension_units));
    match extension {
        Some(extension) => format!("{stem}.{extension}"),
        None => stem.to_string(),
    }
}

fn is_reserved(name: &str) -> bool {
    let device = name.split('.').next().unwrap_or(name).trim_end();
    RESERVED
        .iter()
        .any(|reserved| device.eq_ignore_ascii_case(reserved))
}
