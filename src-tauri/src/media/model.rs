use base64::{engine::general_purpose, Engine as _};
use md5::Md5;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::extensions::{is_allowed_live_photo_pair, live_photo_kind_for_path as live_photo_kind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checksums {
    pub(crate) sha1_base64: String,
    pub(crate) md5_hex: String,
    pub(crate) sha256_hex: String,
}

/// Calculate every checksum required by the upload protocol and audit record
/// in one streaming pass. Immich continues to receive SHA-1/base64.
pub(crate) fn calculate_checksums(path: &str) -> io::Result<Checksums> {
    let mut file = File::open(path)?;
    let mut sha1 = Sha1::new();
    let mut md5 = Md5::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha1.update(&buffer[..n]);
        md5.update(&buffer[..n]);
        sha256.update(&buffer[..n]);
    }
    Ok(Checksums {
        sha1_base64: general_purpose::STANDARD.encode(sha1.finalize()),
        md5_hex: format!("{:x}", md5.finalize()),
        sha256_hex: format!("{:x}", sha256.finalize()),
    })
}

#[derive(Debug, Clone)]
pub(crate) struct SyncAsset {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) checksums: Option<Checksums>,
    pub(crate) mtime: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct Asset {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) mtime: i64,
}

#[derive(Debug)]
pub(crate) struct UploadUnit {
    pub(crate) image: Asset,
    pub(crate) live_photo_video: Option<Asset>,
}

#[derive(Debug)]
pub(crate) struct HashedUploadUnit {
    pub(crate) image: SyncAsset,
    pub(crate) live_photo_video: Option<SyncAsset>,
}

impl UploadUnit {
    pub(crate) fn assets(&self) -> impl Iterator<Item = &Asset> {
        std::iter::once(&self.image).chain(self.live_photo_video.iter())
    }
}

impl HashedUploadUnit {
    pub(crate) fn assets(&self) -> impl Iterator<Item = &SyncAsset> {
        std::iter::once(&self.image).chain(self.live_photo_video.iter())
    }

    pub(crate) fn byte_size(&self) -> u64 {
        self.assets().map(|asset| asset.size).sum()
    }
}

pub(crate) fn upload_units_from_assets(assets: Vec<Asset>) -> Vec<UploadUnit> {
    let mut groups: BTreeMap<(PathBuf, String), Vec<Asset>> = BTreeMap::new();
    for asset in assets {
        let path = Path::new(&asset.path);
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            groups
                .entry((PathBuf::from(&asset.path), String::new()))
                .or_default()
                .push(asset);
            continue;
        };
        groups
            .entry((
                path.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
                stem.to_string(),
            ))
            .or_default()
            .push(asset);
    }

    let mut units = Vec::new();
    for mut group in groups.into_values() {
        let image_indexes: Vec<_> = group
            .iter()
            .enumerate()
            .filter_map(|(index, asset)| {
                live_photo_kind(Path::new(&asset.path))
                    .filter(|is_image| *is_image)
                    .map(|_| index)
            })
            .collect();
        let video_indexes: Vec<_> = group
            .iter()
            .enumerate()
            .filter_map(|(index, asset)| {
                live_photo_kind(Path::new(&asset.path))
                    .filter(|is_image| !*is_image)
                    .map(|_| index)
            })
            .collect();
        if group.len() == 2
            && image_indexes.len() == 1
            && video_indexes.len() == 1
            && is_allowed_live_photo_pair(
                Path::new(&group[image_indexes[0]].path),
                Path::new(&group[video_indexes[0]].path),
            )
        {
            let video = group.swap_remove(video_indexes[0]);
            let image_index = image_indexes[0] - usize::from(video_indexes[0] < image_indexes[0]);
            let image = group.swap_remove(image_index);
            units.push(UploadUnit {
                image,
                live_photo_video: Some(video),
            });
        }
        units.extend(group.into_iter().map(|image| UploadUnit {
            image,
            live_photo_video: None,
        }));
    }
    units
}

#[derive(Debug)]
pub(crate) struct FileFailure {
    pub(crate) path: String,
    pub(crate) error: String,
}

#[derive(Debug, Default)]
pub(crate) struct ScanResult {
    pub(crate) files: Vec<Asset>,
    pub(crate) failures: Vec<FileFailure>,
    pub(crate) overlapping_folders: Vec<(PathBuf, PathBuf)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_asset(path: &str) -> Asset {
        Asset {
            path: path.to_string(),
            size: 1,
            mtime: 1,
        }
    }

    #[test]
    fn groups_only_canonical_live_and_motion_photo_pairs() {
        let units = upload_units_from_assets(vec![
            scan_asset("C:/photos/IMG_1234.HEIC"),
            scan_asset("C:/photos/IMG_1234.mov"),
            scan_asset("C:/photos/PXL_1234.JpG"),
            scan_asset("C:/photos/PXL_1234.MP4"),
        ]);
        assert_eq!(units.len(), 2);
        assert!(units.iter().all(|unit| unit.live_photo_video.is_some()));
        assert!(units
            .iter()
            .any(|unit| unit.image.path.ends_with("IMG_1234.HEIC")
                && unit
                    .live_photo_video
                    .as_ref()
                    .unwrap()
                    .path
                    .ends_with("IMG_1234.mov")));
    }

    #[test]
    fn leaves_ambiguous_and_noncanonical_candidates_as_singles() {
        let units = upload_units_from_assets(vec![
            scan_asset("C:/one/IMG_1.HEIC"),
            scan_asset("C:/two/IMG_1.MOV"),
            scan_asset("C:/one/IMG_2.HEIC"),
            scan_asset("C:/one/IMG_2.MP4"),
            scan_asset("C:/one/IMG_3.HEIC"),
            scan_asset("C:/one/IMG_3.MOV"),
            scan_asset("C:/one/IMG_3.mov"),
        ]);
        assert_eq!(units.len(), 7);
        assert!(units.iter().all(|unit| unit.live_photo_video.is_none()));
    }

    #[test]
    fn calculates_all_upload_and_audit_checksums_in_one_pass() {
        let path = std::env::temp_dir().join(format!(
            "lymic-checksum-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"abc").unwrap();
        let checksums = calculate_checksums(path.to_str().unwrap()).unwrap();
        assert_eq!(checksums.sha1_base64, "qZk+NkcGgWq6PiVxeFDCbJzQ2J0=");
        assert_eq!(checksums.md5_hex, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            checksums.sha256_hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_file(path).unwrap();
    }
}
