#[cfg(test)]
mod database_tests {
    use super::super::init::quarantine_unusable_db;
    use crate::db::*;
    use sqlx::{
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
        Row, SqlitePool,
    };
    use std::time::Duration;

    async fn test_pool() -> SqlitePool {
        let database_name = format!("lymic_test_{}", uuid::Uuid::new_v4());
        let uri = format!("sqlite:file:{}?mode=memory&cache=private", database_name);
        let options: SqliteConnectOptions = uri.parse().unwrap();
        let options = options.foreign_keys(true);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn initial_migration_creates_schema() {
        let pool = test_pool().await;

        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        add_folder(&pool, "C:/photos", None).await.unwrap();
        update_sync_state(&pool, "photo.jpg", "hash", 42, 123, "SYNCED", None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn running_queue_is_recovered_as_paused() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("UPDATE sync_jobs SET status = 'RUNNING' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();

        let snapshot = recover_sync_job(&pool).await.unwrap();
        assert_eq!(snapshot.status, "PAUSED");
    }

    #[tokio::test]
    async fn queue_deduplicates_paths_and_preserves_order() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let assets = [
            QueueAsset {
                path: "b.jpg",
                size: 2,
                mtime: 2,
                target_album_id: None,
            },
            QueueAsset {
                path: "a.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
        ];
        enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        let changed = [QueueAsset {
            path: "b.jpg",
            size: 3,
            mtime: 3,
            target_album_id: None,
        }];
        let snapshot = enqueue_sync_assets(&pool, &changed, &[]).await.unwrap();

        assert_eq!(snapshot.total, 2);
        let queued = next_sync_queue_block(&pool, 10).await.unwrap();
        assert_eq!(
            queued
                .iter()
                .map(|asset| asset.local_path.as_str())
                .collect::<Vec<_>>(),
            ["b.jpg", "a.jpg"]
        );
        assert_eq!(queued[0].size, 3);
    }

    #[tokio::test]
    async fn queue_pages_after_the_first_window_is_finalized() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let paths: Vec<_> = (0..101).map(|index| format!("{index:03}.jpg")).collect();
        let assets: Vec<_> = paths
            .iter()
            .map(|path| QueueAsset {
                path,
                size: 1,
                mtime: 1,
                target_album_id: None,
            })
            .collect();
        enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();

        let first = next_sync_queue_block(&pool, 50).await.unwrap();
        assert_eq!(first.len(), 50);
        assert_eq!(first[0].local_path, "000.jpg");
        assert_eq!(first[49].local_path, "049.jpg");
        let first_results: Vec<_> = first
            .iter()
            .map(|asset| (asset.local_path.clone(), "SYNCED".to_string()))
            .collect();
        finalize_queued_block(&pool, &first_results).await.unwrap();

        let second = next_sync_queue_block(&pool, 50).await.unwrap();
        assert_eq!(second.len(), 50);
        assert_eq!(second[0].local_path, "050.jpg");
        assert_eq!(second[49].local_path, "099.jpg");
    }

    #[tokio::test]
    async fn paused_queue_accepts_new_items_without_resuming() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        set_sync_status(&pool, "PAUSED").await.unwrap();
        let assets = [QueueAsset {
            path: "later.jpg",
            size: 7,
            mtime: 9,
            target_album_id: None,
        }];

        let snapshot = enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        assert_eq!(snapshot.status, "PAUSED");
        assert_eq!(snapshot.total, 1);
        assert_eq!(
            next_sync_queue_block(&pool, 1).await.unwrap()[0].local_path,
            "later.jpg"
        );
    }

    #[tokio::test]
    async fn paused_queue_preserves_terminal_records_when_new_items_are_enqueued() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let terminal_assets = [
            QueueAsset {
                path: "completed.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
            QueueAsset {
                path: "failed.jpg",
                size: 2,
                mtime: 2,
                target_album_id: None,
            },
        ];
        crate::db::enqueue_sync_assets(&pool, &terminal_assets, &[])
            .await
            .unwrap();
        finalize_queued_block(
            &pool,
            &[
                ("completed.jpg".to_string(), "SYNCED".to_string()),
                ("failed.jpg".to_string(), "FAILED".to_string()),
            ],
        )
        .await
        .unwrap();
        set_sync_status(&pool, "PAUSED").await.unwrap();

        let new_asset = [QueueAsset {
            path: "new.jpg",
            size: 3,
            mtime: 3,
            target_album_id: None,
        }];
        let snapshot = crate::db::enqueue_sync_assets(&pool, &new_asset, &[])
            .await
            .unwrap();

        assert_eq!(snapshot.status, "PAUSED");
        assert_eq!(snapshot.total, 3);
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT local_path, status FROM sync_queue WHERE job_id = 1 ORDER BY sequence",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                ("completed.jpg".to_string(), "SYNCED".to_string()),
                ("failed.jpg".to_string(), "FAILED".to_string()),
                ("new.jpg".to_string(), "PENDING".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn completion_waits_for_all_queue_items() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let assets = [
            QueueAsset {
                path: "one.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
            QueueAsset {
                path: "two.jpg",
                size: 2,
                mtime: 2,
                target_album_id: None,
            },
        ];
        enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        finalize_queued_block(&pool, &[("one.jpg".to_string(), "SYNCED".to_string())])
            .await
            .unwrap();
        assert!(complete_sync_job_if_finished(&pool)
            .await
            .unwrap()
            .is_none());
        finalize_queued_block(&pool, &[("two.jpg".to_string(), "FAILED".to_string())])
            .await
            .unwrap();
        let snapshot = complete_sync_job_if_finished(&pool).await.unwrap().unwrap();
        assert_eq!(snapshot.status, "IDLE");
        assert_eq!(snapshot.succeeded, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.total_bytes, 3);
        assert_eq!(snapshot.completed_bytes, 3);
    }

    #[tokio::test]
    async fn pending_queue_check_distinguishes_open_and_terminal_items() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        assert!(!has_pending_sync_queue_items(&pool).await.unwrap());

        let assets = [
            QueueAsset {
                path: "one.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
            QueueAsset {
                path: "two.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
        ];
        enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        assert!(has_pending_sync_queue_items(&pool).await.unwrap());

        finalize_queued_block(&pool, &[("one.jpg".to_string(), "SYNCED".to_string())])
            .await
            .unwrap();
        assert!(has_pending_sync_queue_items(&pool).await.unwrap());
        finalize_queued_block(&pool, &[("two.jpg".to_string(), "SYNCED".to_string())])
            .await
            .unwrap();
        assert!(!has_pending_sync_queue_items(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn queue_tracks_total_and_completed_bytes_accurately() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let assets = [
            QueueAsset {
                path: "small.jpg",
                size: 1_000,
                mtime: 1,
                target_album_id: None,
            },
            QueueAsset {
                path: "large.mov",
                size: 9_000,
                mtime: 2,
                target_album_id: None,
            },
        ];
        let snapshot = enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        assert_eq!(snapshot.total_bytes, 10_000);
        assert_eq!(snapshot.completed_bytes, 0);

        let snapshot =
            finalize_queued_block(&pool, &[("small.jpg".to_string(), "SYNCED".to_string())])
                .await
                .unwrap();
        assert_eq!(snapshot.total_bytes, 10_000);
        assert_eq!(snapshot.completed_bytes, 1_000);

        let snapshot =
            finalize_queued_block(&pool, &[("large.mov".to_string(), "FAILED".to_string())])
                .await
                .unwrap();
        assert_eq!(snapshot.total_bytes, 10_000);
        assert_eq!(snapshot.completed_bytes, 10_000);
    }

    #[tokio::test]
    async fn upload_parallelism_defaults_persists_and_validates_range() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_eq!(
            get_upload_parallelism(&pool).await.unwrap(),
            DEFAULT_UPLOAD_PARALLELISM
        );

        set_upload_parallelism(&pool, 6).await.unwrap();
        assert_eq!(get_upload_parallelism(&pool).await.unwrap(), 6);
        assert_eq!(validate_upload_parallelism(1), Some(1));
        assert_eq!(validate_upload_parallelism(8), Some(8));
        assert_eq!(validate_upload_parallelism(0), None);
        assert_eq!(validate_upload_parallelism(9), None);
        assert!(set_upload_parallelism(&pool, 0).await.is_err());
        assert!(set_upload_parallelism(&pool, 9).await.is_err());
    }

    #[tokio::test]
    async fn invalid_stored_upload_parallelism_uses_default() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query(
            "UPDATE app_settings SET value = 'not-a-number' WHERE key = 'upload_parallelism'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            get_upload_parallelism(&pool).await.unwrap(),
            DEFAULT_UPLOAD_PARALLELISM
        );

        sqlx::query("UPDATE app_settings SET value = '0' WHERE key = 'upload_parallelism'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            get_upload_parallelism(&pool).await.unwrap(),
            DEFAULT_UPLOAD_PARALLELISM
        );
    }

    #[tokio::test]
    async fn virtual_library_status_defaults_and_persists() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_eq!(
            get_virtual_library_status(&pool).await.unwrap(),
            VirtualLibraryStatus {
                enabled: false,
                path: DEFAULT_VIRTUAL_LIBRARY_PATH.to_string(),
                connection_status:
                    crate::virtual_library::sync_root::ConnectionStatus::Disconnected,
            }
        );

        set_virtual_library_enabled(&pool, true).await.unwrap();
        assert_eq!(
            get_virtual_library_status(&pool).await.unwrap(),
            VirtualLibraryStatus {
                enabled: true,
                path: DEFAULT_VIRTUAL_LIBRARY_PATH.to_string(),
                connection_status:
                    crate::virtual_library::sync_root::ConnectionStatus::Disconnected,
            }
        );
    }

    #[tokio::test]
    async fn thumbnail_cache_cap_defaults_and_persists_a_valid_value() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_eq!(
            get_thumbnail_cache_cap_bytes(&pool).await.unwrap(),
            DEFAULT_THUMBNAIL_CACHE_CAP_BYTES
        );
        set_thumbnail_cache_cap_bytes(&pool, 512 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(
            get_thumbnail_cache_cap_bytes(&pool).await.unwrap(),
            512 * 1024 * 1024
        );
    }

    #[tokio::test]
    async fn cached_hash_misses_when_mtime_differs_within_a_second() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        update_sync_state(
            &pool,
            "photo.jpg",
            "cached-hash",
            1_700_000_000_100_000_000,
            123,
            "SYNCED",
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            get_cached_hash(&pool, "photo.jpg", 1_700_000_000_200_000_000, 123)
                .await
                .unwrap(),
            None,
        );
    }

    #[tokio::test]
    async fn cached_hash_propagates_query_errors() {
        let pool = test_pool().await;

        assert!(get_cached_hash(&pool, "photo.jpg", 42, 123).await.is_err());
    }

    #[tokio::test]
    async fn uploaded_unit_persistence_syncs_all_members_and_only_images_store_remote_ids() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let assets = [
            UploadedAssetState {
                path: "photo.heic",
                hash: "photo-hash",
                mtime: 11,
                size: 101,
            },
            UploadedAssetState {
                path: "photo.mov",
                hash: "video-hash",
                mtime: 12,
                size: 202,
            },
        ];

        persist_uploaded_unit(&pool, &assets, "remote-photo")
            .await
            .unwrap();

        let rows: Vec<(String, String, i64, i64, String, Option<String>)> = sqlx::query_as(
            "SELECT local_path, file_hash, last_modified, size, status, remote_id \
             FROM sync_state ORDER BY local_path",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "photo.heic".to_string(),
                    "photo-hash".to_string(),
                    11,
                    101,
                    "SYNCED".to_string(),
                    Some("remote-photo".to_string()),
                ),
                (
                    "photo.mov".to_string(),
                    "video-hash".to_string(),
                    12,
                    202,
                    "SYNCED".to_string(),
                    None,
                ),
            ]
        );
    }

    #[tokio::test]
    async fn uploaded_unit_persistence_rolls_back_when_a_later_write_fails() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        update_sync_state(&pool, "photo.heic", "old-photo", 1, 1, "PENDING", None)
            .await
            .unwrap();
        update_sync_state(&pool, "photo.mov", "old-video", 2, 2, "PENDING", None)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TRIGGER abort_uploaded_video_update BEFORE UPDATE ON sync_state \
             WHEN NEW.local_path = 'photo.mov' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let assets = [
            UploadedAssetState {
                path: "photo.heic",
                hash: "new-photo",
                mtime: 11,
                size: 101,
            },
            UploadedAssetState {
                path: "photo.mov",
                hash: "new-video",
                mtime: 12,
                size: 202,
            },
        ];

        assert!(persist_uploaded_unit(&pool, &assets, "remote-photo")
            .await
            .is_err());

        let rows: Vec<(String, String, i64, i64, String, Option<String>)> = sqlx::query_as(
            "SELECT local_path, file_hash, last_modified, size, status, remote_id \
             FROM sync_state ORDER BY local_path",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "photo.heic".to_string(),
                    "old-photo".to_string(),
                    1,
                    1,
                    "PENDING".to_string(),
                    None,
                ),
                (
                    "photo.mov".to_string(),
                    "old-video".to_string(),
                    2,
                    2,
                    "PENDING".to_string(),
                    None,
                ),
            ]
        );
    }

    #[tokio::test]
    async fn failed_state_clears_hash_and_is_never_cached() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        update_sync_state(&pool, "photo.jpg", "old-hash", 42, 123, "SYNCED", None)
            .await
            .unwrap();
        mark_sync_failed(&pool, "photo.jpg", 42, 123, "Upload rejected")
            .await
            .unwrap();

        assert_eq!(
            get_cached_hash(&pool, "photo.jpg", 42, 123).await.unwrap(),
            None
        );

        let row = sqlx::query("SELECT file_hash, status FROM sync_state WHERE local_path = ?")
            .bind("photo.jpg")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("file_hash"), "");
        assert_eq!(row.get::<String, _>("status"), "FAILED");
    }

    #[tokio::test]
    async fn failed_sync_entries_persist_update_and_clear_on_success() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        mark_sync_failed(&pool, "b.jpg", 1, 2, "First error")
            .await
            .unwrap();
        mark_sync_failed(&pool, "a.jpg", 3, 4, "Other error")
            .await
            .unwrap();
        mark_sync_failed(&pool, "b.jpg", 5, 6, "Updated error")
            .await
            .unwrap();

        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].local_path, "a.jpg");
        assert_eq!(failures[1].failure_reason, "Updated error");

        update_sync_state(&pool, "b.jpg", "hash", 5, 6, "PENDING", None)
            .await
            .unwrap();
        assert_eq!(get_failed_syncs(&pool).await.unwrap().len(), 2);

        update_sync_state(&pool, "b.jpg", "hash", 5, 6, "SYNCED", None)
            .await
            .unwrap();
        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].local_path, "a.jpg");
    }

    #[tokio::test]
    async fn failed_sync_entries_include_legacy_rows_without_a_reason() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO sync_state (local_path, file_hash, last_modified, size, status, remote_id, failure_reason)
             VALUES (?, '', 0, 0, 'FAILED', NULL, NULL)",
        )
        .bind("legacy.jpg")
        .execute(&pool)
        .await
        .unwrap();

        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].local_path, "legacy.jpg");
        assert_eq!(
            failures[0].failure_reason,
            "Failure details are unavailable; retry to obtain details."
        );
    }

    #[tokio::test]
    async fn discarding_missing_failed_paths_removes_state_queue_and_updates_job() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let assets = [
            QueueAsset {
                path: "missing.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
            QueueAsset {
                path: "retry.jpg",
                size: 2,
                mtime: 2,
                target_album_id: None,
            },
        ];
        enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        finalize_queued_block(
            &pool,
            &[
                ("missing.jpg".to_string(), "FAILED".to_string()),
                ("retry.jpg".to_string(), "FAILED".to_string()),
            ],
        )
        .await
        .unwrap();
        mark_sync_failed(&pool, "missing.jpg", 1, 1, "Upload failed")
            .await
            .unwrap();
        mark_sync_failed(&pool, "retry.jpg", 2, 2, "Upload failed")
            .await
            .unwrap();

        discard_failed_sync_paths(&pool, &["missing.jpg".to_string()])
            .await
            .unwrap();

        let failures = get_failed_syncs(&pool).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].local_path, "retry.jpg");
        let queue_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sync_queue WHERE local_path = 'missing.jpg'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queue_count, 0);
        let snapshot = get_sync_snapshot(&pool).await.unwrap();
        assert_eq!(snapshot.status, "IDLE");
        assert_eq!(snapshot.total, 1);
        assert_eq!(snapshot.failed, 1);

        discard_failed_sync_paths(&pool, &[]).await.unwrap();
        assert_eq!(get_sync_snapshot(&pool).await.unwrap().total, 1);

        discard_failed_sync_paths(&pool, &["retry.jpg".to_string()])
            .await
            .unwrap();
        let snapshot = get_sync_snapshot(&pool).await.unwrap();
        assert_eq!(snapshot.status, "IDLE");
        assert_eq!(snapshot.total, 0);
        assert_eq!(snapshot.failed, 0);
    }

    #[tokio::test]
    async fn discarding_folder_sync_data_keeps_component_prefix_siblings() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let assets = [
            QueueAsset {
                path: "C:/Photos/one.jpg",
                size: 1,
                mtime: 1,
                target_album_id: None,
            },
            QueueAsset {
                path: "C:/Photos/nested/two.jpg",
                size: 2,
                mtime: 2,
                target_album_id: None,
            },
            QueueAsset {
                path: "C:/Photos-alt/keep.jpg",
                size: 3,
                mtime: 3,
                target_album_id: None,
            },
        ];
        enqueue_sync_assets(&pool, &assets, &[]).await.unwrap();
        mark_sync_failed(&pool, "C:/Photos/failed.jpg", 4, 4, "failed")
            .await
            .unwrap();
        mark_sync_failed(&pool, "C:/Photos-alt/keep-failed.jpg", 5, 5, "failed")
            .await
            .unwrap();

        discard_sync_data_in_folder(&pool, "C:/Photos")
            .await
            .unwrap();

        let queue_paths: Vec<String> =
            sqlx::query_scalar("SELECT local_path FROM sync_queue ORDER BY local_path")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(queue_paths, vec!["C:/Photos-alt/keep.jpg"]);
        let failure_paths: Vec<_> = get_failed_syncs(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|entry| entry.local_path)
            .collect();
        assert_eq!(failure_paths, vec!["C:/Photos-alt/keep-failed.jpg"]);
    }

    #[tokio::test]
    async fn discarding_folder_sync_data_preserves_completed_hash_cache() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        update_sync_state(
            &pool,
            "C:/Photos/uploaded.jpg",
            "hash",
            1,
            1,
            "SYNCED",
            Some("remote"),
        )
        .await
        .unwrap();
        mark_sync_failed(&pool, "C:/Photos/failed.jpg", 2, 2, "failed")
            .await
            .unwrap();

        discard_sync_data_in_folder(&pool, "C:/Photos")
            .await
            .unwrap();

        assert_eq!(
            get_cached_hash(&pool, "C:/Photos/uploaded.jpg", 1, 1)
                .await
                .unwrap(),
            Some("hash".to_string())
        );
        assert!(get_failed_syncs(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn corrupt_database_is_quarantined_and_repaired() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("corrupt_test.db");

        // Write non-database invalid data to simulate corrupted SQLite file
        std::fs::write(
            &db_path,
            b"NOT A VALID SQLITE DATABASE FILE - CORRUPTED DATA",
        )
        .unwrap();

        let result = init_at_path(&db_path).await.unwrap();
        match result.status {
            DatabaseStatus::Repaired {
                backup_path,
                salvaged_folders,
                reason,
            } => {
                assert!(backup_path.exists());
                assert_eq!(salvaged_folders, 0);
                assert!(!reason.is_empty());
            }
            DatabaseStatus::Ready => panic!("Expected database to be repaired, but got Ready"),
        }

        // New database should be fully functional
        add_folder(&result.pool, "C:/test_photos", None)
            .await
            .unwrap();
        let folders = get_folders(&result.pool).await.unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "C:/test_photos");

        result.pool.close().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn corrupt_database_salvages_watched_folders_when_possible() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("salvage_test.db");

        // 1. Create a healthy DB, add a folder and add sync_state rows
        let result1 = init_at_path(&db_path).await.unwrap();
        assert_eq!(result1.status, DatabaseStatus::Ready);
        add_folder(&result1.pool, "C:/my_important_photos", None)
            .await
            .unwrap();
        for i in 0..50 {
            update_sync_state(
                &result1.pool,
                &format!("photo{}.jpg", i),
                "hash",
                42,
                100,
                "SYNCED",
                None,
            )
            .await
            .unwrap();
        }

        let sync_rootpage: i64 =
            sqlx::query_scalar("SELECT rootpage FROM sqlite_master WHERE name = 'sync_state'")
                .fetch_one(&result1.pool)
                .await
                .unwrap();

        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&result1.pool)
            .await
            .unwrap();
        result1.pool.close().await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 2. Corrupt the sync_state table b-tree page specifically while leaving watched_folders page intact
        let mut bytes = std::fs::read(&db_path).unwrap();
        let offset = ((sync_rootpage - 1) * 4096) as usize;
        assert!(bytes.len() >= offset + 100);
        for b in &mut bytes[offset + 20..offset + 80] {
            *b = 0xFF;
        }
        std::fs::write(&db_path, bytes).unwrap();

        // 3. Now re-init at the same path. quick_check will detect corruption in sync_state!
        let result2 = init_at_path(&db_path).await.unwrap();
        match result2.status {
            DatabaseStatus::Repaired {
                salvaged_folders,
                reason,
                ..
            } => {
                assert_eq!(salvaged_folders, 1);
                assert!(!reason.is_empty());
            }
            DatabaseStatus::Ready => panic!("Expected database to be repaired"),
        }

        let folders = get_folders(&result2.pool).await.unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "C:/my_important_photos");

        result2.pool.close().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn corrupt_database_with_wal_and_shm_quarantines_all_files() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("wal_test.db");

        // 1. Create a healthy DB
        let init_res = init_at_path(&db_path).await.unwrap();
        assert_eq!(init_res.status, DatabaseStatus::Ready);
        add_folder(&init_res.pool, "C:/photos", None).await.unwrap();
        init_res.pool.close().await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Ensure -wal and -shm files exist
        let wal_path = temp_dir.join("wal_test.db-wal");
        let shm_path = temp_dir.join("wal_test.db-shm");
        std::fs::write(&wal_path, b"DUMMY_RETAINED_WAL_DATA").unwrap();
        std::fs::write(&shm_path, b"DUMMY_RETAINED_SHM_DATA").unwrap();

        // 2. Corrupt db file header so it fails with SQLITE_NOTADB / CORRUPT
        let backup_path = quarantine_unusable_db(&db_path).await.unwrap();
        assert!(backup_path.exists());
        let parent = backup_path.parent().unwrap();
        let backup_name = backup_path.file_name().unwrap().to_str().unwrap();
        assert!(parent.join(format!("{}-wal", backup_name)).exists());
        assert!(parent.join(format!("{}-shm", backup_name)).exists());

        // Verify the original wal and shm are no longer lingering
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn locked_database_is_not_quarantined() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("locked_test.db");

        // 1. Create a valid DB
        let result = init_at_path(&db_path).await.unwrap();
        assert_eq!(result.status, DatabaseStatus::Ready);
        add_folder(&result.pool, "C:/photos", None).await.unwrap();
        result.pool.close().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 2. Simulate locked/inaccessible database holding it exclusively
        #[cfg(windows)]
        let lock_file = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&db_path)
                .unwrap()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o000)).unwrap();
        }

        // 3. Try to init_at_path while locked
        let attempt = init_at_path(&db_path).await;
        assert!(attempt.is_err(), "Expected locked database to return error");

        // 4. Verify that NO .corrupted file was created!
        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupted"))
            .collect();
        assert!(
            entries.is_empty(),
            "Locked database must not be quarantined! Found: {:?}",
            entries
        );

        #[cfg(windows)]
        drop(lock_file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644));
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn rapid_repairs_avoid_name_collision() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("collision_test.db");

        // Corrupt 1
        std::fs::write(&db_path, b"CORRUPT DATA 1").unwrap();
        let res1 = init_at_path(&db_path).await.unwrap();
        let path1 = match res1.status {
            DatabaseStatus::Repaired { backup_path, .. } => backup_path,
            _ => panic!("Expected repair 1"),
        };
        res1.pool.close().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Corrupt 2 immediately (within same second)
        std::fs::write(&db_path, b"CORRUPT DATA 2").unwrap();
        let wal_file = temp_dir.join("collision_test.db-wal");
        if wal_file.exists() {
            std::fs::write(&wal_file, b"CORRUPT WAL 2").unwrap();
        }
        let res2 = init_at_path(&db_path).await.unwrap();
        let path2 = match res2.status {
            DatabaseStatus::Repaired { backup_path, .. } => backup_path,
            _ => panic!("Expected repair 2"),
        };
        res2.pool.close().await;

        assert_ne!(path1, path2);
        assert!(path1.exists());
        assert!(path2.exists());
        assert_eq!(std::fs::read(&path1).unwrap(), b"CORRUPT DATA 1");
        assert_eq!(std::fs::read(&path2).unwrap(), b"CORRUPT DATA 2");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn migration_checksum_mismatch_rebuilds_database_and_restores_folders() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("checksum_mismatch_test.db");

        let initial = init_at_path(&db_path).await.unwrap();
        add_folder(&initial.pool, "C:/important_photos", None)
            .await
            .unwrap();
        initial.pool.close().await;

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 1")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let repaired = init_at_path(&db_path).await.unwrap();
        match &repaired.status {
            DatabaseStatus::Repaired {
                backup_path,
                salvaged_folders,
                reason,
            } => {
                assert!(backup_path.exists());
                assert_eq!(*salvaged_folders, 1);
                assert!(reason.contains("Migration checksum mismatch for version 1"));
            }
            DatabaseStatus::Ready => panic!("Expected checksum mismatch to rebuild the database"),
        }

        let folders = get_folders(&repaired.pool).await.unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "C:/important_photos");

        repaired.pool.close().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn migration_failure_without_corruption_is_not_quarantined() {
        let temp_dir = std::env::temp_dir().join(format!("lymic_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("migration_err_test.db");

        // 1. Create a healthy DB
        let result = init_at_path(&db_path).await.unwrap();
        assert_eq!(result.status, DatabaseStatus::Ready);
        add_folder(&result.pool, "C:/important_photos", None)
            .await
            .unwrap();
        result.pool.close().await;

        // 2. Insert a future migration entry (simulate DB was created by a newer version of the app)
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
             VALUES (999999, 'future_migration', CURRENT_TIMESTAMP, 1, X'1234', 10)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        // 3. Re-init. sqlx::migrate will return VersionMissing(999999)
        let attempt = init_at_path(&db_path).await;
        assert!(attempt.is_err(), "Expected migration error to fail init");

        // Verify that NO .corrupted file was created!
        let corrupted_files: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupted"))
            .collect();
        assert!(
            corrupted_files.is_empty(),
            "Healthy DB with version mismatch must not be quarantined! Found: {:?}",
            corrupted_files
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn target_album_id_persists_on_watched_folder_and_is_retrieved() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let album_id = crate::immich::AlbumId::new("album-123");
        let id = add_folder(&pool, "C:/vacation", Some(&album_id))
            .await
            .unwrap();
        let folders = get_folders(&pool).await.unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, id);
        assert_eq!(folders[0].path, "C:/vacation");
        assert_eq!(folders[0].target_album_id.as_deref(), Some("album-123"));
    }

    #[tokio::test]
    async fn target_album_id_and_remote_id_persist_in_sync_queue() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let album_id = crate::immich::AlbumId::new("album-123");
        let asset = [QueueAsset {
            path: "beach.jpg",
            size: 100,
            mtime: 10,
            target_album_id: Some(&album_id),
        }];
        enqueue_sync_assets(&pool, &asset, &[]).await.unwrap();

        let queued = next_sync_queue_block(&pool, 10).await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].local_path, "beach.jpg");
        assert_eq!(queued[0].target_album_id.as_deref(), Some("album-123"));
        assert_eq!(queued[0].remote_id, None);

        let remote_id = crate::immich::RemoteAssetId::new("remote-asset-uuid-789");
        update_sync_queue_remote_id(&pool, "beach.jpg", &remote_id)
            .await
            .unwrap();

        let queued = next_sync_queue_block(&pool, 10).await.unwrap();
        assert_eq!(
            queued[0].remote_id.as_deref(),
            Some("remote-asset-uuid-789")
        );
    }

    #[tokio::test]
    async fn recovery_reconstructs_a_live_photo_upload_unit_with_its_fixed_album_and_primary_remote_id(
    ) {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let album_id = crate::immich::AlbumId::new("album-original");
        let image = QueueUploadAsset {
            path: "live.jpg".to_string(),
            size: 100,
            mtime: 10,
            target_album_id: Some(album_id.clone()),
        };
        let video = QueueUploadAsset {
            path: "live.mov".to_string(),
            size: 200,
            mtime: 10,
            target_album_id: Some(album_id.clone()),
        };
        let unit = QueueUploadUnit {
            image,
            live_photo_video: Some(video),
        };
        enqueue_upload_units(&pool, &[unit], &[]).await.unwrap();

        let primary_remote_id = crate::immich::RemoteAssetId::new("remote-live");
        update_upload_unit_remote_id(&pool, "live.jpg", &primary_remote_id)
            .await
            .unwrap();

        let recovered = next_queued_upload_units(&pool, 10).await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].image.local_path, "live.jpg");
        assert_eq!(
            recovered[0].image.target_album_id.as_deref(),
            Some("album-original")
        );
        assert_eq!(recovered[0].image.remote_id.as_deref(), Some("remote-live"));
        assert_eq!(
            recovered[0]
                .live_photo_video
                .as_ref()
                .map(|asset| asset.local_path.as_str()),
            Some("live.mov")
        );
    }

    #[tokio::test]
    async fn reenqueueing_a_single_image_with_its_live_photo_preserves_one_upload_unit() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        enqueue_upload_units(
            &pool,
            &[QueueUploadUnit {
                image: QueueUploadAsset {
                    path: "live.jpg".to_string(),
                    size: 100,
                    mtime: 10,
                    target_album_id: None,
                },
                live_photo_video: None,
            }],
            &[],
        )
        .await
        .unwrap();
        enqueue_upload_units(
            &pool,
            &[QueueUploadUnit {
                image: QueueUploadAsset {
                    path: "live.jpg".to_string(),
                    size: 100,
                    mtime: 10,
                    target_album_id: None,
                },
                live_photo_video: Some(QueueUploadAsset {
                    path: "live.mov".to_string(),
                    size: 200,
                    mtime: 10,
                    target_album_id: None,
                }),
            }],
            &[],
        )
        .await
        .unwrap();

        let recovered = next_queued_upload_units(&pool, 10).await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].image.local_path, "live.jpg");
        assert_eq!(
            recovered[0]
                .live_photo_video
                .as_ref()
                .map(|asset| asset.local_path.as_str()),
            Some("live.mov")
        );
    }

    #[tokio::test]
    async fn target_album_id_is_immutable_on_queue_upsert_conflict() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let original_album = crate::immich::AlbumId::new("album-original");
        let initial_asset = [QueueAsset {
            path: "beach.jpg",
            size: 100,
            mtime: 10,
            target_album_id: Some(&original_album),
        }];
        enqueue_sync_assets(&pool, &initial_asset, &[])
            .await
            .unwrap();

        let queued = next_sync_queue_block(&pool, 10).await.unwrap();
        assert_eq!(queued[0].target_album_id.as_deref(), Some("album-original"));

        // Attempting to re-enqueue the same path with a different album or None must preserve original album
        let different_album = crate::immich::AlbumId::new("album-changed");
        let updated_asset = [QueueAsset {
            path: "beach.jpg",
            size: 200,
            mtime: 20,
            target_album_id: Some(&different_album),
        }];
        enqueue_sync_assets(&pool, &updated_asset, &[])
            .await
            .unwrap();

        let queued = next_sync_queue_block(&pool, 10).await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].target_album_id.as_deref(), Some("album-original"));
        assert_eq!(queued[0].size, 200);
        assert_eq!(queued[0].last_modified, 20);

        let none_asset = [QueueAsset {
            path: "beach.jpg",
            size: 300,
            mtime: 30,
            target_album_id: None,
        }];
        enqueue_sync_assets(&pool, &none_asset, &[]).await.unwrap();

        let queued = next_sync_queue_block(&pool, 10).await.unwrap();
        assert_eq!(queued[0].target_album_id.as_deref(), Some("album-original"));
        assert_eq!(queued[0].size, 300);
    }

    #[tokio::test]
    async fn removable_watched_folder_binding_persists_queries_and_cascades_on_folder_deletion() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let folder_id = add_folder(&pool, "E:/DCIM", None).await.unwrap();

        // Initially no binding
        assert_eq!(
            get_removable_folder_binding(&pool, folder_id)
                .await
                .unwrap(),
            None
        );

        // Bind folder to removable volume
        bind_removable_folder(&pool, folder_id, "vol-uuid-1234", "DCIM")
            .await
            .unwrap();

        let binding = get_removable_folder_binding(&pool, folder_id)
            .await
            .unwrap()
            .expect("binding should exist");
        assert_eq!(binding.watched_folder_id, folder_id);
        assert_eq!(binding.volume_id, "vol-uuid-1234");
        assert_eq!(binding.relative_path, "DCIM");

        // Query by volume
        let volume_folders = get_removable_folders_by_volume(&pool, "vol-uuid-1234")
            .await
            .unwrap();
        assert_eq!(volume_folders.len(), 1);
        assert_eq!(volume_folders[0].watched_folder_id, folder_id);
        assert_eq!(volume_folders[0].relative_path, "DCIM");

        // Unbind manually
        unbind_removable_folder(&pool, folder_id).await.unwrap();
        assert_eq!(
            get_removable_folder_binding(&pool, folder_id)
                .await
                .unwrap(),
            None
        );
        // Folder itself is still intact
        let folders = get_folders(&pool).await.unwrap();
        assert_eq!(folders.len(), 1);

        // Re-bind and delete folder -> cascade must delete the binding
        bind_removable_folder(&pool, folder_id, "vol-uuid-1234", "DCIM")
            .await
            .unwrap();
        assert!(get_removable_folder_binding(&pool, folder_id)
            .await
            .unwrap()
            .is_some());

        remove_folder(&pool, folder_id).await.unwrap();
        assert_eq!(
            get_removable_folder_binding(&pool, folder_id)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn remap_watched_folder_path_atomically_updates_descendants_and_preserves_siblings() {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Setup watched folder and sibling folder
        let folder_id = add_folder(&pool, "E:/DCIM", None).await.unwrap();
        let sibling_id = add_folder(&pool, "E:/DCIM_Backup", None).await.unwrap();

        // Populate sync_state
        update_sync_state(
            &pool,
            "E:/DCIM/100EOS/IMG_0001.JPG",
            "hash1",
            100,
            1000,
            "SYNCED",
            Some("rem1"),
        )
        .await
        .unwrap();
        update_sync_state(
            &pool,
            "E:/DCIM/100EOS/IMG_0002.JPG",
            "hash2",
            200,
            2000,
            "FAILED",
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE sync_state SET failure_reason = 'network timeout' WHERE local_path = 'E:/DCIM/100EOS/IMG_0002.JPG'")
            .execute(&pool)
            .await
            .unwrap();
        update_sync_state(
            &pool,
            "E:/DCIM_Backup/100EOS/IMG_0003.JPG",
            "hash3",
            300,
            3000,
            "SYNCED",
            Some("rem3"),
        )
        .await
        .unwrap();

        // Populate sync_queue
        let queue_units = vec![
            QueueUploadUnit {
                image: QueueUploadAsset {
                    path: "E:/DCIM/100EOS/IMG_0002.JPG".to_string(),
                    size: 2000,
                    mtime: 200,
                    target_album_id: None,
                },
                live_photo_video: None,
            },
            QueueUploadUnit {
                image: QueueUploadAsset {
                    path: "E:/DCIM_Backup/100EOS/IMG_0003.JPG".to_string(),
                    size: 3000,
                    mtime: 300,
                    target_album_id: None,
                },
                live_photo_video: None,
            },
        ];
        enqueue_upload_units(&pool, &queue_units, &[])
            .await
            .unwrap();

        // Perform remap
        remap_watched_folder_path(&pool, folder_id, "E:/DCIM", "F:/DCIM")
            .await
            .unwrap();

        // 1. Verify watched_folders
        let folders = get_folders(&pool).await.unwrap();
        let remapped_folder = folders.iter().find(|f| f.id == folder_id).unwrap();
        assert_eq!(remapped_folder.path, "F:/DCIM");

        let sibling_folder = folders.iter().find(|f| f.id == sibling_id).unwrap();
        assert_eq!(sibling_folder.path, "E:/DCIM_Backup");

        // 2. Verify sync_state
        let old_item = sqlx::query(
            "SELECT local_path FROM sync_state WHERE local_path = 'E:/DCIM/100EOS/IMG_0001.JPG'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(old_item.is_none());

        let new_item_1: (String, String, Option<String>, String) = sqlx::query_as(
            "SELECT local_path, file_hash, remote_id, status FROM sync_state WHERE local_path = 'F:/DCIM/100EOS/IMG_0001.JPG'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(new_item_1.0, "F:/DCIM/100EOS/IMG_0001.JPG");
        assert_eq!(new_item_1.1, "hash1");
        assert_eq!(new_item_1.2, Some("rem1".to_string()));
        assert_eq!(new_item_1.3, "SYNCED");

        let new_item_2: (String, String, Option<String>, String, Option<String>) = sqlx::query_as(
            "SELECT local_path, file_hash, remote_id, status, failure_reason FROM sync_state WHERE local_path = 'F:/DCIM/100EOS/IMG_0002.JPG'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(new_item_2.0, "F:/DCIM/100EOS/IMG_0002.JPG");
        assert_eq!(new_item_2.1, "hash2");
        assert_eq!(new_item_2.3, "FAILED");
        assert_eq!(new_item_2.4, Some("network timeout".to_string()));

        // Sibling sync_state MUST be unchanged
        let sibling_state: (String, String) = sqlx::query_as(
            "SELECT local_path, file_hash FROM sync_state WHERE local_path = 'E:/DCIM_Backup/100EOS/IMG_0003.JPG'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(sibling_state.0, "E:/DCIM_Backup/100EOS/IMG_0003.JPG");
        assert_eq!(sibling_state.1, "hash3");

        // 3. Verify sync_queue
        let old_queued = sqlx::query(
            "SELECT local_path FROM sync_queue WHERE local_path = 'E:/DCIM/100EOS/IMG_0002.JPG'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(old_queued.is_none());

        let new_queued: (String, String) = sqlx::query_as(
            "SELECT local_path, status FROM sync_queue WHERE local_path = 'F:/DCIM/100EOS/IMG_0002.JPG'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(new_queued.0, "F:/DCIM/100EOS/IMG_0002.JPG");
        assert_eq!(new_queued.1, "PENDING");

        // Sibling queue entry MUST be unchanged
        let sibling_queued: (String, String) = sqlx::query_as(
            "SELECT local_path, status FROM sync_queue WHERE local_path = 'E:/DCIM_Backup/100EOS/IMG_0003.JPG'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(sibling_queued.0, "E:/DCIM_Backup/100EOS/IMG_0003.JPG");
        assert_eq!(sibling_queued.1, "PENDING");
    }

    #[tokio::test]
    async fn sql_path_is_or_descendant_of_predicate_format() {
        let predicate = crate::db::sql_path_is_or_descendant_of("local_path", "?1");
        assert_eq!(
            predicate,
            "(local_path = ?1 OR substr(local_path, 1, length(?1) + 1) = ?1 || '/' OR substr(local_path, 1, length(?1) + 1) = ?1 || '\\\\')"
        );
    }

    #[tokio::test]
    async fn reset_failed_queue_items_for_folder_resets_descendants_and_preserves_sibling_folders()
    {
        let pool = test_pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // 1. Populate sync_queue with failed items in target folder and a sibling folder
        sqlx::query(
            "INSERT INTO sync_queue (job_id, sequence, local_path, size, last_modified, status, upload_unit_id, is_primary, failure_reason) \
             VALUES \
             (1, 1, 'E:/DCIM/sub/1.jpg', 100, 1, 'FAILED', 'E:/DCIM/sub/1.jpg', 1, 'timeout'), \
             (1, 2, 'E:/DCIM/2.jpg', 200, 2, 'FAILED', 'E:/DCIM/2.jpg', 1, 'io error'), \
             (1, 3, 'E:/DCIM_Backup/3.jpg', 300, 3, 'FAILED', 'E:/DCIM_Backup/3.jpg', 1, 'sibling failure')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 2. Populate sync_state with matching failed items
        update_sync_state(&pool, "E:/DCIM/sub/1.jpg", "h1", 100, 1, "FAILED", None)
            .await
            .unwrap();
        sqlx::query("UPDATE sync_state SET failure_reason = 'timeout' WHERE local_path = 'E:/DCIM/sub/1.jpg'")
            .execute(&pool)
            .await
            .unwrap();

        update_sync_state(&pool, "E:/DCIM/2.jpg", "h2", 200, 2, "FAILED", None)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE sync_state SET failure_reason = 'io error' WHERE local_path = 'E:/DCIM/2.jpg'",
        )
        .execute(&pool)
        .await
        .unwrap();

        update_sync_state(&pool, "E:/DCIM_Backup/3.jpg", "h3", 300, 3, "FAILED", None)
            .await
            .unwrap();
        sqlx::query("UPDATE sync_state SET failure_reason = 'sibling failure' WHERE local_path = 'E:/DCIM_Backup/3.jpg'")
            .execute(&pool)
            .await
            .unwrap();

        // Reset failed queue items for target folder 'E:/DCIM'
        let affected = reset_failed_queue_items_for_folder(&pool, "E:/DCIM")
            .await
            .unwrap();
        assert_eq!(affected, 2);

        // Verify sync_jobs was transitioned to RUNNING
        let job_status: String = sqlx::query_scalar("SELECT status FROM sync_jobs WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(job_status, "RUNNING");

        // Verify E:/DCIM items in sync_queue were reset
        let target_queue: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT local_path, status, failure_reason FROM sync_queue WHERE local_path LIKE 'E:/DCIM/%' ORDER BY sequence",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(target_queue.len(), 2);
        assert_eq!(
            target_queue[0],
            ("E:/DCIM/sub/1.jpg".to_string(), "PENDING".to_string(), None)
        );
        assert_eq!(
            target_queue[1],
            ("E:/DCIM/2.jpg".to_string(), "PENDING".to_string(), None)
        );

        // Verify sibling E:/DCIM_Backup item in sync_queue was NOT reset
        let sibling_queue: (String, String, Option<String>) = sqlx::query_as(
            "SELECT local_path, status, failure_reason FROM sync_queue WHERE local_path = 'E:/DCIM_Backup/3.jpg'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            sibling_queue,
            (
                "E:/DCIM_Backup/3.jpg".to_string(),
                "FAILED".to_string(),
                Some("sibling failure".to_string())
            )
        );

        // Verify sync_state as well
        let target_state: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT local_path, status, failure_reason FROM sync_state WHERE local_path LIKE 'E:/DCIM/%' ORDER BY local_path",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(target_state.len(), 2);
        assert_eq!(target_state[0].1, "PENDING");
        assert_eq!(target_state[0].2, None);
        assert_eq!(target_state[1].1, "PENDING");
        assert_eq!(target_state[1].2, None);

        let sibling_state: (String, String, Option<String>) = sqlx::query_as(
            "SELECT local_path, status, failure_reason FROM sync_state WHERE local_path = 'E:/DCIM_Backup/3.jpg'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(sibling_state.1, "FAILED");
        assert_eq!(sibling_state.2, Some("sibling failure".to_string()));
    }
}
