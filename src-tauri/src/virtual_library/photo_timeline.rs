//! `All Photos`: the Photo Timeline assets arranged into the date hierarchy.

use super::path::{month_dir_name, year_dir_name, Dir, UNDATED};
use super::{Entry, EntryKind};
use crate::immich::LibraryAsset;
use chrono::{DateTime, Datelike, NaiveDateTime, Utc};
use std::collections::BTreeSet;

/// Where an asset is filed below `All Photos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Month(i32, u32),
    Undated,
}

/// The local capture month of `asset`. Immich's `localDateTime` already is the wall-clock time at
/// the place of capture, so its components are read as written and no time zone is applied.
/// An asset without a usable `localDateTime` is undated.
fn bucket_of(asset: &LibraryAsset) -> Bucket {
    asset
        .local_date_time
        .as_deref()
        .and_then(local_wall_clock)
        .map_or(Bucket::Undated, |time| {
            Bucket::Month(time.year(), time.month())
        })
}

fn local_wall_clock(text: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(text)
        .map(|time| time.naive_local())
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f"))
        .ok()
        .filter(|time| (1..=9999).contains(&time.year()))
}

/// The entries of `dir` for the given Photo Timeline `assets`, or `None` when the directory is a
/// year, month or `Undated` that holds no asset and therefore does not exist.
pub(super) fn children(assets: &[LibraryAsset], dir: Dir) -> Option<Vec<Entry>> {
    let entries: Vec<Entry> = match dir {
        Dir::AllPhotos => {
            let years: BTreeSet<i32> = assets
                .iter()
                .filter_map(|asset| match bucket_of(asset) {
                    Bucket::Month(year, _) => Some(year),
                    Bucket::Undated => None,
                })
                .collect();
            let has_undated = assets
                .iter()
                .any(|asset| bucket_of(asset) == Bucket::Undated);
            let mut entries: Vec<Entry> = years
                .into_iter()
                .map(|year| Entry::directory(year_dir_name(year)))
                .collect();
            if has_undated {
                entries.push(Entry::directory(UNDATED));
            }
            return Some(entries);
        }
        Dir::Year(year) => {
            let months: BTreeSet<u32> = assets
                .iter()
                .filter_map(|asset| match bucket_of(asset) {
                    Bucket::Month(asset_year, month) if asset_year == year => Some(month),
                    _ => None,
                })
                .collect();
            months
                .into_iter()
                .map(|month| Entry::directory(month_dir_name(month)))
                .collect()
        }
        Dir::Month(year, month) => files_in(assets, Bucket::Month(year, month)),
        Dir::Undated => files_in(assets, Bucket::Undated),
        Dir::Root | Dir::Albums => return None,
    };
    (!entries.is_empty()).then_some(entries)
}

fn files_in(assets: &[LibraryAsset], bucket: Bucket) -> Vec<Entry> {
    let mut files: Vec<Entry> = assets
        .iter()
        .filter(|asset| bucket_of(asset) == bucket)
        .map(file_entry)
        .collect();
    files.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.remote_asset.clone()));
    files
}

fn file_entry(asset: &LibraryAsset) -> Entry {
    let timestamp = |text: &Option<String>| {
        text.as_deref()
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.with_timezone(&Utc))
    };
    Entry {
        name: asset
            .original_file_name
            .clone()
            .unwrap_or_else(|| asset.id.to_string()),
        kind: EntryKind::File,
        remote_asset: Some(asset.id.clone()),
        size: asset.file_size,
        created: timestamp(&asset.file_created_at),
        modified: timestamp(&asset.file_modified_at),
    }
}
