[CmdletBinding()]
param(
    [string]$PackagePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -lt 7) {
    throw 'This script requires PowerShell 7 (pwsh) for predictable UTF-8 output.'
}

function Get-DirectChildPath {
    param(
        [Parameter(Mandatory)] [string]$Parent,
        [Parameter(Mandatory)] [string]$Child
    )

    $parentPath = [System.IO.Path]::GetFullPath($Parent).TrimEnd('\', '/')
    $childPath = [System.IO.Path]::GetFullPath($Child).TrimEnd('\', '/')
    $childParent = [System.IO.Path]::GetDirectoryName($childPath).TrimEnd('\', '/')
    if (-not $childParent.Equals($parentPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify a path that is not a direct child of $parentPath`: $childPath"
    }
    return $childPath
}

function Remove-DirectChild {
    param(
        [Parameter(Mandatory)] [string]$Parent,
        [Parameter(Mandatory)] [string]$Child
    )

    $safePath = Get-DirectChildPath -Parent $Parent -Child $Child
    if (-not (Test-Path -LiteralPath $safePath)) { return }
    $item = Get-Item -LiteralPath $safePath -Force
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Refusing to remove a reparse point: $safePath"
    }
    Remove-Item -LiteralPath $safePath -Recurse -Force
}

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$stagingRoot = Get-DirectChildPath -Parent $repoRoot -Child (Join-Path $repoRoot 'release-staging')
if (-not $PackagePath) {
    $PackagePath = Get-ChildItem -LiteralPath $stagingRoot -Filter 'AIMeeting-*-windows-x64-no-install.zip' -File |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $PackagePath -or -not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) {
    throw 'No-install ZIP was not found. Run scripts\build-portable.ps1 first.'
}
$PackagePath = (Resolve-Path -LiteralPath $PackagePath).Path

$checksumPath = "$PackagePath.sha256"
if (-not (Test-Path -LiteralPath $checksumPath -PathType Leaf)) {
    throw "Checksum sidecar is missing: $checksumPath"
}
$checksumLine = (Get-Content -LiteralPath $checksumPath -Encoding UTF8 | Select-Object -First 1).Trim()
if ($checksumLine -notmatch '^([0-9a-fA-F]{64})\s+(.+)$') {
    throw "Checksum sidecar has an invalid format: $checksumPath"
}
$expectedHash = $Matches[1].ToLowerInvariant()
$expectedName = $Matches[2].Trim()
if ($expectedName -ne [System.IO.Path]::GetFileName($PackagePath)) {
    throw "Checksum filename does not match the ZIP: $expectedName"
}
$actualHash = (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualHash -ne $expectedHash) {
    throw "SHA-256 mismatch. Expected $expectedHash, got $actualHash"
}

$verificationRoot = Get-DirectChildPath -Parent $stagingRoot -Child (Join-Path $stagingRoot 'package-verification')
New-Item -ItemType Directory -Path $stagingRoot -Force | Out-Null
Remove-DirectChild -Parent $stagingRoot -Child $verificationRoot
New-Item -ItemType Directory -Path $verificationRoot | Out-Null
Expand-Archive -LiteralPath $PackagePath -DestinationPath $verificationRoot

$archiveRoots = @(Get-ChildItem -LiteralPath $verificationRoot -Directory)
if ($archiveRoots.Count -ne 1) {
    throw "Expected exactly one package root directory, found $($archiveRoots.Count)."
}
$packageRoot = $archiveRoots[0].FullName
$files = @(Get-ChildItem -LiteralPath $packageRoot -Recurse -File)
$requiredFiles = @(
    'AIMeeting.exe',
    'README.md',
    'CHANGELOG.md',
    'SECURITY.md',
    'THIRD_PARTY_LICENSES.md',
    'docs\privacy.md',
    'docs\quickstart-windows.md',
    'docs\release-windows.md',
    'docs\release-readiness-0.2.0.md'
)
foreach ($required in $requiredFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $packageRoot $required) -PathType Leaf)) {
        throw "Required file is missing from the package: $required"
    }
}

$forbiddenNames = @('Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json', '.npmrc')
$forbiddenExtensions = @('.pdb', '.log', '.db', '.sqlite', '.sqlite3', '.opus', '.wav', '.mp3', '.flac', '.pem', '.key')
$forbiddenDirectories = @('node_modules', 'target', '.git', 'meetings', 'trash', 'logs', 'EBWebView')
foreach ($file in $files) {
    if ($file.Name -like '.env*' -or $file.Name -in $forbiddenNames -or $file.Extension.ToLowerInvariant() -in $forbiddenExtensions) {
        throw "Forbidden file found in no-install package: $($file.FullName)"
    }
    $relative = [System.IO.Path]::GetRelativePath($packageRoot, $file.FullName)
    $segments = $relative.Split([System.IO.Path]::DirectorySeparatorChar)
    if ($segments | Where-Object { $_ -in $forbiddenDirectories }) {
        throw "Forbidden directory found in no-install package: $($file.FullName)"
    }
}

Write-Host "No-install package structurally verified: $PackagePath"
Write-Host "Files: $($files.Count)"
Write-Host "SHA-256: $actualHash"
Write-Host 'Executable launch intentionally skipped. Use Windows Sandbox or a clean test account for runtime verification.'
