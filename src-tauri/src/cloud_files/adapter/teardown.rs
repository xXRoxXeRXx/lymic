//! The Windows side of removing the Virtual Library (#283): reading what the platform says about each
//! item, and unregistering the sync root. The decisions are in `cloud_files::cleanup`; this only
//! looks and calls. It cannot be unit tested and is covered by `docs/virtual-library-verification.md`.
//!
//! Reading a placeholder never hydrates it: the handle is opened for attributes only and as a
//! reparse point, and `CfGetPlaceholderInfo` reads the identity and the sync state from it. Nothing
//! here asks the platform to populate a directory, because nothing is connected while this runs, so a
//! directory it could not list is reported as unreadable instead of being filled from Immich.

use super::register::sync_root_id;
use crate::cloud_files::cleanup::{Children, CleanupFs, CleanupHost, Kind, Node, Observed, StdFs};
use std::ffi::OsString;
use std::io;
use std::mem::{offset_of, size_of};
use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Storage::Provider::StorageProviderSyncRootManager;
use windows::Win32::Foundation::{CloseHandle, ERROR_NOT_FOUND, HANDLE};
use windows::Win32::Storage::CloudFilters::{
    CfGetPlaceholderInfo, CF_IN_SYNC_STATE_IN_SYNC, CF_PLACEHOLDER_BASIC_INFO,
    CF_PLACEHOLDER_INFO_BASIC,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// Deeper than any projected tree; a folder tree beyond it is left unexamined.
const MAX_DEPTH: usize = 64;
/// `CF_PLACEHOLDER_MAX_FILE_IDENTITY_LENGTH`.
const MAX_IDENTITY: usize = 4096;

pub(super) struct WindowsCleanup;

impl CleanupFs for WindowsCleanup {
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        StdFs.remove_file(path)
    }

    fn remove_folder(&self, path: &Path) -> io::Result<()> {
        StdFs.remove_folder(path)
    }
}

impl CleanupHost for WindowsCleanup {
    fn disconnect(&self) {
        super::stop();
    }

    fn scan(&self, root: &Path) -> Result<Option<Node>, String> {
        match std::fs::symlink_metadata(root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!("'{}' is not a folder", root.display()))
            }
            Ok(_) => {}
        }
        Ok(Some(Node {
            name: OsString::new(),
            kind: Kind::Directory,
            // The root is the sync root folder, never a placeholder itself.
            observed: Observed::Plain,
            children: list(root, 0),
        }))
    }

    fn unregister(&self) -> Result<(), String> {
        unregister()
    }
}

fn list(directory: &Path, depth: usize) -> Children {
    if depth >= MAX_DEPTH {
        return Children::Unreadable("folders are nested too deeply".to_string());
    }
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => return Children::Unreadable(error.to_string()),
    };
    let mut nodes = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Children::Unreadable(error.to_string()),
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => return Children::Unreadable(error.to_string()),
        };
        let path = entry.path();
        // A link or junction is never followed and never taken for a folder of ours.
        let kind = if file_type.is_dir() {
            Kind::Directory
        } else {
            Kind::File
        };
        let observed = if file_type.is_symlink() {
            Observed::Plain
        } else {
            observe(&path)
        };
        let children = match kind {
            Kind::Directory if !file_type.is_symlink() => list(&path, depth + 1),
            _ => Children::None,
        };
        nodes.push(Node {
            name: entry.file_name(),
            kind,
            observed,
            children,
        });
    }
    Children::Listed(nodes)
}

/// What the platform says about `path`. Anything that cannot be read is `Plain`, which is the
/// answer that keeps the item.
fn observe(path: &Path) -> Observed {
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL terminated and outlives the call; the handle is closed below. Opened for
    // attributes only and as a reparse point, so a placeholder is not hydrated.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    };
    let Ok(handle) = handle else {
        return Observed::Plain;
    };
    let observed = placeholder_state(handle);
    // SAFETY: the handle was opened above and is closed once.
    let _ = unsafe { CloseHandle(handle) };
    observed
}

fn placeholder_state(handle: HANDLE) -> Observed {
    let bytes = size_of::<CF_PLACEHOLDER_BASIC_INFO>() + MAX_IDENTITY;
    // 8-byte aligned storage for the structure the call writes into.
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    let mut returned = 0u32;
    // SAFETY: `buffer` is writable for `bytes` bytes and aligned for the structure.
    let result = unsafe {
        CfGetPlaceholderInfo(
            handle,
            CF_PLACEHOLDER_INFO_BASIC,
            buffer.as_mut_ptr().cast(),
            bytes as u32,
            Some(&mut returned),
        )
    };
    // Not a cloud file (or unreadable): not something Lymic can claim.
    if result.is_err() {
        return Observed::Plain;
    }
    // SAFETY: the call succeeded, so the buffer holds a `CF_PLACEHOLDER_BASIC_INFO` followed by
    // `FileIdentityLength` bytes of identity; the length is checked against the buffer below.
    let info = unsafe { &*buffer.as_ptr().cast::<CF_PLACEHOLDER_BASIC_INFO>() };
    let length = info.FileIdentityLength as usize;
    let start = offset_of!(CF_PLACEHOLDER_BASIC_INFO, FileIdentity);
    if length > MAX_IDENTITY || start + length > bytes {
        return Observed::Plain;
    }
    // SAFETY: bounds checked above.
    let identity =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>().add(start), length) }
            .to_vec();
    Observed::Placeholder {
        identity,
        in_sync: info.InSyncState == CF_IN_SYNC_STATE_IN_SYNC,
    }
}

/// Whether the sync root with `id` is among this user's registered sync roots.
fn is_registered(id: &str) -> Result<bool, String> {
    let roots = StorageProviderSyncRootManager::GetCurrentSyncRoots().map_err(|e| e.to_string())?;
    let count = roots.Size().map_err(|e| e.to_string())?;
    for index in 0..count {
        let root = roots.GetAt(index).map_err(|e| e.to_string())?;
        if root.Id().map_err(|e| e.to_string())?.to_string_lossy() == id {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Removes the sync root registration through WinRT. A root that is not registered (never was, or a
/// cleanup already did it) is success.
fn unregister() -> Result<(), String> {
    let id = sync_root_id().map_err(|error| error.to_string())?;
    match StorageProviderSyncRootManager::Unregister(&HSTRING::from(id.as_str())) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(error) => match is_registered(&id) {
            Ok(false) => Ok(()),
            _ => Err(error.to_string()),
        },
    }
}
