[CmdletBinding()]
param(
    [switch]$Strict,
    [switch]$Enrich
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$python = Get-Command python -ErrorAction SilentlyContinue
if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') -Directory -ErrorAction SilentlyContinue | ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } | Sort-Object FullName -Descending | Select-Object -First 1
}
if ($null -eq $python) {
    Write-Error 'Python 3 is required for the pinned, dependency-free license validator; validation was not skipped.'
    exit 2
}

$pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
$pythonArguments = '"{0}" --root "{1}"' -f (Join-Path $PSScriptRoot 'check_lock_licenses.py'), $repoRoot
$pythonProcess = Start-Process -FilePath $pythonPath -ArgumentList $pythonArguments -NoNewWindow -Wait -PassThru
if ($pythonProcess.ExitCode -ne 0) { exit $pythonProcess.ExitCode }

if ($Enrich) {
    $manifest = Get-Content -LiteralPath (Join-Path $repoRoot 'tools/security-tools.json') -Raw | ConvertFrom-Json
    $expected = [string]$manifest.optional_enrichment.'cargo-deny'.version
    $cargoDeny = Get-Command cargo-deny -ErrorAction SilentlyContinue
    if ($null -eq $cargoDeny) {
        Write-Error "Optional enrichment was requested, but pinned cargo-deny $expected is unavailable. Nothing was auto-installed."
        exit 2
    }
    $versionOutput = (& cargo-deny --version 2>&1 | Out-String)
    if ($versionOutput -notmatch [regex]::Escape($expected)) {
        Write-Error "cargo-deny version does not match pinned version $expected."
        exit 2
    }
    Push-Location $repoRoot
    try {
        & cargo deny check bans licenses sources
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } finally { Pop-Location }
}

Write-Host 'Offline lock/license and model provenance policy passed.' -ForegroundColor Green
exit 0
