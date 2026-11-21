# Local MSIX build with Package Identity

Developer guide for building Lymic as an MSIX and registering it on your own machine, so the app runs **with Package Identity**.

Why this exists: the planned read-only Windows Virtual Library ([ADR-0003](../adr/0003-windows-virtual-cloud-library.md)) is gated on actual Package Identity at runtime, in addition to `LYMIC_RELEASE_CHANNEL=store` ([ADR-0004](../adr/0004-microsoft-store-release-channel.md)). It is therefore unavailable in `npm run tauri dev` and in any unpackaged run of the executable, including the raw `lymic-desktop.exe` from `target/release`. A locally registered MSIX is how developers exercise it. This guide reuses the release tooling, so the local package has the same manifest and capabilities as the Store package:

- [`scripts/package-msix.ps1`](../../scripts/package-msix.ps1) stages the executable and icons, fills [`src-tauri/msix/AppxManifest.template.xml`](../../src-tauri/msix/AppxManifest.template.xml) and runs `makeappx.exe`.
- [`scripts/validate-msix.ps1`](../../scripts/validate-msix.ps1) checks the manifest of the produced package.

> **Verification status.** The packaging, validation and self-signed signing commands below were run on a Windows Server 2022 machine, using a stand-in executable instead of a real Lymic build. The registration, launch and removal steps (`Add-AppxPackage`, `Remove-AppxPackage`, certificate trust) were **not** run there, because Developer Mode was off on that machine and was not changed. Treat those steps as written from the documented Windows behavior and report any deviation. Nothing here has been verified against a real Lymic build, and the runtime Package Identity check does not exist yet (see [Pending](#pending-runtime-package-identity-check)).

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

## Pending: runtime Package Identity check

One acceptance criterion of ticket #274 is that the runtime Package Identity check from the Virtual Library settings gating ticket (#272) reports an identity in this local build and none in `tauri dev`. That check is not part of this change and does not exist in the codebase yet, so this criterion is **pending #272** and has not been verified. Once #272 lands, run it in a build registered through this guide (expect an identity) and in `npm run tauri dev` (expect none), and record the result here.
