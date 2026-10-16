use serde::Serialize;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConnectionStatus {
    Connected,
    Disconnected,
    Error,
}

pub(crate) trait CloudFilter: Send + Sync {
    fn register(&self, path: &str) -> Result<(), String>;
    fn connect(&self, path: &str) -> Result<i64, String>;
    fn disconnect(&self, key: i64) -> Result<(), String>;
    fn unregister(&self, path: &str) -> Result<(), String>;
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
}
#[cfg(windows)]
mod native {
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
