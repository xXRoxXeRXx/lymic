# Removable watched folders with volume identity and path remapping

Watched folders located on removable storage (such as SD cards) are bound to a stable operating-system volume identifier and relative volume path rather than treated as static absolute paths. When a known volume reconnects under a different mount path or drive letter, Lymic transactionally remaps the watched folder and its associated cached and queued paths in SQLite, reconciling the watcher and scanning only the remapped folder. New volumes trigger a non-blocking notification or banner rather than immediate focus-stealing dialogs, and support optional target albums identically to standard watched folders.

## Considered Options

- **Refactoring the entire sync engine to volume-relative paths**: Store all paths in `watched_folders`, `sync_state`, and `sync_queue` as `(volume_id, relative_path)`. Rejected because fixed local folders lack cross-platform volume identifiers without invasive probing, and rewriting the entire queue and hashing engine adds high complexity for a concern specific to removable media.
- **Immediate modal directory picker on volume arrival**: Steal foreground focus and immediately pop the OS directory picker upon device insertion. Rejected because inserting a USB drive or SD card during unrelated user tasks creates disruptive, accidental input interception. A non-blocking notification or banner gives the user explicit control over when to configure sync.
- **Dedicated import pipeline for removable media**: Implement a separate import workflow distinct from watched folders. Rejected because removable media folders share the exact same synchronization lifecycle, deduplication, watcher semantics, and target-album assignments as standard watched folders.
- **Drive-letter and mount-path polling**: Poll existing folder paths periodically without hardware arrival monitors. Rejected because drive letters on Windows and mount points on Linux/macOS are non-deterministic and can reassign across insertions, causing missed synchronization or reading the wrong drive.

## Consequences

- Database schema migration `0006_removable_watched_folders.sql` adds `removable_watched_folders` linked to `watched_folders.id`.
- Changing mount paths triggers an atomic database transaction that updates `watched_folders.path` alongside descendant paths in `sync_state.local_path` and `sync_queue.local_path`.
- Volume monitoring relies on platform-native mechanisms (Windows `WM_DEVICECHANGE`, macOS Disk Arbitration, Linux udev).
- Removable watched folders support optional target albums through `target_album_id` just like standard watched folders.
