use serde::Serialize;
use std::sync::Mutex;

#[allow(dead_code)] // Used by timeline reconciliation in a follow-up ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaceholderMetadata {
    pub(crate) file_size: u64,
    pub(crate) captured_at: chrono::DateTime<chrono::Utc>,
    pub(crate) asset_id: String,
    pub(crate) version: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConnectionStatus {
    Connected,
    Disconnected,
    Error,
}

#[allow(dead_code)] // Placeholder operations are consumed by timeline reconciliation.
pub(crate) trait CloudFilter: Send + Sync {
    fn register(&self, path: &str) -> Result<(), String>;
    fn connect(&self, path: &str) -> Result<i64, String>;
    fn disconnect(&self, key: i64) -> Result<(), String>;
    fn unregister(&self, path: &str) -> Result<(), String>;
    fn create_directory(&self, path: &str) -> Result<(), String>;
    fn create_placeholder(&self, path: &str, metadata: PlaceholderMetadata) -> Result<(), String>;
}

pub(crate) struct SyncRootLifecycle {
    api: Box<dyn CloudFilter>,
    state: Mutex<(Option<(String, i64)>, ConnectionStatus)>,
}
impl SyncRootLifecycle {
    pub(crate) fn platform() -> Self {
        Self::new(Box::new(PlatformCloudFilter))
    }
    pub(crate) fn new(api: Box<dyn CloudFilter>) -> Self {
        Self {
            api,
            state: Mutex::new((None, ConnectionStatus::Disconnected)),
        }
    }
    pub(crate) fn status(&self) -> ConnectionStatus {
        self.state.lock().expect("sync root mutex poisoned").1
    }
    pub(crate) fn enable(&self, path: &str) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if state.0.as_ref().is_some_and(|(current, _)| current == path) {
            return Ok(());
        }
        if let Some((old, key)) = state.0.take() {
            self.api.disconnect(key)?;
            self.api.unregister(&old)?;
        }
        if let Err(error) = self.api.register(path) {
            state.1 = ConnectionStatus::Error;
            return Err(error);
        }
        match self.api.connect(path) {
            Ok(key) => {
                state.0 = Some((path.to_owned(), key));
                state.1 = ConnectionStatus::Connected;
                Ok(())
            }
            Err(error) => {
                let _ = self.api.unregister(path);
                state.1 = ConnectionStatus::Error;
                Err(error)
            }
        }
    }
    pub(crate) fn disable(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if let Some((path, key)) = state.0.take() {
            self.api.disconnect(key)?;
            self.api.unregister(&path)?;
        }
        state.1 = ConnectionStatus::Disconnected;
        Ok(())
    }
}

#[cfg(not(windows))]
struct PlatformCloudFilter;
#[cfg(not(windows))]
impl CloudFilter for PlatformCloudFilter {
    fn register(&self, _: &str) -> Result<(), String> {
        Err("Virtual Library is only available on Windows".into())
    }
    fn connect(&self, _: &str) -> Result<i64, String> {
        unreachable!()
    }
    fn disconnect(&self, _: i64) -> Result<(), String> {
        Ok(())
    }
    fn unregister(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_directory(&self, _: &str) -> Result<(), String> {
        Err("Virtual Library is only available on Windows".into())
    }
    fn create_placeholder(&self, _: &str, _: PlaceholderMetadata) -> Result<(), String> {
        Err("Virtual Library is only available on Windows".into())
    }
}

#[cfg(windows)]
struct PlatformCloudFilter;
#[cfg(windows)]
impl CloudFilter for PlatformCloudFilter {
    fn register(&self, path: &str) -> Result<(), String> {
        native::register(path)
    }
    fn connect(&self, path: &str) -> Result<i64, String> {
        native::connect(path)
    }
    fn disconnect(&self, key: i64) -> Result<(), String> {
        native::result("CfDisconnectSyncRoot", unsafe {
            windows_sys::Win32::Storage::CloudFilters::CfDisconnectSyncRoot(key)
        })
    }
    fn unregister(&self, path: &str) -> Result<(), String> {
        native::unregister(path)
    }
    fn create_directory(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|error| error.to_string())
    }
    fn create_placeholder(&self, path: &str, metadata: PlaceholderMetadata) -> Result<(), String> {
        native::create_placeholder(path, metadata)
    }
}
#[cfg(windows)]
mod native {
    #![allow(dead_code)] // Some Cloud Filter calls are consumed by follow-up reconciliation work.
    use super::PlaceholderMetadata;
    use std::path::Path;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Storage::CloudFilters::*;
    pub(super) fn wide(v: &str) -> Vec<u16> {
        v.encode_utf16().chain(Some(0)).collect()
    }
    pub(super) fn result(op: &str, hr: i32) -> Result<(), String> {
        (hr >= 0)
            .then_some(())
            .ok_or_else(|| format!("{op} failed (HRESULT 0x{hr:08X})"))
    }
    pub(super) fn register(path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
        let path = wide(path);
        let name = wide("Lymic");
        let version = wide(env!("CARGO_PKG_VERSION"));
        let registration = CF_SYNC_REGISTRATION {
            StructSize: std::mem::size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: name.as_ptr(),
            ProviderVersion: version.as_ptr(),
            SyncRootIdentity: b"lymic".as_ptr().cast(),
            SyncRootIdentityLength: 5,
            FileIdentity: std::ptr::null(),
            FileIdentityLength: 0,
            ProviderId: GUID::from_u128(0x6b3af5bc_6e32_40d8_b0a2_2c14b3fa481c),
        };
        let policies = CF_SYNC_POLICIES {
            StructSize: std::mem::size_of::<CF_SYNC_POLICIES>() as u32,
            Hydration: CF_HYDRATION_POLICY {
                Primary: CF_HYDRATION_POLICY_PARTIAL,
                Modifier: CF_HYDRATION_POLICY_MODIFIER_NONE,
            },
            Population: CF_POPULATION_POLICY {
                Primary: CF_POPULATION_POLICY_PARTIAL,
                Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
            },
            InSync: CF_INSYNC_POLICY_NONE,
            HardLink: CF_HARDLINK_POLICY_NONE,
            PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT,
        };
        result("CfRegisterSyncRoot", unsafe {
            CfRegisterSyncRoot(
                path.as_ptr(),
                &registration,
                &policies,
                CF_REGISTER_FLAG_NONE,
            )
        })
    }
    pub(super) fn connect(path: &str) -> Result<i64, String> {
        let path = wide(path);
        let table = [CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        }];
        let mut key = 0;
        result("CfConnectSyncRoot", unsafe {
            CfConnectSyncRoot(
                path.as_ptr(),
                table.as_ptr(),
                std::ptr::null(),
                CF_CONNECT_FLAG_NONE,
                &mut key,
            )
        })?;
        Ok(key)
    }
    pub(super) fn unregister(path: &str) -> Result<(), String> {
        let path = wide(path);
        result("CfUnregisterSyncRoot", unsafe {
            CfUnregisterSyncRoot(path.as_ptr())
        })
    }
    pub(super) fn create_placeholder(
        path: &str,
        metadata: PlaceholderMetadata,
    ) -> Result<(), String> {
        let path = Path::new(path);
        let directory = path
            .parent()
            .ok_or_else(|| "placeholder path has no parent directory".to_string())?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "placeholder path has no Unicode file name".to_string())?;
        let directory = wide(&directory.to_string_lossy());
        let name = wide(name);
        let identity = serde_json::to_vec(&serde_json::json!({
            "assetId": metadata.asset_id,
            "version": metadata.version,
        }))
        .map_err(|error| error.to_string())?;
        let timestamp = windows_file_time(metadata.captured_at);
        let size = i64::try_from(metadata.file_size)
            .map_err(|_| "remote file size exceeds Windows maximum".to_string())?;
        let mut placeholder = CF_PLACEHOLDER_CREATE_INFO {
            RelativeFileName: name.as_ptr(),
            FsMetadata: CF_FS_METADATA {
                BasicInfo: windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO {
                    CreationTime: timestamp,
                    LastAccessTime: timestamp,
                    LastWriteTime: timestamp,
                    ChangeTime: timestamp,
                    FileAttributes: windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
                },
                FileSize: size,
            },
            FileIdentity: identity.as_ptr().cast(),
            FileIdentityLength: identity
                .len()
                .try_into()
                .map_err(|_| "file identity is too long".to_string())?,
            Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
            Result: 0,
            CreateUsn: 0,
        };
        let mut processed = 0;
        result("CfCreatePlaceholders", unsafe {
            CfCreatePlaceholders(
                directory.as_ptr(),
                &mut placeholder,
                1,
                CF_CREATE_FLAG_NONE,
                &mut processed,
            )
        })?;
        (processed == 1 && placeholder.Result >= 0)
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "CfCreatePlaceholders failed (HRESULT 0x{:08X})",
                    placeholder.Result
                )
            })
    }
    fn windows_file_time(timestamp: chrono::DateTime<chrono::Utc>) -> i64 {
        const WINDOWS_EPOCH_OFFSET_SECONDS: i64 = 11_644_473_600;
        timestamp
            .timestamp()
            .saturating_add(WINDOWS_EPOCH_OFFSET_SECONDS)
            .saturating_mul(10_000_000)
            .saturating_add(i64::from(timestamp.timestamp_subsec_nanos() / 100))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    struct Fake(Arc<Mutex<Vec<&'static str>>>);
    impl CloudFilter for Fake {
        fn register(&self, _: &str) -> Result<(), String> {
            self.0.lock().unwrap().push("register");
            Ok(())
        }
        fn connect(&self, _: &str) -> Result<i64, String> {
            self.0.lock().unwrap().push("connect");
            Ok(1)
        }
        fn disconnect(&self, _: i64) -> Result<(), String> {
            self.0.lock().unwrap().push("disconnect");
            Ok(())
        }
        fn unregister(&self, _: &str) -> Result<(), String> {
            self.0.lock().unwrap().push("unregister");
            Ok(())
        }
        fn create_directory(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_placeholder(&self, _: &str, _: PlaceholderMetadata) -> Result<(), String> {
            Ok(())
        }
    }
    #[test]
    fn lifecycle_registers_connects_and_cleans_up() {
        let calls = Arc::new(Mutex::new(vec![]));
        let root = SyncRootLifecycle::new(Box::new(Fake(calls.clone())));
        root.enable("test").unwrap();
        root.disable().unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            ["register", "connect", "disconnect", "unregister"]
        );
    }
}
