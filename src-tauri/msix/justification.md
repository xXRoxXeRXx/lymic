# Microsoft Store Restricted Capability Justification

## Package Information
- **Application Name**: Lymic
- **Package Identity**: xXRoxXeRXx.Lymic
- **Store Category**: Photo & Video

## Declared Restricted Capabilities
1. `runFullTrust`
2. `broadFileSystemAccess`

## Narrowly Scoped Certification Justification

### Purpose of Lymic
Lymic is an independent cross-platform desktop synchronization client for self-hosted Immich media servers. The core product functionality is continuously discovering, monitoring, and synchronizing user-selected local media folders (**Watched Folders**) and removable media volumes (such as camera SD cards and external hard drives) to the user's private Immich server.

### Why `broadFileSystemAccess` is Required
1. **Persistent Watched Folders across Arbitrary Volumes:** Users select custom folders located anywhere on internal and external drives (e.g. `D:\Photos`, `E:\DCIM`, network-mounted shares). Lymic must retain access to these configured folders across application restarts and system reboots without requiring the user to re-pick folders in a file picker on every launch.
2. **Recursive Filesystem Change Monitoring:** Lymic watches configured folders recursively using native operating system filesystem change notifications (`notify` crate / `ReadDirectoryChangesW`) to automatically detect new or updated photos and videos in real time.
3. **Removable Media Reconciliation:** When a bound removable storage volume is disconnected and later reconnected, Lymic recognizes the volume identifier and re-attaches filesystem watching automatically.

### Scope Limitation
Lymic never scans, indexes, or modifies filesystem locations outside the user-configured Watched Folders. No telemetry or analytics are collected. All media asset transfers occur directly and exclusively to the user-configured self-hosted Immich server.
