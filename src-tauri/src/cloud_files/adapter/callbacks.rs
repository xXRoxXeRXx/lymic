//! The Cloud Filter callbacks: `FETCH_PLACEHOLDERS` (a directory listed for the first time) and
//! `FETCH_DATA` (a placeholder opened). Each one copies what it needs, spawns the work onto the
//! Tokio runtime and returns at once; the spawned work answers through `CfExecute`.
//!
//! `NOTIFY_DELETE` and `NOTIFY_RENAME` (#279) are the exception to "spawn and return": they only
//! refuse (`write_guard`), do no I/O, and acknowledge inline.

use crate::cloud_files::diagnostic_log::DiagnosticLog;
use crate::cloud_files::failure::{CloudStatus, Failure, FailureKind};
use crate::cloud_files::hydration::{self, Block, DataRequest, DataSink};
use crate::cloud_files::identity;
use crate::cloud_files::inflight::InFlight;
use crate::cloud_files::library::StoredCredentialsImmich;
use crate::cloud_files::listing_log::ListingLog;
use crate::cloud_files::placeholder::PlaceholderSpec;
use crate::cloud_files::population;
use crate::cloud_files::readers::{self, Reader, Verdict};
use crate::cloud_files::reporting::{self, Operation, Report};
use crate::cloud_files::shell_thumbnails::WarmingLibrary;
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
use windows::Win32::Storage::FileSystem::FILE_BASIC_INFO;

const STATUS_SUCCESS: NTSTATUS = NTSTATUS(0);
/// The downloads in flight, so `CANCEL_FETCH_DATA` can stop one.
static IN_FLIGHT: LazyLock<InFlight> = LazyLock::new(InFlight::default);
/// Placeholders handed over per `TRANSFER_PLACEHOLDERS` call.
pub(super) const PLACEHOLDER_BATCH: usize = 1000;

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
#[derive(Clone)]
pub(super) struct Request {
    connection: CF_CONNECTION_KEY,
    transfer: i64,
    request: i64,
    /// The placeholder's file identity: its path relative to the Virtual Library root.
    identity: Vec<u8>,
    file_size: i64,
    /// The process that triggered the callback (`CF_CONNECT_FLAG_REQUIRE_PROCESS_INFO`).
    reader: Reader,
}

/// Reads a platform string that may be null.
///
/// # Safety
/// `text` must be null or point to a NUL terminated string valid for the call.
unsafe fn optional_string(text: PCWSTR) -> Option<String> {
    if text.is_null() {
        None
    } else {
        unsafe { text.to_string() }.ok()
    }
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
        let reader = if info.ProcessInfo.is_null() {
            Reader::default()
        } else {
            // SAFETY: the platform passes a valid `CF_PROCESS_INFO` when the connection asked for
            // process information, valid while the callback runs; its strings are NUL terminated
            // or null.
            let process = unsafe { &*info.ProcessInfo };
            Reader {
                process_id: process.ProcessId,
                image_path: unsafe { optional_string(process.ImagePath) },
                package_name: unsafe { optional_string(process.PackageName) },
            }
        };
        Self {
            connection: info.ConnectionKey,
            transfer: info.TransferKey,
            request: info.RequestKey,
            identity,
            file_size: info.FileSize,
            reader,
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

/// `lymic-virtual-library.log` in the package's local state folder, next to the thumbnail
/// provider's log. `None` without Package Identity.
static LOG: LazyLock<Option<DiagnosticLog>> = LazyLock::new(|| {
    lymic_shell_ext::shell::package_local_state_dir()
        .map(|directory| DiagnosticLog::new(directory.join("lymic-virtual-library.log")))
});

/// Diagnostics for failures nobody is waiting on, and the record of who read what. User-facing
/// notices go through `reporting`.
pub(in crate::cloud_files) fn note(message: &str) {
    eprintln!("virtual library: {message}");
    if let Some(log) = LOG.as_ref() {
        log.append(message);
    }
}

/// The text of a panic payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_string())
}

/// Runs the body of a callback. A panic must not unwind into the platform (the process would be
/// aborted without a word); it is written to the log instead.
fn guarded(name: &str, body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        note(&format!(
            "{name}: panic in the callback: {}",
            panic_text(&*payload)
        ));
    }
}

/// Spawns `work` and, when it panics, writes that to the log and runs `on_panic`, which must
/// still answer the platform: a task that panics silently leaves its request open for good.
fn spawn_guarded(
    runtime: &tokio::runtime::Handle,
    name: &'static str,
    on_panic: impl FnOnce() + Send + 'static,
    work: impl std::future::Future<Output = ()> + Send + 'static,
) {
    let task = runtime.spawn(work);
    runtime.spawn(async move {
        if let Err(error) = task.await {
            let reason = if error.is_panic() {
                panic_text(&*error.into_panic())
            } else {
                "the task was cancelled".to_string()
            };
            note(&format!("{name}: the work stopped: {reason}"));
            on_panic();
        }
    });
}

/// Makes a panic anywhere in the process appear in the log, whatever thread it happens on. The
/// previous hook keeps running.
pub(super) fn log_panics() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            note(&format!("panic: {info}"));
            previous(info);
        }));
    });
}

fn utf16z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ FETCH_PLACEHOLDERS

unsafe extern "system" fn on_fetch_placeholders(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    guarded("FETCH_PLACEHOLDERS", || {
        // SAFETY: called by the platform with the callback's own `info`.
        let request = unsafe { Request::capture(info) };
        let listed = request.path().unwrap_or_default();
        note(&format!(
            "FETCH_PLACEHOLDERS '{listed}' reader={}",
            readers::describe(&request.reader)
        ));
        let Some(runtime) = super::runtime() else {
            // Answered all the same: a request nobody completes hangs the program that listed.
            note("FETCH_PLACEHOLDERS: the application runtime is not up, failing the listing");
            fail_placeholders(&request, CloudStatus::Unsuccessful);
            return;
        };
        let on_panic = request.clone();
        spawn_guarded(
            runtime,
            "FETCH_PLACEHOLDERS",
            move || fail_placeholders(&on_panic, CloudStatus::Unsuccessful),
            list_directory(request),
        );
    });
}

/// The work of one `FETCH_PLACEHOLDERS`: list the directory, hand the placeholders over.
async fn list_directory(request: Request) {
    let path = request.path();
    let plan = match &path {
        Ok(path) => {
            // Says what every listing returned, caches the thumbnails of the listed files for the
            // shell extension (#282).
            let library = ListingLog::new(WarmingLibrary::system(StoredCredentialsImmich));
            population::populate(&library, &StoredCredentialsImmich, path).await
        }
        Err(failure) => Err(failure.clone()),
    };
    let shown = path.as_deref().unwrap_or("?");
    match plan {
        Ok(plan) => {
            reporting::report_succeeded();
            if !plan.skipped.is_empty() {
                note(&format!(
                    "'{shown}': {} item(s) not shown for now (no size or too long a path): {}",
                    plan.skipped.len(),
                    plan.skipped.join(", ")
                ));
            }
            // Reconciliation (#280) diffs a directory only once it is populated.
            if transfer_placeholders(&request, &plan.specs) {
                note(&format!(
                    "'{shown}': {} placeholder(s) handed to the platform",
                    plan.specs.len()
                ));
                if let Ok(path) = request.path() {
                    // The platform will not ask for this listing again, so a record that fails is
                    // kept and retried by the next reconciliation run, and is reported now.
                    if let Err(error) = crate::cloud_files::mark_populated(&path).await {
                        let failure = Failure::new(
                            FailureKind::Failed,
                            format!("could not record the populated directory: {error}"),
                        );
                        note(&format!("'{shown}': {}", failure.message));
                        reporting::report_failed(&Report {
                            operation: Operation::List,
                            path: &path,
                            failure: &failure,
                        });
                    }
                }
            } else {
                note(&format!(
                    "'{shown}': the platform refused the {} placeholder(s)",
                    plan.specs.len()
                ));
            }
        }
        Err(failure) => {
            note(&format!(
                "could not list a directory: '{shown}': {:?}: {}",
                failure.kind, failure.message
            ));
            reporting::report_failed(&Report {
                operation: Operation::List,
                path: path.as_deref().unwrap_or(""),
                failure: &failure,
            });
            fail_placeholders(&request, failure.status());
        }
    }
}

pub(super) fn create_info(spec: &PlaceholderSpec, name: &[u16]) -> CF_PLACEHOLDER_CREATE_INFO {
    CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: PCWSTR(name.as_ptr()),
        FsMetadata: CF_FS_METADATA {
            BasicInfo: FILE_BASIC_INFO {
                CreationTime: spec.created,
                LastAccessTime: spec.modified,
                LastWriteTime: spec.modified,
                ChangeTime: spec.modified,
                // Not content indexed: see `PlaceholderSpec::attributes`.
                FileAttributes: spec.attributes(),
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
/// `false` when the platform refused them.
fn transfer_placeholders(request: &Request, specs: &[PlaceholderSpec]) -> bool {
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
            return false;
        }
    }
    true
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
    guarded("FETCH_DATA", || {
        // SAFETY: called by the platform with the callback's own `info` and `parameters`.
        let (request, fetch) =
            unsafe { (Request::capture(info), (*parameters).Anonymous.FetchData) };
        fetch_data(request, fetch.RequiredFileOffset, fetch.RequiredLength);
    });
}

fn fetch_data(request: Request, offset: i64, length: i64) {
    // Every read is on record with who asked, so a Hydration nobody expected can be traced.
    let verdict = readers::judge(&request.reader);
    note(&readers::fetch_log_line(
        &request.reader,
        &request.path().unwrap_or_default(),
        offset.max(0) as u64,
        length.max(0) as u64,
        verdict,
    ));
    if let Verdict::Refuse(_) = verdict {
        // A background service reading on its own would download the Library: nothing is fetched
        // and nobody is told (it is not the user's read).
        fail_data(&request, offset, length, CloudStatus::Unsuccessful);
        return;
    }
    let Some(runtime) = super::runtime() else {
        note("FETCH_DATA: the application runtime is not up, failing the read");
        fail_data(&request, offset, length, CloudStatus::Unsuccessful);
        return;
    };
    // Registered before the callback returns, so a cancel that follows at once finds it.
    let registration = IN_FLIGHT.begin(request.transfer);
    let on_panic = request.clone();
    spawn_guarded(
        runtime,
        "FETCH_DATA",
        move || fail_data(&on_panic, offset, length, CloudStatus::Unsuccessful),
        async move {
            let path = request.path();
            let hydrated = registration
                .run(async {
                    match &path {
                        Ok(path) if !path.is_empty() => {
                            let data_request = DataRequest {
                                path: path.clone(),
                                required_offset: offset.max(0) as u64,
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
                    note(&format!(
                        "could not hydrate '{}': {:?}: {}",
                        path.as_deref().unwrap_or("?"),
                        failure.kind,
                        failure.message
                    ));
                    let report = Report {
                        operation: Operation::Open,
                        path: path.as_deref().unwrap_or(""),
                        failure: &failure,
                    };
                    if let Some(removed) = report.removed_asset_path() {
                        // Reconciliation (#280) drops this placeholder once the asset is gone from its
                        // listing; this is the point to hand it the path sooner.
                        note(&format!("'{removed}' was removed from Immich"));
                    }
                    reporting::report_failed(&report);
                    fail_data(&request, offset, length, failure.status());
                }
            }
        },
    );
}

/// `CANCEL_FETCH_DATA`: the reader gave up, so the download for it stops. Nothing is completed: the
/// platform has already ended the read.
unsafe extern "system" fn on_cancel_fetch_data(
    info: *const CF_CALLBACK_INFO,
    _parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: called by the platform with the callback's own `info`.
    let transfer = unsafe { (*info).TransferKey };
    note("CANCEL_FETCH_DATA: the reader gave up");
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
    // The one delete that is allowed: Lymic removing a placeholder whose Remote Asset is gone
    // (reconciliation, #280). Everything else is refused.
    if request
        .path()
        .is_ok_and(|path| super::reconcile::is_own_removal(&path))
    {
        allow_delete(&request);
        return;
    }
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

/// Acknowledges a delete Lymic itself is making during reconciliation.
fn allow_delete(request: &Request) {
    let operation = operation_info(request, CF_OPERATION_TYPE_ACK_DELETE);
    let mut params = parameters(size_of::<CF_OPERATION_PARAMETERS_0_7>());
    params.Anonymous.AckDelete = CF_OPERATION_PARAMETERS_0_7 {
        Flags: CF_OPERATION_ACK_DELETE_FLAG_NONE,
        CompletionStatus: STATUS_SUCCESS,
    };
    // SAFETY: valid structures for the call.
    if let Err(error) = unsafe { CfExecute(&operation, &mut params) } {
        note(&format!("ACK_DELETE (allow) failed: {error}"));
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

impl Connection {
    /// Whether the platform still knows the connection: telling it the provider is idle fails when
    /// the connection key is no longer valid.
    pub(super) fn heartbeat(&self) -> windows::core::Result<()> {
        // SAFETY: the key is the live connection.
        unsafe { CfUpdateSyncProviderStatus(self.key, CF_PROVIDER_STATUS_IDLE) }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        note("disconnecting the sync root");
        // SAFETY: the key is the live connection; a refused update is only diagnostic.
        if let Err(error) =
            unsafe { CfUpdateSyncProviderStatus(self.key, CF_PROVIDER_STATUS_DISCONNECTED) }
        {
            note(&format!(
                "CfUpdateSyncProviderStatus(DISCONNECTED) failed: {error}"
            ));
        }
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

/// What the connection asks the platform for:
///
/// - `REQUIRE_PROCESS_INFO`: the process behind each callback (`CF_CALLBACK_INFO.ProcessInfo`),
///   which `readers` logs and judges. Without it nobody can tell who hydrates a file.
/// - `REQUIRE_FULL_FILE_PATH`: the full path of the file in the callback, for the log.
/// - `BLOCK_SELF_IMPLICIT_HYDRATION`: a read of a placeholder by this very process fails instead of
///   downloading it. Lymic never reads placeholder content (it lists and updates them), so such a
///   read is a bug, and the log line says where it came from.
const CONNECT_FLAGS: CF_CONNECT_FLAGS = CF_CONNECT_FLAGS(
    CF_CONNECT_FLAG_REQUIRE_PROCESS_INFO.0
        | CF_CONNECT_FLAG_REQUIRE_FULL_FILE_PATH.0
        | CF_CONNECT_FLAG_BLOCK_SELF_IMPLICIT_HYDRATION.0,
);

/// Connects the registered sync root at `root` and starts serving callbacks.
pub(super) fn connect(root: &Path) -> windows::core::Result<Connection> {
    let result = connect_root(root);
    match &result {
        Ok(_) => note(&format!(
            "connected the sync root '{}' (flags 0x{:X})",
            root.display(),
            CONNECT_FLAGS.0
        )),
        Err(error) => note(&format!(
            "CfConnectSyncRoot failed for '{}' (flags 0x{:X}): {error}",
            root.display(),
            CONNECT_FLAGS.0
        )),
    }
    result
}

fn connect_root(root: &Path) -> windows::core::Result<Connection> {
    // The platform may keep the table, so the connection owns it until it is disconnected.
    let table = callback_table().into_boxed_slice();
    let root = utf16z(&root.to_string_lossy());
    // SAFETY: `root` is NUL terminated and `table` is terminated and outlives the connection.
    let key =
        unsafe { CfConnectSyncRoot(PCWSTR(root.as_ptr()), table.as_ptr(), None, CONNECT_FLAGS)? };
    // Explorer shows a sync root's state (the cloud overlay, the status column) from what the
    // provider reports; without a report it stays blank. Lymic has no queue of its own, so the
    // state is "idle" for as long as it is connected.
    // SAFETY: `key` is the live connection returned above.
    if let Err(error) = unsafe { CfUpdateSyncProviderStatus(key, CF_PROVIDER_STATUS_IDLE) } {
        note(&format!("CfUpdateSyncProviderStatus(IDLE) failed: {error}"));
    }
    Ok(Connection { key, _table: table })
}
