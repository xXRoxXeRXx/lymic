[CmdletBinding()]
param (
    [string]$Version,
    [string]$IdentityName = "xXRoxXeRXx.Lymic",
    [string]$Publisher = "CN=EB6D92C3-1B82-4D11-B562-6EA2D7C2FDEC",
    [string]$PublisherDisplayName = "xXRoxXeRXx",
    [string]$SourceExe,
    # The Virtual Library thumbnail provider (src-tauri/lymic-shell-ext, #282). The COM server is an
    # executable by default; "surrogate" packages the DLL instead (see docs/store/local-msix-build.md).
    [ValidateSet("exe", "surrogate")][string]$ComServer = "exe",
    [string]$ShellExtExe,
    [string]$ShellExtDll,
    [string]$OutputDir = "dist/msix"
)

$ErrorActionPreference = "Stop"

$repoRoot = (Get-Item $PSScriptRoot).Parent.FullName

if (-not $Version) {
    $tauriConf = Get-Content "$repoRoot/src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
    $Version = $tauriConf.version
}

# 4-part version for MSIX
$parts = $Version -replace '^v','' -split '\.'
while ($parts.Count -lt 4) { $parts += "0" }
$quadVersion = ($parts[0..3] -join '.')

Write-Host "Packaging Lymic MSIX version $quadVersion ($IdentityName, thumbnail provider: $ComServer)..."

# Locate makeappx.exe
$cmd = Get-Command makeappx.exe -ErrorAction SilentlyContinue
$makeappx = if ($cmd) { $cmd.Source } else { $null }
if (-not $makeappx) {
    $kits = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter "makeappx.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like "*\x64\makeappx.exe" } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if ($kits) {
        $makeappx = $kits.FullName
    }
}

if (-not $makeappx) {
    throw "makeappx.exe not found. Please install the Windows SDK."
}

Write-Host "Using makeappx: $makeappx"

if ($SourceExe) {
    if (-not (Test-Path $SourceExe)) {
        throw "Specified SourceExe not found at: $SourceExe"
    }
    $SourceExe = (Get-Item $SourceExe).FullName
} else {
    $candidates = @(
        "$repoRoot/src-tauri/target/release/lymic-desktop.exe",
        "$repoRoot/src-tauri/target/release/Lymic.exe",
        "$repoRoot/src-tauri/target/release/lymic.exe"
    ) | Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending

    if ($candidates.Count -gt 0) {
        $SourceExe = $candidates[0]
    } else {
        throw "Source executable not found in src-tauri/target/release. Please run 'cargo build --release' first."
    }
}

# The thumbnail provider build output. Build it with:
#   cargo build --manifest-path src-tauri/Cargo.toml --release --locked -p lymic-shell-ext
# (add --features surrogate-dll for -ComServer surrogate)
$shellExtClsid = "9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41"
if ($ComServer -eq "exe") {
    if (-not $ShellExtExe) { $ShellExtExe = "$repoRoot/src-tauri/target/release/lymic-shell-ext-host.exe" }
    if (-not (Test-Path $ShellExtExe)) {
        throw "Thumbnail provider host not found at: $ShellExtExe. Run 'cargo build --manifest-path src-tauri/Cargo.toml --release --locked -p lymic-shell-ext' first."
    }
    $ShellExtExe = (Get-Item $ShellExtExe).FullName
} else {
    if (-not $ShellExtDll) { $ShellExtDll = "$repoRoot/src-tauri/target/release/lymic_shell_ext.dll" }
    if (-not (Test-Path $ShellExtDll)) {
        throw "Thumbnail provider DLL not found at: $ShellExtDll. Run 'cargo build --manifest-path src-tauri/Cargo.toml --release --locked -p lymic-shell-ext --features surrogate-dll' first."
    }
    $ShellExtDll = (Get-Item $ShellExtDll).FullName
}

$stagingDir = "$repoRoot/target/msix-staging"
if (Test-Path $stagingDir) {
    Remove-Item -Recurse -Force $stagingDir
}
New-Item -ItemType Directory -Path "$stagingDir/Assets" -Force | Out-Null

# Copy executable
Copy-Item $SourceExe "$stagingDir/Lymic.exe"

# Copy the thumbnail provider (declared in the manifest below)
if ($ComServer -eq "exe") {
    Copy-Item $ShellExtExe "$stagingDir/LymicShellExt.exe"
    $comServerBlock = @"
        <com:Extension Category="windows.comServer">
          <com:ComServer>
            <com:ExeServer Executable="LymicShellExt.exe" Arguments="-Embedding" DisplayName="Lymic Thumbnail Provider">
              <com:Class Id="$shellExtClsid" DisplayName="Lymic Thumbnail Provider" />
            </com:ExeServer>
          </com:ComServer>
        </com:Extension>
"@
} else {
    Copy-Item $ShellExtDll "$stagingDir/LymicShellExt.dll"
    $comServerBlock = @"
        <com:Extension Category="windows.comServer">
          <com:ComServer>
            <com:SurrogateServer DisplayName="Lymic Thumbnail Provider">
              <com:Class Id="$shellExtClsid" Path="LymicShellExt.dll" ThreadingModel="Both" DisplayName="Lymic Thumbnail Provider" />
            </com:SurrogateServer>
          </com:ComServer>
        </com:Extension>
"@
}

# Copy icons
$iconsDir = "$repoRoot/src-tauri/icons"
Copy-Item "$iconsDir/StoreLogo.png" "$stagingDir/Assets/StoreLogo.png"
Copy-Item "$iconsDir/Square150x150Logo.png" "$stagingDir/Assets/Square150x150Logo.png"
Copy-Item "$iconsDir/Square44x44Logo.png" "$stagingDir/Assets/Square44x44Logo.png"
Copy-Item "$iconsDir/Square71x71Logo.png" "$stagingDir/Assets/Square71x71Logo.png"

# Generate AppxManifest.xml
$template = Get-Content "$repoRoot/src-tauri/msix/AppxManifest.template.xml" -Raw
$manifest = $template `
    -replace '\{\{IDENTITY_NAME\}\}', $IdentityName `
    -replace '\{\{PUBLISHER\}\}', $Publisher `
    -replace '\{\{PUBLISHER_DISPLAY_NAME\}\}', $PublisherDisplayName `
    -replace '\{\{VERSION\}\}', $quadVersion `
    -replace '\{\{ARCHITECTURE\}\}', "x64" `
    -replace '\{\{DISPLAY_NAME\}\}', "Lymic" `
    -replace '\{\{EXECUTABLE\}\}', "Lymic.exe" `
    -replace '\{\{DESCRIPTION\}\}', "High-performance desktop synchronization client for self-hosted Immich servers." `
    -replace '\{\{THUMBNAIL_CLSID\}\}', $shellExtClsid `
    -replace '\{\{THUMBNAIL_COM_SERVER\}\}', $comServerBlock.TrimEnd()

Set-Content -Path "$stagingDir/AppxManifest.xml" -Value $manifest -Encoding utf8

# Ensure output directory exists
$targetDir = [System.IO.Path]::GetFullPath("$repoRoot/$OutputDir")
if (-not (Test-Path $targetDir)) {
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
}

$msixPath = "$targetDir/Lymic_${Version}_x64.msix"
if (Test-Path $msixPath) {
    Remove-Item -Force $msixPath
}

Write-Host "Packing MSIX to $msixPath..."
& $makeappx pack /d $stagingDir /p $msixPath /o
if ($LASTEXITCODE -ne 0) {
    throw "makeappx pack failed with exit code $LASTEXITCODE"
}

Write-Host "Successfully produced Store-ready MSIX package: $msixPath" -ForegroundColor Green
