[CmdletBinding()]
param([string]$Root)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot '../windows/node-tooling.ps1')

if ([string]::IsNullOrWhiteSpace($Root)) {
    $Root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
} else {
    $Root = (Resolve-Path -LiteralPath $Root).Path
}
$python = Get-Command python -ErrorAction SilentlyContinue
if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') `
        -Directory -ErrorAction SilentlyContinue |
        ForEach-Object {
            Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue
        } | Sort-Object FullName -Descending | Select-Object -First 1
}
if ($null -eq $python) { throw 'Python 3 is required for source hygiene validation.' }
$pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
$arguments = @(
    (Join-Path $PSScriptRoot 'check_source_hygiene.py'),
    '--root', $Root
)
$process = Invoke-NpcHiddenProcess -FilePath $pythonPath -ArgumentList $arguments -NoReplayOutput
if ($process.ExitCode -ne 0) {
    throw "Source hygiene validation failed: $($process.StandardError)$($process.StandardOutput)"
}
$process.StandardOutput.TrimEnd() | Write-Output
