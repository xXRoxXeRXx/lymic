use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const AVATAR_FILE_NAME: &str = "profile_image.bin";
const METADATA_FILE_NAME: &str = "profile_meta.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CachedProfileMeta {
    pub user_id: String,
    pub display_name: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CurrentUser {
    pub display_name: String,
    pub profile_image: Option<String>,
}

pub(crate) fn load_cached_profile(cache_dir: &Path) -> Option<CurrentUser> {
    let meta_path = cache_dir.join(METADATA_FILE_NAME);
    let meta_bytes = fs::read(&meta_path).ok()?;
    let meta: CachedProfileMeta = serde_json::from_slice(&meta_bytes).ok()?;

    let image_path = cache_dir.join(AVATAR_FILE_NAME);
    let image_bytes = fs::read(&image_path).ok();

    let profile_image = image_bytes.map(|bytes| to_data_url(&bytes, meta.content_type.as_deref()));

    Some(CurrentUser {
        display_name: meta.display_name,
        profile_image,
    })
}

pub(crate) fn save_cached_profile(
    cache_dir: &Path,
    user_id: &str,
    display_name: &str,
    image_bytes: Option<&[u8]>,
    content_type: Option<&str>,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(cache_dir)?;

    let meta = CachedProfileMeta {
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        content_type: content_type.map(str::to_string),
    };
    let meta_json = serde_json::to_vec(&meta)?;
    fs::write(cache_dir.join(METADATA_FILE_NAME), meta_json)?;

    let image_path = cache_dir.join(AVATAR_FILE_NAME);
    match image_bytes {
        Some(bytes) => {
            fs::write(image_path, bytes)?;
        }
        None => {
            if image_path.exists() {
                let _ = fs::remove_file(image_path);
            }
        }
    }

    Ok(())
}

pub(crate) fn clear_profile_cache(cache_dir: &Path) -> Result<(), std::io::Error> {
    if cache_dir.exists() {
        let _ = fs::remove_file(cache_dir.join(AVATAR_FILE_NAME));
        let _ = fs::remove_file(cache_dir.join(METADATA_FILE_NAME));
        let _ = fs::remove_dir(cache_dir);
    }
    Ok(())
}

pub(crate) fn to_data_url(bytes: &[u8], content_type: Option<&str>) -> String {
    let mime = content_type.unwrap_or_else(|| detect_mime_type(bytes));
    let encoded = general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{encoded}")
}

fn detect_mime_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("lymic-profile-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saves_and_loads_profile_cache_with_image() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let fake_image = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";

        save_cached_profile(
            &cache_dir,
            "user-1",
            "Meyer",
            Some(fake_image),
            Some("image/jpeg"),
        )
        .unwrap();

        let loaded = load_cached_profile(&cache_dir).expect("should load cached profile");
        assert_eq!(loaded.display_name, "Meyer");
        assert!(loaded.profile_image.is_some());
        let data_url = loaded.profile_image.unwrap();
        assert!(data_url.starts_with("data:image/jpeg;base64,"));

        // Verify image bytes were stored in replaceable file, not SQLite
        let raw_file = cache_dir.join(AVATAR_FILE_NAME);
        assert!(raw_file.exists());
        assert_eq!(fs::read(raw_file).unwrap(), fake_image);
    }

    #[test]
    fn replaces_changed_image() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let image_v1 = b"\xFF\xD8\xFFv1";
        let image_v2 = b"\x89PNGv2";

        save_cached_profile(
            &cache_dir,
            "user-1",
            "Meyer",
            Some(image_v1),
            Some("image/jpeg"),
        )
        .unwrap();

        let loaded_v1 = load_cached_profile(&cache_dir).unwrap();
        assert!(loaded_v1
            .profile_image
            .unwrap()
            .starts_with("data:image/jpeg"));

        // New image arrives
        save_cached_profile(
            &cache_dir,
            "user-1",
            "Meyer",
            Some(image_v2),
            Some("image/png"),
        )
        .unwrap();

        let loaded_v2 = load_cached_profile(&cache_dir).unwrap();
        let data_url_v2 = loaded_v2.profile_image.unwrap();
        assert!(data_url_v2.starts_with("data:image/png"));
        assert_eq!(
            fs::read(cache_dir.join(AVATAR_FILE_NAME)).unwrap(),
            image_v2
        );
    }

    #[test]
    fn removes_cached_image_when_user_has_no_profile_image() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let fake_image = b"\xFF\xD8\xFF";

        save_cached_profile(&cache_dir, "user-1", "Meyer", Some(fake_image), None).unwrap();
        assert!(cache_dir.join(AVATAR_FILE_NAME).exists());

        // Now user has no image in Immich
        save_cached_profile(&cache_dir, "user-1", "Meyer", None, None).unwrap();
        assert!(!cache_dir.join(AVATAR_FILE_NAME).exists());

        let loaded = load_cached_profile(&cache_dir).unwrap();
        assert_eq!(loaded.display_name, "Meyer");
        assert_eq!(loaded.profile_image, None);
    }

    #[test]
    fn clear_profile_cache_removes_all_cached_files() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let fake_image = b"\xFF\xD8\xFF";

        save_cached_profile(&cache_dir, "user-1", "Meyer", Some(fake_image), None).unwrap();
        assert!(cache_dir.exists());

        clear_profile_cache(&cache_dir).unwrap();
        assert!(!cache_dir.join(AVATAR_FILE_NAME).exists());
        assert!(!cache_dir.join(METADATA_FILE_NAME).exists());
        assert!(load_cached_profile(&cache_dir).is_none());
    }

    #[test]
    fn detects_mime_type_from_magic_bytes() {
        assert_eq!(detect_mime_type(b"\xFF\xD8\xFF\xE0"), "image/jpeg");
        assert_eq!(detect_mime_type(b"\x89PNG\r\n\x1a\n"), "image/png");
        assert_eq!(
            detect_mime_type(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            "image/webp"
        );
        assert_eq!(detect_mime_type(b"GIF89a"), "image/gif");
    }
}
