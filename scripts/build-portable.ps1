[CmdletBinding()]
param(
    [switch]$SkipChecks
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
$package = Get-Content -LiteralPath (Join-Path $repoRoot 'package.json') -Encoding UTF8 | ConvertFrom-Json
$version = [string]$package.version
$stagingRoot = Get-DirectChildPath -Parent $repoRoot -Child (Join-Path $repoRoot 'release-staging')
$packageName = "AIMeeting-$version-windows-x64-no-install"
$packageDirectory = Get-DirectChildPath -Parent $stagingRoot -Child (Join-Path $stagingRoot $packageName)
$zipPath = Get-DirectChildPath -Parent $stagingRoot -Child (Join-Path $stagingRoot "$packageName.zip")
$checksumPath = Get-DirectChildPath -Parent $stagingRoot -Child "$zipPath.sha256"

Push-Location $repoRoot
try {
    if (-not $SkipChecks) {
        & npm.cmd run check
        if ($LASTEXITCODE -ne 0) { throw 'Project checks failed.' }
    } else {
        Write-Warning 'Project checks were explicitly skipped. Do not distribute this artifact as verified.'
    }

    & npm.cmd run desktop:build -- --no-bundle
    if ($LASTEXITCODE -ne 0) { throw 'Tauri no-install executable build failed.' }

    New-Item -ItemType Directory -Path $stagingRoot -Force | Out-Null
    Remove-DirectChild -Parent $stagingRoot -Child $packageDirectory
    New-Item -ItemType Directory -Path $packageDirectory | Out-Null

    $executable = Join-Path $repoRoot 'src-tauri\target\release\aimeeting.exe'
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Built executable was not found: $executable"
    }

    $packageFiles = @(
        @{ Source = $executable; Destination = 'AIMeeting.exe' },
        @{ Source = (Join-Path $repoRoot 'README.md'); Destination = 'README.md' },
        @{ Source = (Join-Path $repoRoot 'CHANGELOG.md'); Destination = 'CHANGELOG.md' },
        @{ Source = (Join-Path $repoRoot 'SECURITY.md'); Destination = 'SECURITY.md' },
        @{ Source = (Join-Path $repoRoot 'THIRD_PARTY_LICENSES.md'); Destination = 'THIRD_PARTY_LICENSES.md' },
        @{ Source = (Join-Path $repoRoot 'docs\privacy.md'); Destination = 'docs\privacy.md' },
        @{ Source = (Join-Path $repoRoot 'docs\quickstart-windows.md'); Destination = 'docs\quickstart-windows.md' },
        @{ Source = (Join-Path $repoRoot 'docs\release-windows.md'); Destination = 'docs\release-windows.md' },
        @{ Source = (Join-Path $repoRoot 'docs\release-readiness-0.2.0.md'); Destination = 'docs\release-readiness-0.2.0.md' }
    )
    foreach ($packageFile in $packageFiles) {
        $source = [string]$packageFile.Source
        $destination = Join-Path $packageDirectory ([string]$packageFile.Destination)
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Required package file is missing: $source"
        }
        $destinationParent = Split-Path -Parent $destination
        New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
        Copy-Item -LiteralPath $source -Destination $destination
    }

    Remove-DirectChild -Parent $stagingRoot -Child $zipPath
    Remove-DirectChild -Parent $stagingRoot -Child $checksumPath
    Compress-Archive -LiteralPath $packageDirectory -DestinationPath $zipPath -CompressionLevel Optimal
    $hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksumLine = "$hash  $([System.IO.Path]::GetFileName($zipPath))`n"
    [System.IO.File]::WriteAllText($checksumPath, $checksumLine, [System.Text.UTF8Encoding]::new($false))

    & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'verify-portable.ps1') -PackagePath $zipPath
    if ($LASTEXITCODE -ne 0) { throw 'No-install package verification failed.' }

    Write-Host "No-install package: $zipPath"
    Write-Host "SHA-256: $hash"
}
finally {
    Pop-Location
}
