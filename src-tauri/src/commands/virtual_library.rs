//! Thin Tauri adapters for the Virtual Library settings section (ADR-0003).
//!
//! The section exists only for a packaged Store-channel Windows run. The decision is made here, at
//! runtime, and never by the frontend: the release-channel flag alone is a compile-time label.
//! Enabling registers and serves the sync root through `cloud_files` (Windows adapter, #278) and
//! disabling stops serving it; unregistering is #283. The root folder (the Known Folder Pictures
//! plus `Lymic` unless the user chose another) and the refusal to overlap a Watched Folder live
//! here and in `virtual_library_root`; the registration side of the rule is in `watched_folders`.

use crate::{
    app::events::log_to_ui_keyed,
    cloud_files::{Notice, NoticeKind, NoticeSink, SyncRootService, SystemSyncRoot},
    db,
    media::paths::canonicalize_folder_path,
    virtual_library_root::{
        self, is_available_at_runtime, load_root, KnownFolders, RootOverlap, SystemKnownFolders,
    },
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Runtime};

pub(crate) const SETTINGS_CHANGED_EVENT: &str = "virtual-library-settings-changed";
/// A failed open or listing the user should hear about, once per failure episode (#281). Its
/// payload is a `cloud_files::Notice`; the frontend shows it and, for an expired sign-in, offers to
/// sign in again.
pub(crate) const NOTICE_EVENT: &str = "virtual-library-notice";
const UNAVAILABLE_MESSAGE: &str =
    "The Virtual Library is available only in the Microsoft Store version of Lymic on Windows.";
/// Error text prefix when the folder is changed while the Virtual Library is enabled. The
/// frontend turns it into a localized request to disable it first.
pub(crate) const DISABLE_TO_CHANGE_FOLDER_ERROR: &str = "VIRTUAL_LIBRARY_DISABLE_TO_CHANGE_FOLDER";
const NO_ROOT_MESSAGE: &str = "The Virtual Library cannot be enabled because Lymic could not find your Pictures folder. Choose a Virtual Library folder first.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VirtualLibrarySettings {
    pub(crate) available: bool,
    pub(crate) enabled: bool,
    /// The effective Virtual Library folder: the user's choice, or the default. `None` while the
    /// Virtual Library is unavailable or the system cannot say where Pictures is.
    pub(crate) root_folder: Option<String>,
}

async fn load_settings(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Ok(VirtualLibrarySettings {
            available,
            enabled: false,
            root_folder: None,
        });
    }
    let enabled = db::get_virtual_library_enabled(pool)
        .await
        .map_err(|error| error.to_string())?;
    let root_folder = load_root(pool, known_folders)
        .await?
        .map(|root| root.to_string_lossy().into_owned());
    Ok(VirtualLibrarySettings {
        available,
        enabled,
        root_folder,
    })
}

/// The Watched Folders that overlap `root`, and how.
async fn overlapping_watched_folders(
    pool: &sqlx::SqlitePool,
    root: &Path,
) -> Result<Vec<(PathBuf, RootOverlap)>, String> {
    let folders = db::get_folders(pool)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|folder| PathBuf::from(folder.path))
        .collect::<Vec<_>>();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || virtual_library_root::overlapping_folders(&root, &folders))
        .await
        .map_err(|error| error.to_string())
}

/// Names every overlapping Watched Folder so the user knows what to remove.
fn overlap_refusal(action: &str, root: &Path, overlaps: &[(PathBuf, RootOverlap)]) -> String {
    let remedy =
        "Remove it from your Watched Folders or choose a different Virtual Library folder.";
    match overlaps {
        [(folder, relation)] => format!(
            "{action}: the Watched Folder '{}' {} '{}'. {remedy}",
            folder.display(),
            relation.describe(),
            root.display()
        ),
        _ => {
            let names = overlaps
                .iter()
                .map(|(folder, _)| format!("'{}'", folder.display()))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{action}: the Watched Folders {names} overlap the Virtual Library folder '{}'. {remedy}",
                root.display()
            )
        }
    }
}

/// Enabling checks the folder (Watched Folders, other sync roots), registers and serves it, and only
/// then remembers the choice; a refusal leaves everything as it was. Disabling remembers the choice
/// and stops serving.
async fn change_enabled(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
    service: &impl SyncRootService,
    enabled: bool,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Err(UNAVAILABLE_MESSAGE.to_string());
    }
    if enabled {
        let root = load_root(pool, known_folders)
            .await?
            .ok_or_else(|| NO_ROOT_MESSAGE.to_string())?;
        let overlaps = overlapping_watched_folders(pool, &root).await?;
        if !overlaps.is_empty() {
            return Err(overlap_refusal(
                "The Virtual Library cannot be enabled",
                &root,
                &overlaps,
            ));
        }
        service
            .start(root)
            .await
            .map_err(|error| error.user_message())?;
        if let Err(error) = db::set_virtual_library_enabled(pool, true).await {
            service.stop();
            return Err(error.to_string());
        }
    } else {
        db::set_virtual_library_enabled(pool, false)
            .await
            .map_err(|error| error.to_string())?;
        service.stop();
    }
    load_settings(pool, available, known_folders).await
}

/// Starts serving again after an app start, when the Virtual Library is available and enabled.
pub(crate) async fn restore(pool: &sqlx::SqlitePool) -> Result<(), String> {
    restore_with(
        pool,
        is_available_at_runtime(),
        &SystemKnownFolders,
        &SystemSyncRoot,
    )
    .await
}

async fn restore_with(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
    service: &impl SyncRootService,
) -> Result<(), String> {
    if !available
        || !db::get_virtual_library_enabled(pool)
            .await
            .map_err(|error| error.to_string())?
    {
        return Ok(());
    }
    let root = load_root(pool, known_folders)
        .await?
        .ok_or_else(|| NO_ROOT_MESSAGE.to_string())?;
    service
        .start(root)
        .await
        .map_err(|error| error.user_message())
}

async fn change_root_folder(
    pool: &sqlx::SqlitePool,
    available: bool,
    known_folders: &impl KnownFolders,
    chosen: &str,
) -> Result<VirtualLibrarySettings, String> {
    if !available {
        return Err(UNAVAILABLE_MESSAGE.to_string());
    }
    if db::get_virtual_library_enabled(pool)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err(format!(
            "{DISABLE_TO_CHANGE_FOLDER_ERROR}: disable the Virtual Library before changing its folder"
        ));
    }
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return Err("Choose a folder for the Virtual Library.".to_string());
    }
    if !Path::new(chosen).is_absolute() {
        return Err(format!(
            "'{chosen}' is not a full folder path. Choose the Virtual Library folder with the folder picker."
        ));
    }
    let chosen_path = PathBuf::from(chosen);
    let root = tokio::task::spawn_blocking(move || {
        let root = canonicalize_folder_path(&chosen_path);
        if root.exists() && !root.is_dir() {
            return Err(format!("'{}' is not a folder.", root.display()));
        }
        Ok(root)
    })
    .await
    .map_err(|error| error.to_string())??;

    let overlaps = overlapping_watched_folders(pool, &root).await?;
    if !overlaps.is_empty() {
        return Err(overlap_refusal(
            &format!("'{}' cannot be the Virtual Library folder", root.display()),
            &root,
            &overlaps,
        ));
    }
    db::set_virtual_library_root(pool, &root.to_string_lossy())
        .await
        .map_err(|error| error.to_string())?;
    load_settings(pool, available, known_folders).await
}

/// Shows a Virtual Library notice in the app: an activity log entry (key
/// `log_virtual_library_<kind>`) and the [`NOTICE_EVENT`] the frontend turns into its banner.
pub(crate) struct AppNoticeSink<R: Runtime>(tauri::AppHandle<R>);

impl<R: Runtime> AppNoticeSink<R> {
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self(app)
    }
}

impl<R: Runtime> NoticeSink for AppNoticeSink<R> {
    fn show(&self, notice: &Notice) {
        let level = if notice.kind == NoticeKind::Failed {
            "ERROR"
        } else {
            "WARN"
        };
        log_to_ui_keyed(
            &self.0,
            level,
            &notice.message(),
            &format!("log_virtual_library_{}", notice.kind.code()),
            std::collections::HashMap::from([("name".to_string(), notice.name.clone())]),
        );
        // A missed event only means no banner; the log entry above still records it.
        let _ = self.0.emit(NOTICE_EVENT, notice);
    }
}

fn announce(app: &tauri::AppHandle, settings: &VirtualLibrarySettings) {
    // The change is already persisted; a failed announcement only delays other views.
    let _ = app.emit(SETTINGS_CHANGED_EVENT, settings);
}

#[tauri::command]
pub(crate) async fn get_virtual_library_settings(
    pool: tauri::State<'_, sqlx::SqlitePool>,
) -> Result<VirtualLibrarySettings, String> {
    load_settings(pool.inner(), is_available_at_runtime(), &SystemKnownFolders).await
}

#[tauri::command]
pub(crate) async fn set_virtual_library_enabled(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    enabled: bool,
) -> Result<VirtualLibrarySettings, String> {
    let settings = change_enabled(
        pool.inner(),
        is_available_at_runtime(),
        &SystemKnownFolders,
        &SystemSyncRoot,
        enabled,
    )
    .await?;
    announce(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn set_virtual_library_root_folder(
    app: tauri::AppHandle,
    pool: tauri::State<'_, sqlx::SqlitePool>,
    path: String,
) -> Result<VirtualLibrarySettings, String> {
    let settings = change_root_folder(
        pool.inner(),
        is_available_at_runtime(),
        &SystemKnownFolders,
        &path,
    )
    .await?;
    announce(&app, &settings);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_files::{ActivationError, NESTED_SYNC_ROOT_ERROR, START_FAILED_ERROR};
    use crate::db::test_pool;
    use crate::virtual_library_root::FixedKnownFolders;
    use std::sync::Mutex;

    /// A sync root service that records what it is asked and can refuse to start.
    struct FakeService {
        refuse: Option<ActivationError>,
        started: Mutex<Vec<PathBuf>>,
        stops: Mutex<usize>,
    }

    impl FakeService {
        fn ok() -> Self {
            Self {
                refuse: None,
                started: Mutex::new(Vec::new()),
                stops: Mutex::new(0),
            }
        }

        fn refusing(error: ActivationError) -> Self {
            Self {
                refuse: Some(error),
                ..Self::ok()
            }
        }

        fn started(&self) -> Vec<PathBuf> {
            self.started.lock().unwrap().clone()
        }

        fn stops(&self) -> usize {
            *self.stops.lock().unwrap()
        }
    }

    impl SyncRootService for FakeService {
        async fn start(&self, root: PathBuf) -> Result<(), ActivationError> {
            self.started.lock().unwrap().push(root);
            match &self.refuse {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }

        fn stop(&self) {
            *self.stops.lock().unwrap() += 1;
        }
    }

    fn base() -> PathBuf {
        std::env::temp_dir().join("lymic-vl-settings-tests")
    }

    fn pictures() -> PathBuf {
        base().join("Pictures")
    }

    fn default_root() -> PathBuf {
        pictures().join("Lymic")
    }

    fn known() -> FixedKnownFolders {
        FixedKnownFolders(Some(pictures()))
    }

    fn settings(available: bool, enabled: bool, root: Option<PathBuf>) -> VirtualLibrarySettings {
        VirtualLibrarySettings {
            available,
            enabled,
            root_folder: root.map(|root| root.to_string_lossy().into_owned()),
        }
    }

    async fn watch(pool: &sqlx::SqlitePool, path: &Path) {
        db::add_folder(pool, &path.to_string_lossy(), None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn settings_start_disabled_and_the_enabled_choice_persists_when_available() {
        let pool = test_pool().await;
        let known = known();

        assert_eq!(
            load_settings(&pool, true, &known).await.unwrap(),
            settings(true, false, Some(default_root()))
        );
        assert_eq!(
            change_enabled(&pool, true, &known, &FakeService::ok(), true)
                .await
                .unwrap(),
            settings(true, true, Some(default_root()))
        );
        assert!(load_settings(&pool, true, &known).await.unwrap().enabled);
        assert!(
            !change_enabled(&pool, true, &known, &FakeService::ok(), false)
                .await
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn unavailable_settings_are_never_enabled_and_cannot_be_changed() {
        let pool = test_pool().await;
        let known = known();
        db::set_virtual_library_enabled(&pool, true).await.unwrap();

        assert_eq!(
            load_settings(&pool, false, &known).await.unwrap(),
            settings(false, false, None)
        );
        assert_eq!(
            change_enabled(&pool, false, &known, &FakeService::ok(), true).await,
            Err(UNAVAILABLE_MESSAGE.to_string())
        );
        assert_eq!(
            change_root_folder(&pool, false, &known, &default_root().to_string_lossy()).await,
            Err(UNAVAILABLE_MESSAGE.to_string())
        );
    }

    #[test]
    fn settings_serialise_with_the_field_names_the_frontend_contract_expects() {
        assert_eq!(
            serde_json::to_value(VirtualLibrarySettings {
                available: true,
                enabled: false,
                root_folder: Some("C:\\Pictures\\Lymic".to_string()),
            })
            .unwrap(),
            serde_json::json!({
                "available": true,
                "enabled": false,
                "root_folder": "C:\\Pictures\\Lymic"
            })
        );
    }

    #[tokio::test]
    async fn the_root_defaults_to_lymic_in_the_known_folder_pictures_and_can_be_changed() {
        let pool = test_pool().await;
        let known = known();
        let chosen = base().join("Elsewhere");

        let changed = change_root_folder(&pool, true, &known, &chosen.to_string_lossy())
            .await
            .unwrap();

        assert_eq!(
            changed,
            settings(true, false, Some(canonicalize_folder_path(&chosen)))
        );
        assert_eq!(load_settings(&pool, true, &known).await.unwrap(), changed);
    }

    #[tokio::test]
    async fn there_is_no_root_until_one_is_chosen_when_pictures_cannot_be_resolved() {
        let pool = test_pool().await;
        let unknown = FixedKnownFolders(None);

        assert_eq!(
            load_settings(&pool, true, &unknown).await.unwrap(),
            settings(true, false, None)
        );
        assert_eq!(
            change_enabled(&pool, true, &unknown, &FakeService::ok(), true).await,
            Err(NO_ROOT_MESSAGE.to_string())
        );
        assert!(!db::get_virtual_library_enabled(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn an_invalid_root_choice_is_refused_and_leaves_the_root_unchanged() {
        let pool = test_pool().await;
        let known = known();

        assert!(change_root_folder(&pool, true, &known, "   ")
            .await
            .is_err());
        assert!(change_root_folder(&pool, true, &known, "relative/folder")
            .await
            .is_err());
        let file = std::env::temp_dir().join(format!("lymic-vl-file-{}", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"x").unwrap();
        let error = change_root_folder(&pool, true, &known, &file.to_string_lossy())
            .await
            .unwrap_err();
        std::fs::remove_file(&file).unwrap();

        assert!(error.contains("is not a folder"), "{error}");
        assert_eq!(
            load_settings(&pool, true, &known)
                .await
                .unwrap()
                .root_folder,
            Some(default_root().to_string_lossy().into_owned())
        );
    }

    #[tokio::test]
    async fn enabling_is_refused_while_a_watched_folder_overlaps_the_root_and_names_it() {
        let known = known();
        for (watched, relation) in [
            (pictures(), "contains the Virtual Library folder"),
            (default_root(), "is the Virtual Library folder"),
            (
                default_root().join("Albums"),
                "is inside the Virtual Library folder",
            ),
        ] {
            let pool = test_pool().await;
            watch(&pool, &watched).await;

            let error = change_enabled(&pool, true, &known, &FakeService::ok(), true)
                .await
                .unwrap_err();

            assert!(
                error.contains(&format!("'{}'", watched.display())),
                "names the folder: {error}"
            );
            assert!(error.contains(relation), "{error}");
            assert!(!db::get_virtual_library_enabled(&pool).await.unwrap());
        }
    }

    #[tokio::test]
    async fn an_unrelated_watched_folder_does_not_block_enabling_and_disabling_is_never_refused() {
        let pool = test_pool().await;
        let known = known();
        watch(&pool, &pictures().join("Holiday")).await;

        assert!(
            change_enabled(&pool, true, &known, &FakeService::ok(), true)
                .await
                .unwrap()
                .enabled
        );
        watch(&pool, &pictures()).await;
        assert!(
            !change_enabled(&pool, true, &known, &FakeService::ok(), false)
                .await
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn enabling_names_every_overlapping_watched_folder() {
        let pool = test_pool().await;
        let inside = default_root().join("Albums");
        watch(&pool, &pictures()).await;
        watch(&pool, &inside).await;

        let error = change_enabled(&pool, true, &known(), &FakeService::ok(), true)
            .await
            .unwrap_err();

        assert!(
            error.contains(&format!("'{}'", pictures().display())),
            "{error}"
        );
        assert!(
            error.contains(&format!("'{}'", inside.display())),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_root_that_overlaps_a_watched_folder_is_refused_and_names_it() {
        let pool = test_pool().await;
        let known = known();
        let watched = base().join("Camera");
        watch(&pool, &watched).await;

        let error = change_root_folder(
            &pool,
            true,
            &known,
            &watched.join("Lymic").to_string_lossy(),
        )
        .await
        .unwrap_err();

        assert!(
            error.contains(&format!("'{}'", watched.display())),
            "{error}"
        );
        assert_eq!(
            load_settings(&pool, true, &known)
                .await
                .unwrap()
                .root_folder,
            Some(default_root().to_string_lossy().into_owned())
        );
    }

    #[tokio::test]
    async fn enabling_registers_and_serves_the_effective_root_then_remembers_the_choice() {
        let pool = test_pool().await;
        let service = FakeService::ok();

        let enabled = change_enabled(&pool, true, &known(), &service, true)
            .await
            .unwrap();

        assert_eq!(service.started(), vec![default_root()]);
        assert!(enabled.enabled);
        assert!(db::get_virtual_library_enabled(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn a_root_inside_another_sync_root_is_refused_with_the_nested_code_and_not_remembered() {
        let pool = test_pool().await;
        let onedrive = base().join("OneDrive");
        let service = FakeService::refusing(ActivationError::NestedSyncRoot(
            crate::cloud_files::NestedRoot {
                relation: crate::cloud_files::NestedRelation::Inside,
                root: onedrive.clone(),
            },
        ));

        let error = change_enabled(&pool, true, &known(), &service, true)
            .await
            .unwrap_err();

        assert_eq!(
            error,
            format!("{NESTED_SYNC_ROOT_ERROR}: {}", onedrive.display())
        );
        assert!(!db::get_virtual_library_enabled(&pool).await.unwrap());
        assert_eq!(service.stops(), 0);
    }

    #[tokio::test]
    async fn another_failure_to_start_is_reported_and_not_remembered() {
        let pool = test_pool().await;
        let service = FakeService::refusing(ActivationError::Register("0x80070490".to_string()));

        let error = change_enabled(&pool, true, &known(), &service, true)
            .await
            .unwrap_err();

        assert!(error.starts_with(START_FAILED_ERROR), "{error}");
        assert!(error.contains("0x80070490"), "{error}");
        assert!(!db::get_virtual_library_enabled(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn a_watched_folder_overlap_is_refused_before_anything_is_registered() {
        let pool = test_pool().await;
        watch(&pool, &pictures()).await;
        let service = FakeService::ok();

        change_enabled(&pool, true, &known(), &service, true)
            .await
            .unwrap_err();

        assert!(service.started().is_empty());
    }

    #[tokio::test]
    async fn disabling_remembers_the_choice_and_stops_serving() {
        let pool = test_pool().await;
        let service = FakeService::ok();
        change_enabled(&pool, true, &known(), &service, true)
            .await
            .unwrap();

        let disabled = change_enabled(&pool, true, &known(), &service, false)
            .await
            .unwrap();

        assert!(!disabled.enabled);
        assert_eq!(service.stops(), 1);
    }

    #[tokio::test]
    async fn an_unavailable_virtual_library_never_starts_the_service() {
        let pool = test_pool().await;
        let service = FakeService::ok();

        assert!(change_enabled(&pool, false, &known(), &service, true)
            .await
            .is_err());

        assert!(service.started().is_empty());
    }

    #[tokio::test]
    async fn the_folder_cannot_be_changed_while_the_virtual_library_is_enabled() {
        let pool = test_pool().await;
        let known = known();
        change_enabled(&pool, true, &known, &FakeService::ok(), true)
            .await
            .unwrap();

        let error = change_root_folder(
            &pool,
            true,
            &known,
            &base().join("Elsewhere").to_string_lossy(),
        )
        .await
        .unwrap_err();

        assert!(error.starts_with(DISABLE_TO_CHANGE_FOLDER_ERROR), "{error}");
        assert_eq!(
            load_settings(&pool, true, &known)
                .await
                .unwrap()
                .root_folder,
            Some(default_root().to_string_lossy().into_owned())
        );
    }

    #[tokio::test]
    async fn app_start_serves_again_only_when_available_and_enabled() {
        let known = known();

        let disabled = test_pool().await;
        let service = FakeService::ok();
        restore_with(&disabled, true, &known, &service)
            .await
            .unwrap();
        assert!(service.started().is_empty(), "not enabled");

        let enabled = test_pool().await;
        db::set_virtual_library_enabled(&enabled, true)
            .await
            .unwrap();
        restore_with(&enabled, false, &known, &service)
            .await
            .unwrap();
        assert!(service.started().is_empty(), "not available");

        restore_with(&enabled, true, &known, &service)
            .await
            .unwrap();
        assert_eq!(service.started(), vec![default_root()]);
    }

    #[tokio::test]
    async fn a_failing_restore_reports_the_reason() {
        let pool = test_pool().await;
        db::set_virtual_library_enabled(&pool, true).await.unwrap();
        let service = FakeService::refusing(ActivationError::Connect("denied".to_string()));

        let error = restore_with(&pool, true, &known(), &service)
            .await
            .unwrap_err();

        assert!(error.contains("denied"), "{error}");
    }

    #[test]
    fn a_notice_becomes_one_keyed_log_entry_and_one_event_for_the_banner() {
        use crate::app::events::CapturedUiLogs;
        use tauri::Listener;

        let app = tauri::test::mock_app();
        let logs = CapturedUiLogs::listen(app.handle());
        let events = std::sync::Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let captured = events.clone();
        app.handle().listen(NOTICE_EVENT, move |event| {
            captured
                .lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        });
        let sink = AppNoticeSink::new(app.handle().clone());

        sink.show(&Notice {
            kind: NoticeKind::Unauthorized,
            name: "IMG_1.jpg".into(),
        });

        let entries = logs.with_key("log_virtual_library_unauthorized");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["level"], "WARN");
        assert_eq!(entries[0]["params"]["name"], "IMG_1.jpg");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [serde_json::json!({ "kind": "unauthorized", "name": "IMG_1.jpg" })]
        );
    }
}
