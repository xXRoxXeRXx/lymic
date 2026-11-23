# Separate Microsoft Store release channel

Lymic will continue to distribute the existing Windows NSIS installer through GitHub, including its Tauri updater, while distributing a separate x64 MSIX package through the Microsoft Store. The Store package is updated only by the Store and has no GitHub/Tauri updater path; this avoids competing updaters and lets the Store provide hosting and package signing. The first Store release is a private Flight, requires reliable persisted Watched Folder access, and does not migrate configuration or Queued Synchronizations from an existing NSIS installation.

## Considered Options

- List the existing NSIS installer in the Microsoft Store. Rejected because it requires publisher code signing and leaves installation updates outside the Store.
- Replace the GitHub installer with MSIX for every Windows user. Rejected for the first release because current installations use the GitHub/NSIS updater and an automatic migration risks local configuration and queued work.

## Implementation Details

- **Build input**: `LYMIC_RELEASE_CHANNEL` (allowed values: `github` [default], `store`). Exposed to the frontend via `VITE_LYMIC_RELEASE_CHANNEL` / `LYMIC_RELEASE_CHANNEL`.
- **Runtime observability**: The Tauri command `get_release_channel` returns the active channel to the desktop adapter and UI.
- **Store channel behaviour**: Disables the in-app Tauri update check on startup and in settings, omits the update dialog and settings update actions, and prevents outbound requests to GitHub update endpoints.
- **Store-only features**: The read-only Windows Virtual Library ([ADR-0003](0003-windows-virtual-cloud-library.md)) relies on the Store package's Package Identity and ships only in this channel. Besides the channel flag it needs real Package Identity at runtime, so it is hidden in `tauri dev` and in any unpackaged run; developers use a locally registered MSIX ([`docs/store/local-msix-build.md`](../store/local-msix-build.md)). Its manifest declarations are the thumbnail provider (`windows.cloudFiles`, `windows.comServer`) and the autostart `StartupTask`.

