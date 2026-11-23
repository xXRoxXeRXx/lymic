//! A small append-only text log for diagnosing the Virtual Library on a real machine (#266).
//!
//! The callbacks run on platform threads of a GUI process: `eprintln!` goes nowhere, and the app's
//! activity log is for things a user acts on, not for one line per file read. This is a plain file
//! (`lymic-virtual-library.log` in the package's `LocalState` folder, next to
//! `lymic-shell-ext.log`) that a maintainer can open after a test run to see who read what.
//!
//! It is bounded: when the file passes the limit it is replaced by a fresh one, so it can never
//! fill the disk, and it only keeps the most recent run of lines. Writing never fails the caller.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

/// Where the log stops growing. The previous file is kept once as `<name>.old`, so a burst of lines
/// cannot erase the context just before it.
pub(super) const LIMIT_BYTES: u64 = 512 * 1024;

/// Where diagnostic lines go. The adapter's implementation is the log file; tests record them.
pub(super) trait Notes: Send + Sync {
    fn note(&self, message: &str);
}

/// A shared sink, so a background task can keep writing after the call that started it returned.
pub(super) type SharedNotes = std::sync::Arc<dyn Notes>;

/// The diagnostic log of the running app (`lymic-virtual-library.log`), or standard error where
/// there is none.
#[derive(Clone, Copy)]
pub(super) struct SystemNotes;

impl Notes for SystemNotes {
    fn note(&self, message: &str) {
        super::system_note(message);
    }
}

/// Throws every line away.
pub(super) struct NoNotes;

impl Notes for NoNotes {
    fn note(&self, _message: &str) {}
}

pub(super) struct DiagnosticLog {
    path: PathBuf,
    limit: u64,
    write: Mutex<()>,
}

impl DiagnosticLog {
    pub(super) fn new(path: PathBuf) -> Self {
        Self::with_limit(path, LIMIT_BYTES)
    }

    fn with_limit(path: PathBuf, limit: u64) -> Self {
        Self {
            path,
            limit,
            write: Mutex::new(()),
        }
    }

    /// Appends `<unix seconds> <message>`. Errors are swallowed: a log must not break a read.
    pub(super) fn append(&self, message: &str) {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        self.append_at(seconds, message);
    }

    fn append_at(&self, seconds: u64, message: &str) {
        let _guard = self.write.lock();
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::metadata(&self.path).is_ok_and(|metadata| metadata.len() >= self.limit) {
            let mut old = self.path.clone().into_os_string();
            old.push(".old");
            let _ = std::fs::rename(&self.path, old);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            // One line per message, whatever the message holds.
            let message = message.replace(['\r', '\n'], " ");
            let _ = writeln!(file, "{seconds} {message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir()
            .join(format!(
                "lymic-diagnostic-log-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
            .join(name)
    }

    fn clean(path: &std::path::Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn lines_are_appended_with_their_time_and_creating_the_folder() {
        let path = temp_path("a.log");
        let log = DiagnosticLog::new(path.clone());

        log.append_at(100, "first");
        log.append_at(101, "second");

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "100 first\n101 second\n"
        );
        clean(&path);
    }

    #[test]
    fn a_message_with_line_breaks_stays_on_one_line() {
        let path = temp_path("a.log");
        let log = DiagnosticLog::new(path.clone());

        log.append_at(1, "a\r\nb\nc");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1 a  b c\n");
        clean(&path);
    }

    #[test]
    fn a_full_log_is_replaced_and_the_previous_one_kept_once() {
        let path = temp_path("a.log");
        let log = DiagnosticLog::with_limit(path.clone(), 20);

        log.append_at(1, "01234567890123456"); // 20 bytes with its time and newline: full
        log.append_at(2, "fresh");
        log.append_at(3, "again");

        let mut old = path.clone().into_os_string();
        old.push(".old");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "2 fresh\n3 again\n"
        );
        assert_eq!(
            std::fs::read_to_string(old).unwrap(),
            "1 01234567890123456\n"
        );
        clean(&path);
    }

    #[test]
    fn an_unwritable_location_does_not_fail_the_caller() {
        // A file where the log's folder should be.
        let path = temp_path("blocker");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"x").unwrap();
        let log = DiagnosticLog::new(path.join("a.log"));

        log.append_at(1, "lost");

        clean(&path);
    }
}
