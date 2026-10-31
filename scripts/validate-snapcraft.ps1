[CmdletBinding()]
param (
    [string]$SnapcraftPath = "snap/snapcraft.yaml",
    [string]$ExpectedVersion,
    [string]$ExpectedName = "lymic",
    [string]$ExpectedBase = "core22",
    [string]$ExpectedConfinement = "strict"
)

$ErrorActionPreference = "Stop"

$repoRoot = (Get-Item $PSScriptRoot).Parent.FullName
$fullSnapcraftPath = Join-Path $repoRoot $SnapcraftPath

if (-not (Test-Path $fullSnapcraftPath)) {
    throw "Snapcraft configuration not found at: $fullSnapcraftPath"
}

Write-Host "Validating Snapcraft configuration: $fullSnapcraftPath"

if (-not $ExpectedVersion) {
    $tauriConf = Get-Content "$repoRoot/src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
    $ExpectedVersion = $tauriConf.version
}

$yamlContent = Get-Content $fullSnapcraftPath -Raw
$errors = @()

# Validate name
if ($yamlContent -match '(?m)^name:\s*[''"]?([a-zA-Z0-9_-]+)[''"]?') {
    $actualName = $Matches[1]
    if ($actualName -ne $ExpectedName) {
        $errors += "Snap name mismatch: expected '$ExpectedName', got '$actualName'"
    }
} else {
    $errors += "Missing 'name:' declaration in snapcraft.yaml"
}

# Validate base
if ($yamlContent -match '(?m)^base:\s*[''"]?([a-zA-Z0-9_-]+)[''"]?') {
    $actualBase = $Matches[1]
    if ($actualBase -ne $ExpectedBase) {
        $errors += "Base mismatch: expected '$ExpectedBase', got '$actualBase'"
    }
} else {
    $errors += "Missing 'base:' declaration in snapcraft.yaml"
}

# Validate confinement
if ($yamlContent -match '(?m)^confinement:\s*[''"]?([a-zA-Z0-9_-]+)[''"]?') {
    $actualConfinement = $Matches[1]
    if ($actualConfinement -ne $ExpectedConfinement) {
        $errors += "Confinement mismatch: expected '$ExpectedConfinement', got '$actualConfinement'"
    }
} else {
    $errors += "Missing 'confinement:' declaration in snapcraft.yaml"
}

# Validate version
if ($yamlContent -match '(?m)^version:\s*[''"]?([^''"\r\n]+)[''"]?') {
    $actualVersion = $Matches[1]
    if ($actualVersion -ne $ExpectedVersion) {
        $errors += "Version mismatch: expected '$ExpectedVersion', got '$actualVersion'"
    }
} else {
    $errors += "Missing 'version:' declaration in snapcraft.yaml"
}

# Validate GNOME extension
if ($yamlContent -notmatch 'extensions:\s*\[[^\]]*gnome[^\]]*\]' -and $yamlContent -notmatch '(?m)^\s*-\s*gnome\b') {
    $errors += "Missing required extension: 'gnome' under apps"
}

# Validate required plugs
$requiredPlugs = @("network", "home", "removable-media", "password-manager-service")
foreach ($plug in $requiredPlugs) {
    if ($yamlContent -notmatch "(?m)^\s*-\s*$plug\b") {
        $errors += "Missing required plug: '$plug'"
    }
}

# Validate WebKitGTK layout
if ($yamlContent -notmatch '(?m)^layout:' -or $yamlContent -notmatch 'webkit2gtk-4\.1') {
    $errors += "Missing required layout mapping for webkit2gtk-4.1"
}

# Validate store metadata (title, icon, license, website)
if ($yamlContent -notmatch '(?m)^title:\s*[''"]?Lymic[''"]?') {
    $errors += "Missing or invalid store title declaration: expected 'title: Lymic'"
}
if ($yamlContent -notmatch '(?m)^icon:\s*[''"]?[^''"\r\n]+[''"]?') {
    $errors += "Missing store icon declaration: expected 'icon: snap/gui/icon.png'"
}
if ($yamlContent -notmatch '(?m)^license:\s*[''"]?MIT[''"]?') {
    $errors += "Missing or invalid license declaration: expected 'license: MIT'"
}
if ($yamlContent -notmatch '(?m)^website:\s*https?://') {
    $errors += "Missing store website URL"
}

# Validate desktop file exists
$desktopPath = Join-Path $repoRoot "snap/gui/lymic.desktop"
if (-not (Test-Path $desktopPath)) {
    $errors += "Missing snap desktop file: $desktopPath"
}

# Validate icon exists
$iconPath = Join-Path $repoRoot "snap/gui/icon.png"
if (-not (Test-Path $iconPath)) {
    $errors += "Missing snap icon file: $iconPath"
}

if ($errors.Count -gt 0) {
    Write-Error "Snapcraft validation failed:`n$($errors -join "`n")"
    exit 1
}

Write-Host "Snapcraft validation passed successfully for $fullSnapcraftPath" -ForegroundColor Green

