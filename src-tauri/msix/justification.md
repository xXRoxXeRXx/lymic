# Microsoft Store Restricted Capability Justification

## Package Information
- **Application Name**: Lymic
- **Package Identity**: xXRoxXeRXx.Lymic
- **Store Category**: Photo & Video

## Declared Restricted Capabilities
1. `runFullTrust`

## Partner Center Submission Text

### Prompt: "Warum Sie dies benötigen runFullTrust funktion, and how will be used in your product?*"

```text
Lymic is a desktop client for self-hosted Immich servers built with Tauri/Rust. It requires runFullTrust to run its native Win32 process, access Windows Credential Manager for secure API keys, and monitor folders via ReadDirectoryChangesW. Used solely to run the app and sync media to the user's server.
```

## Detailed Technical Justification

### Purpose of Lymic
Lymic is an independent desktop synchronization client for self-hosted Immich media servers, built with Tauri v2 (Rust backend and WebView2).

### Why `runFullTrust` is Required
1. **Native Win32 Desktop Execution:** Lymic runs as a compiled native Win32 application (`Windows.FullTrustApplication`). It requires standard execution permissions to host the WebView2 UI runtime, execute background synchronization tasks, and run its embedded SQLite database.
2. **Native OS Filesystem Monitoring:** Lymic monitors user-selected local folders and removable volumes (e.g. SD cards, external drives) using Windows filesystem notification APIs (`ReadDirectoryChangesW` via the `notify` crate) to detect added or modified media assets in real time.
3. **Secure Credential Storage:** Lymic uses the native Windows Credential Manager (DPAPI / keyring) to store Immich server API keys securely rather than in plaintext config files.
4. **Direct Streaming I/O:** Large media assets (photos and 4K videos) are streamed directly from disk via HTTP multipart upload without loading entire files into memory.

### Scope Limitation
Lymic never scans, indexes, or modifies filesystem locations outside the user-configured Watched Folders. No telemetry or analytics are collected. All media asset transfers occur directly and exclusively to the user-configured private Immich server.
