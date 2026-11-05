[CmdletBinding()]
param (
    [string]$Version,
    [string]$IdentityName = "xXRoxXeRXx.Lymic",
    [string]$Publisher = "CN=EB6D92C3-1B82-4D11-B562-6EA2D7C2FDEC",
    [string]$PublisherDisplayName = "xXRoxXeRXx",
    [string]$SourceExe,
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

Write-Host "Packaging Lymic MSIX version $quadVersion ($IdentityName)..."

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

$stagingDir = "$repoRoot/target/msix-staging"
if (Test-Path $stagingDir) {
    Remove-Item -Recurse -Force $stagingDir
}
New-Item -ItemType Directory -Path "$stagingDir/Assets" -Force | Out-Null

# Copy executable
Copy-Item $SourceExe "$stagingDir/Lymic.exe"

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
    -replace '\{\{DESCRIPTION\}\}', "High-performance desktop synchronization client for self-hosted Immich servers."

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
