# Local MSIX build with Package Identity

Developer guide for building Lymic as an MSIX and registering it on your own machine, so the app runs **with Package Identity**.

Why this exists: the planned read-only Windows Virtual Library ([ADR-0003](../adr/0003-windows-virtual-cloud-library.md)) is gated on actual Package Identity at runtime, in addition to `LYMIC_RELEASE_CHANNEL=store` ([ADR-0004](../adr/0004-microsoft-store-release-channel.md)). It is therefore unavailable in `npm run tauri dev` and in any unpackaged run of the executable, including the raw `lymic-desktop.exe` from `target/release`. A locally registered MSIX is how developers exercise it. This guide reuses the release tooling, so the local package has the same manifest and capabilities as the Store package:

- [`scripts/package-msix.ps1`](../../scripts/package-msix.ps1) stages the executable and icons, fills [`src-tauri/msix/AppxManifest.template.xml`](../../src-tauri/msix/AppxManifest.template.xml) and runs `makeappx.exe`.
- [`scripts/validate-msix.ps1`](../../scripts/validate-msix.ps1) checks the manifest of the produced package.

> **Verification status.** The packaging, validation and self-signed signing commands below were run on a Windows Server 2022 machine, using a stand-in executable instead of a real Lymic build. The registration, launch and removal steps (`Add-AppxPackage`, `Remove-AppxPackage`, certificate trust) were **not** run there, because Developer Mode was off on that machine and was not changed. Treat those steps as written from the documented Windows behavior and report any deviation. Nothing here has been verified against a real Lymic build, and the runtime Package Identity check (#272) has not been run in a registered build either (see [Confirming Package Identity](#confirming-package-identity-at-runtime)).

## Prerequisites

- Windows 10 version 1809 (build 17763) or later, x64. The manifest targets `Windows.Desktop` 10.0.17763.0 and up.
- The normal [build prerequisites](../README.md#3-requirements) (Rust, Node.js, WebView2).
- The Windows SDK, for `makeappx.exe` (and `signtool.exe` for the signed route). `package-msix.ps1` finds `makeappx.exe` on `PATH` or under `C:\Program Files (x86)\Windows Kits\10\bin\*\x64`. `signtool.exe` lives in the same folder and is usually not on `PATH`.
- For the **loose** route: Windows **Developer Mode** (Settings, System, For developers). Registering an unpacked manifest with `Add-AppxPackage -Register` requires it.
- For the **signed** route: an elevated PowerShell for trusting the certificate, and sideloading of apps enabled (the default on current Windows 10/11).

## Pick a dev identity (do not reuse the Store identity)

The defaults of both scripts are the Store identity (`xXRoxXeRXx.Lymic`, publisher `CN=EB6D92C3-1B82-4D11-B562-6EA2D7C2FDEC`). Do not register a local package under that name: it collides with a Store or Flight installation of Lymic, and a self-signed package cannot carry the Store publisher. Use a separate identity for local builds:

| Value | Used in this guide |
| --- | --- |
| Identity name | `Lymic.Dev` |
| Publisher | `CN=LymicDev` |
| Publisher display name | `Lymic Dev` |
| Version | any, for example `0.0.1` |

Because the identity differs from the Store one, pass the matching `-ExpectedIdentity` and `-ExpectedPublisher` to `validate-msix.ps1` (shown below). The packaged app has its own per-package state, so it does not share the database or settings of an unpackaged Lymic or of a Store installation, and removing the package discards that state. This matches [ADR-0004](../adr/0004-microsoft-store-release-channel.md), which does no configuration migration between channels.

## 1. Build the Store-channel executable

Run from the repository root. This is the same build the release workflow does for the Store package:

```powershell
$env:LYMIC_RELEASE_CHANNEL = "store"
npm run build
cargo build --manifest-path src-tauri/Cargo.toml --release --locked --features custom-protocol
```

The output is `src-tauri/target/release/lymic-desktop.exe`.

## 2. Package and validate

```powershell
./scripts/package-msix.ps1 `
  -Version 0.0.1 `
  -IdentityName "Lymic.Dev" `
  -Publisher "CN=LymicDev" `
  -PublisherDisplayName "Lymic Dev" `
  -SourceExe "src-tauri/target/release/lymic-desktop.exe"

./scripts/validate-msix.ps1 `
  -MsixPath "$PWD\dist\msix\Lymic_0.0.1_x64.msix" `
  -ExpectedVersion 0.0.1 `
  -ExpectedIdentity "Lymic.Dev" `
  -ExpectedPublisher "CN=LymicDev"
```

Notes:

- Give `validate-msix.ps1` an **absolute** `-MsixPath`. It checks the file with PowerShell's current location but opens it relative to the process working directory, so a relative path can fail with "path not found" in a shell that was started elsewhere.
- Omitting `-SourceExe` makes the script pick the newest of `lymic-desktop.exe`, `Lymic.exe` or `lymic.exe` under `src-tauri/target/release`.
- Outputs: the package `dist/msix/Lymic_<version>_x64.msix` and the unpacked staging folder `target/msix-staging/` (it contains `AppxManifest.xml`, `Lymic.exe` and `Assets/`). Both are git-ignored. The staging folder is wiped on every run.
- Packaging with the default (Store) identity and validating with the default arguments is exactly what CI does; local builds only differ in the identity arguments.

Then register the package with one of the two routes below.

## 3a. Loose registration (Developer Mode, no signing)

Fastest route and the best one while iterating. The package is registered from the staging folder, so no certificate is needed.

```powershell
Add-AppxPackage -Register "$PWD\target\msix-staging\AppxManifest.xml"
```

The registration points at `target/msix-staging/`, so keep that folder in place while the package is registered. After rebuilding, run `package-msix.ps1` again (it recreates the folder) and repeat the `Add-AppxPackage -Register` command to pick up the new files. If Windows refuses because the app is running, close Lymic (quit from the tray menu) first.

## 3b. Dev-signed registration (self-signed certificate)

Use this when you want to install the real `.msix` file, for example to test the install path. The certificate subject must equal the manifest publisher (`CN=LymicDev`).

1. Create a code-signing certificate once (current user store):

   ```powershell
   $cert = New-SelfSignedCertificate -Type Custom `
     -Subject "CN=LymicDev" `
     -KeyUsage DigitalSignature `
     -FriendlyName "Lymic dev signing" `
     -CertStoreLocation "Cert:\CurrentUser\My" `
     -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
   $cert.Thumbprint
   ```

2. Trust it for sideloading. Elevated PowerShell:

   ```powershell
   Export-Certificate -Cert $cert -FilePath "$PWD\dist\msix\LymicDev.cer"
   Import-Certificate -FilePath "$PWD\dist\msix\LymicDev.cer" -CertStoreLocation "Cert:\LocalMachine\TrustedPeople"
   ```

3. Sign the package (use the `signtool.exe` from the Windows SDK folder if it is not on `PATH`):

   ```powershell
   signtool sign /fd SHA256 /sha1 <thumbprint> "$PWD\dist\msix\Lymic_0.0.1_x64.msix"
   ```

4. Install it:

   ```powershell
   Add-AppxPackage "$PWD\dist\msix\Lymic_0.0.1_x64.msix"
   ```

Sign again after every re-package (packaging replaces the file). To install a newer build over an existing one, raise `-Version` or remove the package first. Signing with the certificate was exercised; `signtool verify` reports the chain as untrusted until step 2 is done, which is expected.

## 4. Launch with Package Identity

Start Lymic from the Start menu (the entry is named **Lymic**) or by its app user model ID:

```powershell
$pkg = Get-AppxPackage -Name "Lymic.Dev"
Start-Process "shell:AppsFolder\$($pkg.PackageFamilyName)!App"
```

Do not run `lymic-desktop.exe` or `target/msix-staging/Lymic.exe` directly: a process started that way has **no** Package Identity, even though it is the same binary.

To confirm that the registration exists and see its install location:

```powershell
Get-AppxPackage -Name "Lymic.Dev" | Select-Object Name, Version, PackageFullName, InstallLocation, SignatureKind
```

`SignatureKind` is `None` for a loose registration and `Developer` for a dev-signed package.

## 5. Remove the registration

Quit Lymic first (tray menu, Quit), then:

```powershell
Get-AppxPackage -Name "Lymic.Dev" | Remove-AppxPackage
```

This works for both routes. It also deletes the package's per-package application data, so Lymic starts with a clean state if you register it again. Confirm with `Get-AppxPackage -Name "Lymic.Dev"`, which should print nothing.

Cleanup for the signed route (elevated PowerShell for the machine store):

```powershell
Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Subject -eq "CN=LymicDev" | Remove-Item
Get-ChildItem Cert:\CurrentUser\My | Where-Object Subject -eq "CN=LymicDev" | Remove-Item
```

Also delete `dist/msix/` and `target/msix-staging/` if you want the build outputs gone (they are git-ignored). Disabling Developer Mode again is optional and independent of the registration.

## Troubleshooting

| Symptom | Likely cause |
| --- | --- |
| `Add-AppxPackage -Register` reports that Developer Mode is required | Turn on Developer Mode, or use the dev-signed route. |
| Installing the `.msix` fails with a trust or signature error (for example `0x800B0109`) | The certificate is not in `LocalMachine\TrustedPeople`, or its subject differs from the manifest publisher. |
| Registration conflicts with an installed Lymic (`0x80073CFB`) | A package with the same identity name but another publisher exists. Use the `Lymic.Dev` identity, or remove the other package. |
| `makeappx.exe not found` | Install the Windows SDK. |
| `validate-msix.ps1` says the package was not found | Pass an absolute `-MsixPath`. |
| `validate-msix.ps1` reports an identity or publisher mismatch | Pass `-ExpectedIdentity` and `-ExpectedPublisher` matching the packaging arguments. |

## Confirming Package Identity at runtime

The Virtual Library settings gate (#272) decides availability at runtime and never from the release-channel label alone: Windows, the `store` release channel **and** real Package Identity (`has_package_identity` in `src-tauri/src/app/package_identity.rs`, which calls `GetCurrentPackageFullName`). To confirm the identity in a build registered through this guide:

1. Check that the package is registered: `Get-AppxPackage -Name "Lymic.Dev"` prints it.
2. Start Lymic from its Start menu entry (or `explorer.exe shell:AppsFolder\<PackageFamilyName>!App`, with the family name from `Get-AppxPackage`), not by running the `.exe` directly.
3. Open Settings. With an identity, the "Virtual Library" section is shown (the backend command `get_virtual_library_settings` reports `available: true`).
4. As a negative control, run `npm run tauri dev`, or start the raw `lymic-desktop.exe` from `target/release`. Neither has a package identity, so the "Virtual Library" section must be absent even though the executable was built with `LYMIC_RELEASE_CHANNEL=store`.

The acceptance criterion of ticket #274 (an identity in this local build, none in `tauri dev`) is **not yet verified**: the check exists, but it has not been run in a registered build. Run the steps above and record the result and the Windows version here.

## Autostart inside the package (#284)

**Finding.** Lymic's existing autostart (`tauri-plugin-autostart`, which writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) is not expected to work with Package Identity. Microsoft documents that for a packaged desktop app "all writes under HKCU are copied on write to a private per-user, per-app location" ([Understanding how packaged desktop apps run on Windows, Registry](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes#registry)), while Explorer starts Run entries from the real hive. The documented mechanism for a packaged desktop app is a `windows.startupTask` extension in the manifest toggled through the `StartupTask` API ([StartupTask class](https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.startuptask), [desktop:StartupTask](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-desktop-startuptask)).

**What Lymic does.**

- `src-tauri/msix/AppxManifest.template.xml` declares a `desktop:Extension Category="windows.startupTask"` with `TaskId="LymicStartup"` and `Enabled="false"` (off until the user turns it on). The template is shared with the Store package, so the Store package has the same declaration. `validate-msix.ps1` still passes (it checks identity, publisher, architecture and capabilities only).
- The backend commands `get_autostart_enabled` and `set_autostart_enabled` (`src-tauri/src/commands/autostart.rs`) use the StartupTask when `has_package_identity()` is true and the login item otherwise (unpackaged runs, `tauri dev`, macOS, Linux). The frontend no longer knows about the mechanism.
- A StartupTask launch carries no `--autostart` argument. It is recognised by its activation kind (`AppInstance::GetActivatedEventArgs`, `ActivationKind::StartupTask`) and starts hidden in the tray when credentials exist, like the login item does.
- Windows may keep the task off after a request (`DisabledByUser` after the user turned it off in Settings or Task Manager, `DisabledByPolicy`). The toggle then reports an error that says where to turn it back on, instead of silently doing nothing.

**Not verified.** None of the following was run, because the machine used for the implementation (Windows Server 2022) could not register a package. Verify them in a registered build (see the steps above) and record the Windows version and result here.

1. `makeappx` accepts the manifest with the startup task. Packaging and `validate-msix.ps1` were run with a stand-in executable; registration was not.
2. The toggle in Settings enables the task: after turning autostart on, **Task Manager, Startup apps** lists "Lymic" as Enabled, and **Settings, Apps, Startup** shows it.
3. After signing out and in again, Lymic starts hidden in the tray (not with a window) when logged in to an Immich server, and with the window when there are no stored credentials. This depends on `AppInstance::GetActivatedEventArgs` reporting `ActivationKind::StartupTask` for a full-trust packaged process, which is documented for packaged apps but unproven here. If it does not, the failure is benign (the window is shown at sign-in) and the launch detection needs another signal.
4. Turning the task off in Task Manager makes the Settings toggle show off after reopening Settings, and turning it on again in Lymic reports the "turn it back on in Windows Settings" error.
5. The Run-key finding itself: in a registered build, the old mechanism would not start Lymic at sign-in. This is inferred from the Microsoft documentation above, not observed.
6. WinRT calls from a Tauri blocking-pool thread (no explicit COM initialisation) work; the WinRT types used are agile, but this was not run inside a package. Outside a package, `StartupTask.GetAsync` fails with an error (observed in a unit test on the build machine, from a thread without COM initialisation, so the WinRT call path itself runs), which is why the packaged path is used only when Package Identity is present.
7. Startup task launches within the Virtual Library: the in-app hint ("Photos you have not opened yet only work while Lymic is running") appears while the Virtual Library is on and autostart is off, and disappears when autostart is turned on.


## Thumbnail provider (#282)

The Virtual Library shows Immich thumbnails for placeholders without Hydration through a COM thumbnail provider, the separate workspace crate `src-tauri/lymic-shell-ext` ([ADR-0003](../adr/0003-windows-virtual-cloud-library.md)). It is declared in the shared manifest template, so the local package and the Store package carry the same declaration.

> **Verification status: NOT OBSERVED IN EXPLORER.** The crate compiles and its pure logic and its decoding are unit tested; the manifest packs with `makeappx` and passes `validate-msix.ps1`. Nothing was registered, no placeholder was shown and no thumbnail was requested by Explorer on the machine this was written on (Windows Server 2022, no Developer Mode). That the provider is invoked, that it shows a thumbnail and that it causes no Hydration is **unproven**. The gate prototype's result (the provider was never invoked) is not explained by this work either; see [What is uncertain](#what-is-uncertain). The manual record is in [`docs/virtual-library-verification.md`](../virtual-library-verification.md), section "Thumbnail provider (#282)".

### How it works

- **App side.** `cloud_files/shell_thumbnails.rs` wraps the projection's `Library`: whenever a directory is listed (the first Explorer visit and every Reconciliation run), the Immich thumbnail (`GET assets/{id}/thumbnail?format=JPEG`) of each file is stored under `<package LocalState>\shell-thumbnails\<sha256 of the placeholder's file identity>.thumb`. The first 48 are fetched before the listing returns (so they exist when Explorer asks), the rest in the background. A failed fetch is ignored. Disabling the Virtual Library and logging out delete the directory. It is separate from the app's own Thumbnail Cache and is bounded to 256 MiB, oldest first.
- **Provider side.** `lymic-shell-ext` implements `IInitializeWithItem` and `IThumbnailProvider`. `Initialize` reads only the item's file system path, opens the placeholder with `FILE_READ_ATTRIBUTES` (an attributes-only open does not read data) and reads its file identity with `CfGetPlaceholderInfo`. `GetThumbnail` hashes the identity, reads that cache file, decodes it with WIC from memory and returns a bitmap no larger than requested. A miss is an error, never a download and never a read of the placeholder: Explorer then shows its generic icon until the app has cached the thumbnail. Every request is logged to `lymic-shell-ext.log` in the package's `LocalState` folder.
- **No server access, no credentials.** The provider never talks to Immich, so it needs no stored credentials and no network access.

### The declarations

In `src-tauri/msix/AppxManifest.template.xml`, under `Package/Applications/Application/Extensions`, next to the startup task:

```xml
<desktop3:Extension Category="windows.cloudFiles">
  <desktop3:CloudFiles>
    <desktop3:CustomStateHandler />
    <desktop3:ThumbnailProviderHandler Clsid="9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41" />
    <desktop3:ExtendedPropertyHandler />
    <desktop3:BannersHandler />
    <desktop3:CloudFilesContextMenus />
  </desktop3:CloudFiles>
</desktop3:Extension>
<com:Extension Category="windows.comServer">
  <com:ComServer>
    <com:ExeServer Executable="LymicShellExt.exe" Arguments="-Embedding" DisplayName="Lymic Thumbnail Provider">
      <com:Class Id="9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41" DisplayName="Lymic Thumbnail Provider" />
    </com:ExeServer>
  </com:ComServer>
</com:Extension>
```

plus `xmlns:desktop3="http://schemas.microsoft.com/appx/manifest/desktop/windows10/3"` and `xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"` on `<Package>` (neither is ignorable). The template holds `{{THUMBNAIL_CLSID}}` and `{{THUMBNAIL_COM_SERVER}}`; `package-msix.ps1` fills them (the CLSID lives in the script and in `lymic_shell_ext::THUMBNAIL_PROVIDER_CLSID`; a test in the crate fails when they differ). `LymicShellExt.exe` is `lymic-shell-ext-host.exe`, copied to the package root. The sync root registration (`StorageProviderSyncRootInfo`, `cloud_files/adapter/register.rs`) needed no change: it has no thumbnail field, the handler comes from the manifest.

The alternative, `package-msix.ps1 -ComServer surrogate`, packages `lymic_shell_ext.dll` and declares

```xml
<com:SurrogateServer DisplayName="Lymic Thumbnail Provider">
  <com:Class Id="9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41" Path="LymicShellExt.dll" ThreadingModel="Both" DisplayName="Lymic Thumbnail Provider" />
</com:SurrogateServer>
```

instead. The DLL needs `--features surrogate-dll` (it exports `DllGetClassObject` and `DllCanUnloadNow`); without the feature the crate exports nothing, so the app that links it does not either.

### Build and package

Add the provider to the steps above (the release workflow does the same on Windows):

```powershell
cargo build --manifest-path src-tauri/Cargo.toml --release --locked -p lymic-shell-ext
# for the surrogate variant: ... -p lymic-shell-ext --features surrogate-dll
./scripts/package-msix.ps1 -Version 0.0.1 -IdentityName "Lymic.Dev" -Publisher "CN=LymicDev" -PublisherDisplayName "Lymic Dev" -SourceExe "src-tauri/target/release/lymic-desktop.exe"
# surrogate: add -ComServer surrogate
```

`package-msix.ps1` stops with the build command when the provider binary is missing (`-ShellExtExe` and `-ShellExtDll` override the paths). `validate-msix.ps1` additionally checks that the `ThumbnailProviderHandler` has a Clsid, that a `com:Class` with that Id exists, and that the declared executable or DLL is in the package. Both variants were packed and validated here with a stand-in executable; a package with the server file removed was rejected.

### Findings that led to these declarations

Read on 2026-11-23 from Microsoft Learn and the Microsoft sample:

- **The handler.** `desktop3:ThumbnailProviderHandler` ([schema page](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-desktop3-thumbnailproviderhandler)) has one optional attribute, `Clsid`, "the class ID of the app that implements the ThumbnailProviderHandler". It sits in `desktop3:CloudFiles` ([page](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-desktop3-cloudfiles), Windows 10 1709). A newer namespace, `cloudFiles:CloudFiles` (`http://schemas.microsoft.com/appx/manifest/cloudfiles/windows10`, [page](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-cloudfiles-cloudfiles)), needs build 19645 or later and was not used, because the template targets 10.0.17763.
- **All five handler elements are required in practice.** The Learn pages mark every child of `CloudFiles` optional. The schema in the Windows SDK 10.0.26100 (`makeappx`) says otherwise, **re-verified here by packing five manifest variants**: only `ThumbnailProviderHandler` (desktop3 namespace or the newer cloudFiles namespace, with an exe or a surrogate server) is rejected with "The app manifest must be valid as per schema ... not complete"; the five elements `CustomStateHandler`, `ThumbnailProviderHandler`, `ExtendedPropertyHandler`, `BannersHandler` and `CloudFilesContextMenus` are accepted, the unused ones without a Clsid. Microsoft's own sample works within the same constraint differently: it gives every handler a Clsid, and uses one made-up GUID (`20000000-0000-0000-0000-000000000001`, with no `com:Class`) for the extended property and banners handlers.
- **The COM server is an executable in Microsoft's sample.** The [CloudMirror sample](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/CloudMirror) declares one `com:ExeServer` (`Executable="CloudMirror\CloudMirror.exe"`, no `Arguments`) per handler class, all hosted by the sync engine's own executable; it is the only first-party working example of packaged Cloud Files handlers and is the reference in [Build a Cloud Sync Engine](https://learn.microsoft.com/en-us/windows/win32/cfapi/build-a-cloud-file-sync-engine). Its `StorageProviderSyncRootInfo` registration has no thumbnail field.
- **A DLL surrogate.** `com:SurrogateServer` ([page](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-com-surrogateserver)) with a `com:Class Path=... ThreadingModel=...` registers an in-process class in the package's private hive; `AppId`, `CustomSurrogateExecutable` and `SystemSurrogate` are optional. The `com4` schema is described as a superset that registers only the surrogate for out-of-process activation and needs build 20348; not used. This is also what Nextcloud's Windows client does, but unpackaged: the `ThumbnailProvider` value under `HKLM\...\Explorer\SyncRootManager\<id>` holds a CLSID whose `InprocServer32` is a DLL, loaded in `dllhost.exe` (their [issue 11022](https://github.com/nextcloud/desktop/issues/11022)). That is the registry route ADR-0003 rejected for the NSIS channel.
- **Choice.** The default is the `ExeServer`, because (1) it is Microsoft's own pattern, (2) in the gate prototype it was the variant for which `StorageProviderSyncRootManager.Register` succeeded while the surrogate failed with 0x80070490, and (3) the host exits by itself when idle ([`idle.rs`](../../src-tauri/lymic-shell-ext/src/idle.rs)), so it does not keep running. The surrogate DLL is kept behind `-ComServer surrogate` so that both can be tried without changing code.

### What is uncertain

Everything below is open until someone runs the manual record. In the order to try them:

1. **Whether the provider is invoked at all.** The prototype's provider was never called with the `exe` variant. Possible causes this work cannot separate: the declarations (the four empty handler elements, or the exe server), the Explorer view (thumbnails are only requested in the Medium icons and larger views, not in Details or List, and the maintainer's report does not say which view was used), a failed activation of the packaged exe, or a design limit. First check, which needs no code: `reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager" /s` after enabling the Virtual Library, and look for a `ThumbnailProvider` value in the `Lymic.VirtualLibrary!...` key. If it is absent, `Register` did not take the handler from the manifest and no provider can be invoked. `lymic-shell-ext.log` shows whether COM activated the host (`CreateInstance`) and whether Explorer asked (`Initialize`, `GetThumbnail`).
2. **Empty handler elements versus CloudMirror's dummy CLSIDs.** The template leaves the four unused handlers empty. If the registry check or the log suggests the empty elements are written as a broken value, try the CloudMirror form: a made-up GUID in `Clsid` for each of them.
3. **Why the surrogate failed `Register`.** Unknown. A guess, not an observation: `Register` resolves the handler CLSIDs in the package's COM registrations and only finds executable servers. Try the surrogate only if the exe variant is invoked but turns out to be the problem.
4. **`Arguments="-Embedding"`.** Copied from the prototype. COM normally passes `-Embedding` to a local server itself, and the host ignores its command line, so this should not matter.
5. **Does the attributes-only open avoid Hydration?** The provider opens the placeholder with `FILE_READ_ATTRIBUTES` to read the identity. Documented behaviour, not observed here; the verification record has the check (the file must stay unhydrated after browsing with thumbnails).
6. **Do the app and the COM server share `LocalState`?** Both are meant to run with the package's identity; the app writes the cache with `ApplicationData.Current.LocalFolder` and the provider reads it the same way. Unobserved. The log file in the same folder doubles as the check.
7. **Image format.** The cache holds JPEG (`format=JPEG`). If the server ignores `format` and sends WebP, WIC needs the WebP image extension (Windows 11 normally has it, Windows 10 may not; not checked here) or the provider returns an error and Explorer shows its generic icon.
8. **A thumbnail Explorer already recorded as failed.** Explorer remembers a failed request; the first visit to a folder warms the first 48 files before the placeholders appear to avoid this, but later arrivals need an Explorer refresh (F5) to show.
9. **Known gaps.** A file hydrated by the user is shown by Explorer from its own content, not by the provider. A placeholder whose path stays but whose asset changes keeps its old cached thumbnail until the cache is trimmed or cleared. A background fetch that is still running when the Virtual Library is disabled can write one more thumbnail after the cache was cleared; the next clear removes it.
