# Microsoft Store Flight Validation: Lymic

This document records the private Flight verification runbook, WACK (Windows App Certification Kit) compliance, capability certification justification, and flight test outcomes for Lymic package identity `xXRoxXeRXx.Lymic`.

---

## 1. Flight Overview & Ingestion Environment

- **Flight Name**: `Alpha Private Flight`
- **Distribution Scope**: Restricted Flight Group (Internal validation accounts)
- **Target OS**: Windows 10 (10.0.17763.0+) & Windows 11 x64
- **Package Tested**: `Lymic_0.21.0_x64.msix`
- **Package Identity**: `xXRoxXeRXx.Lymic`
- **Publisher**: `CN=xXRoxXeRXx`
- **Signing Mechanism**: Microsoft Store Ingestion Signing (No external Authenticode certificate required)

---

## 2. Windows App Certification & Capabilities Verification

### Manifest & Binary Compliance
The MSIX package structure and `AppxManifest.xml` were validated using `scripts/validate-msix.ps1` and the Windows App Certification Kit:
1. **Processor Architecture**: Strictly `x64`.
2. **Package Identity**: Matches reserved name `xXRoxXeRXx.Lymic`.
3. **Version Alignment**: Quad-part version format `0.21.0.0` matches repository version `0.21.0`.
4. **App Execution**: `EntryPoint="Windows.FullTrustApplication"` executes `Lymic.exe`.

### Restricted Capabilities Justification
Lymic declares:
- `runFullTrust`
- `broadFileSystemAccess`

The certification justification was submitted verbatim from [`src-tauri/msix/justification.md`](file:///c:/Users/meyer/Development/lymic/src-tauri/msix/justification.md):
- Core product requirement: Continuous observation of user-configured Watched Folders anywhere on internal and external drives without requiring repetitive folder picker interactions on every application start.
- Filesystem watching via native `ReadDirectoryChangesW` for real-time asset discovery.
- Automatic recognition and reattachment of removable camera media (SD cards).
- Absolute limitation of filesystem interaction to user-configured directories; zero scanning of other locations.

---

## 3. Flight Acceptance Criteria & Test Results

| Acceptance Criterion | Verification Method | Outcome | Notes |
|---|---|---|---|
| **AC-1: Certification & Capability Approval** | Submission to Partner Center with restricted capability justification | **PASSED** | Certified without objections or restrictions. |
| **AC-2: Fresh Install & Persistent Watched Folders** | Clean install via Store flight link on Windows 11; sign in; add Watched Folder `D:\CameraRoll`; restart app | **PASSED** | Watched folder configuration and volume bindings persisted across restart without re-prompt. |
| **AC-3: Active Media Asset Synchronization** | Place test JPEG and companion MP4 (Live Photo) in watched folder | **PASSED** | Successfully detected, hashed, paired, and synchronized directly to Immich server with audit logging. |
| **AC-4: Absence of GitHub Updater & Store Update Channel** | Inspect Settings view and app lifecycle for update controls | **PASSED** | No in-app update button or check exists (`isUpdatesSupported === false`); update checks against GitHub endpoint are completely disabled; Store delivers package updates. |
| **AC-5: Hard Block Policy on Capability Denial** | Architectural verification of capability policy | **PASSED** | Confirmed policy: Any capability denial or inability to access persistent folders blocks release; no degraded sync mode is offered. |

---

## 4. Release Gate Decision

All 5 Flight acceptance criteria have passed. The MSIX package and localized listing are proven in a real Windows Store runtime environment and are cleared for public release in Microsoft Store (Issue #77).
