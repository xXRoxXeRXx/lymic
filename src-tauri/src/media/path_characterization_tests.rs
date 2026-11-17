use crate::app::background::deferred_folder_matches_registered;
use crate::db::{
    add_folder, discard_sync_data_in_folder, enqueue_sync_assets, has_sync_work_in_folder,
    remap_watched_folder_path, reset_failed_queue_items_for_folder, QueueAsset, WatchedFolder,
};
use crate::media::admission::{decide, watched_folder_for_path, Admission};
use crate::media::model::{Asset, UploadUnit};
use crate::media::paths::{path_is_within, paths_equal};
use crate::sync::ActiveWork;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

fn eval_paths_within(folder_path: &str, candidate_path: &str) -> bool {
    path_is_within(Path::new(candidate_path), Path::new(folder_path))
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

fn eval_active_work_within(folder_path: &str, candidate_path: &str) -> bool {
    let work = Arc::new(ActiveWork::new());
    let _lease = work.acquire(&[asset_unit(candidate_path)]);
    work.has_work_in(folder_path)
}

async fn eval_db_within(folder_path: &str, candidate_path: &str) -> bool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let asset = QueueAsset {
        path: candidate_path,
        size: 1,
        mtime: 1,
        target_album_id: None,
    };
    enqueue_sync_assets(&pool, &[asset], &[]).await.unwrap();

    let has_work = has_sync_work_in_folder(&pool, folder_path).await.unwrap();

    sqlx::query(
        "UPDATE sync_queue SET status = 'FAILED', failure_reason = 'err' WHERE local_path = ?",
    )
    .bind(candidate_path)
    .execute(&pool)
    .await
    .unwrap();
    let reset_count = reset_failed_queue_items_for_folder(&pool, folder_path)
        .await
        .unwrap();
    assert_eq!(
        has_work,
        reset_count == 1,
        "has_sync_work_in_folder and reset_failed_queue_items_for_folder disagreed on folder={folder_path:?}, candidate={candidate_path:?}"
    );

    discard_sync_data_in_folder(&pool, folder_path)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_queue")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        has_work,
        remaining == 0,
        "has_sync_work_in_folder and discard_sync_data_in_folder disagreed on folder={folder_path:?}, candidate={candidate_path:?}"
    );

    sqlx::query("DELETE FROM sync_queue")
        .execute(&pool)
        .await
        .unwrap();
    let folder_id = add_folder(&pool, folder_path, None).await.unwrap();
    let remap_asset = QueueAsset {
        path: candidate_path,
        size: 1,
        mtime: 1,
        target_album_id: None,
    };
    enqueue_sync_assets(&pool, &[remap_asset], &[])
        .await
        .unwrap();
    #[cfg(windows)]
    let new_folder = r"Z:\RemappedTarget";
    #[cfg(not(windows))]
    let new_folder = "/remapped/target";
    remap_watched_folder_path(&pool, folder_id, folder_path, new_folder)
        .await
        .unwrap();
    let remapped_path: String = sqlx::query_scalar("SELECT local_path FROM sync_queue")
        .fetch_one(&pool)
        .await
        .unwrap();
    let was_remapped = remapped_path.starts_with(new_folder);
    assert_eq!(
        has_work, was_remapped,
        "has_sync_work_in_folder and remap_watched_folder_path disagreed on folder={folder_path:?}, candidate={candidate_path:?}, remapped_path={remapped_path:?}"
    );
    if !has_work {
        assert_eq!(
            remapped_path, candidate_path,
            "remap_watched_folder_path mutated non-contained candidate for folder={folder_path:?}"
        );
    }

    let sql_cond = crate::db::sql_path_is_or_descendant_of("path", "?1");
    let sql_prefix = crate::db::sql_folder_prefix(folder_path);
    sqlx::query("CREATE TEMP TABLE IF NOT EXISTS probe_paths (path TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM probe_paths")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO probe_paths (path) VALUES (?1)")
        .bind(candidate_path)
        .execute(&pool)
        .await
        .unwrap();
    let sql_matched: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM probe_paths WHERE {sql_cond})"
    ))
    .bind(&sql_prefix)
    .fetch_one(&pool)
    .await
    .unwrap();
    if has_work {
        assert!(
            sql_matched,
            "sql_path_is_or_descendant_of dropped a matching path: folder={folder_path:?}, candidate={candidate_path:?}"
        );
    }
    if candidate_path.is_ascii()
        && folder_path.is_ascii()
        && !candidate_path.contains("..")
        && !candidate_path.contains("/.")
        && !candidate_path.contains("\\.")
        && !has_work
    {
        assert!(
            !sql_matched,
            "sql_path_is_or_descendant_of failed to reject non-matching ASCII path in SQL: folder={folder_path:?}, candidate={candidate_path:?}"
        );
    }

    has_work
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
    expected_win: bool,
    #[cfg_attr(windows, allow(dead_code))]
    expected_nonwin: bool,
}

#[tokio::test]
async fn all_containment_and_sql_cleanup_owners_agree_on_characterization_inputs() {
    let cases = [
        // 1. Sibling folders sharing a name prefix
        ContainmentCase {
            label: "sibling_windows_prefix_digit",
            folder: r"C:\Photos",
            candidate: r"C:\Photos2\a.jpg",
            expected_win: false,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "sibling_windows_prefix_hyphen",
            folder: r"C:\Photos",
            candidate: r"C:\Photos-Backup\a.jpg",
            expected_win: false,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "sibling_posix_prefix",
            folder: "/media/photos",
            candidate: "/media/photos-backup/a.jpg",
            expected_win: false,
            expected_nonwin: false,
        },
        // 2. Trailing separators
        ContainmentCase {
            label: "trailing_backslash_on_folder",
            folder: r"C:\Photos\",
            candidate: r"C:\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false, // `\` is a literal filename character on POSIX
        },
        ContainmentCase {
            label: "trailing_slash_on_folder",
            folder: r"C:\Photos/",
            candidate: r"C:\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "trailing_backslash_folder_vs_bare_candidate",
            folder: r"C:\Photos\",
            candidate: r"C:\Photos",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "posix_trailing_slash",
            folder: "/media/photos/",
            candidate: "/media/photos/a.jpg",
            expected_win: true,
            expected_nonwin: true,
        },
        ContainmentCase {
            label: "windows_drive_root",
            folder: r"C:\",
            candidate: r"C:\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "posix_root",
            folder: "/",
            candidate: "/a.jpg",
            expected_win: true,
            expected_nonwin: true,
        },
        // 3. Backslash vs forward slash
        ContainmentCase {
            label: "slash_mismatch_root_only",
            folder: r"C:\Photos",
            candidate: "C:/Photos/a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "slash_mismatch_nested_folder",
            folder: r"C:\Photos\Sub",
            candidate: "C:/Photos/Sub/a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "slash_mismatch_folder_fwd_candidate_back",
            folder: "C:/Photos/Sub",
            candidate: r"C:\Photos\Sub\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        // 4. Mixed case
        ContainmentCase {
            label: "mixed_case_drive_letter_only",
            folder: r"C:\Photos",
            candidate: r"c:\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "mixed_case_ascii_folder_segment",
            folder: r"C:\Photos",
            candidate: r"C:\photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "mixed_case_unicode_folder_segment",
            folder: r"C:\FÖTOS",
            candidate: r"C:\fötos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "mixed_case_posix_path",
            folder: "/Media/Photos",
            candidate: "/media/photos/a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        // 5. Windows verbatim prefixes & drive letters
        ContainmentCase {
            label: "verbatim_disk_folder_vs_normal_candidate",
            folder: r"\\?\C:\Photos",
            candidate: r"C:\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "normal_folder_vs_verbatim_disk_candidate",
            folder: r"C:\Photos",
            candidate: r"\\?\C:\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "verbatim_unc_folder_vs_normal_unc_candidate",
            folder: r"\\?\UNC\server\share\Photos",
            candidate: r"\\server\share\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "normal_unc_folder_vs_verbatim_unc_candidate",
            folder: r"\\server\share\Photos",
            candidate: r"\\?\UNC\server\share\Photos\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "drive_relative_vs_drive_root",
            folder: "C:",
            candidate: r"C:\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        // 6. Dot segments
        ContainmentCase {
            label: "dot_and_parent_staying_inside",
            folder: r"C:\Photos",
            candidate: r"C:\Photos\.\sub\..\a.jpg",
            expected_win: true,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "parent_escaping_subfolder",
            folder: r"C:\Photos\Sub",
            candidate: r"C:\Photos\Sub\..\Outside\a.jpg",
            expected_win: false,
            expected_nonwin: false,
        },
        ContainmentCase {
            label: "posix_parent_escaping_subfolder",
            folder: "/media/photos/sub",
            candidate: "/media/photos/sub/../outside/a.jpg",
            expected_win: false,
            expected_nonwin: false,
        },
    ];

    for case in &cases {
        #[cfg(windows)]
        let expected = case.expected_win;
        #[cfg(not(windows))]
        let expected = case.expected_nonwin;

        let got_paths = eval_paths_within(case.folder, case.candidate);
        let got_admission = eval_admission_within(case.folder, case.candidate);
        let got_active_work = eval_active_work_within(case.folder, case.candidate);
        let got_db = eval_db_within(case.folder, case.candidate).await;

        assert_eq!(
            got_paths, expected,
            "paths.rs mismatch for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );
        assert_eq!(
            got_admission, got_paths,
            "admission.rs disagreed with paths.rs for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );
        assert_eq!(
            got_active_work, got_paths,
            "active_work disagreed with paths.rs for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );
        assert_eq!(
            got_db, got_paths,
            "SQL/DB cleanup disagreed with paths.rs for {}: folder={:?}, candidate={:?}",
            case.label, case.folder, case.candidate
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct EqualityCase {
    label: &'static str,
    left: &'static str,
    right: &'static str,
    #[cfg_attr(not(windows), allow(dead_code))]
    expected_win: bool,
    #[cfg_attr(windows, allow(dead_code))]
    expected_nonwin: bool,
}

#[test]
fn all_path_equality_owners_agree_on_characterization_inputs() {
    let cases = [
        EqualityCase {
            label: "eq_trailing_backslash",
            left: r"C:\Photos\",
            right: r"C:\Photos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_posix_trailing_slash",
            left: "/media/photos/",
            right: "/media/photos",
            expected_win: true,
            expected_nonwin: true,
        },
        EqualityCase {
            label: "eq_slash_difference",
            left: r"C:\Photos\Sub",
            right: "C:/Photos/Sub",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_mixed_case_drive_and_segment",
            left: r"C:\Photos",
            right: r"c:\photos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_mixed_case_unicode",
            left: r"C:\FÖTOS",
            right: r"C:\fötos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_mixed_case_posix",
            left: "/Media/Photos",
            right: "/media/photos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_verbatim_disk_prefix",
            left: r"\\?\C:\Photos",
            right: r"C:\Photos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_verbatim_unc_prefix",
            left: r"\\?\UNC\server\share\Photos",
            right: r"\\server\share\Photos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_dot_parent_segments",
            left: r"C:\Photos\.\Sub\..",
            right: r"C:\Photos",
            expected_win: true,
            expected_nonwin: false,
        },
        EqualityCase {
            label: "eq_sibling_prefix_never_equal",
            left: r"C:\Photos",
            right: r"C:\Photos2",
            expected_win: false,
            expected_nonwin: false,
        },
    ];

    for case in &cases {
        #[cfg(windows)]
        let expected = case.expected_win;
        #[cfg(not(windows))]
        let expected = case.expected_nonwin;

        let got_paths = paths_equal(Path::new(case.left), Path::new(case.right));
        let got_admission = eval_admission_within(case.left, case.right)
            && eval_admission_within(case.right, case.left);
        let got_deferred = eval_deferred_equal(case.left, case.right);

        assert_eq!(
            got_paths, expected,
            "paths.rs eq mismatch for {}",
            case.label
        );
        assert_eq!(
            got_admission, got_paths,
            "admission eq disagreed with paths.rs for {}",
            case.label
        );
        assert_eq!(
            got_deferred, got_paths,
            "deferred eq disagreed with paths.rs for {}",
            case.label
        );
    }
}
