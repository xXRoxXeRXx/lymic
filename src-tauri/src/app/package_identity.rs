//! Runtime detection of Package Identity (MSIX), the precondition of the Virtual Library (ADR-0003).
//!
//! The release-channel flag is a compile-time label; only the operating system knows whether this
//! process actually runs with a package identity. Unpackaged runs (`tauri dev`, NSIS installs, a
//! Store-labelled build started from `target`) have none.

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// Interprets the status of `GetCurrentPackageFullName` called with an empty buffer: a packaged
/// process is told the buffer is too small, every other status (notably `APPMODEL_ERROR_NO_PACKAGE`,
/// 15700) means there is no identity.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_packaged(status: u32) -> bool {
    status == ERROR_INSUFFICIENT_BUFFER
}

/// Whether the current process runs with real Package Identity. Always false outside Windows.
#[cfg(windows)]
pub(crate) fn has_package_identity() -> bool {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    let mut length: u32 = 0;
    // SAFETY: a null buffer with a zero length is the documented way to probe for a package.
    let status = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
    is_packaged(status)
}

#[cfg(not(windows))]
pub(crate) fn has_package_identity() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_too_small_buffer_status_means_the_process_is_packaged() {
        assert!(is_packaged(122));
        assert!(!is_packaged(15700), "APPMODEL_ERROR_NO_PACKAGE");
        assert!(!is_packaged(0));
        assert!(!is_packaged(87));
    }

    #[test]
    fn test_binaries_run_without_package_identity() {
        assert!(!has_package_identity());
    }
}
