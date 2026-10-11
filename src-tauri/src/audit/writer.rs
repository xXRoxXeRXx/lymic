use super::AuditEvent;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const MAX_AUDIT_FILES: usize = 5;
pub const MAX_AUDIT_FILE_BYTES: u64 = 5 * 1024 * 1024;
pub const AUDIT_QUEUE_CAPACITY: usize = 4_096;
pub const AUDIT_QUEUE_BYTE_CAPACITY: usize = 16 * 1024 * 1024;
const AUDIT_QUEUE_RECOVERY_BYTES: usize = AUDIT_QUEUE_BYTE_CAPACITY / 2;
const AUDIT_QUEUE_RECOVERY_RECORDS: usize = AUDIT_QUEUE_CAPACITY / 2;

#[derive(Debug)]
pub enum AuditError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    OversizedRecord,
    QueueSaturated,
    FlushTimeout,
    LoggerUnavailable,
    StatePoisoned,
    AppDataUnavailable,
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "audit log I/O error: {}", error),
            Self::Serialize(error) => write!(f, "audit log serialization error: {}", error),
            Self::OversizedRecord => write!(f, "audit record exceeds the 5 MiB file limit"),
            Self::QueueSaturated => write!(f, "audit delivery queue is saturated"),
            Self::FlushTimeout => write!(f, "audit delivery flush timed out"),
            Self::LoggerUnavailable => write!(f, "audit logger is unavailable"),
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

    #[allow(dead_code)]
    pub fn append(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let mut line = serde_json::to_vec(event).map_err(AuditError::Serialize)?;
        line.push(b'\n');
        if line.len() as u64 > MAX_AUDIT_FILE_BYTES {
            return Err(AuditError::OversizedRecord);
        }
        self.append_serialized(&line, event.is_terminal())
    }

    fn append_serialized(&self, line: &[u8], terminal: bool) -> Result<(), AuditError> {
        let mut state = self.state.lock().map_err(|_| AuditError::StatePoisoned)?;
        if state.active_size + line.len() as u64 > MAX_AUDIT_FILE_BYTES {
            let next_index = (state.active_index + 1) % MAX_AUDIT_FILES;
            let file = File::create(slot_path(&self.directory, next_index))?;
            state.active_index = next_index;
            state.file = file;
            state.active_size = 0;
        }
        if let Err(error) = state.file.write_all(line) {
            state.active_size = state
                .file
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(state.active_size);
            return Err(error.into());
        }
        state.active_size += line.len() as u64;
        if terminal {
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
    Event {
        line: Vec<u8>,
        terminal: bool,
        charged_bytes: usize,
    },
    Flush(mpsc::SyncSender<Result<(), AuditError>>),
}

pub struct AuditLogger {
    sender: mpsc::SyncSender<AuditCommand>,
    pending_bytes: Arc<AtomicUsize>,
    pending_records: Arc<AtomicUsize>,
    overflow: Arc<OverflowState>,
    report: Arc<dyn Fn(&str) + Send + Sync>,
}

struct OverflowState {
    active: AtomicBool,
    dropped: AtomicUsize,
    report_lock: Mutex<()>,
}

impl OverflowState {
    fn record_drop(&self, report: &dyn Fn(&str)) {
        let _guard = self
            .report_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.dropped.fetch_add(1, Ordering::Relaxed);
        if !self.active.swap(true, Ordering::AcqRel) {
            report("Audit logging is saturated; audit events are being dropped");
        }
    }

    fn record_accept(&self, pending_bytes: usize, pending_records: usize, report: &dyn Fn(&str)) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let _guard = self
            .report_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending_bytes <= AUDIT_QUEUE_RECOVERY_BYTES
            && pending_records <= AUDIT_QUEUE_RECOVERY_RECORDS
            && self.active.swap(false, Ordering::AcqRel)
        {
            let dropped = self.dropped.swap(0, Ordering::AcqRel);
            report(&format!(
                "Audit logging recovered; {} audit event(s) were dropped",
                dropped
            ));
        }
    }
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
        report: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<Self, AuditError> {
        Self::from_writer_with_start(writer, Arc::new(report), || {})
    }

    fn from_writer_with_start(
        writer: AuditWriter,
        report: Arc<dyn Fn(&str) + Send + Sync>,
        before_consume: impl FnOnce() + Send + 'static,
    ) -> Result<Self, AuditError> {
        let (sender, receiver) = mpsc::sync_channel(AUDIT_QUEUE_CAPACITY);
        let pending_bytes = Arc::new(AtomicUsize::new(0));
        let pending_records = Arc::new(AtomicUsize::new(0));
        let writer_pending_bytes = pending_bytes.clone();
        let writer_pending_records = pending_records.clone();
        let writer_report = report.clone();
        std::thread::Builder::new()
            .name("audit-writer".to_string())
            .spawn(move || {
                before_consume();
                while let Ok(command) = receiver.recv() {
                    match command {
                        AuditCommand::Event {
                            line,
                            terminal,
                            charged_bytes,
                        } => {
                            let result = writer.append_serialized(&line, terminal);
                            writer_pending_bytes.fetch_sub(charged_bytes, Ordering::AcqRel);
                            writer_pending_records.fetch_sub(1, Ordering::AcqRel);
                            report_audit(result, |message| writer_report(message));
                        }
                        AuditCommand::Flush(reply) => {
                            let _ = reply.send(writer.flush());
                        }
                    }
                }
            })
            .map_err(AuditError::Io)?;
        Ok(Self {
            sender,
            pending_bytes,
            pending_records,
            overflow: Arc::new(OverflowState {
                active: AtomicBool::new(false),
                dropped: AtomicUsize::new(0),
                report_lock: Mutex::new(()),
            }),
            report,
        })
    }

    #[cfg(test)]
    fn initialize_at_with_reporter(
        directory: PathBuf,
        report: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<Self, AuditError> {
        Self::from_writer(AuditWriter::initialize_at(directory)?, report)
    }

    pub fn append(&self, event: AuditEvent) -> Result<(), AuditError> {
        let terminal = event.is_terminal();
        let mut line = serde_json::to_vec(&event).map_err(AuditError::Serialize)?;
        line.push(b'\n');
        if line.len() as u64 > MAX_AUDIT_FILE_BYTES {
            return Err(AuditError::OversizedRecord);
        }
        let charged_bytes = line.len();
        if !reserve_bytes(&self.pending_bytes, charged_bytes) {
            self.overflow.record_drop(&*self.report);
            return Ok(());
        }
        self.pending_records.fetch_add(1, Ordering::AcqRel);
        match self.sender.try_send(AuditCommand::Event {
            line,
            terminal,
            charged_bytes,
        }) {
            Ok(()) => {
                self.overflow.record_accept(
                    self.pending_bytes.load(Ordering::Acquire),
                    self.pending_records.load(Ordering::Acquire),
                    &*self.report,
                );
                Ok(())
            }
            Err(mpsc::TrySendError::Full(_)) => {
                self.pending_bytes
                    .fetch_sub(charged_bytes, Ordering::AcqRel);
                self.pending_records.fetch_sub(1, Ordering::AcqRel);
                self.overflow.record_drop(&*self.report);
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.pending_bytes
                    .fetch_sub(charged_bytes, Ordering::AcqRel);
                self.pending_records.fetch_sub(1, Ordering::AcqRel);
                Err(AuditError::LoggerUnavailable)
            }
        }
    }

    pub fn flush(&self) -> Result<(), AuditError> {
        let (sender, receiver) = mpsc::sync_channel(0);
        self.sender
            .try_send(AuditCommand::Flush(sender))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => AuditError::QueueSaturated,
                mpsc::TrySendError::Disconnected(_) => AuditError::LoggerUnavailable,
            })?;
        receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => AuditError::FlushTimeout,
                mpsc::RecvTimeoutError::Disconnected => AuditError::LoggerUnavailable,
            })?
    }
}

fn reserve_bytes(pending_bytes: &AtomicUsize, charged_bytes: usize) -> bool {
    let mut current = pending_bytes.load(Ordering::Acquire);
    loop {
        let Some(next) = current.checked_add(charged_bytes) else {
            return false;
        };
        if next > AUDIT_QUEUE_BYTE_CAPACITY {
            return false;
        }
        match pending_bytes.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
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
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};
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

    fn queued_event(message: &str) -> AuditEvent {
        AuditEvent::new(
            "operation",
            "file.upload.started",
            Outcome::Started,
            Severity::Info,
            Some(actor_id("https://example.test/", "secret")),
            "manual_sync",
            message,
            json!({}),
        )
    }

    fn blocked_logger(
        directory: PathBuf,
        reports: Arc<Mutex<Vec<String>>>,
    ) -> (AuditLogger, Arc<(Mutex<bool>, Condvar)>) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let consumer_gate = gate.clone();
        let report_target = reports.clone();
        let logger = AuditLogger::from_writer_with_start(
            AuditWriter::initialize_at(directory).unwrap(),
            Arc::new(move |message| report_target.lock().unwrap().push(message.to_string())),
            move || {
                let (locked, ready) = &*consumer_gate;
                let mut released = locked.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
            },
        )
        .unwrap();
        (logger, gate)
    }

    fn release_consumer(gate: &Arc<(Mutex<bool>, Condvar)>) {
        let (locked, ready) = &**gate;
        *locked.lock().unwrap() = true;
        ready.notify_one();
    }

    fn wait_for_drain(logger: &AuditLogger) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while logger.pending_bytes.load(Ordering::Acquire) != 0
            || logger.pending_records.load(Ordering::Acquire) != 0
        {
            assert!(
                Instant::now() < deadline,
                "queued audit records did not drain"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn disconnected_logger(report: Arc<dyn Fn(&str) + Send + Sync>) -> AuditLogger {
        let (sender, receiver) = mpsc::sync_channel(AUDIT_QUEUE_CAPACITY);
        drop(receiver);
        AuditLogger {
            sender,
            pending_bytes: Arc::new(AtomicUsize::new(0)),
            pending_records: Arc::new(AtomicUsize::new(0)),
            overflow: Arc::new(OverflowState {
                active: AtomicBool::new(false),
                dropped: AtomicUsize::new(0),
                report_lock: Mutex::new(()),
            }),
            report,
        }
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
    fn oversized_logger_record_is_rejected_before_admission() {
        let dir = temp_dir();
        let reports = std::sync::Arc::new(Mutex::new(Vec::new()));
        let report_target = reports.clone();
        let logger = AuditLogger::initialize_at_with_reporter(dir.clone(), move |message| {
            report_target.lock().unwrap().push(message.to_string());
        })
        .unwrap();
        let mut oversized = event("ignored");
        oversized.set_message_for_test("x".repeat(MAX_AUDIT_FILE_BYTES as usize));
        assert!(matches!(
            logger.append(oversized),
            Err(AuditError::OversizedRecord)
        ));
        logger.flush().unwrap();
        let reports = reports.lock().unwrap();
        assert!(reports.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn asynchronous_writer_failure_is_reported_after_flush_and_releases_bytes() {
        let dir = temp_dir();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let report_target = reports.clone();
        let mut writer = AuditWriter::initialize_at(dir.clone()).unwrap();
        writer.state = Mutex::new(AuditState {
            active_index: 0,
            active_size: 0,
            file: File::open(slot_path(&dir, 0)).unwrap(),
        });
        let logger = AuditLogger::from_writer(writer, move |message| {
            report_target.lock().unwrap().push(message.to_string());
        })
        .unwrap();

        logger.append(queued_event("write failure")).unwrap();
        assert!(matches!(logger.flush(), Err(AuditError::Io(_))));

        assert_eq!(logger.pending_bytes.load(Ordering::Acquire), 0);
        assert_eq!(logger.pending_records.load(Ordering::Acquire), 0);
        let reports = reports.lock().unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].contains("Audit logging failed"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn disconnected_logger_releases_reserved_bytes() {
        let logger = disconnected_logger(Arc::new(|_| {}));
        assert!(matches!(
            logger.append(queued_event("disconnected")),
            Err(AuditError::LoggerUnavailable)
        ));
        assert_eq!(logger.pending_bytes.load(Ordering::Acquire), 0);
        assert_eq!(logger.pending_records.load(Ordering::Acquire), 0);
    }

    #[test]
    fn queue_count_saturation_drops_without_blocking_and_recovers_once() {
        let dir = temp_dir();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let (logger, gate) = blocked_logger(dir.clone(), reports.clone());
        for index in 0..AUDIT_QUEUE_CAPACITY {
            logger
                .append(queued_event(&format!("queued-{index}")))
                .unwrap();
        }
        let started = Instant::now();
        for index in 0..10 {
            logger
                .append(queued_event(&format!("dropped-{index}")))
                .unwrap();
        }
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(logger.pending_bytes.load(Ordering::Acquire) > 0);
        assert_eq!(reports.lock().unwrap().len(), 1);

        release_consumer(&gate);
        wait_for_drain(&logger);
        logger.append(queued_event("recovery")).unwrap();
        let report_messages = reports.lock().unwrap();
        assert_eq!(report_messages.len(), 2);
        assert!(report_messages[0].contains("saturated"));
        assert!(report_messages[1].contains("10 audit event(s) were dropped"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn queue_byte_saturation_drops_before_count_capacity() {
        let dir = temp_dir();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let (logger, gate) = blocked_logger(dir.clone(), reports.clone());
        let mut accepted = 0;
        loop {
            let mut large = queued_event("large");
            large.set_message_for_test("x".repeat(4 * 1024 * 1024));
            logger.append(large).unwrap();
            accepted += 1;
            if !reports.lock().unwrap().is_empty() {
                break;
            }
        }
        assert!(accepted < AUDIT_QUEUE_CAPACITY);
        assert!(logger.pending_bytes.load(Ordering::Acquire) <= AUDIT_QUEUE_BYTE_CAPACITY);
        release_consumer(&gate);
        wait_for_drain(&logger);
        assert_eq!(logger.pending_bytes.load(Ordering::Acquire), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_producers_stay_nonblocking_within_byte_budget() {
        let dir = temp_dir();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let (logger, gate) = blocked_logger(dir.clone(), reports.clone());
        let logger = Arc::new(logger);
        let barrier = Arc::new(std::sync::Barrier::new(9));
        let producers = (0..8)
            .map(|producer| {
                let logger = logger.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for event_index in 0..1_000 {
                        logger.append(queued_event(&format!("{producer}-{event_index}")))?;
                    }
                    Ok::<(), AuditError>(())
                })
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        barrier.wait();
        for producer in producers {
            producer.join().unwrap().unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(logger.pending_bytes.load(Ordering::Acquire) <= AUDIT_QUEUE_BYTE_CAPACITY);
        assert_eq!(reports.lock().unwrap().len(), 1);

        release_consumer(&gate);
        wait_for_drain(&logger);
        assert_eq!(logger.pending_bytes.load(Ordering::Acquire), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn overflow_state_starts_a_new_episode_after_recovery() {
        let overflow = OverflowState {
            active: AtomicBool::new(false),
            dropped: AtomicUsize::new(0),
            report_lock: Mutex::new(()),
        };
        let reports = Mutex::new(Vec::new());
        let report = |message: &str| reports.lock().unwrap().push(message.to_string());

        overflow.record_drop(&report);
        overflow.record_drop(&report);
        overflow.record_accept(0, AUDIT_QUEUE_RECOVERY_RECORDS + 1, &report);
        assert_eq!(reports.lock().unwrap().len(), 1);
        overflow.record_accept(0, 0, &report);
        overflow.record_drop(&report);

        let reports = reports.lock().unwrap();
        assert_eq!(reports.len(), 3);
        assert!(reports[0].contains("saturated"));
        assert!(reports[1].contains("2 audit event(s) were dropped"));
        assert!(reports[2].contains("saturated"));
    }

    #[test]
    fn flush_fails_promptly_when_blocked_queue_is_full() {
        let dir = temp_dir();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let (logger, gate) = blocked_logger(dir.clone(), reports);
        for index in 0..AUDIT_QUEUE_CAPACITY {
            logger
                .append(queued_event(&format!("queued-{index}")))
                .unwrap();
        }
        let started = Instant::now();
        assert!(matches!(logger.flush(), Err(AuditError::QueueSaturated)));
        assert!(started.elapsed() < Duration::from_millis(500));
        release_consumer(&gate);
        wait_for_drain(&logger);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn flush_times_out_when_accepted_barrier_cannot_run() {
        let dir = temp_dir();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let (logger, gate) = blocked_logger(dir.clone(), reports);
        logger.append(queued_event("queued")).unwrap();
        let started = Instant::now();
        assert!(matches!(logger.flush(), Err(AuditError::FlushTimeout)));
        assert!(started.elapsed() >= Duration::from_secs(1));
        release_consumer(&gate);
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
