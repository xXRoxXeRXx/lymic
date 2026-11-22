//! Reconciliation on disk (#280): reads what is in a Virtual Library directory and applies a
//! `diff::Plan` with `CfCreatePlaceholders`, `CfUpdatePlaceholder` and plain deletion. What to do
//! is decided in `diff` and `reconcile`; this only performs it, so it stays small. Like the rest of
//! this module it cannot be unit tested and is covered by `docs/virtual-library-verification.md`.
//!
//! Everything here is blocking and runs on `spawn_blocking` threads.
//!
//! Safety, on top of the rules in `diff`: before deleting or updating anything the item is read
//! again and must still be a placeholder of ours that is in sync. Only a directory whose every
//! child was ours is removed; one that still holds something else stays, and so does everything the
//! platform would otherwise have to throw away with it.
//!
//! Lymic's own deletions are registered with [`OWN_REMOVALS`] so that the delete veto of #279
//! (`NOTIFY_DELETE` in `callbacks.rs`) can let them pass: it asks [`is_own_removal`].

use crate::cloud_files::diff::{LocalEntry, LocalState, Plan, Removal, Update};
use crate::cloud_files::identity;
use crate::cloud_files::own_removals::OwnRemovals;
use crate::cloud_files::reconcile::{Applied, LocalError, LocalTree};
use std::collections::HashSet;
use std::mem::{offset_of, size_of};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindFirstFileW, FindNextFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_BASIC_INFO,
    WIN32_FIND_DATAW,
};

/// Placeholders Lymic is deleting right now, by root-relative path.
static OWN_REMOVALS: OwnRemovals = OwnRemovals::new();

/// Whether Lymic itself is deleting the placeholder at the root-relative `path` (`/` separated).
/// For the delete veto (#279): such a deletion must be acknowledged, not refused.
#[allow(dead_code)] // The delete veto of #279 calls it.
pub(in crate::cloud_files) fn is_own_removal(path: &str) -> bool {
    OWN_REMOVALS.is_own(path)
}

/// The Virtual Library folder on disk.
#[derive(Clone)]
pub(in crate::cloud_files) struct DiskTree {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    /// Directories that were populated. Only these are listed; a directory that was never populated
    /// would be populated by listing it.
    populated: HashSet<String>,
}

impl DiskTree {
    pub(in crate::cloud_files) fn new(root: PathBuf, populated: &[String]) -> Self {
        Self {
            inner: Arc::new(Inner {
                root,
                populated: populated.iter().cloned().collect(),
            }),
        }
    }
}

impl LocalTree for DiskTree {
    async fn list(&self, path: &str) -> Result<Vec<LocalEntry>, LocalError> {
        let (inner, path) = (self.inner.clone(), path.to_string());
        tokio::task::spawn_blocking(move || list_directory(&inner.root, &path))
            .await
            .unwrap_or_else(|error| Err(LocalError::Other(error.to_string())))
    }

    async fn apply(&self, path: &str, plan: &Plan) -> Applied {
        let (inner, path, plan) = (self.inner.clone(), path.to_string(), plan.clone());
        tokio::task::spawn_blocking(move || apply_plan(&inner, &path, &plan))
            .await
            .unwrap_or_else(|error| Applied {
                failed: vec![error.to_string()],
                ..Applied::default()
            })
    }
}

fn utf16z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    utf16z(&path.to_string_lossy())
}

fn directory_path(root: &Path, relative: &str) -> PathBuf {
    if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative.replace('/', "\\"))
    }
}

fn file_time(time: windows::Win32::Foundation::FILETIME) -> i64 {
    (i64::from(time.dwHighDateTime) << 32) | i64::from(time.dwLowDateTime)
}

fn file_name(data: &WIN32_FIND_DATAW) -> String {
    let length = data
        .cFileName
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(data.cFileName.len());
    String::from_utf16_lossy(&data.cFileName[..length])
}

/// Whether the platform calls this item a placeholder.
fn is_placeholder(data: &WIN32_FIND_DATAW) -> bool {
    // SAFETY: plain computation on two integers.
    let state =
        unsafe { CfGetPlaceholderStateFromAttributeTag(data.dwFileAttributes, data.dwReserved0) };
    state != CF_PLACEHOLDER_STATE_INVALID && state.0 & CF_PLACEHOLDER_STATE_PLACEHOLDER.0 != 0
}

/// The file identity (decoded as a path) and whether the placeholder at `path` is in sync. `None`
/// when it cannot be read, which makes the item not ours.
fn placeholder_details(path: &Path) -> Option<(Option<String>, bool)> {
    let wide = wide_path(path);
    // SAFETY: `wide` is NUL terminated; the handle is closed below.
    let handle =
        unsafe { CfOpenFileWithOplock(PCWSTR(wide.as_ptr()), CF_OPEN_FILE_FLAG_NONE) }.ok()?;
    let mut buffer =
        vec![0u8; size_of::<CF_PLACEHOLDER_BASIC_INFO>() + identity::MAX_IDENTITY_BYTES];
    // SAFETY: `buffer` is writable for its whole length.
    let result = unsafe {
        CfGetPlaceholderInfo(
            handle,
            CF_PLACEHOLDER_INFO_BASIC,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            None,
        )
    };
    // SAFETY: the handle came from `CfOpenFileWithOplock` and is closed once.
    unsafe { CfCloseHandle(handle) };
    result.ok()?;
    // SAFETY: the platform filled a `CF_PLACEHOLDER_BASIC_INFO`; the buffer is only byte aligned.
    let info =
        unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<CF_PLACEHOLDER_BASIC_INFO>()) };
    let start = offset_of!(CF_PLACEHOLDER_BASIC_INFO, FileIdentity);
    let end = start.checked_add(info.FileIdentityLength as usize)?;
    let identity = buffer.get(start..end).and_then(identity::decode);
    Some((identity, info.InSyncState == CF_IN_SYNC_STATE_IN_SYNC))
}

fn entry_of(directory: &Path, data: &WIN32_FIND_DATAW) -> LocalEntry {
    let name = file_name(data);
    let is_directory = data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let state = if is_placeholder(data) {
        match placeholder_details(&directory.join(&name)) {
            Some((identity, in_sync)) => LocalState::Placeholder { identity, in_sync },
            None => LocalState::Foreign,
        }
    } else {
        LocalState::Foreign
    };
    LocalEntry {
        name,
        is_directory,
        state,
        size: if is_directory {
            0
        } else {
            (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow)
        },
        created: file_time(data.ftCreationTime),
        modified: file_time(data.ftLastWriteTime),
    }
}

fn is_missing(error: &windows::core::Error) -> bool {
    error.code() == HRESULT::from_win32(ERROR_PATH_NOT_FOUND.0)
        || error.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
}

/// The items in the directory at the root-relative `relative`. Listing a directory that was never
/// populated would populate it, so callers list populated directories only.
fn list_directory(root: &Path, relative: &str) -> Result<Vec<LocalEntry>, LocalError> {
    list_at(&directory_path(root, relative))
}

fn list_at(directory: &Path) -> Result<Vec<LocalEntry>, LocalError> {
    let pattern = utf16z(&format!("{}\\*", directory.display()));
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `pattern` is NUL terminated and `data` is a valid out structure.
    let handle = match unsafe { FindFirstFileW(PCWSTR(pattern.as_ptr()), &mut data) } {
        Ok(handle) => handle,
        Err(error) if is_missing(&error) => return Err(LocalError::Missing),
        Err(error) => return Err(LocalError::Other(error.to_string())),
    };
    let mut entries = Vec::new();
    loop {
        let name = file_name(&data);
        if name != "." && name != ".." {
            entries.push(entry_of(directory, &data));
        }
        // SAFETY: `handle` is the live search handle and `data` a valid out structure. The loop
        // ends when the search is exhausted (`ERROR_NO_MORE_FILES`).
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break;
        }
    }
    // SAFETY: closes the handle opened above, once.
    let _ = unsafe { FindClose(handle) };
    Ok(entries)
}

/// The item at `directory\name`, read again just before it is touched.
fn stat(directory: &Path, name: &str) -> Option<LocalEntry> {
    let pattern = wide_path(&directory.join(name));
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: as in `list_directory`.
    let handle = unsafe { FindFirstFileW(PCWSTR(pattern.as_ptr()), &mut data) }.ok()?;
    // SAFETY: closes the handle opened above, once.
    let _ = unsafe { FindClose(handle) };
    Some(entry_of(directory, &data))
}

/// Whether `entry`, which sits at the root-relative `path`, is a placeholder Lymic created that the
/// user has not modified.
fn is_ours(entry: &LocalEntry, path: &str) -> bool {
    matches!(
        &entry.state,
        LocalState::Placeholder { identity: Some(held), in_sync: true } if held == path
    )
}

fn apply_plan(inner: &Inner, path: &str, plan: &Plan) -> Applied {
    let directory = directory_path(&inner.root, path);
    let mut applied = Applied::default();
    for removal in &plan.remove {
        let child = identity::child_path(path, &removal.name);
        if remove_placeholder(inner, &directory, &child, removal) {
            applied.removed.push(removal.clone());
        }
    }
    for update in &plan.update {
        match update_placeholder(&directory, path, update) {
            Ok(true) => applied.updated += 1,
            Ok(false) => {}
            Err(error) => applied
                .failed
                .push(format!("update '{}': {error}", update.spec.name)),
        }
    }
    create_placeholders(&directory, plan, &mut applied);
    applied
}

/// Deletes one placeholder (a directory only when everything in it was ours); `true` when it is
/// gone. Anything that is not ours any more is left alone.
fn remove_placeholder(inner: &Inner, directory: &Path, child: &str, removal: &Removal) -> bool {
    let Some(entry) = stat(directory, &removal.name) else {
        return false;
    };
    if !is_ours(&entry, child) {
        return false;
    }
    let target = directory.join(&removal.name);
    if removal.is_directory {
        remove_tree(inner, &target, child)
    } else {
        let _removing = OWN_REMOVALS.begin(child);
        std::fs::remove_file(&target).is_ok()
    }
}

/// Deletes the directory placeholder at `relative`, after its children that are ours. A directory
/// that was never populated has no children to look at, and listing it would populate it, so it is
/// deleted as it is.
fn remove_tree(inner: &Inner, target: &Path, relative: &str) -> bool {
    if inner.populated.contains(relative) {
        let Ok(children) = list_at(target) else {
            return false;
        };
        for child in children {
            let child_relative = identity::child_path(relative, &child.name);
            if !is_ours(&child, &child_relative) {
                continue;
            }
            let child_target = target.join(&child.name);
            if child.is_directory {
                remove_tree(inner, &child_target, &child_relative);
            } else {
                let _removing = OWN_REMOVALS.begin(&child_relative);
                let _ = std::fs::remove_file(&child_target);
            }
        }
    }
    let _removing = OWN_REMOVALS.begin(relative);
    // Fails, and leaves the directory, while something that is not ours is still inside.
    std::fs::remove_dir(target).is_ok()
}

fn metadata_of(spec: &crate::cloud_files::placeholder::PlaceholderSpec) -> CF_FS_METADATA {
    CF_FS_METADATA {
        BasicInfo: FILE_BASIC_INFO {
            CreationTime: spec.created,
            LastAccessTime: spec.modified,
            LastWriteTime: spec.modified,
            ChangeTime: spec.modified,
            // 0 leaves the attributes as they are.
            FileAttributes: 0,
        },
        FileSize: spec.size as i64,
    }
}

/// Updates one in-sync placeholder to the metadata Immich reports now; `Ok(false)` when it is not
/// (any more) one we may touch. A changed size means a different file, so the hydrated copy, if
/// any, is dropped and the next open downloads the new original.
fn update_placeholder(directory: &Path, parent: &str, update: &Update) -> Result<bool, String> {
    let name = &update.spec.name;
    let child = identity::child_path(parent, name);
    match stat(directory, name) {
        Some(entry) if is_ours(&entry, &child) => {}
        _ => return Ok(false),
    }
    let wide = wide_path(&directory.join(name));
    // SAFETY: `wide` is NUL terminated; the handle is closed below. A file in use cannot be opened
    // exclusively: it is skipped now and updated by a later run.
    let handle = unsafe {
        CfOpenFileWithOplock(
            PCWSTR(wide.as_ptr()),
            CF_OPEN_FILE_FLAGS(CF_OPEN_FILE_FLAG_EXCLUSIVE.0 | CF_OPEN_FILE_FLAG_WRITE_ACCESS.0),
        )
    }
    .map_err(|error| error.to_string())?;
    let metadata = metadata_of(&update.spec);
    let mut flags = CF_UPDATE_FLAG_MARK_IN_SYNC.0;
    if update.content_changed {
        flags |= CF_UPDATE_FLAG_DEHYDRATE.0;
    }
    // SAFETY: `metadata` and the identity outlive the call; the handle is valid.
    let result = unsafe {
        CfUpdatePlaceholder(
            handle,
            Some(&metadata),
            Some(update.spec.identity.as_ptr().cast()),
            update.spec.identity.len() as u32,
            None,
            CF_UPDATE_FLAGS(flags),
            None,
            None,
        )
    };
    // SAFETY: closes the handle opened above, once.
    unsafe { CfCloseHandle(handle) };
    result.map(|()| true).map_err(|error| error.to_string())
}

fn create_placeholders(directory: &Path, plan: &Plan, applied: &mut Applied) {
    if plan.create.is_empty() {
        return;
    }
    let names: Vec<Vec<u16>> = plan.create.iter().map(|spec| utf16z(&spec.name)).collect();
    let mut entries: Vec<CF_PLACEHOLDER_CREATE_INFO> = plan
        .create
        .iter()
        .zip(&names)
        .map(|(spec, name)| super::callbacks::create_info(spec, name))
        .collect();
    let base = wide_path(directory);
    for (chunk, specs) in entries
        .chunks_mut(super::callbacks::PLACEHOLDER_BATCH)
        .zip(plan.create.chunks(super::callbacks::PLACEHOLDER_BATCH))
    {
        // SAFETY: `base` is NUL terminated; the names and identities the entries point at outlive
        // the call.
        let result = unsafe {
            CfCreatePlaceholders(PCWSTR(base.as_ptr()), chunk, CF_CREATE_FLAG_NONE, None)
        };
        // The platform reports each entry's outcome in its `Result`, and the call itself fails
        // when any entry did. A failure with every entry still untouched is a failure of the call.
        if let Err(error) = &result {
            if chunk.iter().all(|entry| entry.Result.is_ok()) {
                applied.failed.push(format!("create: {error}"));
                continue;
            }
        }
        for (entry, spec) in chunk.iter().zip(specs) {
            if entry.Result.is_ok() {
                applied.created += 1;
            } else {
                applied.failed.push(format!(
                    "create '{}': {}",
                    spec.name,
                    windows::core::Error::from(entry.Result)
                ));
            }
        }
    }
}
