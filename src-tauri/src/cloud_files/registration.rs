//! What the sync root registration says about itself that does not depend on Windows calls: the
//! icon Explorer shows for it. The calls that register it are in `adapter/register.rs`.

use std::path::Path;

/// The stock "cloud folder" icon of Windows, used when Lymic's own cannot be named.
pub(super) const FALLBACK_ICON: &str = r"%SystemRoot%\system32\imageres.dll,-1043";

/// The icon resource for the sync root (`path,index`, the format `IconResource` takes): the first
/// icon of Lymic's own executable, which is the application icon, so Explorer shows Lymic's icon
/// beside the Virtual Library instead of a stock one. `None` (the executable's path is unknown)
/// gives [`FALLBACK_ICON`].
pub(super) fn icon_resource(executable: Option<&Path>) -> String {
    executable
        .filter(|path| !path.as_os_str().is_empty())
        .map_or_else(
            || FALLBACK_ICON.to_string(),
            |path| format!("{},0", path.display()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_the_first_icon_of_lymics_own_executable() {
        let executable =
            Path::new(r"C:\Program Files\WindowsApps\Lymic_1.0.0.0_x64__abc\Lymic.exe");

        assert_eq!(
            icon_resource(Some(executable)),
            r"C:\Program Files\WindowsApps\Lymic_1.0.0.0_x64__abc\Lymic.exe,0"
        );
    }

    #[test]
    fn without_a_known_executable_the_stock_cloud_icon_is_used() {
        assert_eq!(icon_resource(None), FALLBACK_ICON);
        assert_eq!(icon_resource(Some(Path::new(""))), FALLBACK_ICON);
    }
}
