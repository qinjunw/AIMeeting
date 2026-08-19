[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -lt 7) {
    throw 'This script requires PowerShell 7 (pwsh) for predictable UTF-8 output.'
}

function Escape-MarkdownCell([string]$Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) { return 'UNKNOWN' }
    return $Value.Replace('|', '\|').Replace("`r", ' ').Replace("`n", ' ')
}

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$packageLock = Get-Content -LiteralPath (Join-Path $repoRoot 'package-lock.json') -Encoding UTF8 | ConvertFrom-Json -AsHashtable
$npmPackages = foreach ($entry in $packageLock.packages.GetEnumerator()) {
    if (-not $entry.Key.StartsWith('node_modules/')) { continue }
    $marker = $entry.Key.LastIndexOf('node_modules/')
    $name = $entry.Key.Substring($marker + 'node_modules/'.Length)
    [pscustomobject]@{
        Name = $name
        Version = [string]$entry.Value.version
        License = if ($entry.Value.ContainsKey('license')) { [string]$entry.Value.license } else { 'UNKNOWN' }
        Scope = if ($entry.Value.ContainsKey('dev') -and $entry.Value.dev) { 'development' } else { 'runtime/transitive' }
    }
}
$npmPackages = @($npmPackages | Sort-Object Name, Version -Unique)

$cargoJson = & cargo metadata --locked --manifest-path (Join-Path $repoRoot 'src-tauri\Cargo.toml') --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
$cargoMetadata = $cargoJson | ConvertFrom-Json
$rustPackages = @($cargoMetadata.packages |
    Where-Object { $_.name -ne 'aimeeting' } |
    Select-Object @{Name = 'Name'; Expression = { $_.name } }, version, license |
    Sort-Object Name, version -Unique)

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add('# Third-Party Software Notices')
$lines.Add('')
$lines.Add('This file is generated from `package-lock.json` and `cargo metadata --locked`. It inventories declared SPDX license expressions but does not replace the complete license texts or a legal review. Regenerate it with `pwsh scripts/generate-third-party-notices.ps1` whenever dependencies change.')
$lines.Add('')
$lines.Add('## JavaScript Packages')
$lines.Add('')
$lines.Add('| Package | Version | Declared license | Scope |')
$lines.Add('| --- | --- | --- | --- |')
foreach ($package in $npmPackages) {
    $lines.Add("| $(Escape-MarkdownCell $package.Name) | $(Escape-MarkdownCell $package.Version) | $(Escape-MarkdownCell $package.License) | $(Escape-MarkdownCell $package.Scope) |")
}
$lines.Add('')
$lines.Add('## Rust Crates')
$lines.Add('')
$lines.Add('| Crate | Version | Declared license |')
$lines.Add('| --- | --- | --- |')
foreach ($package in $rustPackages) {
    $lines.Add("| $(Escape-MarkdownCell $package.Name) | $(Escape-MarkdownCell ([string]$package.version)) | $(Escape-MarkdownCell ([string]$package.license)) |")
}
$lines.Add('')
$lines.Add('Dependencies marked `UNKNOWN` require manual inspection before public distribution. Source package manifests and their bundled license files remain authoritative.')
$lines.Add('')

$output = Join-Path $repoRoot 'THIRD_PARTY_LICENSES.md'
[System.IO.File]::WriteAllText($output, ($lines -join "`n"), [System.Text.UTF8Encoding]::new($false))
Write-Host "Generated $output with $($npmPackages.Count) JavaScript packages and $($rustPackages.Count) Rust crates."
