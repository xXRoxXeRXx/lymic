//! The Windows side: a COM `IThumbnailProvider` that answers from the thumbnail cache and never
//! reads the placeholder's content (ADR-0003, #282).
//!
//! What it does for one request:
//!
//! 1. `IInitializeWithItem::Initialize`: Explorer hands over an `IShellItem`. Only its file system
//!    path is read (that opens nothing), then the placeholder's *file identity* is read with
//!    `CfGetPlaceholderInfo` through a handle opened with `FILE_READ_ATTRIBUTES` only. An
//!    attributes-only open does not read the file's data, so it cannot start Hydration. The shell
//!    must not be given an `IInitializeWithStream` path: that would open the content.
//! 2. `IThumbnailProvider::GetThumbnail`: the identity selects a cached image
//!    ([`crate::cache`]); it is decoded from memory with WIC and scaled down to the requested size.
//!    A thumbnail that is not cached is never a download. It is not an error either: a failing
//!    provider makes Explorer fall back to the file type's default handler, which reads the
//!    placeholder and downloads the original. A miss (or an image that cannot be decoded) is
//!    answered with a plain generic bitmap and a request for the app to cache the thumbnail, which
//!    then refreshes the item.
//!
//! None of this can be unit tested without Explorer and a registered package; see the manual record
//! in `docs/virtual-library-verification.md`. Every request is logged to
//! `lymic-shell-ext.log` in the package's local state folder, which is how a maintainer tells
//! whether Explorer asked at all.

#![allow(non_snake_case)]

use crate::cache::{self, ThumbnailCache, Unavailable};
use crate::idle;
use std::ffi::c_void;
use std::io::Write as _;
use std::mem::offset_of;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HANDLE, WINCODEC_ERR_COMPONENTNOTFOUND,
};
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::Storage::CloudFilters::{
    CfGetPlaceholderInfo, CF_PLACEHOLDER_BASIC_INFO, CF_PLACEHOLDER_INFO_BASIC,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoRegisterClassObject, CoRevokeClassObject, CoTaskMemFree, CoUninitialize,
    IClassFactory, IClassFactory_Impl, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER,
    COINIT_MULTITHREADED, REGCLS_MULTIPLEUSE,
};
use windows::Win32::UI::Shell::{
    IInitializeWithItem, IInitializeWithItem_Impl, IShellItem, IThumbnailProvider,
    IThumbnailProvider_Impl, SIGDN_FILESYSPATH, WTSAT_ARGB, WTS_ALPHATYPE,
};

/// The largest identity the platform stores (`CF_PLACEHOLDER_MAX_FILE_IDENTITY_LENGTH`).
const MAX_IDENTITY_BYTES: usize = 4096;

/// The thumbnail edge never asked for or served beyond this.
const MAX_EDGE: u32 = 1024;

// ------------------------------------------------------------------ package folder, cache

/// The package's local state folder (`%LOCALAPPDATA%\Packages\<family>\LocalState`), which the
/// app and this provider both see because both run with the package's identity. `None` without
/// Package Identity.
pub fn package_local_state_dir() -> Option<PathBuf> {
    let folder = windows::Storage::ApplicationData::Current()
        .ok()?
        .LocalFolder()
        .ok()?;
    Some(PathBuf::from(folder.Path().ok()?.to_string()))
}

/// The thumbnail cache of the running package, `None` when the process has no Package Identity.
pub fn package_cache() -> Option<ThumbnailCache> {
    Some(ThumbnailCache::new(cache::directory_under(
        &package_local_state_dir()?,
    )))
}

// ------------------------------------------------------------------ activity and logging

static LIVE_OBJECTS: AtomicUsize = AtomicUsize::new(0);
static LAST_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);
static STARTED: OnceLock<Instant> = OnceLock::new();
static LOG: Mutex<()> = Mutex::new(());

/// The log stops growing here; it is diagnostics for a human, not a history.
const LOG_LIMIT_BYTES: u64 = 256 * 1024;

fn started() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

fn touch() {
    LAST_ACTIVITY_MS.store(started().elapsed().as_millis() as u64, Ordering::Relaxed);
}

fn idle_for() -> Duration {
    let now = started().elapsed().as_millis() as u64;
    Duration::from_millis(now.saturating_sub(LAST_ACTIVITY_MS.load(Ordering::Relaxed)))
}

/// Appends one line to `lymic-shell-ext.log`. Never fails the caller.
fn log(message: &str) {
    let Some(directory) = package_local_state_dir() else {
        return;
    };
    let _guard = LOG.lock();
    let path = directory.join("lymic-shell-ext.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_LIMIT_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "{seconds} pid={} {message}", std::process::id());
    }
}

// ------------------------------------------------------------------ the placeholder

/// The file identity of the placeholder at `path`, read without touching its data.
fn read_identity(path: &str) -> Result<Vec<u8>> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL terminated and outlives the call. Only attributes are requested, so the
    // open does not read data and does not start Hydration.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )?
    };
    let identity = identity_of(handle);
    // SAFETY: `handle` was opened above and is closed once.
    let _ = unsafe { CloseHandle(handle) };
    identity
}

fn identity_of(handle: HANDLE) -> Result<Vec<u8>> {
    let mut buffer =
        vec![0u8; std::mem::size_of::<CF_PLACEHOLDER_BASIC_INFO>() + MAX_IDENTITY_BYTES];
    // SAFETY: `buffer` is writable for its whole length.
    unsafe {
        CfGetPlaceholderInfo(
            handle,
            CF_PLACEHOLDER_INFO_BASIC,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            None,
        )?;
    }
    // SAFETY: the platform filled a `CF_PLACEHOLDER_BASIC_INFO`; the buffer is only byte aligned.
    let info =
        unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<CF_PLACEHOLDER_BASIC_INFO>()) };
    let start = offset_of!(CF_PLACEHOLDER_BASIC_INFO, FileIdentity);
    let end = start.saturating_add(info.FileIdentityLength as usize);
    buffer
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| E_FAIL.into())
}

// ------------------------------------------------------------------ the provider

#[implement(IInitializeWithItem, IThumbnailProvider)]
struct ThumbnailProvider {
    file_identity: Mutex<Option<Vec<u8>>>,
    /// The item's file system path, for the log and for the request the app refreshes by.
    item_path: Mutex<String>,
}

impl ThumbnailProvider {
    fn new() -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
        touch();
        Self {
            file_identity: Mutex::new(None),
            item_path: Mutex::new(String::new()),
        }
    }
}

impl Drop for ThumbnailProvider {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::Relaxed);
        touch();
    }
}

impl IInitializeWithItem_Impl for ThumbnailProvider_Impl {
    fn Initialize(&self, psi: Ref<IShellItem>, _grfmode: u32) -> Result<()> {
        touch();
        let item = psi.ok()?;
        // SIGDN_FILESYSPATH only resolves the name; it does not open the file.
        let name: PWSTR = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? };
        let path = unsafe { name.to_string() }.unwrap_or_default();
        // SAFETY: the shell allocated the string with the COM allocator.
        unsafe { CoTaskMemFree(Some(name.0 as *const c_void)) };
        match read_identity(&path) {
            Ok(identity) => {
                log(&format!(
                    "Initialize ok: {} identity byte(s) for {path}",
                    identity.len()
                ));
                *self.file_identity.lock().unwrap() = Some(identity);
                *self.item_path.lock().unwrap() = path;
                Ok(())
            }
            Err(error) => {
                log(&format!("Initialize failed for {path}: {error}"));
                Err(error)
            }
        }
    }
}

impl IThumbnailProvider_Impl for ThumbnailProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> Result<()> {
        touch();
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(E_POINTER.into());
        }
        let identity = self
            .file_identity
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default();
        let path = self.item_path.lock().unwrap().clone();
        let cache = package_cache();
        let served = cache
            .as_ref()
            .ok_or(Unavailable::Unusable)
            .and_then(|cache| cache.load(&identity))
            .map_err(|reason| format!("nothing cached ({reason:?})"))
            .and_then(|image| {
                decode(&image, cx)
                    .map_err(|error| format!("the cached image cannot be decoded: {error}"))
            });
        let (bitmap, outcome) = match served {
            Ok(bitmap) => (bitmap, "served from the cache".to_string()),
            Err(reason) => {
                // Never an error: Explorer would read the placeholder with its default handler.
                // The app is asked to cache the thumbnail and then refreshes the item.
                if let Some(cache) = &cache {
                    cache.record_miss(&identity, &path);
                }
                (
                    generic_bitmap(cx)?,
                    format!("{reason}; answered with the generic image and asked the app"),
                )
            }
        };
        // SAFETY: both out pointers were checked above.
        unsafe {
            *phbmp = bitmap;
            *pdwalpha = WTSAT_ARGB;
        }
        log(&format!("GetThumbnail cx={cx} {path}: {outcome}"));
        Ok(())
    }
}

/// The generic image of [`cache::generic_thumbnail`] as a bitmap: what a placeholder shows until
/// its thumbnail is cached.
fn generic_bitmap(cx: u32) -> Result<HBITMAP> {
    let image = cache::generic_thumbnail(cx.clamp(1, MAX_EDGE));
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: image.width as i32,
            biHeight: -(image.height as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: `info` describes the section being created; `bits` receives its pixel memory, which
    // holds `bgra.len()` bytes for these dimensions.
    unsafe {
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if bits.is_null() {
            return Err(E_POINTER.into());
        }
        std::ptr::copy_nonoverlapping(image.bgra.as_ptr(), bits.cast::<u8>(), image.bgra.len());
        Ok(bitmap)
    }
}

/// Decodes `image` from memory and returns a premultiplied 32 bit bitmap whose longer edge is `cx`
/// (never enlarged). WIC reads JPEG and PNG everywhere; WebP needs Windows' WebP extension, which
/// Windows 11 includes. Without a codec the request fails and Explorer shows its generic icon.
fn decode(image: &[u8], cx: u32) -> Result<HBITMAP> {
    let edge = cx.clamp(1, MAX_EDGE);
    // SAFETY: plain COM calls on interfaces created here; `image` outlives the stream that reads it.
    unsafe {
        let factory: IWICImagingFactory = windows::Win32::System::Com::CoCreateInstance(
            &CLSID_WICImagingFactory,
            None,
            CLSCTX_INPROC_SERVER,
        )?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(image)?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .inspect_err(|error| {
                if error.code() == WINCODEC_ERR_COMPONENTNOTFOUND {
                    log("no WIC decoder for the cached image format");
                }
            })?;
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0u32, 0u32);
        frame.GetSize(&mut width, &mut height)?;
        if width == 0 || height == 0 {
            return Err(E_FAIL.into());
        }
        let scale = (edge as f32 / width.max(height) as f32).min(1.0);
        let target_width = ((width as f32 * scale) as u32).max(1);
        let target_height = ((height as f32 * scale) as u32).max(1);
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(
            &frame,
            target_width,
            target_height,
            WICBitmapInterpolationModeFant,
        )?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: target_width as i32,
                biHeight: -(target_height as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if bits.is_null() {
            return Err(E_POINTER.into());
        }
        let stride = target_width * 4;
        let pixels =
            std::slice::from_raw_parts_mut(bits.cast::<u8>(), (stride * target_height) as usize);
        converter.CopyPixels(std::ptr::null(), stride, pixels)?;
        Ok(bitmap)
    }
}

// ------------------------------------------------------------------ class factory

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<windows::core::IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        log("CreateInstance");
        let unknown: windows::core::IUnknown = ThumbnailProvider::new().into();
        // SAFETY: COM passes valid pointers for the requested interface.
        unsafe { unknown.query(iid, object).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        // A locked server counts as alive, so the host does not exit under a client that asked
        // for it to stay.
        if lock.as_bool() {
            LIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
        } else {
            LIVE_OBJECTS.fetch_sub(1, Ordering::Relaxed);
        }
        touch();
        Ok(())
    }
}

/// The class factory of the thumbnail provider.
pub fn class_factory() -> IClassFactory {
    Factory.into()
}

fn provider_clsid() -> GUID {
    GUID::try_from(crate::THUMBNAIL_PROVIDER_CLSID).expect("the CLSID constant is a valid GUID")
}

// ------------------------------------------------------------------ the out-of-process host

/// Runs the `com:ExeServer` host: registers the class factory, serves until it has been idle (see
/// [`idle`]) and exits. Returns the process exit code.
pub fn run_host() -> i32 {
    // SAFETY: COM initialisation for this process, undone before returning.
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return 1;
        }
    }
    touch();
    log("host starting");
    let factory: windows::core::IUnknown = class_factory().into();
    // SAFETY: the class object stays alive until it is revoked below.
    let cookie = unsafe {
        CoRegisterClassObject(
            &provider_clsid(),
            &factory,
            CLSCTX_LOCAL_SERVER,
            REGCLS_MULTIPLEUSE,
        )
    };
    let code = match cookie {
        Ok(cookie) => {
            while !idle::should_exit(LIVE_OBJECTS.load(Ordering::Relaxed), idle_for()) {
                std::thread::sleep(Duration::from_secs(1));
            }
            // SAFETY: `cookie` came from `CoRegisterClassObject`.
            let _ = unsafe { CoRevokeClassObject(cookie) };
            0
        }
        Err(error) => {
            log(&format!("the class could not be registered: {error}"));
            1
        }
    };
    log("host exiting");
    drop(factory);
    // SAFETY: paired with the successful `CoInitializeEx` above.
    unsafe { CoUninitialize() };
    code
}

// ------------------------------------------------------------------ the DLL surrogate variant

/// `com:SurrogateServer` entry point. Compiled only with the `surrogate-dll` feature so that the
/// app, which links this crate, exports nothing.
///
/// # Safety
/// Called by COM with valid pointers.
#[cfg(feature = "surrogate-dll")]
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> windows::core::HRESULT {
    use windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE;
    if class.is_null() || iid.is_null() || object.is_null() {
        return E_POINTER;
    }
    // SAFETY: checked for null above; COM guarantees the pointers are valid.
    unsafe {
        *object = std::ptr::null_mut();
        if *class != provider_clsid() {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        class_factory().query(iid, object)
    }
}

/// `com:SurrogateServer` entry point: the DLL may unload once no object is alive.
#[cfg(feature = "surrogate-dll")]
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> windows::core::HRESULT {
    use windows::Win32::Foundation::{S_FALSE, S_OK};
    if LIVE_OBJECTS.load(Ordering::Relaxed) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetObjectW, BITMAP, HGDIOBJ};
    use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;

    /// An 8 x 4 pixel PNG, generated with Python's zlib, not by this crate.
    const PNG_8X4: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xB3,
        0xCD, 0x7E, 0xF0, 0x00, 0x00, 0x00, 0x12, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x38,
        0x21, 0x27, 0xF7, 0x1F, 0x1F, 0x66, 0xA0, 0xBD, 0x02, 0x00, 0x8F, 0xE4, 0x40, 0x61, 0x25,
        0x69, 0xE5, 0x7D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn size_of(bitmap: HBITMAP) -> (i32, i32) {
        let mut info = BITMAP::default();
        // SAFETY: `info` is a BITMAP and the size passed is its size.
        let written = unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(std::ptr::from_mut(&mut info).cast()),
            )
        };
        assert!(written > 0);
        (info.bmWidth, info.bmHeight)
    }

    fn init_com() {
        // SAFETY: COM initialisation of the test thread; a repeated call is harmless.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    }

    #[test]
    fn a_cached_image_is_scaled_down_to_the_requested_edge_keeping_its_proportions() {
        init_com();

        let bitmap = decode(PNG_8X4, 4).unwrap();

        assert_eq!(size_of(bitmap), (4, 2));
    }

    #[test]
    fn a_cached_image_is_never_enlarged() {
        init_com();

        let bitmap = decode(PNG_8X4, 256).unwrap();

        assert_eq!(size_of(bitmap), (8, 4));
    }

    #[test]
    fn bytes_that_are_not_an_image_do_not_decode() {
        init_com();

        assert!(decode(b"<html>not an image</html>", 64).is_err());
    }

    #[test]
    fn a_file_that_is_not_a_placeholder_has_no_identity_and_is_refused() {
        let path =
            std::env::temp_dir().join(format!("lymic-shell-ext-plain-{}.jpg", std::process::id()));
        std::fs::write(&path, b"plain file").unwrap();

        let identity = read_identity(&path.to_string_lossy());

        let _ = std::fs::remove_file(&path);
        assert!(identity.is_err());
    }
}
