# ADR-0011: Move to Trash is a soft delete, only on explicit user action

Lymic may move Remote Assets into the Immich trash, and only that: **Move to Trash**, from the Lightbox or a timeline multi-selection (any view except the Trash Timeline), after an explicit confirmation that names the count and says the assets can be restored in the Trash. It sends `DELETE /api/assets` with `{ids, force: false}`; `force: true` is never sent, so the action is a soft delete and the assets stay recoverable in the Trash Timeline until Immich removes them after its retention period. Move to Trash and Restore are the only Immich writes that touch trash state, and both need the API key permission `asset.delete`.

This narrows the earlier rule "Lymic never moves a Remote Asset into the trash". What stays unchanged:

- Lymic never deletes permanently, never empties the trash and offers no "restore all".
- Lymic never touches a local file, a Watched Folder or Synchronization state. There is no option to also remove the local file.
- Synchronization never trashes anything. Move to Trash happens only when the user asks for it.
- The planned Virtual Library stays read-only ([ADR-0003](0003-windows-virtual-cloud-library.md)); Explorer operations never become a Move to Trash.

Whether the batch request must fall back to one request per asset, and which response shapes it produces, was observed live on Immich v3.4.1 before any code relied on it; see "Verification of the Move to Trash write (#253)" in the [application map](../agents/application-map.md).

## Considered Options

- **Keep Lymic strictly non-destructive (previous rule)**: Rejected. The user has to leave Lymic and open Immich Web to remove an unwanted photo, although Lymic already shows the Library, the Trash Timeline and Restore. Soft delete with confirmation and a way back (Restore) keeps the library safe.
- **Allow permanent deletion or emptying the trash**: Rejected. There is no way back; this needs its own decision if it is ever wanted.
- **Also remove the local file, or move it to the operating-system recycle bin**: Rejected. Lymic never deletes anything from the user's disk; the local file belongs to the user.
- **Let Synchronization trash the Remote Asset when the local file is deleted**: Rejected. Synchronization is one-directional and does not apply local deletions remotely.
- **Check ownership of the assets before trashing**: Rejected. The backend has no owner information, and Immich decides: an asset the user does not own (including one in a shared Album the user may edit) is refused, and that is handled by the normal partial-failure path (the asset stays visible and selected). Observed on v3.4.1: even an all-permissions key of an admin cannot trash another user's asset.
- **A "do not ask again" setting, an Undo button or a keyboard shortcut**: Rejected for now. The confirmation is always shown, Restore in the Trash Timeline is the way back, and no shortcut can trigger a destructive action by accident.

## Consequences

- The next Synchronization finds a local file in a Watched Folder whose Remote Asset was moved to the trash and reports it as a Trashed Duplicate ([ADR-0010](0010-trashed-duplicates-stay-in-the-trash.md)): a `WARN` in the activity log, no new upload, no restore. This is accepted behaviour. To stop the log entry the user removes the local file themselves; Lymic never does.
- The Trash Timeline offers Restore and Download, never Move to Trash.
- Moving at least one asset makes the cached views that may still show it reload on their next open (the Photo Timeline unless the action ran in it, Explore and Tags). Immich also leaves a trashed asset out of its Album's asset count and out of metadata searches by Album unless `withDeleted` is sent.
- Lymic never forwards the response body or the API key in an error; failures are credential-free codes (`trash_move_authentication_required`, `trash_move_permission_denied:asset.delete`, `trash_move_unavailable`).
- A future extension (permanent deletion, emptying the trash, removing local files, trashing from the Virtual Library, trashing by Synchronization) needs a new decision that supersedes this ADR.
