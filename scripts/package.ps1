[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [switch]$SkipChecks,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not ($env:OS -eq 'Windows_NT')) {
    Write-Error 'Packaging is supported only on Windows 10 22H2 or Windows 11 x64.'
    exit 1
}
if ([System.Environment]::Is64BitOperatingSystem -ne $true) {
    Write-Error 'A 64-bit Windows host is required.'
    exit 1
}

$releasePolicyPath = Join-Path $repoRoot 'packaging/security/release-policy.json'
if (-not (Test-Path -LiteralPath $releasePolicyPath -PathType Leaf)) {
    Write-Error "Missing release policy: $releasePolicyPath"
    exit 1
}
$releasePolicy = Get-Content -LiteralPath $releasePolicyPath -Raw | ConvertFrom-Json
foreach ($requiredGate in @('require_clean_tests', 'require_secret_scan', 'require_sbom', 'require_license_review', 'require_artifact_sha256', 'require_source_identity', 'require_all_dependency_locks', 'require_reachable_history_secret_scan')) {
    if ($releasePolicy.$requiredGate -ne $true) {
        Write-Error "Release policy must require $requiredGate."
        exit 1
    }
}
if ($releasePolicy.allow_publication -ne $false -or $releasePolicy.allow_update_feed_activation -ne $false) {
    Write-Error 'Release policy must keep publication and update-feed activation disabled.'
    exit 1
}
if ($releasePolicy.artifact_hash_algorithm -ne 'SHA-256') {
    Write-Error 'Only SHA-256 artifact evidence is accepted by this package pipeline.'
    exit 1
}
if ([string]::IsNullOrWhiteSpace([string]$releasePolicy.skip_checks_classification)) {
    Write-Error 'Release policy must classify unchecked development packages.'
    exit 1
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot 'artifacts/package'
}
if (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot $OutputDirectory
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$securityEvidenceRoot = Join-Path $OutputDirectory '.security-precheck'
New-Item -ItemType Directory -Path $securityEvidenceRoot -Force | Out-Null

$securityStatus = [ordered]@{
    skipped = [bool]$SkipChecks
    tests = $false
    include_untracked_secret_scan = $false
    reachable_history_secret_scan = $false
    strict_license_provenance = $false
    deterministic_complete_sbom = $false
}
if (-not $SkipChecks) {
    & (Join-Path $PSScriptRoot 'dev.ps1') lint
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & (Join-Path $PSScriptRoot 'dev.ps1') test
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $securityStatus.tests = $true
    & (Join-Path $PSScriptRoot 'security/scan-secrets.ps1') -IncludeUntracked
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $securityStatus.include_untracked_secret_scan = $true
    $securityStatus.reachable_history_secret_scan = $true
    & (Join-Path $PSScriptRoot 'security/check-licenses.ps1') -Strict
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $securityStatus.strict_license_provenance = $true
    & (Join-Path $PSScriptRoot 'security/generate-sbom.ps1') -Strict -OutputDirectory $securityEvidenceRoot
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $securityStatus.deterministic_complete_sbom = $true
}

$packagePath = Join-Path $repoRoot 'package.json'
if (-not (Test-Path $packagePath)) { $packagePath = Join-Path $repoRoot 'apps/control/package.json' }
if (-not (Test-Path $packagePath)) {
    Write-Error 'The 2.0 package.json was not found; refusing to package the legacy prototype.'
    exit 1
}

$tauriConfig = Join-Path $repoRoot 'packaging/windows/tauri.release.conf.json'
if (-not (Test-Path $tauriConfig)) {
    Write-Error "Missing release packaging config: $tauriConfig"
    exit 1
}

$package = Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json
$packageRoot = Split-Path -Parent $packagePath
$scripts = @()
if ($null -ne $package.scripts) { $scripts = @($package.scripts.PSObject.Properties.Name) }
if ($scripts -notcontains 'tauri') {
    Write-Error 'package.json must expose the pinned Tauri CLI as the `tauri` script.'
    exit 1
}
if ($null -eq (Get-Command corepack -ErrorAction SilentlyContinue)) {
    Write-Error 'Corepack was not found. Install the Node.js version pinned by package.json, then run scripts/dev.ps1 setup.'
    exit 1
}

$python = Get-Command python -ErrorAction SilentlyContinue
if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') -Directory -ErrorAction SilentlyContinue | ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } | Sort-Object FullName -Descending | Select-Object -First 1
}
if ($null -eq $python) {
    Write-Error 'Python 3 is required to generate source identity evidence; packaging did not continue.'
    exit 2
}
$sourceEvidencePath = Join-Path $securityEvidenceRoot 'source-evidence.json'
$pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
$sourceArguments = '"{0}" --root "{1}" --out "{2}"' -f (Join-Path $PSScriptRoot 'security/generate_source_evidence.py'), $repoRoot, $sourceEvidencePath
$sourceProcess = Start-Process -FilePath $pythonPath -ArgumentList $sourceArguments -NoNewWindow -Wait -PassThru
if ($sourceProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $sourceEvidencePath -PathType Leaf)) { exit 1 }
$sourceEvidence = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
$sourceHeadBeforeBuild = [string]$sourceEvidence.head_commit
$sourceDigestBeforeBuild = [string]$sourceEvidence.source_candidate_digest.sha256
$sourceDirtyBeforeBuild = [bool]$sourceEvidence.dirty

$configPathForCli = $tauriConfig.Replace('\', '/')
$arguments = @('run', 'tauri', 'build', '--config', $configPathForCli)
if ($Configuration -eq 'Debug') { $arguments += '--debug' }

& (Join-Path $PSScriptRoot 'prepare-sidecars.ps1') -Configuration $Configuration
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host '==> Building a local NSIS installer (no publication or updater activation)' -ForegroundColor Cyan
Push-Location $packageRoot
try {
    & corepack pnpm @arguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally { Pop-Location }

# Re-read source identity after compilation so a concurrent edit cannot be
# packaged under stale evidence. Ignored build outputs do not affect this digest.
$sourceProcess = Start-Process -FilePath $pythonPath -ArgumentList $sourceArguments -NoNewWindow -Wait -PassThru
if ($sourceProcess.ExitCode -ne 0) { exit $sourceProcess.ExitCode }
$sourceEvidence = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
if (([string]$sourceEvidence.head_commit) -ne $sourceHeadBeforeBuild -or
    ([string]$sourceEvidence.source_candidate_digest.sha256) -ne $sourceDigestBeforeBuild -or
    ([bool]$sourceEvidence.dirty) -ne $sourceDirtyBeforeBuild) {
    Write-Error 'Source changed during packaging; the artifact was not staged with stale evidence.'
    exit 1
}

$targetProfile = $(if ($Configuration -eq 'Debug') { 'debug' } else { 'release' })
$bundleCandidates = @(
    (Join-Path $repoRoot "apps/control/src-tauri/target/$targetProfile/bundle/nsis"),
    (Join-Path $repoRoot "target/$targetProfile/bundle/nsis"),
    (Join-Path $packageRoot "src-tauri/target/$targetProfile/bundle/nsis"),
    (Join-Path $packageRoot "target/$targetProfile/bundle/nsis")
)
$bundleRoot = $bundleCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Container } | Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($bundleRoot) -or -not (Test-Path $bundleRoot)) {
    Write-Error "Tauri completed but no NSIS bundle was found in: $($bundleCandidates -join ', ')."
    exit 1
}

$stageRoot = Join-Path $OutputDirectory ((Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ'))
New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null
Get-ChildItem -LiteralPath $bundleRoot -File | Copy-Item -Destination $stageRoot
Copy-Item -LiteralPath $sourceEvidencePath -Destination $stageRoot
$sbomPath = Join-Path $securityEvidenceRoot 'lockfiles.cdx.json'
if (Test-Path -LiteralPath $sbomPath -PathType Leaf) {
    Copy-Item -LiteralPath $sbomPath -Destination $stageRoot
}

$hashes = foreach ($file in Get-ChildItem -LiteralPath $stageRoot -File | Sort-Object Name) {
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName
    $signatureStatus = $null
    if ($file.Extension -eq '.exe') {
        $signatureStatus = [string](Get-AuthenticodeSignature -LiteralPath $file.FullName).Status
    }
    [pscustomobject]@{
        file = $file.Name
        sha256 = $hash.Hash.ToLowerInvariant()
        size_bytes = $file.Length
        authenticode_status = $signatureStatus
    }
}
$installerFiles = @($hashes | Where-Object { $_.file -like '*-setup.exe' })
$unsigned = $installerFiles.Count -gt 0 -and @($installerFiles | Where-Object { $_.authenticode_status -ne 'NotSigned' }).Count -eq 0
$manifest = [ordered]@{
    schema_version = 1
    created_at_utc = [DateTime]::UtcNow.ToString('o')
    configuration = $Configuration
    distribution = 'local-review-only'
    updater_enabled = $false
    unsigned = $unsigned
    evidence_class = $(if ($SkipChecks) { [string]$releasePolicy.skip_checks_classification } elseif ($sourceEvidence.dirty) { 'dirty-local-review' } else { 'local-release-candidate-evidence' })
    immutable_release_candidate = (-not [bool]$SkipChecks -and -not [bool]$sourceEvidence.dirty)
    source = $sourceEvidence
    security_gates = $securityStatus
    release_policy = [ordered]@{
        path = 'packaging/security/release-policy.json'
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $releasePolicyPath).Hash.ToLowerInvariant()
        artifact_hash_algorithm = 'SHA-256'
        publication_allowed = [bool]$releasePolicy.allow_publication
        updater_activation_allowed = [bool]$releasePolicy.allow_update_feed_activation
    }
    files = @($hashes)
}
$manifestJson = ($manifest | ConvertTo-Json -Depth 5) + [Environment]::NewLine
[System.IO.File]::WriteAllText(
    (Join-Path $stageRoot 'package-manifest.json'),
    $manifestJson,
    (New-Object System.Text.UTF8Encoding($false))
)
Write-Host "Local package staged at: $stageRoot" -ForegroundColor Green
Write-Host 'Nothing was uploaded, signed, released, or added to an update feed.' -ForegroundColor Yellow
exit 0
