//! The thumbnail cache the shell extension serves from (ADR-0003, #282).
//!
//! The Lymic app fills it, the provider only reads it. Both sides identify a placeholder by its
//! *file identity*, the bytes the Cloud Files platform stores with it and hands to whoever asks
//! (`CfGetPlaceholderInfo`), so the provider never has to know what the identity means: it hashes
//! the bytes to a file name and looks there. Whatever the app puts under that name is what Explorer
//! shows.
//!
//! The provider must never cause Hydration, so a miss is an error ([`Unavailable`]), never a reason
//! to fetch anything or to read the placeholder. Explorer then shows its generic icon until the app
//! has cached a thumbnail.
//!
//! Everything here is plain file I/O and runs on every platform.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The largest thumbnail kept. Immich's `thumbnail` size is a few tens of kilobytes; this only
/// guards against a runaway response filling the disk.
pub const MAX_THUMBNAIL_BYTES: usize = 8 * 1024 * 1024;

/// Extension of a cached thumbnail. Anything else in the directory is left alone by [`ThumbnailCache::trim`].
const EXTENSION: &str = "thumb";

/// Why there is nothing to show for a placeholder. Every case ends the request with an error; none
/// of them is a reason to touch the placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The placeholder carries no identity (not one of Lymic's, or the root).
    NoIdentity,
    /// The app has not cached a thumbnail for it (yet).
    NotCached,
    /// A cached file exists but cannot be used (empty, oversized, unreadable).
    Unusable,
}

/// Why a thumbnail was not stored.
#[derive(Debug)]
pub enum StoreError {
    /// The identity is empty.
    NoIdentity,
    /// The bytes are not a JPEG, PNG, WebP or GIF image (an error page, say).
    NotAnImage,
    /// More than [`MAX_THUMBNAIL_BYTES`].
    TooLarge,
    Io(io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoIdentity => f.write_str("the placeholder has no identity"),
            Self::NotAnImage => f.write_str("the bytes are not a supported image"),
            Self::TooLarge => f.write_str("the thumbnail is too large"),
            Self::Io(error) => write!(f, "the thumbnail could not be written: {error}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// The file name stem a placeholder identity maps to: lowercase hex SHA-256 of the identity bytes.
///
/// A hash, not the identity itself: the identity is a path with arbitrary characters and up to
/// 4096 bytes, which is not a safe or short file name.
pub fn key(identity: &[u8]) -> String {
    Sha256::digest(identity)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `bytes` start like an image the provider can decode: JPEG, PNG, WebP or GIF.
pub fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
}

/// The directory below the package's local state folder that holds the cache. Both the app and the
/// provider derive the location from the same folder, so they meet without talking to each other.
pub fn directory_under(local_state: &Path) -> PathBuf {
    local_state.join("shell-thumbnails")
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory of cached thumbnails.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    directory: PathBuf,
}

impl ThumbnailCache {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path_of(&self, identity: &[u8]) -> PathBuf {
        self.directory
            .join(format!("{}.{EXTENSION}", key(identity)))
    }

    /// Whether a thumbnail is cached for the placeholder.
    pub fn contains(&self, identity: &[u8]) -> bool {
        !identity.is_empty() && self.path_of(identity).is_file()
    }

    /// The cached image for the placeholder. Reads the cache file only.
    pub fn load(&self, identity: &[u8]) -> Result<Vec<u8>, Unavailable> {
        if identity.is_empty() {
            return Err(Unavailable::NoIdentity);
        }
        let path = self.path_of(identity);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(Unavailable::NotCached)
            }
            Err(_) => return Err(Unavailable::Unusable),
        };
        if bytes.is_empty() || bytes.len() > MAX_THUMBNAIL_BYTES {
            return Err(Unavailable::Unusable);
        }
        Ok(bytes)
    }

    /// Caches `image` for the placeholder, replacing an earlier one. The file appears whole or not
    /// at all, so a provider reading at the same moment never sees half an image.
    pub fn store(&self, identity: &[u8], image: &[u8]) -> Result<(), StoreError> {
        if identity.is_empty() {
            return Err(StoreError::NoIdentity);
        }
        if image.len() > MAX_THUMBNAIL_BYTES {
            return Err(StoreError::TooLarge);
        }
        if !looks_like_image(image) {
            return Err(StoreError::NotAnImage);
        }
        fs::create_dir_all(&self.directory).map_err(StoreError::Io)?;
        let target = self.path_of(identity);
        let temporary = self.directory.join(format!(
            "{}.{}-{}.tmp",
            key(identity),
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&temporary, image).map_err(StoreError::Io)?;
        fs::rename(&temporary, &target).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            StoreError::Io(error)
        })
    }

    /// Deletes the oldest thumbnails until what is left fits in `limit_bytes`. Returns how many
    /// were removed. The age is the time of writing, so this is first in, first out; a thumbnail
    /// that is still wanted is written again the next time its directory is listed.
    pub fn trim(&self, limit_bytes: u64) -> io::Result<usize> {
        let mut files = Vec::new();
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let mut total: u64 = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            total += metadata.len();
            let written = metadata.modified().ok();
            files.push((written, path, metadata.len()));
        }
        if total <= limit_bytes {
            return Ok(0);
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let mut removed = 0;
        for (_, path, size) in files {
            if total <= limit_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= size;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Deletes every cached thumbnail and the directory. Nothing to delete is fine. Used when the
    /// Virtual Library is disabled or the user logs out, so nothing of the account stays behind.
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.directory) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// A directory under the system temp folder, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "lymic-shell-ext-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F'];

    fn cache(dir: &TempDir) -> ThumbnailCache {
        ThumbnailCache::new(dir.0.join("shell-thumbnails"))
    }

    fn age(cache: &ThumbnailCache, identity: &[u8], seconds_ago: u64) {
        let file = fs::File::options()
            .write(true)
            .open(cache.path_of(identity))
            .unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
            .unwrap();
    }

    #[test]
    fn the_key_is_the_sha256_of_the_identity_in_lowercase_hex() {
        // Known answers (FIPS 180-2 test vectors), not recomputed with the code under test.
        assert_eq!(
            key(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            key(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn different_identities_get_different_keys_and_the_same_one_the_same_key() {
        let path = "Albums/Urlaub \u{2600}/IMG_0001 (a1b2c3).jpg".as_bytes();

        assert_eq!(key(path), key(path));
        assert_ne!(key(path), key(b"Albums/Urlaub/IMG_0001.jpg"));
    }

    #[test]
    fn a_stored_thumbnail_is_served_back_unchanged() {
        let dir = TempDir::new();
        let cache = cache(&dir);
        let identity = b"All Photos/2024/03 March/IMG_0001.jpg";

        cache.store(identity, JPEG).unwrap();

        assert_eq!(cache.load(identity).unwrap(), JPEG);
        assert!(cache.contains(identity));
    }

    #[test]
    fn a_placeholder_without_a_cached_thumbnail_is_unavailable_not_fetched() {
        let dir = TempDir::new();
        let cache = cache(&dir);

        assert_eq!(cache.load(b"All Photos/x.jpg"), Err(Unavailable::NotCached));
        assert!(!cache.contains(b"All Photos/x.jpg"));
    }

    #[test]
    fn a_placeholder_without_an_identity_is_unavailable() {
        let dir = TempDir::new();
        let cache = cache(&dir);

        assert_eq!(cache.load(b""), Err(Unavailable::NoIdentity));
        assert!(matches!(cache.store(b"", PNG), Err(StoreError::NoIdentity)));
    }

    #[test]
    fn storing_again_replaces_the_thumbnail() {
        let dir = TempDir::new();
        let cache = cache(&dir);

        cache.store(b"a/b.jpg", JPEG).unwrap();
        cache.store(b"a/b.jpg", PNG).unwrap();

        assert_eq!(cache.load(b"a/b.jpg").unwrap(), PNG);
        let leftovers = fs::read_dir(&cache.directory).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file stays behind");
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused() {
        let dir = TempDir::new();
        let cache = cache(&dir);

        assert!(matches!(
            cache.store(b"a.jpg", b"{\"message\":\"Not found\"}"),
            Err(StoreError::NotAnImage)
        ));
        assert!(matches!(
            cache.store(b"a.jpg", b""),
            Err(StoreError::NotAnImage)
        ));
        assert_eq!(cache.load(b"a.jpg"), Err(Unavailable::NotCached));
    }

    #[test]
    fn jpeg_png_webp_and_gif_are_images() {
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0x10, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPVP8 ");

        assert!(looks_like_image(JPEG));
        assert!(looks_like_image(PNG));
        assert!(looks_like_image(&webp));
        assert!(looks_like_image(b"GIF89a\x01\x00"));
        assert!(!looks_like_image(b"RIFF\x10\0\0\0WAVEfmt "));
        assert!(!looks_like_image(b"<html>"));
    }

    #[test]
    fn a_thumbnail_over_the_size_limit_is_refused() {
        let dir = TempDir::new();
        let cache = cache(&dir);
        let mut huge = JPEG.to_vec();
        huge.resize(MAX_THUMBNAIL_BYTES + 1, 0);

        assert!(matches!(
            cache.store(b"a.jpg", &huge),
            Err(StoreError::TooLarge)
        ));
    }

    #[test]
    fn a_cache_file_that_is_empty_is_unusable() {
        let dir = TempDir::new();
        let cache = cache(&dir);
        cache.store(b"a.jpg", JPEG).unwrap();
        fs::write(cache.path_of(b"a.jpg"), b"").unwrap();

        assert_eq!(cache.load(b"a.jpg"), Err(Unavailable::Unusable));
    }

    #[test]
    fn trimming_removes_the_oldest_thumbnails_until_the_rest_fits() {
        let dir = TempDir::new();
        let cache = cache(&dir);
        let mut image = JPEG.to_vec();
        image.resize(100, 7);
        for name in [&b"old"[..], b"middle", b"new"] {
            cache.store(name, &image).unwrap();
        }
        age(&cache, b"old", 300);
        age(&cache, b"middle", 200);
        age(&cache, b"new", 100);

        let removed = cache.trim(250).unwrap();

        assert_eq!(removed, 1);
        assert_eq!(cache.load(b"old"), Err(Unavailable::NotCached));
        assert!(cache.contains(b"middle"));
        assert!(cache.contains(b"new"));
    }

    #[test]
    fn trimming_within_the_limit_or_without_a_directory_removes_nothing() {
        let dir = TempDir::new();
        let cache = cache(&dir);
        assert_eq!(cache.trim(0).unwrap(), 0);

        cache.store(b"a.jpg", JPEG).unwrap();
        assert_eq!(cache.trim(1_000_000).unwrap(), 0);
        assert!(cache.contains(b"a.jpg"));
    }

    #[test]
    fn clearing_removes_every_thumbnail_and_works_when_there_is_nothing() {
        let dir = TempDir::new();
        let cache = cache(&dir);
        cache.clear().unwrap();

        cache.store(b"a.jpg", JPEG).unwrap();
        cache.store(b"b.jpg", PNG).unwrap();
        cache.clear().unwrap();

        assert!(!cache.contains(b"a.jpg"));
        assert!(!cache.contains(b"b.jpg"));
        assert!(!cache.directory.exists());
    }

    #[test]
    fn the_cache_directory_is_below_the_local_state_folder() {
        assert_eq!(
            directory_under(Path::new("local-state")),
            Path::new("local-state").join("shell-thumbnails")
        );
    }
}
