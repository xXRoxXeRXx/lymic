//! `All Photos`: the Photo Timeline assets arranged into the date hierarchy.
//!
//! Each directory is produced from what it alone needs (#271). The year and month directories
//! come from Immich's timeline buckets ([`Calendar`]) without fetching an asset; a month directory
//! fetches the assets created in its [`creation_window`] and no other.

use super::live_photo;
use super::path::{month_dir_name, year_dir_name, Dir, UNDATED};
use super::Entry;
use crate::immich::{LibraryAsset, LivePhotoVideo, RemoteAssetId, TimelineBucket};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Where an asset is filed below `All Photos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Month(i32, u32),
    Undated,
}

/// The month Immich reports for an asset that has no capture time: it sends `localDateTime` as the
/// Unix epoch (`1970-01-01T00:00:00.000Z`, observed on a real server), which is January 1970.
/// Nothing real was captured then, so the whole month is read as "no capture time": the exact epoch
/// instant and the rest of that month are filed under `Undated` and no `1970` directory exists. The
/// timeline buckets are per month, so the calendar and the assets have to agree on the whole month.
const UNDATED_PLACEHOLDER_MONTH: (i32, u32) = (1970, 1);

/// The local capture month of `asset`. Immich's `localDateTime` already is the wall-clock time at
/// the place of capture, so its components are read as written and no time zone is applied.
/// An asset without a usable `localDateTime`, or with the epoch placeholder, is undated.
fn bucket_of(asset: &LibraryAsset) -> Bucket {
    asset
        .local_date_time
        .as_deref()
        .and_then(local_wall_clock)
        .map(|time| (time.year(), time.month()))
        .filter(|month| *month != UNDATED_PLACEHOLDER_MONTH)
        .map_or(Bucket::Undated, |(year, month)| Bucket::Month(year, month))
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
    /// The assets Immich reports per month.
    months: BTreeMap<(i32, u32), u64>,
    /// The assets Immich reports without a usable capture month.
    undated: u64,
}

impl Calendar {
    /// A bucket names its month in its first seven characters (`2025-03-01T00:00:00.000Z`, or
    /// `2025-03-01`); one that names no calendar month, or the epoch placeholder month
    /// ([`UNDATED_PLACEHOLDER_MONTH`]), holds assets without a usable capture time. Empty buckets
    /// hold nothing.
    pub(super) fn from_buckets(buckets: &[TimelineBucket]) -> Self {
        let mut calendar = Self {
            months: BTreeMap::new(),
            undated: 0,
        };
        for bucket in buckets.iter().filter(|bucket| bucket.count > 0) {
            match bucket_month(&bucket.time_bucket) {
                Some(month) if month != UNDATED_PLACEHOLDER_MONTH => {
                    *calendar.months.entry(month).or_default() += bucket.count;
                }
                _ => calendar.undated += bucket.count,
            }
        }
        calendar
    }

    /// The entries of `All Photos`: the years, then `Undated` when there is such an asset.
    pub(super) fn years(&self) -> Vec<Entry> {
        let years: BTreeSet<i32> = self.months.keys().map(|(year, _)| *year).collect();
        let mut entries: Vec<Entry> = years
            .into_iter()
            .map(|year| Entry::directory(year_dir_name(year)))
            .collect();
        if self.undated > 0 {
            entries.push(Entry::directory(UNDATED));
        }
        entries
    }

    /// How many assets Immich reports in the month, 0 when it reports none.
    pub(super) fn count_in(&self, year: i32, month: u32) -> u64 {
        self.months.get(&(year, month)).copied().unwrap_or(0)
    }

    /// How many assets Immich reports without a usable capture time.
    pub(super) fn undated_count(&self) -> u64 {
        self.undated
    }

    /// The months Immich reports in `year`, as month numbers.
    pub(super) fn months_of(&self, year: i32) -> Vec<u32> {
        self.months
            .keys()
            .filter(|(month_year, _)| *month_year == year)
            .map(|(_, month)| *month)
            .collect()
    }

    /// The month directories of `year`, or `None` when no asset is filed under it.
    pub(super) fn months(&self, year: i32) -> Option<Vec<Entry>> {
        let entries: Vec<Entry> = self
            .months
            .keys()
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

/// Whether `asset` is filed in the file directory `dir` (a month or `Undated`).
pub(super) fn is_filed_in(asset: &LibraryAsset, dir: &Dir) -> bool {
    match dir {
        Dir::Month(year, month) => bucket_of(asset) == Bucket::Month(*year, *month),
        Dir::Undated => bucket_of(asset) == Bucket::Undated,
        _ => false,
    }
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
