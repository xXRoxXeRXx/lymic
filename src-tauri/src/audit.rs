use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

pub const MAX_AUDIT_FILES: usize = 5;
pub const MAX_AUDIT_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 2_048;

#[derive(Debug)]
pub enum AuditError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    OversizedRecord,
    StatePoisoned,
    AppDataUnavailable,
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "audit log I/O error: {}", error),
            Self::Serialize(error) => write!(f, "audit log serialization error: {}", error),
            Self::OversizedRecord => write!(f, "audit record exceeds the 5 MiB file limit"),
            Self::StatePoisoned => write!(f, "audit logger state is unavailable"),
            Self::AppDataUnavailable => write!(f, "application data directory is unavailable"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<std::io::Error> for AuditError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
    Started,
    Skipped,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    schema_version: u8,
    timestamp: String,
    event_id: String,
    operation_id: String,
    event_type: String,
    outcome: Outcome,
    severity: Severity,
    actor_id: Option<String>,
    source: String,
    message: String,
    details: Value,
}

impl AuditEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: impl Into<String>,
        event_type: impl Into<String>,
        outcome: Outcome,
        severity: Severity,
        actor_id: Option<String>,
        source: impl Into<String>,
        message: impl AsRef<str>,
        details: Value,
    ) -> Self {
        Self {
            schema_version: 1,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            event_id: Uuid::new_v4().to_string(),
            operation_id: operation_id.into(),
            event_type: event_type.into(),
            outcome,
            severity,
            actor_id,
            source: source.into(),
            message: bounded_message(message.as_ref()),
            details,
        }
    }

    pub fn operation_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.outcome, Outcome::Success | Outcome::Failure)
    }
}

pub fn actor_id(server_url: &str, api_key: &str) -> String {
    let normalized_url = server_url.trim().trim_end_matches('/').to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(b"lymic.audit.actor.v1\0");
    hasher.update(normalized_url.as_bytes());
    hasher.update(b"\0");
    hasher.update(api_key.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[derive(Clone)]
pub struct SyncAuditContext {
    operation_id: String,
    actor_id: Option<String>,
    source: &'static str,
    server_url: String,
    api_key: String,
}

impl SyncAuditContext {
    pub fn from_credentials(server_url: &str, api_key: &str, source: &'static str) -> Self {
        Self {
            operation_id: AuditEvent::operation_id(),
            actor_id: Some(actor_id(server_url, api_key)),
            source,
            server_url: server_url.to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn event(
        &self,
        event_type: &str,
        outcome: Outcome,
        severity: Severity,
        message: &str,
        details: Value,
    ) -> AuditEvent {
        AuditEvent::new(
            &self.operation_id,
            event_type,
            outcome,
            severity,
            self.actor_id.clone(),
            self.source,
            message,
            details,
        )
    }

    pub fn safe_error(&self, error: &str) -> String {
        audit_safe_error(error, Some(&self.server_url), Some(&self.api_key))
    }
}

pub fn audit_safe_error(error: &str, server_url: Option<&str>, api_key: Option<&str>) -> String {
    let mut redacted = error.replace(['\r', '\n'], " ");
    for secret in [server_url, api_key]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_empty())
    {
        redacted = redacted.replace(secret, "[redacted]");
    }
    redacted
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(512)
        .collect()
}

pub fn upload_details(
    asset: &crate::sync::SyncAsset,
    checksums: &crate::sync::Checksums,
    file_operation_id: &str,
    upload_started_at: &str,
    upload_finished_at: Option<String>,
    remote_asset_id: Option<&str>,
    failure_reason: Option<String>,
) -> Value {
    let mut details = json!({
        "file_operation_id": file_operation_id,
        "local_path": asset.path,
        "file_name": Path::new(&asset.path).file_name().and_then(|name| name.to_str()).unwrap_or("file"),
        "file_size_bytes": asset.size,
        "checksums": {
            "md5": checksums.md5_hex,
            "sha256": checksums.sha256_hex,
            "sha1_base64": checksums.sha1_base64,
        },
        "upload_started_at": upload_started_at,
    });
    if let Some(finished_at) = upload_finished_at {
        details["upload_finished_at"] = json!(finished_at);
    }
    if let Some(remote_id) = remote_asset_id {
        details["remote_asset_id"] = json!(remote_id);
    }
    if let Some(reason) = failure_reason {
        details["failure_reason"] = json!(reason);
    }
    details
}

fn bounded_message(message: &str) -> String {
    let single_line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.len() <= MAX_MESSAGE_BYTES {
        return single_line;
    }
    let mut end = MAX_MESSAGE_BYTES;
    while !single_line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &single_line[..end])
}

struct AuditState {
    active_index: usize,
    active_size: u64,
    file: File,
}

pub struct AuditWriter {
    directory: PathBuf,
    state: Mutex<AuditState>,
}

impl AuditWriter {
    fn initialize(app: &AppHandle) -> Result<Self, AuditError> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|_| AuditError::AppDataUnavailable)?;
        Self::initialize_at(app_data.join("logs"))
    }

    fn initialize_at(directory: PathBuf) -> Result<Self, AuditError> {
        fs::create_dir_all(&directory)?;
        let mut newest_non_full: Option<(usize, u64, std::time::SystemTime)> = None;
        let mut newest_full: Option<(usize, u64, std::time::SystemTime)> = None;
        for index in 0..MAX_AUDIT_FILES {
            let path = slot_path(&directory, index);
            if let Ok(metadata) = fs::metadata(path) {
                let size = metadata.len();
                let modified = metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let newest = if size < MAX_AUDIT_FILE_BYTES {
                    &mut newest_non_full
                } else {
                    &mut newest_full
                };
                if newest
                    .as_ref()
                    .is_none_or(|(_, _, current)| modified > *current)
                {
                    *newest = Some((index, size, modified));
                }
            }
        }
        // When every slot is full, preserve all five until the next append rotates
        // from the newest full slot into its successor and truncates that successor.
        let (active_index, active_size) = newest_non_full
            .or(newest_full)
            .map(|(index, size, _)| (index, size))
            .unwrap_or((0, 0));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(slot_path(&directory, active_index))?;
        Ok(Self {
            directory,
            state: Mutex::new(AuditState {
                active_index,
                active_size,
                file,
            }),
        })
    }

    pub fn append(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let mut line = serde_json::to_vec(event).map_err(AuditError::Serialize)?;
        line.push(b'\n');
        if line.len() as u64 > MAX_AUDIT_FILE_BYTES {
            return Err(AuditError::OversizedRecord);
        }
        let mut state = self.state.lock().map_err(|_| AuditError::StatePoisoned)?;
        if state.active_size + line.len() as u64 > MAX_AUDIT_FILE_BYTES {
            let next_index = (state.active_index + 1) % MAX_AUDIT_FILES;
            let file = File::create(slot_path(&self.directory, next_index))?;
            state.active_index = next_index;
            state.file = file;
            state.active_size = 0;
        }
        if let Err(error) = state.file.write_all(&line) {
            state.active_size = state
                .file
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(state.active_size);
            return Err(error.into());
        }
        state.active_size += line.len() as u64;
        if event.is_terminal() {
            if let Err(error) = state.file.sync_data() {
                state.active_size = state
                    .file
                    .metadata()
                    .map(|metadata| metadata.len())
                    .unwrap_or(state.active_size);
                return Err(error.into());
            }
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), AuditError> {
        let state = self.state.lock().map_err(|_| AuditError::StatePoisoned)?;
        state.file.sync_data().map_err(AuditError::Io)
    }

    #[cfg(test)]
    fn active_slot(&self) -> (usize, u64) {
        let state = self.state.lock().unwrap();
        (state.active_index, state.active_size)
    }
}

enum AuditCommand {
    Event(AuditEvent),
    Flush(mpsc::SyncSender<Result<(), AuditError>>),
}

pub struct AuditLogger {
    sender: mpsc::Sender<AuditCommand>,
}

impl AuditLogger {
    pub fn initialize(app: &AppHandle) -> Result<Self, AuditError> {
        let writer = AuditWriter::initialize(app)?;
        let app = app.clone();
        Self::from_writer(writer, move |message| emit_audit_failure(&app, message))
    }

    fn from_writer(
        writer: AuditWriter,
        report: impl Fn(&str) + Send + 'static,
    ) -> Result<Self, AuditError> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("audit-writer".to_string())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        AuditCommand::Event(event) => {
                            report_audit(writer.append(&event), |message| report(message))
                        }
                        AuditCommand::Flush(reply) => {
                            let _ = reply.send(writer.flush());
                        }
                    }
                }
            })
            .map_err(AuditError::Io)?;
        Ok(Self { sender })
    }

    #[cfg(test)]
    fn initialize_at_with_reporter(
        directory: PathBuf,
        report: impl Fn(&str) + Send + 'static,
    ) -> Result<Self, AuditError> {
        Self::from_writer(AuditWriter::initialize_at(directory)?, report)
    }

    pub fn append(&self, event: AuditEvent) -> Result<(), AuditError> {
        self.sender
            .send(AuditCommand::Event(event))
            .map_err(|_| AuditError::StatePoisoned)
    }

    pub fn flush(&self) -> Result<(), AuditError> {
        let (sender, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(AuditCommand::Flush(sender))
            .map_err(|_| AuditError::StatePoisoned)?;
        receiver.recv().map_err(|_| AuditError::StatePoisoned)?
    }
}

pub fn report_audit(result: Result<(), AuditError>, mut report: impl FnMut(&str)) {
    if let Err(error) = result {
        report(&format!("Audit logging failed: {}", error));
    }
}

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

fn slot_path(directory: &Path, index: usize) -> PathBuf {
    directory.join(format!("audit-{}.jsonl", index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("lymic-audit-test-{}", Uuid::new_v4()))
    }
    fn event(message: &str) -> AuditEvent {
        AuditEvent::new(
            "operation",
            "file.upload.completed",
            Outcome::Success,
            Severity::Info,
            Some(actor_id("https://example.test/", "secret")),
            "manual_sync",
            message,
            json!({}),
        )
    }

    #[test]
    fn serializes_schema_without_secret_input() {
        let value = serde_json::to_value(event("Upload completed")).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert!(value["timestamp"].as_str().unwrap().ends_with('Z'));
        assert!(!value.to_string().contains("secret"));
        assert_eq!(
            value["actor_id"].as_str().unwrap().len(),
            "sha256:".len() + 64
        );
    }
    #[test]
    fn actor_is_stable_and_domain_separated() {
        assert_eq!(
            actor_id("https://EXAMPLE.test/", "key"),
            actor_id("https://example.test", "key")
        );
        assert_ne!(
            actor_id("https://example.test", "key"),
            actor_id("https://other.test", "key")
        );
    }
    #[test]
    fn rejects_oversized_record() {
        let dir = temp_dir();
        let logger = AuditWriter::initialize_at(dir.clone()).unwrap();
        let mut oversized = event("ignored");
        oversized.message = "x".repeat(MAX_AUDIT_FILE_BYTES as usize);
        assert!(matches!(
            logger.append(&oversized),
            Err(AuditError::OversizedRecord)
        ));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn rotates_and_recovers_newest_non_full_slot() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(slot_path(&dir, 3), b"old\n").unwrap();
        let logger = AuditWriter::initialize_at(dir.clone()).unwrap();
        assert_eq!(logger.active_slot().0, 3);
        {
            let mut state = logger.state.lock().unwrap();
            state.active_size = MAX_AUDIT_FILE_BYTES;
        }
        logger.append(&event("rotated")).unwrap();
        assert_eq!(logger.active_slot().0, 4);
        assert!(fs::read_to_string(slot_path(&dir, 4))
            .unwrap()
            .contains("rotated"));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn restart_with_all_full_slots_preserves_them_until_rotation() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        for index in 0..MAX_AUDIT_FILES {
            let path = slot_path(&dir, index);
            let file = File::create(&path).unwrap();
            file.set_len(MAX_AUDIT_FILE_BYTES).unwrap();
            // Filesystem mtime resolution varies; a short delay keeps the order observable.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let logger = AuditWriter::initialize_at(dir.clone()).unwrap();
        assert_eq!(logger.active_slot().0, MAX_AUDIT_FILES - 1);
        logger.append(&event("after-restart")).unwrap();
        assert_eq!(logger.active_slot().0, 0);
        assert!(fs::read_to_string(slot_path(&dir, 0))
            .unwrap()
            .contains("after-restart"));
        assert_eq!(
            fs::metadata(slot_path(&dir, 1)).unwrap().len(),
            MAX_AUDIT_FILE_BYTES
        );
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn wraps_and_truncates_the_next_slot() {
        let dir = temp_dir();
        let logger = AuditWriter::initialize_at(dir.clone()).unwrap();
        for expected in 1..=MAX_AUDIT_FILES {
            {
                let mut state = logger.state.lock().unwrap();
                state.active_size = MAX_AUDIT_FILE_BYTES;
            }
            logger.append(&event(&format!("slot-{expected}"))).unwrap();
            assert_eq!(logger.active_slot().0, expected % MAX_AUDIT_FILES);
        }
        let content = fs::read_to_string(slot_path(&dir, 0)).unwrap();
        assert!(content.contains("slot-5"));
        assert!(!content.contains("slot-0"));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn concurrent_writers_produce_parseable_json_lines() {
        let dir = temp_dir();
        let logger = std::sync::Arc::new(AuditWriter::initialize_at(dir.clone()).unwrap());
        let writers = (0..8)
            .map(|index| {
                let logger = logger.clone();
                std::thread::spawn(move || {
                    logger.append(&event(&format!("writer-{index}"))).unwrap()
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().unwrap();
        }
        let content = fs::read_to_string(slot_path(&dir, 0)).unwrap();
        let lines = content.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 8);
        for line in lines {
            serde_json::from_str::<Value>(line).unwrap();
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reports_writer_errors_without_retrying_audit() {
        let mut reported = Vec::new();
        report_audit(Err(AuditError::OversizedRecord), |message| {
            reported.push(message.to_string())
        });
        assert_eq!(reported.len(), 1);
        assert!(reported[0].contains("Audit logging failed"));
    }

    #[test]
    fn asynchronous_writer_failure_is_reported_after_flush() {
        let dir = temp_dir();
        let reports = std::sync::Arc::new(Mutex::new(Vec::new()));
        let report_target = reports.clone();
        let logger = AuditLogger::initialize_at_with_reporter(dir.clone(), move |message| {
            report_target.lock().unwrap().push(message.to_string());
        })
        .unwrap();
        let mut oversized = event("ignored");
        oversized.message = "x".repeat(MAX_AUDIT_FILE_BYTES as usize);
        logger.append(oversized).unwrap();
        logger.flush().unwrap();
        let reports = reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].contains("Audit logging failed"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn initialization_fails_when_log_directory_is_a_file() {
        let path = temp_dir();
        fs::write(&path, b"not a directory").unwrap();
        assert!(matches!(
            AuditWriter::initialize_at(path.clone()),
            Err(AuditError::Io(_))
        ));
        let _ = fs::remove_file(path);
    }
}
