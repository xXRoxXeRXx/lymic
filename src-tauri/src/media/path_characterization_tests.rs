use crate::app::background::deferred_folder_matches_registered;
use crate::db::{sql_path_is_or_descendant_of, WatchedFolder};
use crate::media::admission::{decide, watched_folder_for_path, Admission};
use crate::media::model::{Asset, UploadUnit};
use crate::media::paths::{normalize_path_lexical, path_is_within_lexical};
use sqlx::sqlite::SqlitePoolOptions;
use std::path::{Path, PathBuf};

fn folder(path: &str, patterns: &[&str]) -> WatchedFolder {
    WatchedFolder {
        id: 1,
        path: path.to_owned(),
        recursive: true,
        target_album_id: None,
        exclusion_patterns: patterns.iter().map(|p| (*p).to_owned()).collect(),
    }
}

fn asset_unit(path: &str) -> UploadUnit {
    UploadUnit {
        image: Asset {
            path: path.to_owned(),
            size: 1,
            mtime: 1,
            target_album_id: None,
            remote_id: None,
        },
        live_photo_video: None,
    }
}

fn eval_paths_lexical_within(folder_path: &str, candidate_path: &str) -> bool {
    let norm_folder = normalize_path_lexical(Path::new(folder_path));
    let norm_candidate = normalize_path_lexical(Path::new(candidate_path));
    path_is_within_lexical(&norm_candidate, &norm_folder)
}

fn eval_paths_lexical_equal(left: &str, right: &str) -> bool {
    let norm_left = normalize_path_lexical(Path::new(left));
    let norm_right = normalize_path_lexical(Path::new(right));
    path_is_within_lexical(&norm_left, &norm_right)
        && path_is_within_lexical(&norm_right, &norm_left)
}

fn eval_admission_within(folder_path: &str, candidate_path: &str) -> bool {
    let f = folder(folder_path, &["**"]);
    let by_folder_lookup =
        watched_folder_for_path(std::slice::from_ref(&f), Path::new(candidate_path)).is_some();
    let by_decide = decide(&asset_unit(candidate_path), &[f]) == Admission::Excluded;
    assert_eq!(
        by_folder_lookup, by_decide,
        "watched_folder_for_path and decide(exclusion=['**']) disagreed on folder={folder_path:?}, candidate={candidate_path:?}"
    );
    by_folder_lookup
}

async fn eval_sql_within(pool: &sqlx::SqlitePool, folder_path: &str, candidate_path: &str) -> bool {
    // Matches the binding normalization in db/sync_state.rs and db/queue.rs.
    let trimmed_folder = folder_path.trim_end_matches(['/', '\\']);
    let cond = sql_path_is_or_descendant_of("?1", "?2");
    let query = format!("SELECT CASE WHEN {cond} THEN 1 ELSE 0 END");
    let matched: i64 = sqlx::query_scalar(&query)
        .bind(candidate_path)
        .bind(trimmed_folder)
        .fetch_one(pool)
        .await
        .unwrap();
    matched == 1
}

fn eval_deferred_equal(folder_path: &str, registered_path: &str) -> bool {
    deferred_folder_matches_registered(folder_path, &[PathBuf::from(registered_path)])
}

#[derive(Debug, Clone, Copy)]
struct ContainmentCase {
    label: &'static str,
    folder: &'static str,
    candidate: &'static str,
    #[cfg_attr(not(windows), allow(dead_code))]
    expected_paths_win: bool,
    #[cfg_attr(windows, allow(dead_code))]
    expected_paths_nonwin: bool,
    expected_admission: bool,
    expected_sql: bool,
}

#[tokio::test]
async fn characterize_all_path_containment_implementations() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();

    let cases = [
        // 1. Sibling folders sharing a name prefix
        ContainmentCase {
            label: "sibling_windows_prefix_digit",
            folder: r"C:\Photos",
            candidate: r"C:\Photos2\a.jpg",
            expected_paths_win: false,
            expected_paths_nonwin: false,
            expected_admission: false,
            expected_sql: false,
        },
        ContainmentCase {
            label: "sibling_windows_prefix_hyphen",
            folder: r"C:\Photos",
            candidate: r"C:\Photos-Backup\a.jpg",
            expected_paths_win: false,
            expected_paths_nonwin: false,
            expected_admission: false,
            expected_sql: false,
        },
        ContainmentCase {
            label: "sibling_posix_prefix",
            folder: "/media/photos",
            candidate: "/media/photos-backup/a.jpg",
            expected_paths_win: false,
            expected_paths_nonwin: false,
            expected_admission: false,
            expected_sql: false,
        },
        // 2. Trailing separators
        ContainmentCase {
            label: "trailing_backslash_on_folder",
            folder: r"C:\Photos\",
            candidate: r"C:\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false, // `\` is filename char on POSIX
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "trailing_slash_on_folder",
            folder: r"C:\Photos/",
            candidate: r"C:\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "trailing_backslash_folder_vs_bare_candidate",
            folder: r"C:\Photos\",
            candidate: r"C:\Photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "posix_trailing_slash",
            folder: "/media/photos/",
            candidate: "/media/photos/a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: true,
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "windows_drive_root",
            folder: r"C:\",
            candidate: r"C:\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "posix_root",
            folder: "/",
            candidate: "/a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: true,
            expected_admission: true,
            expected_sql: true,
        },
        // 3. Backslash vs forward slash
        ContainmentCase {
            label: "slash_mismatch_root_only",
            folder: r"C:\Photos",
            candidate: "C:/Photos/a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: false, // DISAGREEMENT: SQL compares `substr` literally for `\` vs `/`
        },
        ContainmentCase {
            label: "slash_mismatch_nested_folder",
            folder: r"C:\Photos\Sub",
            candidate: "C:/Photos/Sub/a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: false, // DISAGREEMENT: SQL compares `substr` literally for non-drive chars
        },
        ContainmentCase {
            label: "slash_mismatch_folder_fwd_candidate_back",
            folder: "C:/Photos/Sub",
            candidate: r"C:\Photos\Sub\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: false, // DISAGREEMENT
        },
        // 4. Mixed case
        ContainmentCase {
            label: "mixed_case_drive_letter_only",
            folder: r"C:\Photos",
            candidate: r"c:\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "mixed_case_ascii_folder_segment",
            folder: r"C:\Photos",
            candidate: r"C:\photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: false, // DISAGREEMENT: SQL is case-sensitive after drive letter
        },
        ContainmentCase {
            label: "mixed_case_unicode_folder_segment",
            folder: r"C:\FÖTOS",
            candidate: r"C:\fötos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false, // DISAGREEMENT: admission uses to_ascii_lowercase(), paths uses to_lowercase()
            expected_sql: false,       // DISAGREEMENT: SQL is case-sensitive after drive letter
        },
        ContainmentCase {
            label: "mixed_case_posix_path",
            folder: "/Media/Photos",
            candidate: "/media/photos/a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false, // paths.rs is case-sensitive on non-Windows
            expected_admission: true,     // DISAGREEMENT on non-Windows: admission always ASCII-lowercases!
            expected_sql: false,          // DISAGREEMENT: SQL is case-sensitive on POSIX paths
        },
        // 5. Windows verbatim prefixes & drive letters
        ContainmentCase {
            label: "verbatim_disk_folder_vs_normal_candidate",
            folder: r"\\?\C:\Photos",
            candidate: r"C:\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false, // DISAGREEMENT: admission does not strip \\?\
            expected_sql: false,       // DISAGREEMENT: SQL does not strip \\?\
        },
        ContainmentCase {
            label: "normal_folder_vs_verbatim_disk_candidate",
            folder: r"C:\Photos",
            candidate: r"\\?\C:\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false, // DISAGREEMENT
            expected_sql: false,       // DISAGREEMENT
        },
        ContainmentCase {
            label: "verbatim_unc_folder_vs_normal_unc_candidate",
            folder: r"\\?\UNC\server\share\Photos",
            candidate: r"\\server\share\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false, // DISAGREEMENT: admission does not strip \\?\UNC\
            expected_sql: false,       // DISAGREEMENT
        },
        ContainmentCase {
            label: "normal_unc_folder_vs_verbatim_unc_candidate",
            folder: r"\\server\share\Photos",
            candidate: r"\\?\UNC\server\share\Photos\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false, // DISAGREEMENT
            expected_sql: false,       // DISAGREEMENT
        },
        ContainmentCase {
            label: "drive_relative_vs_drive_root",
            folder: "C:",
            candidate: r"C:\a.jpg",
            expected_paths_win: true, // Path("C:") has [Prefix(Disk('C'))] which is prefix of [Prefix, RootDir, Normal]
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: true,
        },
        // 6. Dot segments
        ContainmentCase {
            label: "dot_and_parent_staying_inside",
            folder: r"C:\Photos",
            candidate: r"C:\Photos\.\sub\..\a.jpg",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_sql: true,
        },
        ContainmentCase {
            label: "parent_escaping_subfolder",
            folder: r"C:\Photos\Sub",
            candidate: r"C:\Photos\Sub\..\Outside\a.jpg",
            expected_paths_win: false,
            expected_paths_nonwin: false,
            expected_admission: true, // DISAGREEMENT: admission does not resolve `..`!
            expected_sql: true,       // DISAGREEMENT: SQL does not resolve `..`!
        },
        ContainmentCase {
            label: "posix_parent_escaping_subfolder",
            folder: "/media/photos/sub",
            candidate: "/media/photos/sub/../outside/a.jpg",
            expected_paths_win: false,
            expected_paths_nonwin: false,
            expected_admission: true, // DISAGREEMENT
            expected_sql: true,       // DISAGREEMENT
        },
    ];

    let mut disagreements = Vec::new();

    for case in &cases {
        let got_paths = eval_paths_lexical_within(case.folder, case.candidate);
        let got_admission = eval_admission_within(case.folder, case.candidate);
        let got_sql = eval_sql_within(&pool, case.folder, case.candidate).await;

        #[cfg(windows)]
        let expected_paths = case.expected_paths_win;
        #[cfg(not(windows))]
        let expected_paths = case.expected_paths_nonwin;

        // Observe actual behavior against expectations so any mistake in expectation is caught immediately
        assert_eq!(
            got_paths, expected_paths,
            "paths.rs mismatch for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );
        assert_eq!(
            got_admission, case.expected_admission,
            "admission.rs mismatch for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );
        assert_eq!(
            got_sql, case.expected_sql,
            "sql mismatch for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );

        if !(got_paths == got_admission && got_admission == got_sql) {
            disagreements.push(format!(
                "{}: paths={got_paths}, admission={got_admission}, sql={got_sql} (folder={:?}, candidate={:?})",
                case.label, case.folder, case.candidate
            ));
        }
    }

    assert!(
        !disagreements.is_empty(),
        "expected characterization to expose disagreements across implementations"
    );
    eprintln!("Containment disagreements ({}):", disagreements.len());
    for d in &disagreements {
        eprintln!("  - {d}");
    }
}

#[derive(Debug, Clone, Copy)]
struct EqualityCase {
    label: &'static str,
    left: &'static str,
    right: &'static str,
    #[cfg_attr(not(windows), allow(dead_code))]
    expected_paths_win: bool,
    #[cfg_attr(windows, allow(dead_code))]
    expected_paths_nonwin: bool,
    expected_admission: bool,
    #[cfg_attr(not(windows), allow(dead_code))]
    expected_deferred_win: bool,
    #[cfg_attr(windows, allow(dead_code))]
    expected_deferred_nonwin: bool,
}

#[test]
fn characterize_path_equality_implementations() {
    let cases = [
        EqualityCase {
            label: "eq_trailing_backslash",
            left: r"C:\Photos\",
            right: r"C:\Photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_deferred_win: true,
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_posix_trailing_slash",
            left: "/media/photos/",
            right: "/media/photos",
            expected_paths_win: true,
            expected_paths_nonwin: true,
            expected_admission: true,
            expected_deferred_win: true,
            expected_deferred_nonwin: true,
        },
        EqualityCase {
            label: "eq_slash_difference",
            left: r"C:\Photos\Sub",
            right: "C:/Photos/Sub",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_deferred_win: true,
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_mixed_case_drive_and_segment",
            left: r"C:\Photos",
            right: r"c:\photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true,
            expected_deferred_win: false, // DISAGREEMENT: PathBuf::eq is case-sensitive!
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_mixed_case_unicode",
            left: r"C:\FÖTOS",
            right: r"C:\fötos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false,    // DISAGREEMENT: admission is ASCII-only lowercase
            expected_deferred_win: false, // DISAGREEMENT: PathBuf::eq is case-sensitive
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_mixed_case_posix",
            left: "/Media/Photos",
            right: "/media/photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: true, // DISAGREEMENT on non-Windows
            expected_deferred_win: false,
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_verbatim_disk_prefix",
            left: r"\\?\C:\Photos",
            right: r"C:\Photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false,    // DISAGREEMENT
            expected_deferred_win: false, // DISAGREEMENT
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_verbatim_unc_prefix",
            left: r"\\?\UNC\server\share\Photos",
            right: r"\\server\share\Photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false,    // DISAGREEMENT
            expected_deferred_win: false, // DISAGREEMENT
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_dot_parent_segments",
            left: r"C:\Photos\.\Sub\..",
            right: r"C:\Photos",
            expected_paths_win: true,
            expected_paths_nonwin: false,
            expected_admission: false,    // DISAGREEMENT
            expected_deferred_win: false, // DISAGREEMENT: PathBuf::eq does not collapse `..`
            expected_deferred_nonwin: false,
        },
        EqualityCase {
            label: "eq_sibling_prefix_never_equal",
            left: r"C:\Photos",
            right: r"C:\Photos2",
            expected_paths_win: false,
            expected_paths_nonwin: false,
            expected_admission: false,
            expected_deferred_win: false,
            expected_deferred_nonwin: false,
        },
    ];

    let mut disagreements = Vec::new();
    for case in &cases {
        let got_paths = eval_paths_lexical_equal(case.left, case.right);
        let got_admission =
            eval_admission_within(case.left, case.right) && eval_admission_within(case.right, case.left);
        let got_deferred = eval_deferred_equal(case.left, case.right);

        #[cfg(windows)]
        let (expected_paths, expected_deferred) =
            (case.expected_paths_win, case.expected_deferred_win);
        #[cfg(not(windows))]
        let (expected_paths, expected_deferred) =
            (case.expected_paths_nonwin, case.expected_deferred_nonwin);

        assert_eq!(got_paths, expected_paths, "paths.rs eq mismatch for {}", case.label);
        assert_eq!(
            got_admission, case.expected_admission,
            "admission eq mismatch for {}",
            case.label
        );
        assert_eq!(
            got_deferred, expected_deferred,
            "deferred eq mismatch for {}",
            case.label
        );

        if !(got_paths == got_admission && got_admission == got_deferred) {
            disagreements.push(format!(
                "{}: paths={got_paths}, admission={got_admission}, deferred={got_deferred} ({:?} vs {:?})",
                case.label, case.left, case.right
            ));
        }
    }

    assert!(!disagreements.is_empty());
    eprintln!("Equality disagreements ({}):", disagreements.len());
    for d in &disagreements {
        eprintln!("  - {d}");
    }
}
