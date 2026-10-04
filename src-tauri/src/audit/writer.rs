use super::AuditEvent;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use tauri::{AppHandle, Manager};

pub const MAX_AUDIT_FILES: usize = 5;
pub const MAX_AUDIT_FILE_BYTES: u64 = 5 * 1024 * 1024;

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
        Self::from_writer(writer, move |message| {
            super::emit_audit_failure(&app, message)
        })
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

fn slot_path(directory: &Path, index: usize) -> PathBuf {
    directory.join(format!("audit-{}.jsonl", index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{actor_id, AuditEvent, Outcome, Severity};
    use serde_json::{json, Value};
    use uuid::Uuid;

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
    fn rejects_oversized_record() {
        let dir = temp_dir();
        let logger = AuditWriter::initialize_at(dir.clone()).unwrap();
        let mut oversized = event("ignored");
        oversized.set_message_for_test("x".repeat(MAX_AUDIT_FILE_BYTES as usize));
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
        logger.state.lock().unwrap().active_size = MAX_AUDIT_FILE_BYTES;
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
            logger.state.lock().unwrap().active_size = MAX_AUDIT_FILE_BYTES;
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
        oversized.set_message_for_test("x".repeat(MAX_AUDIT_FILE_BYTES as usize));
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
