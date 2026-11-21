//! Unique names within one directory: a short suffix from the Immich ID, only on collision.
//!
//! Names are compared the way NTFS does, without regard to case. The rule is:
//! - a name held by a single entry stays as it is (sanitized, see [`super::ntfs`]);
//! - when several entries collide, *every* one of them gets ` (<suffix>)`, before the extension
//!   of a file, so none of them is privileged and the result depends only on the colliding
//!   entries, never on the order Immich returned them in or on unrelated entries;
//! - the suffix is the last six characters of the Immich ID, lowercased and stripped of
//!   dashes. The end of an ID is the random part (the start of a time-ordered UUID is shared by
//!   assets created together). Should two suffixed names still clash, for instance with an asset
//!   literally called `IMG_0001 (a1b2c3).jpg`, the clashing entry takes a longer tail of its ID
//!   until its name is free.
//!
//! So a listing repeats its names, and an entry only ever changes its name when another entry
//! with the same name appears in or leaves its own directory.

use super::ntfs;
use std::collections::{HashMap, HashSet};

/// The suffix lengths tried in turn; the last one stands for the whole ID.
const SUFFIX_LENGTHS: [usize; 5] = [6, 8, 12, 16, usize::MAX];

/// An entry that wants `name` (already sanitized) and is identified by `id`.
pub(super) struct Candidate {
    pub name: String,
    /// The Immich ID the suffix comes from. The two files of a Live Photo share the still's ID so
    /// that they get the same suffix.
    pub id: String,
}

/// Whether a name has an extension that a suffix goes in front of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    File,
    Directory,
}

/// The unique names for `candidates`, in the same order.
pub(super) fn unique(candidates: &[Candidate], shape: Shape) -> Vec<String> {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        groups
            .entry(candidate.name.to_lowercase())
            .or_default()
            .push(index);
    }
    let mut names: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    let mut taken: HashSet<String> = groups
        .iter()
        .filter(|(_, members)| members.len() == 1)
        .map(|(key, _)| key.clone())
        .collect();
    let mut colliding: Vec<usize> = groups
        .into_values()
        .filter(|members| members.len() > 1)
        .flatten()
        .collect();
    colliding.sort_by_cached_key(|&index| {
        (
            candidates[index].name.to_lowercase(),
            candidates[index].id.clone(),
        )
    });
    for index in colliding {
        let candidate = &candidates[index];
        let chosen = SUFFIX_LENGTHS
            .iter()
            .map(|&length| with_suffix(&candidate.name, &tail(&candidate.id, length), shape))
            .find(|name| !taken.contains(&name.to_lowercase()))
            .unwrap_or_else(|| {
                with_suffix(&candidate.name, &tail(&candidate.id, usize::MAX), shape)
            });
        taken.insert(chosen.to_lowercase());
        names[index] = chosen;
    }
    names
}

/// The last `length` characters of `id`, lowercased, without dashes.
fn tail(id: &str, length: usize) -> String {
    let characters: Vec<char> = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let start = characters.len().saturating_sub(length);
    characters[start..].iter().collect()
}

/// `name` with ` (<suffix>)` in front of its extension, the stem shortened where the result
/// would exceed the NTFS name limit.
fn with_suffix(name: &str, suffix: &str, shape: Shape) -> String {
    let (stem, extension) = match (shape, name.rsplit_once('.')) {
        (Shape::File, Some((stem, extension))) if !stem.is_empty() && !extension.is_empty() => {
            (stem, format!(".{extension}"))
        }
        _ => (name, String::new()),
    };
    let suffix = format!(" ({suffix})");
    let budget = ntfs::MAX_NAME_UNITS
        .saturating_sub(ntfs::units(&suffix))
        .saturating_sub(ntfs::units(&extension));
    format!("{}{suffix}{extension}", ntfs::truncate_units(stem, budget))
}
