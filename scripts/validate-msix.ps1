[CmdletBinding()]
param (
    [Parameter(Mandatory = $true)]
    [string]$MsixPath,

    [string]$ExpectedVersion,
    [string]$ExpectedIdentity = "xXRoxXeRXx.Lymic",
    [string]$ExpectedPublisher = "CN=EB6D92C3-1B82-4D11-B562-6EA2D7C2FDEC",
    [string]$ExpectedArchitecture = "x64"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $MsixPath)) {
    throw "MSIX package not found at: $MsixPath"
}

$fullPath = [System.IO.Path]::GetFullPath($MsixPath)
Write-Host "Validating MSIX package: $fullPath"

Add-Type -AssemblyName System.IO.Compression.FileSystem

$zip = $null
$manifestXml = ""
try {
    $zip = [System.IO.Compression.ZipFile]::OpenRead($fullPath)
    $entry = $zip.GetEntry("AppxManifest.xml")
    if (-not $entry) {
        throw "AppxManifest.xml not found inside MSIX package: $fullPath"
    }

    $stream = $entry.Open()
    $reader = New-Object System.IO.StreamReader($stream, [System.Text.Encoding]::UTF8)
    $manifestXml = $reader.ReadToEnd()
    $reader.Close()
    $stream.Close()
} finally {
    if ($zip) {
        $zip.Dispose()
    }
}

$errors = @()

# Validate Identity
if ($manifestXml -notmatch '(?s)<Identity\s+([^>]+?)\s*\/?>') {
    $errors += "Missing <Identity> element in AppxManifest.xml"
} else {
    $identityAttrs = $Matches[1]
    
    if ($identityAttrs -match 'Name="([^"]+)"') {
        $name = $Matches[1]
        if ($name -ne $ExpectedIdentity) {
            $errors += "Identity Name mismatch: expected '$ExpectedIdentity', got '$name'"
        }
    } else {
        $errors += "Missing Name attribute on Identity element"
    }

    if ($identityAttrs -match 'Publisher="([^"]+)"') {
        $pub = $Matches[1]
        if ($ExpectedPublisher -and $pub -ne $ExpectedPublisher) {
            $errors += "Publisher mismatch: expected '$ExpectedPublisher', got '$pub'"
        }
    } else {
        $errors += "Missing Publisher attribute on Identity element"
    }

    if ($identityAttrs -match 'ProcessorArchitecture="([^"]+)"') {
        $arch = $Matches[1]
        if ($arch -ne $ExpectedArchitecture) {
            $errors += "ProcessorArchitecture mismatch: expected '$ExpectedArchitecture', got '$arch'"
        }
    } else {
        $errors += "Missing ProcessorArchitecture attribute on Identity element"
    }

    if ($ExpectedVersion) {
        $parts = $ExpectedVersion -replace '^v','' -split '\.'
        while ($parts.Count -lt 4) { $parts += "0" }
        $expectedQuad = ($parts[0..3] -join '.')

        if ($identityAttrs -match 'Version="([^"]+)"') {
            $ver = $Matches[1]
            if ($ver -ne $expectedQuad) {
                $errors += "Version mismatch: expected '$expectedQuad', got '$ver'"
            }
        } else {
            $errors += "Missing Version attribute on Identity element"
        }
    }
}

# Validate restricted capabilities
if ($manifestXml -notmatch 'Name="runFullTrust"') {
    $errors += "Missing required restricted capability: 'runFullTrust'"
}

if ($manifestXml -notmatch 'Name="broadFileSystemAccess"') {
    $errors += "Missing required restricted capability: 'broadFileSystemAccess'"
}

if ($errors.Count -gt 0) {
    Write-Error "MSIX validation failed:`n$($errors -join "`n")"
    exit 1
}

Write-Host "MSIX validation passed successfully for $fullPath" -ForegroundColor Green
