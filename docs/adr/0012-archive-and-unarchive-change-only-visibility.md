# ADR-0012: Archive and Unarchive change only the visibility between `timeline` and `archive`

Lymic may Archive a Remote Asset (visibility `timeline` to `archive`) and Unarchive it (`archive` to `timeline`), from the Lightbox and from a multi-selection, at once and without a confirmation dialog because both are fully reversible. These are the only writes of an asset's visibility. Lymic never writes `locked` or `hidden`, never moves a Remote Asset into or out of the Immich trash through them, and never deletes one. Synchronization never reads or writes the archive status: an archived Duplicate stays archived, and Synchronization neither archives nor unarchives anything. The write is one bulk `PUT /api/assets` (`{"ids": [...], "visibility": "archive" | "timeline"}`, `asset.update`, answered 204 without a per-asset result); because a stale ID fails the whole batch with 400, a rejected or unclear batch falls back to one `PUT /api/assets/{id}` per asset, and a single asset is never requested twice. The Tauri command accepts only the two values (a Rust enum, rejected at deserialization), since Immich itself also accepts `hidden` and `locked`: the restriction is Lymic's alone. Each view offers one action in a multi-selection (Unarchive in the Archive Timeline, Archive elsewhere), and the Trash Timeline and the Map View offer none.

## Considered Options

- **Keep the Archive Timeline read-only (previous rule)**: Rejected. Users could see Archived Assets but had to leave Lymic to change them, which is the daily use of the feature.
- **Let the command take any visibility Immich accepts**: Rejected. `locked` pulls an asset out of the user's library views and `hidden` would touch a Live Photo's video; neither is something Lymic means to offer, and the server does not guard them (verified on v3.4.1), so the allowed set is enforced in the command and the enum.
- **Ask for confirmation**: Rejected. Archive and Unarchive undo each other, so a dialog would only slow down tidying a run of photos, in line with Favorite.
- **Per-asset requests only**: Rejected. A selection would cost one request per asset, and the bulk endpoint covers the common case where every ID is valid.
- **Offer Archive in the Trash Timeline**: Rejected as a product decision, not because the server refuses it: Immich v3.4.1 accepts an Archive of a trashed asset (204, the asset stays in the trash). A Trashed Asset is handled by Restore only.

## Consequences

- The Glossary entries Archived Asset and Archive Timeline, and the Application Map's product boundary and Archive Timeline section, no longer say that Lymic never archives or unarchives.
- A new `asset.update` use appears in the permission table; users whose key already allows Favorite need nothing new. A key without it gets a message that names the permission.
- A stale selection can contain an asset that was trashed in Immich or another client while a Lymic view was open. The server archives it silently (204), it stays in the trash, and it comes back archived when restored. Lymic does not pre-check this.
- The bulk contract was observed on Immich v3.4.1 only; the per-asset fallback request (the Favorite endpoint with the `visibility` field) and a live run through the Lymic UI are not verified. Lymic claims no minimum Immich version.
- Synchronization is unchanged. The Photo Timeline is marked stale after an Unarchive so it reloads once when it is next opened from another view; Favorites, Archive and Trash already reload on open.
