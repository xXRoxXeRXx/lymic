[CmdletBinding()]
param (
    [string]$Version,
    [string]$IdentityName = "xXRoxXeRXx.Lymic",
    [string]$Publisher = "CN=xXRoxXeRXx",
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
$makeappx = (Get-Command makeappx.exe -ErrorAction SilentlyContinue)?.Source
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

if (-not $SourceExe) {
    $candidate1 = "$repoRoot/src-tauri/target/release/Lymic.exe"
    $candidate2 = "$repoRoot/src-tauri/target/release/lymic.exe"
    if (Test-Path $candidate1) {
        $SourceExe = $candidate1
    } elseif (Test-Path $candidate2) {
        $SourceExe = $candidate2
    } else {
        throw "Source executable not found at $candidate1 or $candidate2. Please run 'cargo build --release' first."
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
