# Folder-scoped target albums without root fallback

Watched folders can be associated with an optional Immich target album. When configured, all media discovered in that folder (both newly uploaded files and pre-existing server duplicates) are added to the specified album. The target album is recorded immutably in the sync queue at enqueue time. If album operations fail (e.g., missing permissions or a deleted album), affected assets fail in the queue without a silent fallback to the root library.

## Considered Options

- **Silent fallback to root library**: If an album cannot be read or written to, upload assets directly into the general library without album membership. Rejected because users who organize photos into target albums expect strict containment; silent fallback misplaces assets into the root library without explicit consent.
- **Dynamic folder-to-album lookup during sync**: Look up the target album from the watched folder table dynamically when processing queue blocks. Rejected because changing or removing a folder in flight could lead to inconsistent states. Persisting `target_album_id` directly in each queue entry preserves idempotency and queue isolation.
- **Immediate single-item album assignment**: Call the album assignment API for each file immediately upon upload. Rejected in favor of batching assignments per processed chunk (up to 100 items), minimizing network round-trips to Immich while preserving accurate failure reporting.

## Consequences

- The database schema introduces `sync_queue.target_album_id` and persists `sync_queue.remote_id` upon successful upload so retries do not re-upload media if album assignment fails.
- Immich API keys require the additional permissions `album.read`, `album.create`, and `albumAsset.create`.
- Folders configured with a missing or inaccessible target album fail during sync until the album or permissions are restored.
