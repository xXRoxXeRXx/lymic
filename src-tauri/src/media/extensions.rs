use std::path::{Path, PathBuf};

pub(crate) const MEDIA_EXTENSIONS: &[&str] = &[
    "3fr", "3gp", "3gpp", "ari", "arw", "avif", "avi", "bmp", "cap", "cin", "cr2", "cr3", "crw",
    "dcr", "dng", "erf", "fff", "flv", "gif", "heic", "heif", "hif", "iiq", "insp", "jfif", "jp2",
    "jpe", "jpeg", "jpg", "jxl", "k25", "kdc", "m2t", "m2ts", "m4v", "mkv", "mov", "mp4", "mpe",
    "mpeg", "mpg", "mpo", "mrw", "mts", "mxf", "nef", "nrw", "orf", "ori", "pef", "png", "psd",
    "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "svg", "tif", "tiff", "ts", "vob", "webm",
    "webp", "wmv", "x3f",
];
const LIVE_PHOTO_IMAGE_EXTENSIONS: &[&str] = &["heic", "heif"];
const LIVE_PHOTO_VIDEO_EXTENSIONS: &[&str] = &["mov"];
const MOTION_PHOTO_IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg"];
const MOTION_PHOTO_VIDEO_EXTENSIONS: &[&str] = &["mp4"];

pub(crate) fn is_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn live_photo_kind(path: &Path) -> Option<bool> {
    if has_extension(path, LIVE_PHOTO_IMAGE_EXTENSIONS)
        || has_extension(path, MOTION_PHOTO_IMAGE_EXTENSIONS)
    {
        Some(true)
    } else if has_extension(path, LIVE_PHOTO_VIDEO_EXTENSIONS)
        || has_extension(path, MOTION_PHOTO_VIDEO_EXTENSIONS)
    {
        Some(false)
    } else {
        None
    }
}

pub(crate) fn is_allowed_live_photo_pair(image: &Path, video: &Path) -> bool {
    (has_extension(image, LIVE_PHOTO_IMAGE_EXTENSIONS)
        && has_extension(video, LIVE_PHOTO_VIDEO_EXTENSIONS))
        || (has_extension(image, MOTION_PHOTO_IMAGE_EXTENSIONS)
            && has_extension(video, MOTION_PHOTO_VIDEO_EXTENSIONS))
}

pub(crate) fn live_photo_kind_for_path(path: &Path) -> Option<bool> {
    live_photo_kind(path)
}

pub(crate) fn live_photo_sibling_paths(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Vec::new();
    };
    let extensions = if has_extension(path, LIVE_PHOTO_IMAGE_EXTENSIONS) {
        LIVE_PHOTO_VIDEO_EXTENSIONS
    } else if has_extension(path, MOTION_PHOTO_IMAGE_EXTENSIONS) {
        MOTION_PHOTO_VIDEO_EXTENSIONS
    } else if has_extension(path, LIVE_PHOTO_VIDEO_EXTENSIONS) {
        LIVE_PHOTO_IMAGE_EXTENSIONS
    } else if has_extension(path, MOTION_PHOTO_VIDEO_EXTENSIONS) {
        MOTION_PHOTO_IMAGE_EXTENSIONS
    } else {
        return Vec::new();
    };
    std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate.is_file()
                && candidate.file_stem().and_then(|value| value.to_str()) == Some(stem)
                && has_extension(candidate, extensions)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_all_immich_supported_asset_extensions() {
        for extension in MEDIA_EXTENSIONS {
            assert!(is_media_file(Path::new(&format!("asset.{extension}"))));
            assert!(is_media_file(Path::new(&format!(
                "asset.{}",
                extension.to_uppercase()
            ))));
        }
        assert!(!is_media_file(Path::new("asset.xmp")));
        assert!(!is_media_file(Path::new("asset.txt")));
    }
}
