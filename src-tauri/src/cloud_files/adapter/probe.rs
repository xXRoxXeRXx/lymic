//! The operating system's answers about other sync roots, for `cloud_files::nested`.

use crate::cloud_files::nested::RegisteredRoot;
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::Storage::CloudFilters::{
    CfGetSyncRootInfoByPath, CF_SYNC_ROOT_INFO_STANDARD, CF_SYNC_ROOT_STANDARD_INFO,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    REG_SZ,
};

/// `HRESULT_FROM_WIN32(ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT)`
const NOT_UNDER_SYNC_ROOT: u32 = 0x8007_0186;
const SYNC_ROOT_MANAGER_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `CfGetSyncRootInfoByPath`: the provider name of the sync root `path` lies in, if any.
pub(super) fn provider_at(path: &Path) -> Result<Option<String>, String> {
    // 8-byte aligned storage with room for the provider name the struct carries inline.
    let mut buffer = vec![0u64; (std::mem::size_of::<CF_SYNC_ROOT_STANDARD_INFO>() + 1024) / 8 + 1];
    let path_text = wide(&path.to_string_lossy());
    let mut returned = 0u32;
    // SAFETY: `path_text` is NUL terminated and `buffer` is writable for the length passed.
    let result = unsafe {
        CfGetSyncRootInfoByPath(
            PCWSTR(path_text.as_ptr()),
            CF_SYNC_ROOT_INFO_STANDARD,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            Some(&mut returned),
        )
    };
    match result {
        Ok(()) => {
            // SAFETY: the call succeeded, so the buffer holds a `CF_SYNC_ROOT_STANDARD_INFO`.
            let info = unsafe { &*(buffer.as_ptr().cast::<CF_SYNC_ROOT_STANDARD_INFO>()) };
            let length = info
                .ProviderName
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(info.ProviderName.len());
            Ok(Some(String::from_utf16_lossy(&info.ProviderName[..length])))
        }
        Err(error) if error.code().0 as u32 == NOT_UNDER_SYNC_ROOT => Ok(None),
        Err(error) => Err(format!("CfGetSyncRootInfoByPath: {error}")),
    }
}

fn open_key(parent: HKEY, subkey: &str) -> Option<HKEY> {
    let subkey = wide(subkey);
    let mut key = HKEY::default();
    // SAFETY: `subkey` is NUL terminated; `key` receives the opened handle.
    let status =
        unsafe { RegOpenKeyExW(parent, PCWSTR(subkey.as_ptr()), None, KEY_READ, &mut key) };
    (status == ERROR_SUCCESS).then_some(key)
}

fn subkey_names(key: HKEY) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0.. {
        let mut buffer = [0u16; 512];
        let mut length = buffer.len() as u32;
        // SAFETY: `buffer` is writable for `length` units.
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status == ERROR_SUCCESS {
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        } else if status != ERROR_MORE_DATA {
            break;
        }
    }
    names
}

/// The string values of a key (`UserSyncRoots` maps a user SID to the root's path).
fn string_values(key: HKEY) -> Vec<String> {
    let mut values = Vec::new();
    for index in 0.. {
        let mut name = [0u16; 256];
        let mut name_length = name.len() as u32;
        let mut data = [0u16; 2048];
        let mut data_bytes = (data.len() * 2) as u32;
        let mut value_type = 0u32;
        // SAFETY: both buffers are writable for the lengths passed.
        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_length,
                None,
                Some(&mut value_type),
                Some(data.as_mut_ptr().cast()),
                Some(&mut data_bytes),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status == ERROR_SUCCESS && value_type == REG_SZ.0 {
            let units = (data_bytes as usize / 2).min(data.len());
            let text = String::from_utf16_lossy(&data[..units]);
            values.push(text.trim_end_matches('\0').to_string());
        } else if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
            break;
        }
    }
    values
}

/// Every sync root under `HKLM\...\Explorer\SyncRootManager\<id>\UserSyncRoots`.
pub(super) fn registered_roots() -> Result<Vec<RegisteredRoot>, String> {
    let Some(manager) = open_key(HKEY_LOCAL_MACHINE, SYNC_ROOT_MANAGER_KEY) else {
        // No provider has ever registered a root.
        return Ok(Vec::new());
    };
    let mut roots = Vec::new();
    for id in subkey_names(manager) {
        if let Some(user_roots) = open_key(manager, &format!(r"{id}\UserSyncRoots")) {
            for path in string_values(user_roots) {
                if !path.is_empty() {
                    roots.push(RegisteredRoot {
                        id: id.clone(),
                        path: PathBuf::from(path),
                    });
                }
            }
            // SAFETY: `user_roots` was opened above and is closed once.
            let _ = unsafe { RegCloseKey(user_roots) };
        }
    }
    // SAFETY: `manager` was opened above and is closed once.
    let _ = unsafe { RegCloseKey(manager) };
    Ok(roots)
}
