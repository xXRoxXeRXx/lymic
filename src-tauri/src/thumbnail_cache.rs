use crate::immich::{ImmichClient, RemoteAssetId};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};
use tauri::Manager;

const THUMBNAIL_EXTENSIONS: &[&str] = &["jpg", "png", "webp", "avif", "gif"];

pub(crate) fn local_thumbnail_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("cache").join("thumbnails"))
        .map_err(|error| format!("Could not locate local app data: {error}"))
}

#[derive(Default)]
struct AccessState {
    clock: u64,
    timestamps: HashMap<PathBuf, u64>,
}

/// Disk cache for image-only Immich thumbnail responses.
///
/// The cache root is deliberately supplied independently from the Virtual Library root.
pub(crate) struct ThumbnailCache {
    root: PathBuf,
    max_bytes: u64,
    access: Mutex<AccessState>,
}

impl ThumbnailCache {
    pub(crate) fn new(root: PathBuf, max_bytes: u64) -> Self {
        Self {
            root,
            max_bytes,
            access: Mutex::new(AccessState::default()),
        }
    }

    pub(crate) fn get(&self, asset_id: &str, revision: &str) -> Option<PathBuf> {
        let prefix = self.cache_key(asset_id, revision);
        let path = fs::read_dir(&self.root)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem == prefix)
                    && is_thumbnail_file(path)
            })?;
        // Updating the timestamp by rewriting the small cached image gives a durable LRU order.
        let bytes = fs::read(&path).ok()?;
        fs::write(&path, bytes).ok()?;
        self.touch(&path);
        Some(path)
    }

    pub(crate) async fn get_or_fetch(
        &self,
        client: &ImmichClient,
        asset_id: &RemoteAssetId,
        revision: &str,
    ) -> Result<PathBuf, String> {
        if let Some(path) = self.get(asset_id.as_str(), revision) {
            return Ok(path);
        }
        let thumbnail = client.fetch_thumbnail(asset_id).await?;
        self.store(
            asset_id.as_str(),
            revision,
            &thumbnail.mime_type,
            &thumbnail.bytes,
        )
    }

    pub(crate) fn store(
        &self,
        asset_id: &str,
        revision: &str,
        mime_type: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, String> {
        let extension = extension_for_mime_type(mime_type)
            .ok_or_else(|| "Immich thumbnail response was not an image".to_string())?;
        fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;

        let asset_prefix = asset_key(asset_id);
        for entry in fs::read_dir(&self.root)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&asset_prefix))
                && is_thumbnail_file(&path)
            {
                let _ = fs::remove_file(path);
            }
        }

        let path = self.root.join(format!(
            "{}.{}",
            self.cache_key(asset_id, revision),
            extension
        ));
        fs::write(&path, bytes).map_err(|error| error.to_string())?;
        self.touch(&path);
        self.evict_to_cap()?;
        Ok(path)
    }

    pub(crate) fn clear(&self) -> Result<(), String> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.root)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            let path = entry.path();
            if is_thumbnail_file(&path) {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
            }
        }
        self.access
            .lock()
            .map_err(|_| "Thumbnail cache lock was poisoned".to_string())?
            .timestamps
            .clear();
        Ok(())
    }

    /// Removes every cached revision for one Remote Asset.
    pub(crate) fn remove_asset(&self, asset_id: &str) -> Result<(), String> {
        if !self.root.exists() {
            return Ok(());
        }
        let prefix = asset_key(asset_id);
        for entry in fs::read_dir(&self.root)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
                && is_thumbnail_file(&path)
            {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
                if let Ok(mut access) = self.access.lock() {
                    access.timestamps.remove(&path);
                }
            }
        }
        Ok(())
    }

    fn cache_key(&self, asset_id: &str, revision: &str) -> String {
        format!("{}-{}", asset_key(asset_id), digest(revision))
    }

    fn touch(&self, path: &Path) {
        if let Ok(mut access) = self.access.lock() {
            let now = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| {
                    duration.as_nanos().min(u128::from(u64::MAX)) as u64
                });
            access.clock = access.clock.max(now).saturating_add(1);
            let clock = access.clock;
            access.timestamps.insert(path.to_path_buf(), clock);
        }
    }

    fn evict_to_cap(&self) -> Result<(), String> {
        let mut entries = Vec::new();
        let mut total = 0_u64;
        let access = self
            .access
            .lock()
            .map_err(|_| "Thumbnail cache lock was poisoned".to_string())?;
        for entry in fs::read_dir(&self.root)
            .map_err(|error| error.to_string())?
            .flatten()
        {
            let path = entry.path();
            if !is_thumbnail_file(&path) {
                continue;
            }
            let metadata = entry.metadata().map_err(|error| error.to_string())?;
            let fallback = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| {
                    duration.as_nanos().min(u128::from(u64::MAX)) as u64
                });
            total = total.saturating_add(metadata.len());
            entries.push((
                access.timestamps.get(&path).copied().unwrap_or(fallback),
                path,
                metadata.len(),
            ));
        }
        drop(access);
        entries.sort_by_key(|(accessed, _, _)| *accessed);
        for (_, path, length) in entries {
            if total <= self.max_bytes {
                break;
            }
            fs::remove_file(&path).map_err(|error| error.to_string())?;
            total = total.saturating_sub(length);
            if let Ok(mut access) = self.access.lock() {
                access.timestamps.remove(&path);
            }
        }
        Ok(())
    }
}

fn asset_key(asset_id: &str) -> String {
    digest(asset_id)
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(crate) fn extension_for_mime_type(mime_type: &str) -> Option<&'static str> {
    match mime_type
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/avif" => Some("avif"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn is_thumbnail_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                THUMBNAIL_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            })
}

#[cfg(test)]
mod tests {
    use super::ThumbnailCache;
    use std::fs;
    use uuid::Uuid;

    fn cache() -> (ThumbnailCache, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("lymic-thumbnail-cache-test-{}", Uuid::new_v4()));
        (ThumbnailCache::new(root.clone(), 1_000_000), root)
    }

    #[test]
    fn replaces_a_cached_thumbnail_when_its_asset_revision_changes() {
        let (cache, root) = cache();

        let first = cache
            .store("asset-1", "v1", "image/jpeg", b"first")
            .unwrap();
        let second = cache
            .store("asset-1", "v2", "image/jpeg", b"second")
            .unwrap();

        assert!(!first.exists());
        assert_eq!(fs::read(second).unwrap(), b"second");
        assert_eq!(cache.get("asset-1", "v1"), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn evicts_the_least_recently_used_thumbnail_to_respect_its_size_cap() {
        let (_, root) = cache();
        let cache = ThumbnailCache::new(root.clone(), 8);

        let oldest = cache.store("asset-1", "v1", "image/jpeg", b"one").unwrap();
        let retained = cache.store("asset-2", "v1", "image/jpeg", b"two").unwrap();
        assert!(cache.get("asset-1", "v1").is_some());
        let newest = cache
            .store("asset-3", "v1", "image/jpeg", b"three")
            .unwrap();

        assert!(oldest.exists());
        assert!(!retained.exists());
        assert!(newest.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removes_every_cached_revision_for_a_deleted_remote_asset() {
        let (cache, root) = cache();
        let first = cache.store("deleted", "v1", "image/jpeg", b"one").unwrap();
        let other = cache.store("retained", "v1", "image/jpeg", b"two").unwrap();

        cache.remove_asset("deleted").unwrap();

        assert!(!first.exists());
        assert!(other.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
