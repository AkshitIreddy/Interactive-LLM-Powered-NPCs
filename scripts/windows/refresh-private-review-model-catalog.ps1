[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SourceDirectory,
    [Parameter(Mandatory = $true)][string]$DestinationDirectory,
    [string]$ArtifactRoot = 'E:\temp\InteractiveNPCs',
    [ValidateRange(1, 14)][int]$ValidDays = 7
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$global:LASTEXITCODE = 0

. (Join-Path $PSScriptRoot 'node-tooling.ps1')

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$source = [System.IO.Path]::GetFullPath($SourceDirectory).TrimEnd('\')
$destination = [System.IO.Path]::GetFullPath($DestinationDirectory).TrimEnd('\')
$artifactRootFull = [System.IO.Path]::GetFullPath($ArtifactRoot).TrimEnd('\')
if (-not $destination.StartsWith("$artifactRootFull\", [System.StringComparison]::OrdinalIgnoreCase) -or
    $destination.Equals($artifactRootFull, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refreshed private review catalog must be below $artifactRootFull."
}
if (-not (Test-Path -LiteralPath $source -PathType Container)) {
    throw "Source private review catalog is missing: $source"
}
if (Test-Path -LiteralPath $destination) {
    throw "Refusing to replace an existing private review catalog: $destination"
}

$cargo = Get-Command cargo.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$now = [DateTimeOffset]::UtcNow
$generated = $now.ToUnixTimeSeconds()
$expires = $now.AddDays($ValidDays).ToUnixTimeSeconds()
$version = [uint64]$now.ToString('yyyyMMddHHmm')
$process = Invoke-NpcHiddenProcess -FilePath $cargo.Source -ArgumentList @(
    'run', '--quiet', '--locked', '--offline', '-p', 'model-manager',
    '--example', 'refresh_private_review_catalog', '--',
    $source, $destination, [string]$version, [string]$generated, [string]$expires
) -WorkingDirectory $repoRoot -NoReplayOutput -TimeoutSeconds 900
if ($process.TimedOut) {
    throw 'Private review model catalog refresh timed out.'
}
if ($process.ExitCode -ne 0) {
    $detail = ($process.StandardError -replace '\x1b\[[0-9;]*m', '').Trim()
    throw "Private review model catalog refresh failed: $detail"
}
$refresh = $process.StandardOutput | ConvertFrom-Json
if ($refresh.status -ne 'passed' -or [bool]$refresh.productionTrust -or
    [bool]$refresh.promotionSupported -or [bool]$refresh.publicationSupported -or
    [bool]$refresh.signerPrivateMaterialPersisted) {
    throw 'Private review model catalog refresh returned an unsafe trust state.'
}

$verification = & (Join-Path $PSScriptRoot 'verify-private-review-model-catalog.ps1') `
    -Directory $destination | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $verification.status -ne 'passed') {
    throw 'Fresh private review model catalog failed independent verification.'
}
[ordered]@{
    schema = 'interactive-npcs-private-catalog-refresh-wrapper/v1'
    status = 'passed'
    source = $source
    destination = $destination
    refresh = $refresh
    verification = $verification
} | ConvertTo-Json -Depth 10
