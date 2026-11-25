//! Sync root registration through the WinRT `StorageProviderSyncRootManager`, which needs Package
//! Identity. Hydration is always the original, never a preview, read progressively; the population policy `Full` is the WinRT name
//! for "the provider fills a directory when it is first listed", which is how the prototype
//! observed `FETCH_PLACEHOLDERS` arriving one directory at a time.
//!
//! The thumbnail provider is not part of the registration: `StorageProviderSyncRootInfo` has no
//! field for it. It comes from the package manifest (`windows.cloudFiles` with a COM server, #282).
//! With a `com:SurrogateServer` the gate prototype's `Register` failed with 0x80070490 while a
//! `com:ExeServer` registered, which is why the manifest declares an executable server by default
//! (`docs/store/local-msix-build.md`).

use crate::cloud_files::{registration, PROVIDER_ID};
use std::path::Path;
use windows::core::{Result, HSTRING, PWSTR};
use windows::Security::Cryptography::{BinaryStringEncoding, CryptographicBuffer};
use windows::Storage::Provider::{
    StorageProviderHardlinkPolicy, StorageProviderHydrationPolicy,
    StorageProviderHydrationPolicyModifier, StorageProviderInSyncPolicy,
    StorageProviderPopulationPolicy, StorageProviderProtectionMode, StorageProviderSyncRootInfo,
    StorageProviderSyncRootManager,
};
use windows::Storage::StorageFolder;
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

fn current_user_sid() -> Result<String> {
    // SAFETY: plain Win32 calls on this process's own token; the buffers outlive the calls and the
    // SID string allocated by `ConvertSidToStringSidW` is released with `LocalFree`.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let sid = (|| {
            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
            // 8-byte aligned storage for the TOKEN_USER the call writes into.
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )?;
            let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text)?;
            let sid = text.to_string().unwrap_or_default();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            Ok(sid)
        })();
        let _ = CloseHandle(token);
        sid
    }
}

/// `[Provider ID]![Windows SID]![Account ID]`, the documented sync root id format.
pub(super) fn sync_root_id() -> Result<String> {
    Ok(format!("{PROVIDER_ID}!{}!Library", current_user_sid()?))
}

/// Registers (or re-registers) `root`, which must exist.
pub(super) fn register(root: &Path) -> Result<()> {
    let folder =
        StorageFolder::GetFolderFromPathAsync(&HSTRING::from(root.to_string_lossy().as_ref()))?
            .join()?;
    let info = StorageProviderSyncRootInfo::new()?;
    info.SetId(&HSTRING::from(sync_root_id()?))?;
    info.SetPath(&folder)?;
    info.SetDisplayNameResource(&HSTRING::from("Lymic"))?;
    // Lymic's own icon (the first one of the executable); the stock imageres icon if its path is
    // unknown.
    info.SetIconResource(&HSTRING::from(registration::icon_resource(
        std::env::current_exe().ok().as_deref(),
    )))?;
    // Progressive: a reader waits only for the range it asked for, so a video starts at once and a
    // seek does not wait for the whole original (`Full` completes the open only after the last
    // byte). `hydration::hydrate` serves the range plus a bounded read-ahead.
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::Progressive)?;
    // Hydrated originals may be given back to the cloud by Storage Sense and "Free up space"; the
    // placeholders are always in sync, so nothing is lost.
    info.SetHydrationPolicyModifier(
        StorageProviderHydrationPolicyModifier::AutoDehydrationAllowed,
    )?;
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::Full)?;
    // Placeholders are created in sync and Lymic only ever updates them through
    // `CfUpdatePlaceholder` with the in-sync mark, so its own changes must not clear that state;
    // otherwise Explorer would show a sync pending glyph on a read-only library.
    info.SetInSyncPolicy(StorageProviderInSyncPolicy::PreserveInsyncForSyncEngine)?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
    info.SetShowSiblingsAsGroup(false)?;
    info.SetVersion(&HSTRING::from(env!("CARGO_PKG_VERSION")))?;
    info.SetProtectionMode(StorageProviderProtectionMode::Personal)?;
    info.SetAllowPinning(true)?;
    info.SetContext(&CryptographicBuffer::ConvertStringToBinary(
        &HSTRING::from(PROVIDER_ID),
        BinaryStringEncoding::Utf8,
    )?)?;
    StorageProviderSyncRootManager::Register(&info)
}
