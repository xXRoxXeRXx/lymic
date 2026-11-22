//! The Cloud Filter callbacks: `FETCH_PLACEHOLDERS` (a directory listed for the first time) and
//! `FETCH_DATA` (a placeholder opened). Each one copies what it needs, spawns the work onto the
//! Tokio runtime and returns at once; the spawned work answers through `CfExecute`.
//!
//! `NOTIFY_DELETE` and `NOTIFY_RENAME` (#279) are the exception to "spawn and return": they only
//! refuse (`write_guard`), do no I/O, and acknowledge inline.

use crate::cloud_files::failure::{CloudStatus, Failure, FailureKind};
use crate::cloud_files::hydration::{self, Block, DataRequest, DataSink};
use crate::cloud_files::identity;
use crate::cloud_files::inflight::InFlight;
use crate::cloud_files::library::StoredCredentialsImmich;
use crate::cloud_files::placeholder::PlaceholderSpec;
use crate::cloud_files::population;
use crate::cloud_files::reporting::{self, Operation, Report};
use crate::cloud_files::write_guard;
use std::mem::{offset_of, size_of};
use std::path::Path;
use std::sync::LazyLock;
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    NTSTATUS, STATUS_CLOUD_FILE_AUTHENTICATION_FAILED, STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE,
    STATUS_CLOUD_FILE_UNSUCCESSFUL, STATUS_OBJECT_NAME_NOT_FOUND,
};
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO,
};

const STATUS_SUCCESS: NTSTATUS = NTSTATUS(0);
/// The downloads in flight, so `CANCEL_FETCH_DATA` can stop one.
static IN_FLIGHT: LazyLock<InFlight> = LazyLock::new(InFlight::default);
/// Placeholders handed over per `TRANSFER_PLACEHOLDERS` call.
const PLACEHOLDER_BATCH: usize = 1000;

fn ntstatus(status: CloudStatus) -> NTSTATUS {
    match status {
        CloudStatus::NetworkUnavailable => STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE,
        CloudStatus::AuthenticationFailed => STATUS_CLOUD_FILE_AUTHENTICATION_FAILED,
        CloudStatus::NotFound => STATUS_OBJECT_NAME_NOT_FOUND,
        CloudStatus::Unsuccessful => STATUS_CLOUD_FILE_UNSUCCESSFUL,
    }
}

/// What a callback copies out of the platform's structures, which are only valid while the callback
/// runs. The keys are plain integers, so the request can move to another thread.
pub(super) struct Request {
    connection: CF_CONNECTION_KEY,
    transfer: i64,
    request: i64,
    /// The placeholder's file identity: its path relative to the Virtual Library root.
    identity: Vec<u8>,
    file_size: i64,
}

impl Request {
    /// # Safety
    /// `info` must be the `CF_CALLBACK_INFO` the platform passed to the running callback.
    unsafe fn capture(info: *const CF_CALLBACK_INFO) -> Self {
        let info = unsafe { &*info };
        let identity = if info.FileIdentity.is_null() || info.FileIdentityLength == 0 {
            Vec::new()
        } else {
            // SAFETY: the platform guarantees `FileIdentityLength` readable bytes at `FileIdentity`.
            unsafe {
                std::slice::from_raw_parts(
                    info.FileIdentity.cast::<u8>(),
                    info.FileIdentityLength as usize,
                )
            }
            .to_vec()
        };
        Self {
            connection: info.ConnectionKey,
            transfer: info.TransferKey,
            request: info.RequestKey,
            identity,
            file_size: info.FileSize,
        }
    }

    /// The path the placeholder stands for, `""` for the root.
    fn path(&self) -> Result<String, Failure> {
        identity::decode(&self.identity).ok_or_else(|| {
            Failure::new(
                FailureKind::NotFound,
                "the placeholder identity is not a path",
            )
        })
    }
}

/// `CF_OPERATION_INFO` for a request captured earlier.
pub(super) fn operation_info(request: &Request, kind: CF_OPERATION_TYPE) -> CF_OPERATION_INFO {
    CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: kind,
        ConnectionKey: request.connection,
        TransferKey: request.transfer,
        CorrelationVector: std::ptr::null(),
        SyncStatus: std::ptr::null(),
        RequestKey: request.request,
    }
}

/// `CF_SIZE_OF_OP_PARAM`: the offset of the union plus the size of the member used.
fn parameters(member_size: usize) -> CF_OPERATION_PARAMETERS {
    CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous) + member_size) as u32,
        ..Default::default()
    }
}

/// Diagnostics for failures nobody is waiting on. User-facing notices go through `reporting`.
fn note(message: &str) {
    eprintln!("virtual library: {message}");
}

fn utf16z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ FETCH_PLACEHOLDERS

unsafe extern "system" fn on_fetch_placeholders(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: called by the platform with the callback's own `info`.
    let request = unsafe { Request::capture(info) };
    let Some(runtime) = super::runtime() else {
        return;
    };
    runtime.spawn(async move {
        let path = request.path();
        let plan = match &path {
            Ok(path) => {
                population::populate(&StoredCredentialsImmich, &StoredCredentialsImmich, path).await
            }
            Err(failure) => Err(failure.clone()),
        };
        match plan {
            Ok(plan) => {
                reporting::report_succeeded();
                if !plan.skipped.is_empty() {
                    note(&format!(
                        "{} item(s) not shown for now (no size or too long a path): {}",
                        plan.skipped.len(),
                        plan.skipped.join(", ")
                    ));
                }
                transfer_placeholders(&request, &plan.specs);
            }
            Err(failure) => {
                note(&format!("could not list a directory: {}", failure.message));
                reporting::report_failed(&Report {
                    operation: Operation::List,
                    path: path.as_deref().unwrap_or(""),
                    failure: &failure,
                });
                fail_placeholders(&request, failure.status());
            }
        }
    });
}

fn create_info(spec: &PlaceholderSpec, name: &[u16]) -> CF_PLACEHOLDER_CREATE_INFO {
    CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: PCWSTR(name.as_ptr()),
        FsMetadata: CF_FS_METADATA {
            BasicInfo: FILE_BASIC_INFO {
                CreationTime: spec.created,
                LastAccessTime: spec.modified,
                LastWriteTime: spec.modified,
                ChangeTime: spec.modified,
                FileAttributes: if spec.is_directory {
                    FILE_ATTRIBUTE_DIRECTORY.0
                } else {
                    FILE_ATTRIBUTE_NORMAL.0
                },
            },
            FileSize: spec.size as i64,
        },
        FileIdentity: spec.identity.as_ptr().cast(),
        FileIdentityLength: spec.identity.len() as u32,
        Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
        Result: HRESULT(0),
        CreateUsn: 0,
    }
}

/// Creates the placeholders in the directory being listed, and marks that directory populated.
fn transfer_placeholders(request: &Request, specs: &[PlaceholderSpec]) {
    let names: Vec<Vec<u16>> = specs.iter().map(|spec| utf16z(&spec.name)).collect();
    let mut entries: Vec<CF_PLACEHOLDER_CREATE_INFO> = specs
        .iter()
        .zip(&names)
        .map(|(spec, name)| create_info(spec, name))
        .collect();
    let operation = operation_info(request, CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS);
    let total = entries.len();
    let batches = entries.chunks_mut(PLACEHOLDER_BATCH).collect::<Vec<_>>();
    // An empty directory still needs the one call that marks it populated.
    let batch_count = batches.len().max(1);
    let mut empty: [CF_PLACEHOLDER_CREATE_INFO; 0] = [];
    let mut batches = batches.into_iter();
    for number in 1..=batch_count {
        let batch: &mut [CF_PLACEHOLDER_CREATE_INFO] = batches.next().unwrap_or(&mut empty);
        let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_4>());
        params.Anonymous.TransferPlaceholders = CF_OPERATION_PARAMETERS_0_4 {
            // Only the last call marks the directory fully populated, so the platform does not
            // ask for its listing again.
            Flags: if number == batch_count {
                CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_DISABLE_ON_DEMAND_POPULATION
            } else {
                CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_NONE
            },
            CompletionStatus: STATUS_SUCCESS,
            PlaceholderTotalCount: total as i64,
            PlaceholderArray: batch.as_mut_ptr(),
            PlaceholderCount: batch.len() as u32,
            EntriesProcessed: 0,
        };
        // SAFETY: `operation` and `params` are valid for the call, and the names and identities
        // the entries point at outlive it.
        if let Err(error) = unsafe { CfExecute(&operation, &mut params) } {
            note(&format!("TRANSFER_PLACEHOLDERS failed: {error}"));
            return;
        }
    }
}

/// Completes a listing with an error and no placeholders; the directory stays unpopulated, so the
/// next listing asks again.
fn fail_placeholders(request: &Request, status: CloudStatus) {
    let operation = operation_info(request, CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS);
    let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_4>());
    params.Anonymous.TransferPlaceholders = CF_OPERATION_PARAMETERS_0_4 {
        Flags: CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_NONE,
        CompletionStatus: ntstatus(status),
        PlaceholderTotalCount: 0,
        PlaceholderArray: std::ptr::null_mut(),
        PlaceholderCount: 0,
        EntriesProcessed: 0,
    };
    // SAFETY: valid structures, no array to read.
    if let Err(error) = unsafe { CfExecute(&operation, &mut params) } {
        note(&format!("TRANSFER_PLACEHOLDERS (failure) failed: {error}"));
    }
}

// ------------------------------------------------------------------ FETCH_DATA

unsafe extern "system" fn on_fetch_data(
    info: *const CF_CALLBACK_INFO,
    parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: called by the platform with the callback's own `info` and `parameters`.
    let (request, fetch) = unsafe { (Request::capture(info), (*parameters).Anonymous.FetchData) };
    let Some(runtime) = super::runtime() else {
        return;
    };
    // Registered before the callback returns, so a cancel that follows at once finds it.
    let registration = IN_FLIGHT.begin(request.transfer);
    runtime.spawn(async move {
        let path = request.path();
        let hydrated = registration
            .run(async {
                match &path {
                    Ok(path) if !path.is_empty() => {
                        let data_request = DataRequest {
                            path: path.clone(),
                            required_offset: fetch.RequiredFileOffset.max(0) as u64,
                            file_size: request.file_size.max(0) as u64,
                        };
                        hydration::hydrate(
                            &StoredCredentialsImmich,
                            &StoredCredentialsImmich,
                            &mut CfSink { request: &request },
                            &data_request,
                        )
                        .await
                        .map(|_| ())
                    }
                    Ok(_) => Err(Failure::new(FailureKind::NotFound, "not a file")),
                    Err(failure) => Err(failure.clone()),
                }
            })
            .await;
        // The platform has already ended a cancelled read: nothing to complete, nothing to report.
        let Some(result) = hydrated else {
            note("a download was cancelled by the reader and stopped");
            return;
        };
        match result {
            Ok(()) => reporting::report_succeeded(),
            Err(failure) => {
                note(&format!("could not hydrate a file: {}", failure.message));
                let report = Report {
                    operation: Operation::Open,
                    path: path.as_deref().unwrap_or(""),
                    failure: &failure,
                };
                if let Some(removed) = report.removed_asset_path() {
                    // #280: reconciliation drops this placeholder once the asset is gone from its
                    // listing; this is the point to hand it the path sooner.
                    note(&format!("'{removed}' was removed from Immich"));
                }
                reporting::report_failed(&report);
                fail_data(
                    &request,
                    fetch.RequiredFileOffset,
                    fetch.RequiredLength,
                    failure.status(),
                );
            }
        }
    });
}

/// `CANCEL_FETCH_DATA`: the reader gave up, so the download for it stops. Nothing is completed: the
/// platform has already ended the read.
unsafe extern "system" fn on_cancel_fetch_data(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: called by the platform with the callback's own `info`.
    let transfer = unsafe { (*info).TransferKey };
    let cancelled = IN_FLIGHT.cancel(transfer);
    if cancelled == 0 {
        note("a cancel arrived for a download that is not (or no longer) in flight");
    }
}

/// Hands blocks of the original to the platform.
struct CfSink<'a> {
    request: &'a Request,
}

impl DataSink for CfSink<'_> {
    fn transfer(&mut self, block: &Block) -> Result<(), Failure> {
        let operation = operation_info(self.request, CF_OPERATION_TYPE_TRANSFER_DATA);
        let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_0>());
        params.Anonymous.TransferData = CF_OPERATION_PARAMETERS_0_0 {
            Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
            CompletionStatus: STATUS_SUCCESS,
            Buffer: block.data.as_ptr().cast(),
            Offset: block.offset as i64,
            Length: block.data.len() as i64,
        };
        // SAFETY: `block.data` outlives the synchronous call.
        unsafe { CfExecute(&operation, &mut params) }.map_err(|error| {
            // The platform refused the data, typically because the reader went away.
            Failure::new(
                FailureKind::Platform,
                format!("TRANSFER_DATA at {} failed: {error}", block.offset),
            )
        })
    }
}

/// Completes the range the reader waits for with an error, which fails its read.
fn fail_data(request: &Request, offset: i64, length: i64, status: CloudStatus) {
    let operation = operation_info(request, CF_OPERATION_TYPE_TRANSFER_DATA);
    let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_0>());
    params.Anonymous.TransferData = CF_OPERATION_PARAMETERS_0_0 {
        Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
        CompletionStatus: ntstatus(status),
        Buffer: std::ptr::null(),
        Offset: offset,
        Length: length,
    };
    // SAFETY: valid structures, no buffer to read.
    if let Err(error) = unsafe { CfExecute(&operation, &mut params) } {
        note(&format!("TRANSFER_DATA (failure) failed: {error}"));
    }
}

// ------------------------------------------------------------------ NOTIFY_DELETE / NOTIFY_RENAME

/// What the platform answers a refused delete or rename with. Observed in the gate prototype
/// (`vl-gates-prototype`): Explorer shows the operation as refused and leaves the placeholder.
const VETO: NTSTATUS = windows::Win32::Foundation::STATUS_CLOUD_FILE_ACCESS_DENIED;

unsafe extern "system" fn on_notify_delete(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: called by the platform with the callback's own `info`.
    let request = unsafe { Request::capture(info) };
    veto_delete(&request);
}

unsafe extern "system" fn on_notify_rename(
    info: *const CF_CALLBACK_INFO,
    parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: called by the platform with the callback's own `info` and `parameters`.
    let (request, rename) = unsafe { (Request::capture(info), (*parameters).Anonymous.Rename) };
    // SAFETY: `TargetPath` is NUL terminated and valid while the callback runs, or null.
    let target = if rename.TargetPath.is_null() {
        String::new()
    } else {
        unsafe { rename.TargetPath.to_string() }.unwrap_or_default()
    };
    veto_rename(
        &request,
        &target,
        rename
            .Flags
            .contains(CF_CALLBACK_RENAME_FLAG_TARGET_IN_SCOPE),
    );
}

/// Refuses the delete. Every `NOTIFY_*` that asks for an answer must get one, or the caller waits.
fn veto_delete(request: &Request) {
    let refusal = write_guard::refuse(write_guard::Operation::Delete);
    note(&format!(
        "refused a {} of '{}'",
        refusal.describe(),
        request.path().unwrap_or_default()
    ));
    let operation = operation_info(request, CF_OPERATION_TYPE_ACK_DELETE);
    let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_7>());
    params.Anonymous.AckDelete = CF_OPERATION_PARAMETERS_0_7 {
        Flags: CF_OPERATION_ACK_DELETE_FLAG_NONE,
        CompletionStatus: VETO,
    };
    // SAFETY: valid structures for the call.
    if let Err(error) = unsafe { CfExecute(&operation, &mut params) } {
        note(&format!("ACK_DELETE failed: {error}"));
    }
}

/// Refuses the rename or move, whatever its target (inside the root, out of it, the Recycle Bin).
fn veto_rename(request: &Request, target: &str, target_in_scope: bool) {
    let refusal = write_guard::refuse(write_guard::Operation::Rename {
        target,
        target_in_scope,
    });
    note(&format!(
        "refused a {} of '{}' to '{target}'",
        refusal.describe(),
        request.path().unwrap_or_default()
    ));
    let operation = operation_info(request, CF_OPERATION_TYPE_ACK_RENAME);
    let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_6>());
    params.Anonymous.AckRename = CF_OPERATION_PARAMETERS_0_6 {
        Flags: CF_OPERATION_ACK_RENAME_FLAG_NONE,
        CompletionStatus: VETO,
    };
    // SAFETY: valid structures for the call.
    if let Err(error) = unsafe { CfExecute(&operation, &mut params) } {
        note(&format!("ACK_RENAME failed: {error}"));
    }
}

// ------------------------------------------------------------------ connection

/// A connected sync root. It owns the callback table the platform was given, so the table lives
/// exactly as long as the connection and is freed after the disconnect, once per connect.
pub(super) struct Connection {
    key: CF_CONNECTION_KEY,
    // Declared for its lifetime only: the platform may read it until the disconnect returns.
    _table: Box<[CF_CALLBACK_REGISTRATION]>,
}

impl Connection {
    /// Stops callbacks. The registration stays.
    pub(super) fn disconnect(self) {
        drop(self);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the key came from a successful `CfConnectSyncRoot` and is disconnected once. The
        // call returns when no callback is running any more; the table is freed after it, as the
        // fields drop after this body.
        if let Err(error) = unsafe { CfDisconnectSyncRoot(self.key) } {
            note(&format!("CfDisconnectSyncRoot failed: {error}"));
        }
    }
}

fn registration(
    kind: CF_CALLBACK_TYPE,
    callback: unsafe extern "system" fn(*const CF_CALLBACK_INFO, *const CF_CALLBACK_PARAMETERS),
) -> CF_CALLBACK_REGISTRATION {
    CF_CALLBACK_REGISTRATION {
        Type: kind,
        Callback: Some(callback),
    }
}

/// The callbacks this adapter serves, ended by the platform's terminator entry.
fn callback_table() -> Vec<CF_CALLBACK_REGISTRATION> {
    vec![
        registration(CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS, on_fetch_placeholders),
        registration(CF_CALLBACK_TYPE_FETCH_DATA, on_fetch_data),
        registration(CF_CALLBACK_TYPE_CANCEL_FETCH_DATA, on_cancel_fetch_data),
        registration(CF_CALLBACK_TYPE_NOTIFY_DELETE, on_notify_delete),
        registration(CF_CALLBACK_TYPE_NOTIFY_RENAME, on_notify_rename),
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ]
}

/// Connects the registered sync root at `root` and starts serving callbacks.
pub(super) fn connect(root: &Path) -> windows::core::Result<Connection> {
    // The platform may keep the table, so the connection owns it until it is disconnected.
    let table = callback_table().into_boxed_slice();
    let root = utf16z(&root.to_string_lossy());
    // SAFETY: `root` is NUL terminated and `table` is terminated and outlives the connection.
    let key = unsafe {
        CfConnectSyncRoot(
            PCWSTR(root.as_ptr()),
            table.as_ptr(),
            None,
            CF_CONNECT_FLAG_NONE,
        )?
    };
    Ok(Connection { key, _table: table })
}
