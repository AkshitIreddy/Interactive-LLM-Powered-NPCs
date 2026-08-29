[CmdletBinding()]
param(
    [switch]$IncludeUntracked,
    [switch]$NoHistory,
    [string]$RepositoryRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
} else {
    $RepositoryRoot = (Resolve-Path $RepositoryRoot).Path
}
$python = Get-Command python -ErrorAction SilentlyContinue
if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') -Directory -ErrorAction SilentlyContinue | ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } | Sort-Object FullName -Descending | Select-Object -First 1
}
if ($null -eq $python) {
    Write-Error 'Python 3 is required for the pinned, dependency-free repository scanner; scanning was not skipped.'
    exit 2
}
$arguments = '"{0}" --root "{1}"' -f (Join-Path $PSScriptRoot 'scan_secrets.py'), $RepositoryRoot
if ($IncludeUntracked) { $arguments += ' --include-untracked' }
if ($NoHistory) { $arguments += ' --no-history' }
$pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
$pythonProcess = Start-Process -FilePath $pythonPath -ArgumentList $arguments -NoNewWindow -Wait -PassThru
exit $pythonProcess.ExitCode
