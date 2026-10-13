mod event;
mod writer;

use tauri::{AppHandle, Manager};

pub use event::{
    actor_id, audit_safe_error, upload_details, AuditEvent, Outcome, Severity, SyncAuditContext,
};
#[allow(unused_imports)]
pub use writer::{
    report_audit, AuditError, AuditLogger, AuditWriter, AUDIT_QUEUE_BYTE_CAPACITY,
    AUDIT_QUEUE_CAPACITY, MAX_AUDIT_FILES, MAX_AUDIT_FILE_BYTES,
};

fn emit_audit_failure(app: &AppHandle, message: &str) {
    let mut params = std::collections::HashMap::new();
    params.insert("error".to_string(), message.to_string());
    crate::app::events::log_to_ui_keyed(
        app,
        "ERROR",
        message,
        "log_audit_failure",
        params,
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
