use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const AVATAR_FILE_NAME: &str = "profile_image.bin";
const METADATA_FILE_NAME: &str = "profile_meta.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CachedProfileMeta {
    pub account_id: String,
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

#[derive(Debug)]
pub(crate) enum ProfileFetchOutcome {
    SuccessWithImage {
        user_id: String,
        display_name: String,
        bytes: Vec<u8>,
        content_type: Option<String>,
    },
    SuccessNoImage {
        user_id: String,
        display_name: String,
    },
    ImageFetchFailed {
        user_id: String,
        display_name: String,
    },
    RefreshFailed,
}

pub(crate) fn load_cached_profile(
    cache_dir: &Path,
    expected_account_id: &str,
    expected_user_id: Option<&str>,
) -> Option<CurrentUser> {
    let meta_path = cache_dir.join(METADATA_FILE_NAME);
    let meta_bytes = fs::read(&meta_path).ok()?;
    let meta: CachedProfileMeta = serde_json::from_slice(&meta_bytes).ok()?;

    if meta.account_id != expected_account_id {
        return None;
    }
    if let Some(uid) = expected_user_id {
        if meta.user_id != uid {
            return None;
        }
    }

    let image_path = cache_dir.join(AVATAR_FILE_NAME);
    let image_bytes = fs::read(&image_path).ok();
    let profile_image = image_bytes
        .as_deref()
        .and_then(|bytes| to_data_url(bytes, meta.content_type.as_deref()));

    Some(CurrentUser {
        display_name: meta.display_name,
        profile_image,
    })
}

pub(crate) fn save_cached_profile(
    cache_dir: &Path,
    account_id: &str,
    user_id: &str,
    display_name: &str,
    image_bytes: Option<&[u8]>,
    content_type: Option<&str>,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(cache_dir)?;

    let image_path = cache_dir.join(AVATAR_FILE_NAME);
    match image_bytes {
        Some(bytes) => {
            let temp_image_path =
                cache_dir.join(format!("{AVATAR_FILE_NAME}.tmp.{}", uuid::Uuid::new_v4()));
            fs::write(&temp_image_path, bytes)?;
            fs::rename(&temp_image_path, &image_path)?;
        }
        None => {
            if image_path.exists() {
                let _ = fs::remove_file(&image_path);
            }
        }
    }

    let meta = CachedProfileMeta {
        account_id: account_id.to_string(),
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        content_type: content_type.map(str::to_string),
    };
    let meta_json = serde_json::to_vec(&meta).map_err(std::io::Error::other)?;
    let temp_meta_path =
        cache_dir.join(format!("{METADATA_FILE_NAME}.tmp.{}", uuid::Uuid::new_v4()));
    fs::write(&temp_meta_path, meta_json)?;
    fs::rename(&temp_meta_path, cache_dir.join(METADATA_FILE_NAME))?;

    Ok(())
}

pub(crate) fn clear_profile_cache(cache_dir: &Path) {
    if !cache_dir.exists() {
        return;
    }
    let avatar_path = cache_dir.join(AVATAR_FILE_NAME);
    if avatar_path.exists() {
        if let Err(e) = fs::remove_file(&avatar_path) {
            eprintln!("Warning: failed to remove cached profile image: {e}");
        }
    }
    let meta_path = cache_dir.join(METADATA_FILE_NAME);
    if meta_path.exists() {
        if let Err(e) = fs::remove_file(&meta_path) {
            eprintln!("Warning: failed to remove cached profile metadata: {e}");
        }
    }
    let _ = fs::remove_dir(cache_dir);
}

pub(crate) fn to_data_url(bytes: &[u8], _content_type: Option<&str>) -> Option<String> {
    let mime = detect_mime_type(bytes)?;
    let encoded = general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

pub(crate) fn detect_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

pub(crate) fn resolve_profile(
    cache_dir: &Path,
    account_id: &str,
    outcome: ProfileFetchOutcome,
) -> CurrentUser {
    match outcome {
        ProfileFetchOutcome::SuccessWithImage {
            user_id,
            display_name,
            bytes,
            content_type,
        } => {
            let data_url = to_data_url(&bytes, content_type.as_deref());
            let _ = save_cached_profile(
                cache_dir,
                account_id,
                &user_id,
                &display_name,
                if data_url.is_some() {
                    Some(&bytes)
                } else {
                    None
                },
                content_type.as_deref(),
            );
            CurrentUser {
                display_name,
                profile_image: data_url,
            }
        }
        ProfileFetchOutcome::SuccessNoImage {
            user_id,
            display_name,
        } => {
            let _ = save_cached_profile(cache_dir, account_id, &user_id, &display_name, None, None);
            CurrentUser {
                display_name,
                profile_image: None,
            }
        }
        ProfileFetchOutcome::ImageFetchFailed {
            user_id,
            display_name,
        } => {
            let cached = load_cached_profile(cache_dir, account_id, Some(&user_id));
            let profile_image = cached.and_then(|c| c.profile_image);
            CurrentUser {
                display_name,
                profile_image,
            }
        }
        ProfileFetchOutcome::RefreshFailed => load_cached_profile(cache_dir, account_id, None)
            .unwrap_or(CurrentUser {
                display_name: String::new(),
                profile_image: None,
            }),
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
            "account-A",
            "user-1",
            "Meyer",
            Some(fake_image),
            Some("image/jpeg"),
        )
        .unwrap();

        let loaded = load_cached_profile(&cache_dir, "account-A", Some("user-1"))
            .expect("should load cached profile");
        assert_eq!(loaded.display_name, "Meyer");
        assert!(loaded.profile_image.is_some());
        let data_url = loaded.profile_image.unwrap();
        assert!(data_url.starts_with("data:image/jpeg;base64,"));

        let raw_file = cache_dir.join(AVATAR_FILE_NAME);
        assert!(raw_file.exists());
        assert_eq!(fs::read(raw_file).unwrap(), fake_image);
    }

    #[test]
    fn enforces_account_and_user_isolation() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let fake_image = b"\xFF\xD8\xFF\xE0";

        save_cached_profile(
            &cache_dir,
            "account-A",
            "user-1",
            "Alice",
            Some(fake_image),
            Some("image/jpeg"),
        )
        .unwrap();

        // Query with different account
        assert!(load_cached_profile(&cache_dir, "account-B", None).is_none());
        assert!(load_cached_profile(&cache_dir, "account-B", Some("user-1")).is_none());

        // Query with same account but different user ID
        assert!(load_cached_profile(&cache_dir, "account-A", Some("user-2")).is_none());

        // Matching account succeeds
        assert!(load_cached_profile(&cache_dir, "account-A", Some("user-1")).is_some());
        assert!(load_cached_profile(&cache_dir, "account-A", None).is_some());
    }

    #[test]
    fn replaces_changed_image() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let image_v1 = b"\xFF\xD8\xFFv1";
        let image_v2 = b"\x89PNGv2";

        save_cached_profile(
            &cache_dir,
            "account-A",
            "user-1",
            "Meyer",
            Some(image_v1),
            Some("image/jpeg"),
        )
        .unwrap();

        let loaded_v1 = load_cached_profile(&cache_dir, "account-A", Some("user-1")).unwrap();
        assert!(loaded_v1
            .profile_image
            .unwrap()
            .starts_with("data:image/jpeg"));

        save_cached_profile(
            &cache_dir,
            "account-A",
            "user-1",
            "Meyer",
            Some(image_v2),
            Some("image/png"),
        )
        .unwrap();

        let loaded_v2 = load_cached_profile(&cache_dir, "account-A", Some("user-1")).unwrap();
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

        save_cached_profile(
            &cache_dir,
            "account-A",
            "user-1",
            "Meyer",
            Some(fake_image),
            None,
        )
        .unwrap();
        assert!(cache_dir.join(AVATAR_FILE_NAME).exists());

        save_cached_profile(&cache_dir, "account-A", "user-1", "Meyer", None, None).unwrap();
        assert!(!cache_dir.join(AVATAR_FILE_NAME).exists());

        let loaded = load_cached_profile(&cache_dir, "account-A", Some("user-1")).unwrap();
        assert_eq!(loaded.display_name, "Meyer");
        assert_eq!(loaded.profile_image, None);
    }

    #[test]
    fn clear_profile_cache_removes_all_cached_files() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let fake_image = b"\xFF\xD8\xFF";

        save_cached_profile(
            &cache_dir,
            "account-A",
            "user-1",
            "Meyer",
            Some(fake_image),
            None,
        )
        .unwrap();
        assert!(cache_dir.exists());

        clear_profile_cache(&cache_dir);
        assert!(!cache_dir.join(AVATAR_FILE_NAME).exists());
        assert!(!cache_dir.join(METADATA_FILE_NAME).exists());
        assert!(load_cached_profile(&cache_dir, "account-A", None).is_none());
    }

    #[test]
    fn detects_mime_type_only_for_known_formats() {
        assert_eq!(detect_mime_type(b"\xFF\xD8\xFF\xE0"), Some("image/jpeg"));
        assert_eq!(detect_mime_type(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(
            detect_mime_type(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(detect_mime_type(b"GIF89a"), Some("image/gif"));
        assert_eq!(detect_mime_type(b"unknown bytes"), None);
    }

    #[test]
    fn rejects_svg_payload_even_when_server_declares_an_image_content_type() {
        assert_eq!(
            to_data_url(
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
                Some("image/svg+xml")
            ),
            None
        );
    }

    #[test]
    fn resolve_profile_handles_success_with_image() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        let fake_image = b"\xFF\xD8\xFF\xE0";

        let outcome = ProfileFetchOutcome::SuccessWithImage {
            user_id: "user-1".to_string(),
            display_name: "Alice".to_string(),
            bytes: fake_image.to_vec(),
            content_type: Some("image/jpeg".to_string()),
        };

        let user = resolve_profile(&cache_dir, "acc-1", outcome);
        assert_eq!(user.display_name, "Alice");
        assert!(user.profile_image.unwrap().starts_with("data:image/jpeg"));

        // Verify persisted to cache
        let cached = load_cached_profile(&cache_dir, "acc-1", Some("user-1")).unwrap();
        assert_eq!(cached.display_name, "Alice");
        assert!(cached.profile_image.is_some());
    }

    #[test]
    fn resolve_profile_handles_success_no_image() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");

        // Seed with prior image
        save_cached_profile(
            &cache_dir,
            "acc-1",
            "user-1",
            "Alice",
            Some(b"\xFF\xD8\xFF"),
            None,
        )
        .unwrap();

        let outcome = ProfileFetchOutcome::SuccessNoImage {
            user_id: "user-1".to_string(),
            display_name: "Alice".to_string(),
        };

        let user = resolve_profile(&cache_dir, "acc-1", outcome);
        assert_eq!(user.display_name, "Alice");
        assert_eq!(user.profile_image, None);

        // Cached image should be gone
        let cached = load_cached_profile(&cache_dir, "acc-1", Some("user-1")).unwrap();
        assert_eq!(cached.profile_image, None);
    }

    #[test]
    fn resolve_profile_retains_cache_on_image_fetch_failure() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        save_cached_profile(
            &cache_dir,
            "acc-1",
            "user-1",
            "Alice",
            Some(b"\xFF\xD8\xFF"),
            Some("image/jpeg"),
        )
        .unwrap();

        let outcome = ProfileFetchOutcome::ImageFetchFailed {
            user_id: "user-1".to_string(),
            display_name: "Alice Updated".to_string(),
        };

        let user = resolve_profile(&cache_dir, "acc-1", outcome);
        assert_eq!(user.display_name, "Alice Updated");
        assert!(user.profile_image.is_some());
    }

    #[test]
    fn resolve_profile_falls_back_to_cache_on_refresh_failure_for_same_account() {
        let dir = TempDir::new();
        let cache_dir = dir.path().join("profile");
        save_cached_profile(
            &cache_dir,
            "acc-1",
            "user-1",
            "Alice",
            Some(b"\xFF\xD8\xFF"),
            Some("image/jpeg"),
        )
        .unwrap();

        // Refresh fails for same account
        let user = resolve_profile(&cache_dir, "acc-1", ProfileFetchOutcome::RefreshFailed);
        assert_eq!(user.display_name, "Alice");
        assert!(user.profile_image.is_some());

        // Refresh fails for DIFFERENT account -> must NOT cross accounts!
        let user_diff = resolve_profile(&cache_dir, "acc-2", ProfileFetchOutcome::RefreshFailed);
        assert_eq!(user_diff.display_name, "");
        assert_eq!(user_diff.profile_image, None);
    }
}
