//! What a projected [`Entry`] looks like as a placeholder: the metadata the platform needs to show
//! it in Explorer without the content. No Windows type is involved, so the mapping is testable on
//! every platform; the adapter only copies these fields into `CF_PLACEHOLDER_CREATE_INFO`.

use super::identity;
use crate::virtual_library::{Entry, EntryKind};
use chrono::{DateTime, Utc};

/// Seconds between 1601-01-01 (the FILETIME epoch) and 1970-01-01.
const UNIX_EPOCH_AS_FILETIME_SECONDS: i64 = 11_644_473_600;
/// FILETIME counts 100 ns ticks.
const TICKS_PER_SECOND: i64 = 10_000_000;

/// One placeholder to create in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlaceholderSpec {
    pub name: String,
    /// The file identity the platform stores with the placeholder.
    pub identity: Vec<u8>,
    pub is_directory: bool,
    /// The size of the original; 0 for directories.
    pub size: u64,
    /// FILETIME ticks; 0 leaves the time to the platform when Immich reported none.
    pub created: i64,
    pub modified: i64,
}

/// The placeholders of one directory listing, plus what could not be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlaceholderPlan {
    pub specs: Vec<PlaceholderSpec>,
    /// Names left out because they cannot be represented correctly.
    pub skipped: Vec<String>,
}

/// FILETIME ticks of a moment, or 0 (the platform's "unspecified") for none or one before 1601.
pub(super) fn filetime(moment: Option<DateTime<Utc>>) -> i64 {
    let Some(moment) = moment else { return 0 };
    let seconds = moment.timestamp() + UNIX_EPOCH_AS_FILETIME_SECONDS;
    if seconds < 0 {
        return 0;
    }
    seconds
        .saturating_mul(TICKS_PER_SECOND)
        .saturating_add(i64::from(moment.timestamp_subsec_nanos() / 100))
}

/// The placeholders for the `entries` of the directory at `parent`.
///
/// A file whose size Immich did not report is skipped, never shown as an empty file: a zero-byte
/// placeholder would hydrate to nothing. So is an entry whose path cannot be stored as an identity.
pub(super) fn plan(parent: &str, entries: &[Entry]) -> PlaceholderPlan {
    let mut specs = Vec::with_capacity(entries.len());
    let mut skipped = Vec::new();
    for entry in entries {
        let is_directory = entry.kind == EntryKind::Directory;
        let size = match (is_directory, entry.size) {
            (true, _) => 0,
            (false, Some(size)) => size,
            (false, None) => {
                skipped.push(entry.name.clone());
                continue;
            }
        };
        let Some(identity) = identity::encode(&identity::child_path(parent, &entry.name)) else {
            skipped.push(entry.name.clone());
            continue;
        };
        // A missing time borrows the other one, so Explorer shows a date whenever Immich gave any.
        let created = entry.created.or(entry.modified);
        let modified = entry.modified.or(entry.created);
        specs.push(PlaceholderSpec {
            name: entry.name.clone(),
            identity,
            is_directory,
            size,
            created: filetime(created),
            modified: filetime(modified),
        });
    }
    PlaceholderPlan { specs, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::RemoteAssetId;
    use chrono::TimeZone;

    fn file(name: &str, size: Option<u64>) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::File,
            remote_asset: Some(RemoteAssetId::new("asset-1")),
            size,
            created: None,
            modified: None,
        }
    }

    fn directory(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            kind: EntryKind::Directory,
            remote_asset: None,
            size: None,
            created: None,
            modified: None,
        }
    }

    #[test]
    fn filetime_counts_100ns_ticks_since_1601() {
        let march = Utc.with_ymd_and_hms(2024, 3, 15, 0, 0, 0).unwrap();
        // Worked by hand: 2024-03-15 is 1_710_460_800 s after 1970; plus 11_644_473_600 s to 1601.
        assert_eq!(filetime(Some(march)), 133_549_344_000_000_000);
        assert_eq!(
            filetime(Some(Utc.with_ymd_and_hms(1601, 1, 1, 0, 0, 1).unwrap())),
            10_000_000
        );
    }

    #[test]
    fn filetime_keeps_sub_second_precision() {
        let moment = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap()
            + chrono::Duration::milliseconds(1_500);
        assert_eq!(filetime(Some(moment)), 116_444_736_000_000_000 + 15_000_000);
    }

    #[test]
    fn a_missing_or_pre_1601_moment_is_left_to_the_platform() {
        assert_eq!(filetime(None), 0);
        assert_eq!(
            filetime(Some(Utc.with_ymd_and_hms(1500, 1, 1, 0, 0, 0).unwrap())),
            0
        );
    }

    #[test]
    fn a_file_becomes_a_placeholder_with_its_size_times_and_path_identity() {
        let created = Utc.with_ymd_and_hms(2024, 3, 15, 8, 0, 0).unwrap();
        let modified = Utc.with_ymd_and_hms(2024, 3, 16, 9, 0, 0).unwrap();
        let entry = Entry {
            created: Some(created),
            modified: Some(modified),
            ..file("IMG_0001.jpg", Some(3_276_800))
        };

        let plan = plan("All Photos/2024/03 March", &[entry]);

        assert_eq!(
            plan.specs,
            vec![PlaceholderSpec {
                name: "IMG_0001.jpg".to_string(),
                identity: b"All Photos/2024/03 March/IMG_0001.jpg".to_vec(),
                is_directory: false,
                size: 3_276_800,
                created: filetime(Some(created)),
                modified: filetime(Some(modified)),
            }]
        );
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn a_directory_becomes_a_zero_size_placeholder_named_by_its_path() {
        let plan = plan("", &[directory("All Photos"), directory("Albums")]);

        assert_eq!(plan.specs.len(), 2);
        assert!(plan
            .specs
            .iter()
            .all(|spec| spec.is_directory && spec.size == 0));
        assert_eq!(plan.specs[0].identity, b"All Photos".to_vec());
        assert_eq!(plan.specs[1].identity, b"Albums".to_vec());
    }

    #[test]
    fn a_missing_time_borrows_the_other_and_none_at_all_stays_unspecified() {
        let only_modified = Utc.with_ymd_and_hms(2024, 3, 16, 9, 0, 0).unwrap();
        let borrowed = Entry {
            modified: Some(only_modified),
            ..file("a.jpg", Some(1))
        };

        let plan = plan("", &[borrowed, file("b.jpg", Some(1))]);

        assert_eq!(plan.specs[0].created, filetime(Some(only_modified)));
        assert_eq!(plan.specs[0].modified, filetime(Some(only_modified)));
        assert_eq!((plan.specs[1].created, plan.specs[1].modified), (0, 0));
    }

    #[test]
    fn a_file_of_unknown_size_is_skipped_rather_than_shown_empty() {
        let plan = plan(
            "Albums/Trip",
            &[file("a.jpg", None), file("b.jpg", Some(5))],
        );

        assert_eq!(plan.skipped, vec!["a.jpg".to_string()]);
        assert_eq!(
            plan.specs
                .iter()
                .map(|spec| spec.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b.jpg"]
        );
    }

    #[test]
    fn an_entry_whose_path_cannot_be_an_identity_is_skipped() {
        let long = "x".repeat(identity::MAX_IDENTITY_BYTES);

        let plan = plan(&long, &[file("a.jpg", Some(1))]);

        assert!(plan.specs.is_empty());
        assert_eq!(plan.skipped, vec!["a.jpg".to_string()]);
    }
}
