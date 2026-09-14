[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Directory,
    [string]$ReceiptPath = '',
    [string]$ImportContextPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$global:LASTEXITCODE = 0

. (Join-Path $PSScriptRoot 'node-tooling.ps1')

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$runtimeRoot = [System.IO.Path]::GetFullPath($Directory).TrimEnd('\')
if (-not (Test-Path -LiteralPath $runtimeRoot -PathType Container)) {
    throw "Private review model catalog is missing: $runtimeRoot"
}
$receipt = if ([string]::IsNullOrWhiteSpace($ReceiptPath)) {
    Join-Path $runtimeRoot 'evidence/catalog-verification-receipt.json'
} else {
    [System.IO.Path]::GetFullPath($ReceiptPath)
}
$importContext = if ([string]::IsNullOrWhiteSpace($ImportContextPath)) {
    Join-Path $runtimeRoot 'evidence/yunet-import-context.json'
} else {
    [System.IO.Path]::GetFullPath($ImportContextPath)
}
foreach ($required in @($receipt, $importContext)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Private review model catalog evidence is missing: $required"
    }
}

$cargo = Get-Command cargo.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$process = Invoke-NpcHiddenProcess -FilePath $cargo.Source -ArgumentList @(
    'run', '--quiet', '--locked', '--offline', '-p', 'model-manager',
    '--example', 'verify_private_review_catalog', '--',
    $runtimeRoot, $receipt, $importContext, [string]$now
) -WorkingDirectory $repoRoot -NoReplayOutput -TimeoutSeconds 900
if ($process.TimedOut) {
    throw 'Private review model catalog verification timed out.'
}
if ($process.ExitCode -ne 0) {
    $detail = ($process.StandardError -replace '\x1b\[[0-9;]*m', '').Trim()
    throw "Private review model catalog verification failed: $detail"
}
$result = $process.StandardOutput | ConvertFrom-Json
if ($result.status -ne 'passed' -or [bool]$result.productionTrust -or
    -not [bool]$result.rotationRequiredBeforeRelease -or
    [bool]$result.promotionSupported -or [bool]$result.publicationSupported -or
    [bool]$result.signerPrivateMaterialPersisted) {
    throw 'Private review model catalog verifier returned an unsafe trust state.'
}
$result | ConvertTo-Json -Depth 8
