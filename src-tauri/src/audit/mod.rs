mod event;
mod writer;

use tauri::{AppHandle, Emitter, Manager};

pub use event::{
    actor_id, audit_safe_error, upload_details, AuditEvent, Outcome, Severity, SyncAuditContext,
};
#[allow(unused_imports)]
pub use writer::{
    report_audit, AuditError, AuditLogger, AuditWriter, MAX_AUDIT_FILES, MAX_AUDIT_FILE_BYTES,
};

fn emit_audit_failure(app: &AppHandle, message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S");
    let _ = app.emit(
        "log-message",
        format!("[{}] [ERROR] {}", timestamp, message),
    );
}

pub fn audit_event(app: &AppHandle, event: AuditEvent) {
    let Some(logger) = app
        .try_state::<std::sync::Arc<AuditLogger>>()
        .map(|state| state.inner().clone())
    else {
        emit_audit_failure(app, "Audit logging failed: logger is unavailable");
        return;
    };
    report_audit(logger.append(event), |message| {
        emit_audit_failure(app, message)
    });
}
