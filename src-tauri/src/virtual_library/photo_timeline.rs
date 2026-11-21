//! `All Photos`: the Photo Timeline assets arranged into the date hierarchy.
//!
//! Each directory is produced from what it alone needs (#271). The year and month directories
//! come from Immich's timeline buckets ([`Calendar`]) without fetching an asset; a month directory
//! fetches the assets created in its [`creation_window`] and no other.

use super::path::{month_dir_name, year_dir_name, Dir, UNDATED};
use super::{live_photo, ntfs};
use super::{Entry, EntryKind};
use crate::immich::{LibraryAsset, LivePhotoVideo, RemoteAssetId, TimelineBucket};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use std::collections::{BTreeSet, HashMap};

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

/// Which months, and whether `Undated`, the Photo Timeline has assets in: all that the year and
/// month directories need, read from Immich's timeline buckets without fetching an asset.
pub(super) struct Calendar {
    months: BTreeSet<(i32, u32)>,
    has_undated: bool,
}

impl Calendar {
    /// A bucket names its month in its first seven characters (`2025-03-01T00:00:00.000Z`, or
    /// `2025-03-01`); one that names no calendar month holds assets without a usable capture
    /// time. Empty buckets hold nothing.
    pub(super) fn from_buckets(buckets: &[TimelineBucket]) -> Self {
        let mut calendar = Self {
            months: BTreeSet::new(),
            has_undated: false,
        };
        for bucket in buckets.iter().filter(|bucket| bucket.count > 0) {
            match bucket_month(&bucket.time_bucket) {
                Some(month) => {
                    calendar.months.insert(month);
                }
                None => calendar.has_undated = true,
            }
        }
        calendar
    }

    /// The entries of `All Photos`: the years, then `Undated` when there is such an asset.
    pub(super) fn years(&self) -> Vec<Entry> {
        let years: BTreeSet<i32> = self.months.iter().map(|(year, _)| *year).collect();
        let mut entries: Vec<Entry> = years
            .into_iter()
            .map(|year| Entry::directory(year_dir_name(year)))
            .collect();
        if self.has_undated {
            entries.push(Entry::directory(UNDATED));
        }
        entries
    }

    /// The month directories of `year`, or `None` when no asset is filed under it.
    pub(super) fn months(&self, year: i32) -> Option<Vec<Entry>> {
        let entries: Vec<Entry> = self
            .months
            .iter()
            .filter(|(month_year, _)| *month_year == year)
            .map(|(_, month)| Entry::directory(month_dir_name(*month)))
            .collect();
        (!entries.is_empty()).then_some(entries)
    }
}

fn bucket_month(time_bucket: &str) -> Option<(i32, u32)> {
    let (year, month) = time_bucket.get(..7)?.split_once('-')?;
    let year: i32 = year.parse().ok().filter(|year| (1..=9999).contains(year))?;
    let month: u32 = month
        .parse()
        .ok()
        .filter(|month| (1..=12).contains(month))?;
    Some((year, month))
}

/// The widest gap between a capture's UTC instant and its local wall-clock time is UTC-12 to
/// UTC+14; a day covers it with room to spare.
const LOCAL_OFFSET_MARGIN: Duration = Duration::days(1);

/// The UTC range `[after, before]` in which every asset captured in the local month lies. It is
/// wider than the month by [`LOCAL_OFFSET_MARGIN`] on both sides, since Immich can only filter on
/// the UTC instant; [`assets_in`] then keeps the assets whose local month it is.
pub(super) fn creation_window(year: i32, month: u32) -> (DateTime<Utc>, DateTime<Utc>) {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    (
        month_start(year, month) - LOCAL_OFFSET_MARGIN,
        month_start(next_year, next_month) + LOCAL_OFFSET_MARGIN,
    )
}

fn month_start(year: i32, month: u32) -> DateTime<Utc> {
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map_or(DateTime::<Utc>::MIN_UTC, |time| time.and_utc())
}

/// Whether the asset has no usable local capture time.
pub(super) fn is_undated(asset: &LibraryAsset) -> bool {
    bucket_of(asset) == Bucket::Undated
}

/// The assets of `assets` filed in the file directory `dir` (a month or `Undated`); none for any
/// other.
pub(super) fn assets_in(assets: Vec<LibraryAsset>, dir: &Dir) -> Vec<LibraryAsset> {
    let bucket = match dir {
        Dir::Month(year, month) => Bucket::Month(*year, *month),
        Dir::Undated => Bucket::Undated,
        _ => return Vec::new(),
    };
    assets
        .into_iter()
        .filter(|asset| bucket_of(asset) == bucket)
        .collect()
}

/// The file entries for the `assets` of a month or `Undated` directory, or `None` when the
/// directory holds no asset and therefore does not exist.
pub(super) fn files(
    assets: &[LibraryAsset],
    videos: &HashMap<RemoteAssetId, LivePhotoVideo>,
) -> Option<Vec<Entry>> {
    let files = live_photo::files(assets, videos);
    (!files.is_empty()).then_some(files)
}

/// The file entry of the still image or plain asset `asset`.
pub(super) fn file_entry(asset: &LibraryAsset) -> Entry {
    let timestamp = |text: &Option<String>| {
        text.as_deref()
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.with_timezone(&Utc))
    };
    Entry {
        name: ntfs::sanitize(
            asset
                .original_file_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(asset.id.as_str()),
        ),
        kind: EntryKind::File,
        remote_asset: Some(asset.id.clone()),
        size: asset.file_size,
        created: timestamp(&asset.file_created_at),
        modified: timestamp(&asset.file_modified_at),
    }
}
