#[cfg(target_os = "windows")]
use super::model::{VolumeEvent, VolumeInfo};
#[cfg(target_os = "windows")]
use super::monitor::RemovableMediaMonitor;
#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use tokio::sync::mpsc::Sender;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    RegisterClassExW, TranslateMessage, MSG, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};

#[cfg(target_os = "windows")]
pub const DRIVE_REMOVABLE: u32 = 2;
#[cfg(target_os = "windows")]
pub const WM_DEVICECHANGE: u32 = 0x0219;
#[cfg(target_os = "windows")]
pub const DBT_DEVICEARRIVAL: u32 = 0x8000;
#[cfg(target_os = "windows")]
pub const DBT_DEVICEREMOVECOMPLETE: u32 = 0x8004;
#[cfg(target_os = "windows")]
pub const DBT_DEVTYP_VOLUME: u32 = 0x00000002;

#[cfg(target_os = "windows")]
pub fn get_volume_info_for_drive(drive_letter: char) -> Option<VolumeInfo> {
    let root_path = format!("{}:\\", drive_letter.to_ascii_uppercase());
    let wide_root: Vec<u16> = OsStr::new(&root_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let drive_type = GetDriveTypeW(wide_root.as_ptr());
        if drive_type != DRIVE_REMOVABLE {
            return None;
        }

        let mut volume_guid_buf = [0u16; 128];
        if GetVolumeNameForVolumeMountPointW(
            wide_root.as_ptr(),
            volume_guid_buf.as_mut_ptr(),
            volume_guid_buf.len() as u32,
        ) == 0
        {
            return None;
        }

        let volume_guid_len = volume_guid_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(volume_guid_buf.len());
        let volume_guid = String::from_utf16_lossy(&volume_guid_buf[..volume_guid_len]);

        let mut volume_name_buf = [0u16; 260];
        let mut serial_number = 0u32;
        let mut max_component_len = 0u32;
        let mut file_system_flags = 0u32;
        let mut fs_name_buf = [0u16; 260];

        let name_res = GetVolumeInformationW(
            wide_root.as_ptr(),
            volume_name_buf.as_mut_ptr(),
            volume_name_buf.len() as u32,
            &mut serial_number,
            &mut max_component_len,
            &mut file_system_flags,
            fs_name_buf.as_mut_ptr(),
            fs_name_buf.len() as u32,
        );

        let display_name = if name_res != 0 {
            let name_len = volume_name_buf.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&volume_name_buf[..name_len])
                .trim()
                .to_string();
            if name.is_empty() {
                format!("SD Card ({}:)", drive_letter.to_ascii_uppercase())
            } else {
                name
            }
        } else {
            format!("SD Card ({}:)", drive_letter.to_ascii_uppercase())
        };

        Some(VolumeInfo {
            volume_id: volume_guid,
            mount_path: root_path,
            display_name,
        })
    }
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct DevBroadcastHdr {
    dbch_size: u32,
    dbch_devicetype: u32,
    dbch_reserved: u32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct DevBroadcastVolume {
    dbcv_size: u32,
    dbcv_devicetype: u32,
    dbcv_reserved: u32,
    dbcv_unitmask: u32,
    dbcv_flags: u16,
}

#[cfg(target_os = "windows")]
pub struct WindowsRemovableMediaMonitor {
    is_running: Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
impl Default for WindowsRemovableMediaMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl WindowsRemovableMediaMonitor {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(target_os = "windows")]
impl RemovableMediaMonitor for WindowsRemovableMediaMonitor {
    fn start(&self, tx: Sender<VolumeEvent>) -> Result<(), String> {
        let is_running = self.is_running.clone();
        if is_running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        std::thread::Builder::new()
            .name("lymic-device-monitor".to_string())
            .spawn(move || {
                run_message_loop(tx, is_running);
            })
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}

#[cfg(target_os = "windows")]
struct WindowContext {
    tx: Sender<VolumeEvent>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, CREATESTRUCTW, GWLP_USERDATA,
    };

    if msg == windows_sys::Win32::UI::WindowsAndMessaging::WM_CREATE {
        let createstruct = lparam as *const CREATESTRUCTW;
        if !createstruct.is_null() {
            let user_data = (*createstruct).lpCreateParams;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, user_data as isize);
        }
        return 0;
    }

    if msg == WM_DEVICECHANGE {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowContext;
        if !ptr.is_null() {
            let ctx = &*ptr;
            let event_type = wparam as u32;

            if event_type == DBT_DEVICEARRIVAL && lparam != 0 {
                let hdr = lparam as *const DevBroadcastHdr;
                if (*hdr).dbch_devicetype == DBT_DEVTYP_VOLUME {
                    let vol = lparam as *const DevBroadcastVolume;
                    let unitmask = (*vol).dbcv_unitmask;
                    for bit in 0..26 {
                        if (unitmask & (1 << bit)) != 0 {
                            let drive_letter = (b'A' + bit as u8) as char;
                            if let Some(info) = get_volume_info_for_drive(drive_letter) {
                                let _ = ctx.tx.blocking_send(VolumeEvent::Arrival(info));
                            }
                        }
                    }
                }
            } else if event_type == DBT_DEVICEREMOVECOMPLETE && lparam != 0 {
                let hdr = lparam as *const DevBroadcastHdr;
                if (*hdr).dbch_devicetype == DBT_DEVTYP_VOLUME {
                    let vol = lparam as *const DevBroadcastVolume;
                    let unitmask = (*vol).dbcv_unitmask;
                    for bit in 0..26 {
                        if (unitmask & (1 << bit)) != 0 {
                            let drive_letter = (b'A' + bit as u8) as char;
                            let drive_str = format!("{}:\\", drive_letter);
                            let _ = ctx.tx.blocking_send(VolumeEvent::Removal(drive_str));
                        }
                    }
                }
            }
        }
        return 0;
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn run_message_loop(tx: Sender<VolumeEvent>, is_running: Arc<AtomicBool>) {
    unsafe {
        let h_instance = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = OsStr::new("LymicDeviceMonitorWindow")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };

        RegisterClassExW(&wnd_class);

        let mut context = WindowContext { tx };

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_instance,
            &mut context as *mut _ as *mut _,
        );

        if hwnd.is_null() {
            is_running.store(false, Ordering::SeqCst);
            return;
        }

        let mut msg: MSG = std::mem::zeroed();
        while is_running.load(Ordering::SeqCst) && GetMessageW(&mut msg, hwnd, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        DestroyWindow(hwnd);
        is_running.store(false, Ordering::SeqCst);
    }
}
