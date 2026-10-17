use serde::Serialize;
use std::sync::Mutex;

#[allow(dead_code)] // Used by timeline reconciliation in a follow-up ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaceholderMetadata {
    pub(crate) file_size: u64,
    pub(crate) captured_at: Option<chrono::DateTime<chrono::Utc>>,
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

/// Observable outcome of returning hydrated placeholders to online-only state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CacheClearResult {
    pub(crate) bytes_freed: u64,
    pub(crate) files_dehydrated: u64,
    pub(crate) pinned_files_preserved: u64,
}

#[allow(dead_code)] // Placeholder operations are consumed by timeline reconciliation.
pub(crate) trait CloudFilter: Send + Sync {
    fn register(&self, path: &str) -> Result<(), String>;
    fn connect(&self, path: &str) -> Result<i64, String>;
    fn disconnect(&self, key: i64) -> Result<(), String>;
    fn unregister(&self, path: &str) -> Result<(), String>;
    fn create_directory(&self, path: &str) -> Result<(), String>;
    fn rename_directory(&self, from: &str, to: &str) -> Result<(), String> {
        Err(format!(
            "renaming virtual directories is not supported: {from} -> {to}"
        ))
    }
    fn remove_directory(&self, path: &str) -> Result<(), String> {
        Err(format!(
            "removing virtual directories is not supported: {path}"
        ))
    }
    fn create_placeholder(&self, path: &str, metadata: PlaceholderMetadata) -> Result<(), String>;
    /// Removes a placeholder that no longer has a remote counterpart. Implementations
    /// must return an error for a locked file so reconciliation can retry it later.
    fn remove_placeholder(&self, path: &str) -> Result<(), String> {
        Err(format!(
            "removing virtual placeholders is not supported: {path}"
        ))
    }
    fn clear_local_cache(&self, _: &str) -> Result<CacheClearResult, String> {
        Err("Virtual Library cache clearing is unavailable.".to_string())
    }
    /// Removes the projection after it has been disconnected and deregistered.
    /// This is intentionally separate from cache clearing: disabling the
    /// feature must not leave stale Explorer-visible placeholders behind.
    fn remove_root(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn acknowledge_change(
        &self,
        notification: ChangeNotification,
        request: ChangeRequest,
        completion_status: i32,
    ) -> Result<(), String>;
}

/// The Cloud Files change notification currently being serviced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChangeNotification {
    Delete,
    Rename,
}

/// The opaque keys that identify one pending Cloud Files callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChangeRequest {
    pub(crate) connection_key: i64,
    pub(crate) transfer_key: i64,
    pub(crate) request_key: i64,
}

// Microsoft requires a STATUS_CLOUD_FILE_* value when rejecting an ACK_DELETE
// or ACK_RENAME request. STATUS_ACCESS_DENIED is converted to
// STATUS_CLOUD_FILE_UNSUCCESSFUL by the platform.
pub(crate) const STATUS_CLOUD_FILE_ACCESS_DENIED: i32 = 0xC000_CF18_u32 as i32;

fn deny_change(
    filter: &dyn CloudFilter,
    notification: ChangeNotification,
    request: ChangeRequest,
) -> Result<(), String> {
    filter.acknowledge_change(notification, request, STATUS_CLOUD_FILE_ACCESS_DENIED)
}

pub(crate) struct SyncRootLifecycle {
    api: std::sync::Arc<dyn CloudFilter>,
    state: Mutex<(Option<(String, i64)>, ConnectionStatus)>,
}
impl SyncRootLifecycle {
    pub(crate) fn platform(client: Option<std::sync::Arc<crate::immich::ImmichClient>>) -> Self {
        Self::new(Box::new(PlatformCloudFilter::new(client)))
    }
    pub(crate) fn new(api: Box<dyn CloudFilter>) -> Self {
        Self {
            api: api.into(),
            state: Mutex::new((None, ConnectionStatus::Disconnected)),
        }
    }
    pub(crate) fn projection_filter(&self) -> std::sync::Arc<dyn CloudFilter> {
        self.api.clone()
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
            self.api.remove_root(&path)?;
        }
        state.1 = ConnectionStatus::Disconnected;
        Ok(())
    }
    pub(crate) fn clear_local_cache(&self) -> Result<CacheClearResult, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?;
        let (path, _) = state
            .0
            .as_ref()
            .ok_or_else(|| "Virtual Library is not enabled.".to_string())?;
        self.api.clear_local_cache(path)
    }
}

#[cfg(not(windows))]
struct PlatformCloudFilter;
#[cfg(not(windows))]
impl PlatformCloudFilter {
    fn new(_: Option<std::sync::Arc<crate::immich::ImmichClient>>) -> Self {
        Self
    }
}
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
    fn remove_placeholder(&self, _: &str) -> Result<(), String> {
        Err("Virtual Library is only available on Windows".into())
    }
    fn clear_local_cache(&self, _: &str) -> Result<CacheClearResult, String> {
        Err("Virtual Library is only available on Windows".into())
    }
    fn acknowledge_change(
        &self,
        _: ChangeNotification,
        _: ChangeRequest,
        _: i32,
    ) -> Result<(), String> {
        Err("Virtual Library is only available on Windows".into())
    }
}

#[cfg(windows)]
struct PlatformCloudFilter {
    hydration: std::sync::RwLock<Option<std::sync::Arc<crate::virtual_library::hydration::HydrationDispatcher>>>,
}
#[cfg(windows)]
impl PlatformCloudFilter {
    fn new(client: Option<std::sync::Arc<crate::immich::ImmichClient>>) -> Self {
        Self {
            hydration: std::sync::RwLock::new(client.map(|client| {
                std::sync::Arc::new(crate::virtual_library::hydration::HydrationDispatcher::new(
                    client,
                    std::sync::Arc::new(native::WindowsDataTransfer::default()),
                ))
            })),
        }
    }

    fn hydration_dispatcher(
        &self,
    ) -> Option<std::sync::Arc<crate::virtual_library::hydration::HydrationDispatcher>> {
        if let Ok(guard) = self.hydration.read() {
            if let Some(dispatcher) = guard.as_ref() {
                return Some(dispatcher.clone());
            }
        }
        let credentials = crate::auth::get_credentials().ok().flatten()?;
        let client = crate::immich::ImmichClient::new(
            credentials.server_url,
            credentials.api_key,
            credentials.allow_local_http,
            credentials.cloudflare_client_id,
            credentials.cloudflare_client_secret,
        )
        .ok()?;
        let dispatcher = std::sync::Arc::new(crate::virtual_library::hydration::HydrationDispatcher::new(
            std::sync::Arc::new(client),
            std::sync::Arc::new(native::WindowsDataTransfer::default()),
        ));
        if let Ok(mut guard) = self.hydration.write() {
            *guard = Some(dispatcher.clone());
        }
        Some(dispatcher)
    }

    fn cancel_hydration(&self, request: crate::virtual_library::hydration::FetchDataRequest) {
        if let Some(hydration) = self.hydration_dispatcher() {
            hydration.cancel(request);
        }
    }
}
#[cfg(windows)]
impl CloudFilter for PlatformCloudFilter {
    fn register(&self, path: &str) -> Result<(), String> {
        native::register(path)
    }
    fn connect(&self, path: &str) -> Result<i64, String> {
        native::connect(path, self)
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
    fn rename_directory(&self, from: &str, to: &str) -> Result<(), String> {
        std::fs::rename(from, to).map_err(|error| error.to_string())
    }
    fn remove_directory(&self, path: &str) -> Result<(), String> {
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())
    }
    fn create_placeholder(&self, path: &str, metadata: PlaceholderMetadata) -> Result<(), String> {
        native::create_placeholder(path, metadata)
    }
    fn remove_placeholder(&self, path: &str) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
    fn clear_local_cache(&self, path: &str) -> Result<CacheClearResult, String> {
        native::clear_local_cache(path)
    }
    fn remove_root(&self, path: &str) -> Result<(), String> {
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
    fn acknowledge_change(
        &self,
        notification: ChangeNotification,
        request: ChangeRequest,
        completion_status: i32,
    ) -> Result<(), String> {
        native::acknowledge_change(notification, request, completion_status)
    }
}
#[cfg(windows)]
mod native {
    use super::CacheClearResult;
    #[allow(dead_code)]
    // Some Cloud Filter calls are consumed by follow-up reconciliation work.
    use super::{ChangeNotification, ChangeRequest, PlaceholderMetadata, PlatformCloudFilter};
    use crate::virtual_library::hydration::{DataTransfer, FetchDataRequest};
    use bytes::Bytes;
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
        let registration_result = unsafe {
            CfRegisterSyncRoot(
                path.as_ptr(),
                &registration,
                &policies,
                CF_REGISTER_FLAG_NONE,
            )
        };
        // Development restarts (and normal application restarts) reconnect to
        // an existing registration. Cloud Files reports this as already
        // connected instead of requiring the sync root to be registered again.
        const ERROR_CLOUD_FILE_SYNC_ROOT_ALREADY_CONNECTED: i32 = 0x8007_017A_u32 as i32;
        if registration_result >= 0
            || registration_result == ERROR_CLOUD_FILE_SYNC_ROOT_ALREADY_CONNECTED
        {
            Ok(())
        } else {
            result("CfRegisterSyncRoot", registration_result)
        }
    }
    pub(super) fn connect(path: &str, filter: &PlatformCloudFilter) -> Result<i64, String> {
        let path = wide(path);
        let table = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch_data),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
                Callback: Some(cancel_fetch_data),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
                Callback: Some(fetch_placeholders),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DELETE,
                Callback: Some(deny_delete),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_RENAME,
                Callback: Some(deny_rename),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        let mut key = 0;
        result("CfConnectSyncRoot", unsafe {
            CfConnectSyncRoot(
                path.as_ptr(),
                table.as_ptr(),
                (filter as *const PlatformCloudFilter).cast_mut().cast(),
                CF_CONNECT_FLAG_NONE,
                &mut key,
            )
        })?;
        Ok(key)
    }
    unsafe extern "system" fn fetch_data(
        callback_info: *const CF_CALLBACK_INFO,
        callback_parameters: *const CF_CALLBACK_PARAMETERS,
    ) {
        let (Some(info), Some(parameters)) = (callback_info.as_ref(), callback_parameters.as_ref())
        else {
            return;
        };
        let filter = (info.CallbackContext as *const PlatformCloudFilter).as_ref();
        let Some(dispatcher) = filter.and_then(|filter| filter.hydration_dispatcher()) else {
            eprintln!("fetch_data callback: no hydration dispatcher available");
            return;
        };
        let identity = std::slice::from_raw_parts(
            info.FileIdentity.cast::<u8>(),
            info.FileIdentityLength as usize,
        );
        let asset_id = serde_json::from_slice::<serde_json::Value>(identity)
            .ok()
            .and_then(|identity| identity.get("assetId")?.as_str().map(str::to_owned));
        let fetch = parameters.Anonymous.FetchData;
        eprintln!(
            "fetch_data callback: asset_id={:?}, offset={}, len={}",
            asset_id, fetch.RequiredFileOffset, fetch.RequiredLength
        );
        let Some(asset_id) =
            asset_id.filter(|_| fetch.RequiredFileOffset >= 0 && fetch.RequiredLength >= 0)
        else {
            return;
        };
        dispatcher.dispatch(
            crate::immich::RemoteAssetId::new(asset_id),
            FetchDataRequest {
                connection_key: info.ConnectionKey,
                transfer_key: info.TransferKey,
                request_key: info.RequestKey,
                required_offset: fetch.RequiredFileOffset,
                required_length: fetch.RequiredLength,
            },
        );
    }

    const fn cf_size_of_op_param<T>() -> u32 {
        (std::mem::offset_of!(CF_OPERATION_PARAMETERS, Anonymous) + std::mem::size_of::<T>()) as u32
    }

    unsafe extern "system" fn fetch_placeholders(
        callback_info: *const CF_CALLBACK_INFO,
        _: *const CF_CALLBACK_PARAMETERS,
    ) {
        let Some(info) = callback_info.as_ref() else {
            return;
        };
        let mut parameters = CF_OPERATION_PARAMETERS {
            ParamSize: cf_size_of_op_param::<CF_OPERATION_PARAMETERS_0_7>(),
            Anonymous: CF_OPERATION_PARAMETERS_0 {
                TransferPlaceholders: CF_OPERATION_PARAMETERS_0_7 {
                    Flags: CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_DISABLE_ON_DEMAND_POPULATION,
                    CompletionStatus: 0,
                    PlaceholderTotalCount: 0,
                    PlaceholderArray: std::ptr::null_mut(),
                    PlaceholderCount: 0,
                    EntriesProcessed: 0,
                },
            },
        };
        let operation = CF_OPERATION_INFO {
            StructSize: std::mem::size_of::<CF_OPERATION_INFO>() as u32,
            Type: CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
            ConnectionKey: info.ConnectionKey,
            TransferKey: info.TransferKey,
            CorrelationVector: std::ptr::null(),
            SyncStatus: std::ptr::null(),
            RequestKey: info.RequestKey,
        };
        let _ = CfExecute(&operation, &mut parameters);
    }

    unsafe extern "system" fn cancel_fetch_data(
        callback_info: *const CF_CALLBACK_INFO,
        _: *const CF_CALLBACK_PARAMETERS,
    ) {
        let Some(info) = callback_info.as_ref() else {
            return;
        };
        let Some(filter) = (info.CallbackContext as *const PlatformCloudFilter).as_ref() else {
            return;
        };
        filter.cancel_hydration(FetchDataRequest {
            connection_key: info.ConnectionKey,
            transfer_key: info.TransferKey,
            request_key: info.RequestKey,
            required_offset: 0,
            required_length: 0,
        });
    }

    #[derive(Default)]
    pub(super) struct WindowsDataTransfer {
        pending: std::sync::Mutex<std::collections::HashMap<i64, (i64, Vec<u8>)>>,
    }

    impl WindowsDataTransfer {
        fn execute_chunk(
            request: FetchDataRequest,
            offset: i64,
            bytes: &[u8],
        ) -> Result<(), String> {
            if bytes.is_empty() {
                return Ok(());
            }
            let length: i64 = bytes
                .len()
                .try_into()
                .map_err(|_| "original chunk is too large")?;
            let mut parameters = CF_OPERATION_PARAMETERS {
                ParamSize: cf_size_of_op_param::<CF_OPERATION_PARAMETERS_0_6>(),
                Anonymous: CF_OPERATION_PARAMETERS_0 {
                    TransferData: CF_OPERATION_PARAMETERS_0_6 {
                        Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                        CompletionStatus: 0,
                        Buffer: bytes.as_ptr().cast(),
                        Offset: offset,
                        Length: length,
                    },
                },
            };
            let operation = CF_OPERATION_INFO {
                StructSize: std::mem::size_of::<CF_OPERATION_INFO>() as u32,
                Type: CF_OPERATION_TYPE_TRANSFER_DATA,
                ConnectionKey: request.connection_key,
                TransferKey: request.transfer_key,
                CorrelationVector: std::ptr::null(),
                SyncStatus: std::ptr::null(),
                RequestKey: request.request_key,
            };
            let hr = unsafe { CfExecute(&operation, &mut parameters) };
            if hr < 0 {
                eprintln!(
                    "CfExecute TransferData failed: hr=0x{:08X}, offset={}, len={}",
                    hr as u32, offset, length
                );
                return Err(format!("CfExecute TransferData failed (0x{:08X})", hr as u32));
            }
            Ok(())
        }
    }

    impl DataTransfer for WindowsDataTransfer {
        fn report_progress(
            &self,
            request: FetchDataRequest,
            total: i64,
            completed: i64,
        ) -> Result<(), String> {
            let res = unsafe {
                CfReportProviderProgress2(
                    request.connection_key,
                    request.transfer_key,
                    request.request_key,
                    total,
                    completed,
                    0,
                )
            };
            if res < 0 {
                eprintln!("CfReportProviderProgress2 returned 0x{:08X}", res as u32);
            }
            Ok(())
        }

        fn transfer_data(
            &self,
            request: FetchDataRequest,
            offset: i64,
            bytes: Bytes,
            completed: bool,
        ) -> Result<(), String> {
            let mut chunks_to_send = Vec::new();
            {
                let mut pending = self.pending.lock().unwrap();
                let entry = pending
                    .entry(request.transfer_key)
                    .or_insert_with(|| (offset, Vec::new()));
                if !bytes.is_empty() {
                    entry.1.extend_from_slice(&bytes);
                }

                const ALIGNMENT: usize = 4096;

                while entry.1.len() >= ALIGNMENT && !completed {
                    let send_len = (entry.1.len() / ALIGNMENT) * ALIGNMENT;
                    let chunk_offset = entry.0;
                    let chunk_bytes = entry.1[..send_len].to_vec();
                    entry.0 += send_len as i64;
                    entry.1.drain(..send_len);
                    chunks_to_send.push((chunk_offset, chunk_bytes));
                }

                if completed {
                    let (final_offset, final_bytes) = pending
                        .remove(&request.transfer_key)
                        .unwrap_or((offset, Vec::new()));
                    if !final_bytes.is_empty() {
                        chunks_to_send.push((final_offset, final_bytes));
                    }
                }
            }

            for (chunk_offset, chunk_bytes) in chunks_to_send {
                Self::execute_chunk(request, chunk_offset, &chunk_bytes)?;
            }

            Ok(())
        }
    }
    unsafe extern "system" fn deny_delete(
        callback_info: *const CF_CALLBACK_INFO,
        _: *const CF_CALLBACK_PARAMETERS,
    ) {
        if let Some(callback_info) = callback_info.as_ref() {
            if let Some(filter) = (callback_info.CallbackContext as *const PlatformCloudFilter).as_ref() {
                let _ = super::deny_change(
                    filter,
                    ChangeNotification::Delete,
                    ChangeRequest {
                        connection_key: callback_info.ConnectionKey,
                        transfer_key: callback_info.TransferKey,
                        request_key: callback_info.RequestKey,
                    },
                );
            }
        }
    }
    unsafe extern "system" fn deny_rename(
        callback_info: *const CF_CALLBACK_INFO,
        _: *const CF_CALLBACK_PARAMETERS,
    ) {
        if let Some(callback_info) = callback_info.as_ref() {
            if let Some(filter) = (callback_info.CallbackContext as *const PlatformCloudFilter).as_ref() {
                let _ = super::deny_change(
                    filter,
                    ChangeNotification::Rename,
                    ChangeRequest {
                        connection_key: callback_info.ConnectionKey,
                        transfer_key: callback_info.TransferKey,
                        request_key: callback_info.RequestKey,
                    },
                );
            }
        }
    }
    pub(super) fn acknowledge_change(
        notification: ChangeNotification,
        request: ChangeRequest,
        completion_status: i32,
    ) -> Result<(), String> {
        let (operation_type, mut parameters) = match notification {
            ChangeNotification::Delete => (
                CF_OPERATION_TYPE_ACK_DELETE,
                CF_OPERATION_PARAMETERS {
                    ParamSize: cf_size_of_op_param::<CF_OPERATION_PARAMETERS_0_2>(),
                    Anonymous: CF_OPERATION_PARAMETERS_0 {
                        AckDelete: CF_OPERATION_PARAMETERS_0_2 {
                            Flags: CF_OPERATION_ACK_DELETE_FLAG_NONE,
                            CompletionStatus: completion_status,
                        },
                    },
                },
            ),
            ChangeNotification::Rename => (
                CF_OPERATION_TYPE_ACK_RENAME,
                CF_OPERATION_PARAMETERS {
                    ParamSize: cf_size_of_op_param::<CF_OPERATION_PARAMETERS_0_3>(),
                    Anonymous: CF_OPERATION_PARAMETERS_0 {
                        AckRename: CF_OPERATION_PARAMETERS_0_3 {
                            Flags: CF_OPERATION_ACK_RENAME_FLAG_NONE,
                            CompletionStatus: completion_status,
                        },
                    },
                },
            ),
        };
        let operation = CF_OPERATION_INFO {
            StructSize: std::mem::size_of::<CF_OPERATION_INFO>() as u32,
            Type: operation_type,
            ConnectionKey: request.connection_key,
            TransferKey: request.transfer_key,
            CorrelationVector: std::ptr::null(),
            SyncStatus: std::ptr::null(),
            RequestKey: request.request_key,
        };
        result("CfExecute", unsafe {
            CfExecute(&operation, &mut parameters)
        })
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
        let timestamp = metadata
            .captured_at
            .map(windows_file_time)
            .unwrap_or_default();
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
            Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC | CF_PLACEHOLDER_CREATE_FLAG_SUPERSEDE,
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
    pub(super) fn clear_local_cache(path: &str) -> Result<CacheClearResult, String> {
        let mut summary = CacheClearResult::default();
        for entry in walkdir::WalkDir::new(path) {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let handle = open_placeholder(path)?;
            let operation = (|| -> Result<(), String> {
                let standard_info = placeholder_standard_info(handle)?;
                if standard_info.PinState == CF_PIN_STATE_PINNED {
                    summary.pinned_files_preserved += 1;
                } else if standard_info.OnDiskDataSize > 0 {
                    summary.bytes_freed = summary
                        .bytes_freed
                        .saturating_add(standard_info.OnDiskDataSize as u64);
                    summary.files_dehydrated += 1;
                    result("CfDehydratePlaceholder", unsafe {
                        CfDehydratePlaceholder(
                            handle,
                            0,
                            0,
                            CF_DEHYDRATE_FLAG_NONE,
                            std::ptr::null_mut(),
                        )
                    })?;
                }
                Ok(())
            })();
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            operation?;
        }
        Ok(summary)
    }
    fn open_placeholder(path: &Path) -> Result<windows_sys::Win32::Foundation::HANDLE, String> {
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        let display_path = path.to_string_lossy();
        let path = wide(&display_path);
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        (handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE)
            .then_some(handle)
            .ok_or_else(|| format!("Could not open '{display_path}' for cache clearing"))
    }
    fn placeholder_standard_info(
        handle: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<CF_PLACEHOLDER_STANDARD_INFO, String> {
        let mut buffer = vec![0_u8; std::mem::size_of::<CF_PLACEHOLDER_STANDARD_INFO>() + 4096];
        let mut returned = 0;
        let res = unsafe {
            CfGetPlaceholderInfo(
                handle,
                CF_PLACEHOLDER_INFO_STANDARD,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut returned,
            )
        };
        const ERROR_MORE_DATA: i32 = 0x800700EA_u32 as i32;
        if res == ERROR_MORE_DATA && returned > buffer.len() as u32 {
            buffer.resize(returned as usize, 0);
            result("CfGetPlaceholderInfo", unsafe {
                CfGetPlaceholderInfo(
                    handle,
                    CF_PLACEHOLDER_INFO_STANDARD,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut returned,
                )
            })?;
        } else if res < 0 {
            return Err(format!("CfGetPlaceholderInfo failed (HRESULT 0x{:08X})", res as u32));
        }
        let info = unsafe { *(buffer.as_ptr() as *const CF_PLACEHOLDER_STANDARD_INFO) };
        Ok(info)
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
        fn remove_root(&self, _: &str) -> Result<(), String> {
            self.0.lock().unwrap().push("remove_root");
            Ok(())
        }
        fn create_directory(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn create_placeholder(&self, _: &str, _: PlaceholderMetadata) -> Result<(), String> {
            Ok(())
        }
        fn clear_local_cache(&self, _: &str) -> Result<CacheClearResult, String> {
            self.0.lock().unwrap().push("clear_local_cache");
            Ok(CacheClearResult {
                bytes_freed: 1024,
                files_dehydrated: 1,
                pinned_files_preserved: 1,
            })
        }
        fn acknowledge_change(
            &self,
            _: ChangeNotification,
            _: ChangeRequest,
            _: i32,
        ) -> Result<(), String> {
            self.0.lock().unwrap().push("acknowledge_change");
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
            [
                "register",
                "connect",
                "disconnect",
                "unregister",
                "remove_root"
            ]
        );
    }

    #[test]
    fn clearing_the_local_cache_dehydrates_unpinned_files_and_reports_reclaimed_space() {
        let calls = Arc::new(Mutex::new(vec![]));
        let root = SyncRootLifecycle::new(Box::new(Fake(calls.clone())));
        root.enable("C:\\Library").unwrap();

        assert_eq!(
            root.clear_local_cache().unwrap(),
            CacheClearResult {
                bytes_freed: 1024,
                files_dehydrated: 1,
                pinned_files_preserved: 1,
            }
        );
        assert_eq!(
            *calls.lock().unwrap(),
            ["register", "connect", "clear_local_cache"]
        );
    }

    #[test]
    fn clearing_the_local_cache_requires_an_enabled_sync_root() {
        let root = SyncRootLifecycle::new(Box::new(Fake(Arc::new(Mutex::new(vec![])))));

        assert_eq!(
            root.clear_local_cache().unwrap_err(),
            "Virtual Library is not enabled."
        );
    }

    #[test]
    fn deleting_a_virtual_placeholder_is_synchronously_rejected_without_a_remote_mutation() {
        #[derive(Default)]
        struct RecordingFilter(Mutex<Vec<(ChangeNotification, ChangeRequest, i32)>>);
        impl CloudFilter for RecordingFilter {
            fn register(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn connect(&self, _: &str) -> Result<i64, String> {
                Ok(1)
            }
            fn disconnect(&self, _: i64) -> Result<(), String> {
                Ok(())
            }
            fn unregister(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn create_directory(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn create_placeholder(&self, _: &str, _: PlaceholderMetadata) -> Result<(), String> {
                Ok(())
            }
            fn acknowledge_change(
                &self,
                notification: ChangeNotification,
                request: ChangeRequest,
                completion_status: i32,
            ) -> Result<(), String> {
                self.0
                    .lock()
                    .unwrap()
                    .push((notification, request, completion_status));
                Ok(())
            }
        }
        let filter = RecordingFilter::default();
        let request = ChangeRequest {
            connection_key: 1,
            transfer_key: 2,
            request_key: 3,
        };

        deny_change(&filter, ChangeNotification::Delete, request).unwrap();

        assert_eq!(
            *filter.0.lock().unwrap(),
            [(
                ChangeNotification::Delete,
                request,
                STATUS_CLOUD_FILE_ACCESS_DENIED
            )]
        );
    }

    #[test]
    fn renaming_a_virtual_placeholder_is_synchronously_rejected_with_a_cloud_file_status() {
        #[derive(Default)]
        struct RecordingFilter(Mutex<Vec<(ChangeNotification, ChangeRequest, i32)>>);
        impl CloudFilter for RecordingFilter {
            fn register(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn connect(&self, _: &str) -> Result<i64, String> {
                Ok(1)
            }
            fn disconnect(&self, _: i64) -> Result<(), String> {
                Ok(())
            }
            fn unregister(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn create_directory(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn create_placeholder(&self, _: &str, _: PlaceholderMetadata) -> Result<(), String> {
                Ok(())
            }
            fn acknowledge_change(
                &self,
                notification: ChangeNotification,
                request: ChangeRequest,
                completion_status: i32,
            ) -> Result<(), String> {
                self.0
                    .lock()
                    .unwrap()
                    .push((notification, request, completion_status));
                Ok(())
            }
        }
        let filter = RecordingFilter::default();
        let request = ChangeRequest {
            connection_key: 4,
            transfer_key: 5,
            request_key: 6,
        };

        deny_change(&filter, ChangeNotification::Rename, request).unwrap();

        assert_eq!(
            *filter.0.lock().unwrap(),
            [(
                ChangeNotification::Rename,
                request,
                STATUS_CLOUD_FILE_ACCESS_DENIED
            )]
        );
    }
}

