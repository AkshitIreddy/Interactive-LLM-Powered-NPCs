[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ProofPath,
    [Parameter(Mandatory = $true)][string]$CaptureEvidencePath,
    [Parameter(Mandatory = $true)][string]$PackageManifestPath,
    [Parameter(Mandatory = $true)][string]$PackagePath,
    [Parameter(Mandatory = $true)][string]$InstalledManifestPath,
    [Parameter(Mandatory = $true)][string]$InstalledExecutablePath,
    [Parameter(Mandatory = $true)][string]$ReleaseCandidateId
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

# This is intentionally a post-package validator. It does not create, install,
# launch, or promote a candidate and cannot turn fixture provenance into a pass.
foreach ($path in @($ProofPath, $CaptureEvidencePath, $PackageManifestPath, $PackagePath, $InstalledManifestPath, $InstalledExecutablePath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Installed privacy validation requires an existing post-package artifact: $path"
    }
}

$python = Get-Command python -ErrorAction SilentlyContinue
if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') `
        -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } |
        Sort-Object FullName -Descending | Select-Object -First 1
}
if ($null -eq $python) { throw 'Python 3 is required for installed privacy proof validation.' }
$pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
$validator = Join-Path $PSScriptRoot 'security/validate_installed_privacy_proof.py'
$schema = Join-Path (Split-Path -Parent $PSScriptRoot) 'crates/diagnostics/schemas/privacy-proof-v1.schema.json'
$arguments = @(
    $validator,
    '--proof', (Resolve-Path -LiteralPath $ProofPath).Path,
    '--capture-evidence', (Resolve-Path -LiteralPath $CaptureEvidencePath).Path,
    '--schema', (Resolve-Path -LiteralPath $schema).Path,
    '--package-manifest', (Resolve-Path -LiteralPath $PackageManifestPath).Path,
    '--package', (Resolve-Path -LiteralPath $PackagePath).Path,
    '--installed-manifest', (Resolve-Path -LiteralPath $InstalledManifestPath).Path,
    '--installed-executable', (Resolve-Path -LiteralPath $InstalledExecutablePath).Path,
    '--release-candidate-id', $ReleaseCandidateId
)
$process = Invoke-NpcHiddenProcess -FilePath $pythonPath -ArgumentList $arguments
exit $process.ExitCode
