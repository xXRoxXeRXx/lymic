//! The Virtual Library's COM thumbnail provider (ADR-0003, #282).
//!
//! Explorer asks the provider registered for a sync root (the `windows.cloudFiles` manifest
//! extension) for the thumbnail of a placeholder instead of opening the file, so no Hydration
//! happens. This crate answers from a cache the Lymic app fills with Immich thumbnails
//! ([`cache`]): the provider never reads the placeholder's content and never talks to the server.
//!
//! The crate is deliberately thin. What can be decided without Windows is here and tested on every
//! platform: how a placeholder identity maps to a cached file and what the cache accepts
//! ([`cache`]), and when the out-of-process host may exit ([`idle`]). The COM code (`shell`) is
//! Windows only, has no unit-test seam and is covered by the manual record in
//! `docs/virtual-library-verification.md`.

// link.exe asks for the COM exports to be PRIVATE; cosmetic.
#![cfg_attr(feature = "surrogate-dll", allow(linker_messages))]

pub mod cache;
pub mod idle;

#[cfg(windows)]
pub mod shell;

/// The class id of the thumbnail provider. It appears in the MSIX manifest (the
/// `ThumbnailProviderHandler` and the COM class) through `scripts/package-msix.ps1`, which holds the
/// same value; a test checks that they agree.
pub const THUMBNAIL_PROVIDER_CLSID: &str = "9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_packaging_script_declares_the_same_class_id_as_the_provider() {
        let script = include_str!("../../../scripts/package-msix.ps1");

        assert!(
            script.contains(THUMBNAIL_PROVIDER_CLSID),
            "scripts/package-msix.ps1 must use the CLSID {THUMBNAIL_PROVIDER_CLSID}"
        );
    }

    #[test]
    fn the_manifest_template_takes_the_class_id_from_the_packaging_script() {
        let template = include_str!("../../msix/AppxManifest.template.xml");

        assert!(template.contains("windows.cloudFiles"));
        assert!(
            template.contains("<desktop3:ThumbnailProviderHandler Clsid=\"{{THUMBNAIL_CLSID}}\"")
        );
        assert!(template.contains("{{THUMBNAIL_COM_SERVER}}"));
    }
}
