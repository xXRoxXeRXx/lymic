# ADR-0010: Trashed Duplicates stay in the Immich trash and are reported

Immich treats a Trashed Asset as an existing Remote Asset: the bulk-upload check answers `reject`/`duplicate` with its ID (plus `isTrashed: true`), and a repeated upload answers `duplicate` with the same ID without restoring it (observed on v3.4.1). A local file in a Watched Folder whose Remote Asset is in the trash is therefore a Trashed Duplicate. Lymic leaves the Remote Asset in the trash, records the Upload Unit as synchronized, still performs Album Assignment where Immich allows it, and reports the file in the activity log as being in the Immich trash (`log_duplicate_in_trash`, a warning). To find the Remote Asset by checksum, Lymic sends `withDeleted: true` with its metadata search, because Immich omits Trashed Assets from that search otherwise.

## Considered Options

- **Restore the Trashed Asset during Synchronization**: Rejected. Moving an asset to the trash is a deliberate user action in Immich; undoing it silently from a background sync would be destructive to that decision. The Trash Timeline already offers an explicit Restore.
- **Upload the file again as a new Remote Asset**: Rejected. Immich deduplicates by checksum including the trash and answers `duplicate`, so this is not possible without changing the bytes, and it would leave two Remote Assets for one file once the trashed one is restored.
- **Treat it as a Failed Synchronization**: Rejected. Nothing is wrong with the local file and a retry cannot change the outcome; it would retry on every sync and bury real failures.
- **Stay silent (previous behaviour)**: Rejected. The file looked synchronized but never appeared in Immich, and checksum lookup could not find the Remote Asset at all.

## Consequences

- The report appears once, when the Duplicate is resolved. Afterwards the Upload Unit is synchronized with its persisted Remote Asset ID and is not checked again until the local file changes.
- Album Assignment of a Trashed Asset is attempted like any other. If Immich refuses it, the existing Target Album failure path applies (Failed Synchronization, no silent fallback, retry keeps the persisted Remote Asset ID). Whether Immich accepts it has not been verified against a live server.
- A file synchronized before its Remote Asset was moved to the trash is not checked again, so it is not reported. Lymic does not scan the trash for synchronized files.
- A Duplicate detected only by a repeated upload answering `duplicate` (the bulk check said `accept`) carries no trash status, so it is not reported.
