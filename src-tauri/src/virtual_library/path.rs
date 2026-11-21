//! The shape of the Virtual Library tree: which paths name which directory.

/// A directory of the tree that can be named without asking Immich.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Dir {
    Root,
    AllPhotos,
    Year(i32),
    Month(i32, u32),
    Undated,
    Albums,
    /// `Albums/<name>`: the folder name as written in the path; which Album it is is for the
    /// listing to say.
    Album(String),
}

impl Dir {
    /// Whether the directory holds files (asset entries) rather than only directories.
    fn holds_files(&self) -> bool {
        matches!(self, Self::Month(..) | Self::Undated | Self::Album(_))
    }
}

/// What a path names.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Target {
    Dir(Dir),
    /// A name inside a directory that holds files; whether the file exists is for the listing to say.
    File {
        dir: Dir,
        name: String,
    },
}

pub(super) const ALL_PHOTOS: &str = "All Photos";
pub(super) const ALBUMS: &str = "Albums";
pub(super) const UNDATED: &str = "Undated";

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `All Photos/<year>` directory name.
pub(super) fn year_dir_name(year: i32) -> String {
    format!("{year:04}")
}

/// `All Photos/<year>/<MM Month>` directory name, in English whatever the app language, so
/// that paths stay stable.
pub(super) fn month_dir_name(month: u32) -> String {
    format!("{month:02} {}", MONTH_NAMES[(month - 1) as usize])
}

pub(super) fn parse(path: &str) -> Option<Target> {
    let segments: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect();
    if let Some(dir) = parse_dir(&segments) {
        return Some(Target::Dir(dir));
    }
    let (name, parent) = segments.split_last()?;
    let dir = parse_dir(parent).filter(|dir| dir.holds_files())?;
    Some(Target::File {
        dir,
        name: (*name).to_string(),
    })
}

fn parse_dir(segments: &[&str]) -> Option<Dir> {
    match segments {
        [] => Some(Dir::Root),
        [first] if first.eq_ignore_ascii_case(ALBUMS) => Some(Dir::Albums),
        [first, name] if first.eq_ignore_ascii_case(ALBUMS) => {
            Some(Dir::Album((*name).to_string()))
        }
        [first, rest @ ..] if first.eq_ignore_ascii_case(ALL_PHOTOS) => match rest {
            [] => Some(Dir::AllPhotos),
            [undated] if undated.eq_ignore_ascii_case(UNDATED) => Some(Dir::Undated),
            [year] => parse_year(year).map(Dir::Year),
            [year, month] => Some(Dir::Month(parse_year(year)?, parse_month(month)?)),
            _ => None,
        },
        _ => None,
    }
}

fn parse_year(segment: &str) -> Option<i32> {
    (segment.len() == 4 && segment.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| segment.parse().ok())
        .flatten()
}

fn parse_month(segment: &str) -> Option<u32> {
    (1..=12).find(|month| segment.eq_ignore_ascii_case(&month_dir_name(*month)))
}
