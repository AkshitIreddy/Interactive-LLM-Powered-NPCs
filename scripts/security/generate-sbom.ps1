[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [switch]$Strict,
    [switch]$Enrich
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = Join-Path $repoRoot 'artifacts/sbom' }
if (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $repoRoot $OutputDirectory }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$python = Get-Command python -ErrorAction SilentlyContinue
if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') -Directory -ErrorAction SilentlyContinue | ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } | Sort-Object FullName -Descending | Select-Object -First 1
}
if ($null -eq $python) {
    Write-Error 'Python 3 is required for the pinned, dependency-free SBOM generator; generation was not skipped.'
    exit 2
}
$pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
$fallbackOutput = Join-Path $OutputDirectory 'lockfiles.cdx.json'
$sourceEvidence = Join-Path $OutputDirectory 'source-evidence.json'
$sourceArguments = '"{0}" --root "{1}" --out "{2}"' -f (Join-Path $PSScriptRoot 'generate_source_evidence.py'), $repoRoot, $sourceEvidence
$sourceProcess = Start-Process -FilePath $pythonPath -ArgumentList $sourceArguments -NoNewWindow -Wait -PassThru
if ($sourceProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $sourceEvidence -PathType Leaf)) {
    Write-Error 'Source identity evidence generation failed.'
    exit 1
}
$sbomArguments = '"{0}" --root "{1}" --out "{2}" --source-evidence "{3}"' -f (Join-Path $PSScriptRoot 'generate_lock_sbom.py'), $repoRoot, $fallbackOutput, $sourceEvidence
$sbomProcess = Start-Process -FilePath $pythonPath -ArgumentList $sbomArguments -NoNewWindow -Wait -PassThru
if ($sbomProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $fallbackOutput -PathType Leaf)) {
    Write-Error 'The complete lockfile CycloneDX generator failed.'
    exit 1
}
Write-Host "Generated deterministic complete lock inventory: $fallbackOutput" -ForegroundColor Green

if ($Enrich) {
    $manifest = Get-Content -LiteralPath (Join-Path $repoRoot 'tools/security-tools.json') -Raw | ConvertFrom-Json
    $expected = [string]$manifest.optional_enrichment.'cargo-cyclonedx'.version
    $generator = Get-Command cargo-cyclonedx -ErrorAction SilentlyContinue
    if ($null -eq $generator) {
        Write-Error "Optional enrichment was requested, but pinned cargo-cyclonedx $expected is unavailable. Nothing was auto-installed."
        exit 2
    }
    $versionOutput = (& cargo-cyclonedx --version 2>&1 | Out-String)
    if ($versionOutput -notmatch [regex]::Escape($expected)) {
        Write-Error "cargo-cyclonedx version does not match pinned version $expected."
        exit 2
    }
    Write-Host 'Pinned external generator is available for explicit enrichment; the deterministic lock BOM remains authoritative.' -ForegroundColor Cyan
}
exit 0
