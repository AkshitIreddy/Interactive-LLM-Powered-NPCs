[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [switch]$SkipChecks,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$global:LASTEXITCODE = 0

. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')
. (Join-Path $PSScriptRoot 'windows/package-output-contract.ps1')
. (Join-Path $PSScriptRoot 'windows/product-resource-paths.ps1')
. (Join-Path $PSScriptRoot 'windows/webview2-offline-installer.ps1')

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not ($env:OS -eq 'Windows_NT')) {
    Write-Error 'Packaging is supported only on Windows 10 22H2 or Windows 11 x64.'
    exit 1
}
if ([System.Environment]::Is64BitOperatingSystem -ne $true) {
    Write-Error 'A 64-bit Windows host is required.'
    exit 1
}

# Fail before dependency graph work or native builds when the checkout contains
# Windows-unaddressable names, accidental shell-redirection files, or native
# in-source build output that cannot belong to a clean review source candidate.
& (Join-Path $PSScriptRoot 'security/check-source-hygiene.ps1') -Root $repoRoot | Out-Null
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$acceptanceEvidence = (& (Join-Path $PSScriptRoot 'assert-acceptance-evidence-consistency.ps1')) | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($acceptanceEvidence.status -ne 'consistent' -or
    $acceptanceEvidence.requirement_rows -ne 40 -or
    $acceptanceEvidence.success_criteria_rows -ne 12) {
    throw 'Acceptance evidence consistency preflight emitted an invalid result.'
}

$releasePolicyPath = Join-Path $repoRoot 'packaging/security/release-policy.json'
if (-not (Test-Path -LiteralPath $releasePolicyPath -PathType Leaf)) {
    Write-Error "Missing release policy: $releasePolicyPath"
    exit 1
}
$releasePolicy = Get-Content -LiteralPath $releasePolicyPath -Raw | ConvertFrom-Json
foreach ($requiredGate in @(
        'require_clean_tests', 'require_secret_scan', 'require_sbom',
        'require_license_review', 'require_artifact_sha256',
        'require_source_identity', 'require_all_dependency_locks',
        'require_distribution_component_ledger',
        'require_extracted_installer_reconciliation',
        'require_review_test_game_reconciliation',
        'require_installed_legal_resources',
        'require_release_crt_import_audit', 'forbid_debug_crt_imports',
        'forbid_unclassified_distribution_files',
        'forbid_ffmpeg_or_ffprobe_in_base_and_review_fixture',
        'require_reachable_history_secret_scan'
    )) {
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
$packageOperationId = [Guid]::NewGuid().ToString('N')
$securityEvidenceRoot = Join-Path $OutputDirectory ".security-precheck/$packageOperationId"
New-Item -ItemType Directory -Path $securityEvidenceRoot -Force | Out-Null

$securityStatus = [ordered]@{
    skipped = [bool]$SkipChecks
    tests = $false
    include_untracked_secret_scan = $false
    reachable_history_secret_scan = $false
    strict_license_provenance = $false
    deterministic_complete_sbom = $false
    artifact_scope_resolved = $false
    exact_license_materials = $false
    extracted_installer_reconciled = $false
    installed_legal_resources_reconciled = $false
    unclassified_installed_files_rejected = $false
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
}
# Installed legal resources are required even for an explicitly classified
# unchecked development package. Resolve the actual runtime graphs, collect
# every included package's exact license body, then bind both into the SBOM.
$artifactLegalJson = & (Join-Path $PSScriptRoot 'prepare-artifact-legal-resources.ps1') `
    -OutputDirectory $securityEvidenceRoot
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$artifactLegal = $artifactLegalJson | ConvertFrom-Json
if ($artifactLegal.status -ne 'prepared') {
    throw 'Artifact-scoped legal resources were not prepared.'
}
$securityStatus.deterministic_complete_sbom = $true
$securityStatus.artifact_scope_resolved = $true
$securityStatus.exact_license_materials = $true
$sbomPath = [string]$artifactLegal.sbom_path
$artifactScopePath = [string]$artifactLegal.artifact_scope_path
$licenseMaterialRoot = [string]$artifactLegal.license_root
$licenseMaterialIndex = [string]$artifactLegal.license_index_path
foreach ($requiredLegalOutput in @(
        $sbomPath, $artifactScopePath, $licenseMaterialIndex
    )) {
    if (-not (Test-Path -LiteralPath $requiredLegalOutput -PathType Leaf)) {
        throw "Artifact legal pipeline completed without required output: $requiredLegalOutput"
    }
}

$packagePath = Join-Path $repoRoot 'package.json'
if (-not (Test-Path $packagePath)) { $packagePath = Join-Path $repoRoot 'apps/control/package.json' }
if (-not (Test-Path $packagePath)) {
    Write-Error 'The 2.0 package.json was not found; refusing to package the legacy prototype.'
    exit 1
}

$tauriConfig = if ($Configuration -eq 'Debug') {
    Join-Path $repoRoot 'packaging/windows/tauri.review.conf.json'
} else {
    Join-Path $repoRoot 'packaging/windows/tauri.release.conf.json'
}
if (-not (Test-Path $tauriConfig)) {
    Write-Error "Missing packaging config: $tauriConfig"
    exit 1
}
$tauriOverlay = Get-Content -LiteralPath $tauriConfig -Raw | ConvertFrom-Json
if ($null -eq $tauriOverlay.build -or
    [string]$tauriOverlay.build.beforeBuildCommand -ne '') {
    throw 'Packaging overlays must disable Tauri beforeBuildCommand; package.ps1 owns the exact hidden Corepack frontend build.'
}
$overlaySidecars = @($tauriOverlay.bundle.externalBin | Sort-Object)
$expectedOverlaySidecars = @(
    'binaries/npc-media-broker',
    'binaries/npc-mouth-worker',
    'binaries/npc-runtime',
    'binaries/npc-subtitle-presenter'
) | Sort-Object
if (($overlaySidecars -join "`n") -ne ($expectedOverlaySidecars -join "`n")) {
    throw 'Packaging overlay must bundle exactly the four audited product sidecars.'
}
$webViewMode = [string]$tauriOverlay.bundle.windows.webviewInstallMode.type
$generatedInstallerHook = [string]$tauriOverlay.bundle.windows.nsis.installerHooks
if ($webViewMode -ne 'skip' -or
    $generatedInstallerHook -ne 'generated-installer-inputs/installer-hooks.generated.nsh') {
    throw 'Packaging must use the exact custom pinned-offline WebView2 hook and Tauri skip mode.'
}
$resourceMapping = $tauriOverlay.bundle.resources.PSObject.Properties['generated-resources/']
if ($null -eq $resourceMapping -or [string]$resourceMapping.Value -ne 'product-audit/') {
    throw 'Packaging overlay must install generated-resources/ at product-audit/.'
}
$baseTauriConfigPath = Join-Path $repoRoot 'apps/control/src-tauri/tauri.conf.json'
$baseTauriConfig = Get-Content -LiteralPath $baseTauriConfigPath -Raw | ConvertFrom-Json
$applicationIdentifier = if (@($tauriOverlay.PSObject.Properties.Name) -contains 'identifier') {
    [string]$tauriOverlay.identifier
} else {
    [string]$baseTauriConfig.identifier
}
$productionIdentifier = [string]$baseTauriConfig.identifier
if ($Configuration -eq 'Debug') {
    if ($applicationIdentifier -ne "$productionIdentifier.review") {
        Write-Error 'Debug local-review packages must use the isolated .review application identifier.'
        exit 1
    }
} elseif ($applicationIdentifier -ne $productionIdentifier) {
    Write-Error 'Release packages must keep the production application identifier.'
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
$pnpmInvocation = Get-NpcCorepackPnpmInvocation

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
$sourceProcess = Invoke-NpcHiddenProcess -FilePath $pythonPath -ArgumentList @(
    (Join-Path $PSScriptRoot 'security/generate_source_evidence.py'), '--root', $repoRoot, '--out', $sourceEvidencePath
)
if ($sourceProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $sourceEvidencePath -PathType Leaf)) { exit 1 }
$sourceEvidence = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
$sourceHeadBeforeBuild = [string]$sourceEvidence.head_commit
$sourceDigestBeforeBuild = [string]$sourceEvidence.source_candidate_digest.sha256
$sourceDirtyBeforeBuild = [bool]$sourceEvidence.dirty
$toolchainEvidencePath = Join-Path $securityEvidenceRoot 'toolchain-evidence.json'
& (Join-Path $PSScriptRoot 'toolchain-evidence.ps1') -OutputPath $toolchainEvidencePath -RequireNative
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $toolchainEvidencePath -PathType Leaf)) {
    throw 'Exact native toolchain evidence generation failed.'
}

$reviewTestGame = $null
$reviewTestGameEvidence = @()
if ($Configuration -eq 'Debug') {
    $reviewTestGameJson = & (Join-Path $PSScriptRoot 'windows/prepare-review-test-game.ps1')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $reviewTestGame = $reviewTestGameJson | ConvertFrom-Json
    if ($reviewTestGame.status -ne 'prepared') {
        Write-Error 'The deterministic review test game was not prepared.'
        exit 1
    }
    if ([int]$reviewTestGame.schema_version -ne 2 -or
        [int]$reviewTestGame.media_files_bundled -ne 0 -or
        @($reviewTestGame.third_party_binaries_bundled).Count -ne 0) {
        throw 'Review test game must use schema v2 and contain no media or third-party binary payload.'
    }
    $reviewEvidenceMappings = @(
        [pscustomobject]@{ PathField = 'distribution_manifest_path'; HashField = 'distribution_manifest_sha256'; Destination = 'review-test-game-manifest.json' },
        [pscustomobject]@{ PathField = 'sbom_path'; HashField = 'sbom_sha256'; Destination = 'review-test-game.cdx.json' },
        [pscustomobject]@{ PathField = 'notices_path'; HashField = 'notices_sha256'; Destination = 'REVIEW-TEST-GAME-NOTICES.md' }
    )
    foreach ($mapping in $reviewEvidenceMappings) {
        $sourcePath = [string]$reviewTestGame.($mapping.PathField)
        $expectedHash = [string]$reviewTestGame.($mapping.HashField)
        if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
            throw "Review test-game evidence is missing: $sourcePath"
        }
        $observedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $sourcePath).Hash.ToLowerInvariant()
        if ($observedHash -ne $expectedHash) {
            throw "Review test-game evidence hash mismatch: $($mapping.PathField)"
        }
        $reviewTestGameEvidence += [pscustomobject]@{
            Source = $sourcePath
            Destination = $mapping.Destination
            Sha256 = $observedHash
            SizeBytes = (Get-Item -LiteralPath $sourcePath).Length
        }
    }
}

$configPathForCli = $tauriConfig.Replace('\', '/')
$arguments = @('run', 'tauri', 'build', '--config', $configPathForCli)
if ($Configuration -eq 'Debug') { $arguments += '--debug' }

# Tauri's inherited base hook names extensionless `corepack`, which can resolve
# to a POSIX shim or a different installation. Product packaging disables that
# hook in its overlay and performs the one frontend build through the exact
# checked node.exe + corepack.js (or exact corepack.cmd) invocation instead.
Write-Host '==> Building frontend through the exact hidden Corepack invocation' -ForegroundColor Cyan
Push-Location $packageRoot
try {
    Invoke-NpcCheckedCommand `
        -FilePath $pnpmInvocation.FilePath `
        -ArgumentList @($pnpmInvocation.Prefix + @('run', 'build')) `
        -FailureMessage 'Pinned frontend build failed'
}
finally {
    Pop-Location
}

$sidecarStageJson = & (Join-Path $PSScriptRoot 'prepare-sidecars.ps1') -Configuration $Configuration
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$sidecarStage = $sidecarStageJson | ConvertFrom-Json
if ($sidecarStage.status -ne 'prepared' -or
    -not (Test-Path -LiteralPath $sidecarStage.manifest_path -PathType Leaf)) {
    throw 'The exact four-sidecar product stage was not prepared.'
}
$sidecarManifest = Get-Content -LiteralPath $sidecarStage.manifest_path -Raw | ConvertFrom-Json

$cargoExecutable = (Get-Command cargo.exe -CommandType Application -ErrorAction Stop |
    Select-Object -First 1).Source
$tauriMetadataProcess = Invoke-NpcHiddenProcess -FilePath $cargoExecutable -ArgumentList @(
    'metadata', '--manifest-path', (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml'),
    '--format-version', '1', '--no-deps', '--locked', '--offline'
) -WorkingDirectory $repoRoot -NoReplayOutput
if ($tauriMetadataProcess.ExitCode -ne 0) {
    throw "Cargo metadata failed while resolving the Tauri target directory (exit code $($tauriMetadataProcess.ExitCode))."
}
$tauriMetadata = $tauriMetadataProcess.StandardOutput | ConvertFrom-Json
$tauriTargetDirectory = [System.IO.Path]::GetFullPath([string]$tauriMetadata.target_directory)

$targetProfile = $(if ($Configuration -eq 'Debug') { 'debug' } else { 'release' })
$bundleCandidates = @(
    (Join-Path $tauriTargetDirectory "$targetProfile/bundle/nsis"),
    (Join-Path $repoRoot "apps/control/src-tauri/target/$targetProfile/bundle/nsis"),
    (Join-Path $repoRoot "target/$targetProfile/bundle/nsis"),
    (Join-Path $packageRoot "src-tauri/target/$targetProfile/bundle/nsis"),
    (Join-Path $packageRoot "target/$targetProfile/bundle/nsis")
) | Select-Object -Unique
Clear-NpcNsisOutputRoots -RepositoryRoot $repoRoot -Candidates $bundleCandidates `
    -OwnedTargetRoots @($tauriTargetDirectory)
$controlBinaryCandidates = @(
    (Join-Path $tauriTargetDirectory "$targetProfile/interactive-npcs-control.exe"),
    (Join-Path $repoRoot "apps/control/src-tauri/target/$targetProfile/interactive-npcs-control.exe"),
    (Join-Path $repoRoot "target/$targetProfile/interactive-npcs-control.exe"),
    (Join-Path $packageRoot "src-tauri/target/$targetProfile/interactive-npcs-control.exe"),
    (Join-Path $packageRoot "target/$targetProfile/interactive-npcs-control.exe")
) | Select-Object -Unique
foreach ($candidate in $controlBinaryCandidates) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        Remove-Item -LiteralPath $candidate -Force
    }
}

$generatedResourceRoot = Join-Path $repoRoot 'apps/control/src-tauri/generated-resources'
$productResourceStageJson = & (Join-Path $PSScriptRoot 'stage-product-resources.ps1') `
    -SbomPath $sbomPath `
    -SidecarManifestPath $sidecarStage.manifest_path `
    -ArtifactScopePath $artifactScopePath `
    -LicenseMaterialRoot $licenseMaterialRoot `
    -DestinationRoot $generatedResourceRoot
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$productResourceStage = $productResourceStageJson | ConvertFrom-Json
if ($productResourceStage.status -ne 'staged' -or
    -not (Test-Path -LiteralPath $productResourceStage.manifest_path -PathType Leaf)) {
    throw 'Required legal, subtitle, runtime, sidecar, and SBOM resources were not staged.'
}
$productResourceManifest = Get-Content -LiteralPath $productResourceStage.manifest_path -Raw | ConvertFrom-Json
$sidecarEvidencePath = Join-Path $securityEvidenceRoot 'sidecar-manifest.v1.json'
$resourceEvidencePath = Join-Path $securityEvidenceRoot 'resource-manifest.v1.json'
Copy-Item -LiteralPath $sidecarStage.manifest_path -Destination $sidecarEvidencePath -Force
Copy-Item -LiteralPath $productResourceStage.manifest_path -Destination $resourceEvidencePath -Force

# The locked Tauri CLI performs a mutable fwlink HEAD even in its built-in
# `offlineInstaller` mode. Verify exact reviewed standalone-installer bytes and
# generate a local NSIS hook instead; package-time acquisition is forbidden.
$webViewContract = Get-NpcPinnedWebView2InstallerContract -RepositoryRoot $repoRoot
$webViewInstallerIdentity = Assert-NpcPinnedWebView2Installer -Contract $webViewContract
$webViewHookStage = New-NpcPinnedWebView2InstallerHook `
    -RepositoryRoot $repoRoot `
    -InstallerIdentity $webViewInstallerIdentity

Write-Host '==> Building a local NSIS installer (no publication or updater activation)' -ForegroundColor Cyan
$buildStartedUtc = [DateTime]::UtcNow
Push-Location $packageRoot
try {
    Invoke-NpcCheckedCommand `
        -FilePath $pnpmInvocation.FilePath `
        -ArgumentList @($pnpmInvocation.Prefix + $arguments) `
        -FailureMessage 'Pinned Tauri package build failed'
}
finally {
    Pop-Location
    Remove-NpcPinnedWebView2InstallerHookStage -RepositoryRoot $repoRoot
    Remove-NpcGeneratedResourceStage `
        -RepositoryRoot $repoRoot `
        -Path $generatedResourceRoot
}

$expectedInstallerName = "$($baseTauriConfig.productName)_$($baseTauriConfig.version)_x64-setup.exe"
$currentInstaller = Resolve-NpcCurrentInstaller `
    -RepositoryRoot $repoRoot `
    -Candidates $bundleCandidates `
    -ExpectedFileName $expectedInstallerName `
    -BuildStartedUtc $buildStartedUtc `
    -OwnedTargetRoots @($tauriTargetDirectory)

# Re-audit every staged child after Tauri has consumed it. This catches a
# concurrent replacement between manifest creation and bundle completion.
foreach ($binary in $sidecarManifest.binaries) {
    $binaryPath = Join-Path $repoRoot "apps/control/src-tauri/binaries/$($binary.file_name)"
    if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
        throw "Tauri build completed after a required sidecar disappeared: $($binary.file_name)"
    }
    $observedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $binaryPath).Hash.ToLowerInvariant()
    if ($observedHash -ne [string]$binary.sha256) {
        throw "Sidecar changed while Tauri bundled it: $($binary.file_name)"
    }
    & (Join-Path $PSScriptRoot 'audit-pe-product-binary.ps1') `
        -Path $binaryPath -ExpectedSubsystem Gui | Out-Null
}

# Explorer launch acceptance is a binary property, not a source-code claim.
# Verify every local-review configuration because Debug packages are used for
# hands-on testing and previously allocated a visible console.
$controlBinary = $controlBinaryCandidates |
    Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
    Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($controlBinary)) {
    Write-Error "Tauri completed but the control executable was not found in: $($controlBinaryCandidates -join ', ')."
    exit 1
}
if ((Get-Item -LiteralPath $controlBinary).LastWriteTimeUtc -lt $buildStartedUtc.AddSeconds(-2)) {
    throw "Control executable predates the current Tauri build: $controlBinary"
}
& (Join-Path $PSScriptRoot 'assert-pe-subsystem.ps1') -Path $controlBinary -Expected Gui
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Re-read source identity after compilation so a concurrent edit cannot be
# packaged under stale evidence. Ignored build outputs do not affect this digest.
$sourceProcess = Invoke-NpcHiddenProcess -FilePath $pythonPath -ArgumentList @(
    (Join-Path $PSScriptRoot 'security/generate_source_evidence.py'), '--root', $repoRoot, '--out', $sourceEvidencePath
)
if ($sourceProcess.ExitCode -ne 0) { exit $sourceProcess.ExitCode }
$sourceEvidence = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
if (([string]$sourceEvidence.head_commit) -ne $sourceHeadBeforeBuild -or
    ([string]$sourceEvidence.source_candidate_digest.sha256) -ne $sourceDigestBeforeBuild -or
    ([bool]$sourceEvidence.dirty) -ne $sourceDirtyBeforeBuild) {
    Write-Error 'Source changed during packaging; the artifact was not staged with stale evidence.'
    exit 1
}

$stageRoot = New-NpcPackageStageRoot `
    -OutputDirectory $OutputDirectory `
    -OperationId $packageOperationId
Copy-Item -LiteralPath $currentInstaller.FullName -Destination $stageRoot
Copy-Item -LiteralPath $sourceEvidencePath -Destination $stageRoot
foreach ($evidencePath in @(
        $sbomPath,
        $toolchainEvidencePath,
        $sidecarEvidencePath,
        $resourceEvidencePath
        $artifactScopePath
        $licenseMaterialIndex
    )) {
    if (-not (Test-Path -LiteralPath $evidencePath -PathType Leaf)) {
        throw "Required package evidence disappeared: $evidencePath"
    }
    Copy-Item -LiteralPath $evidencePath -Destination $stageRoot
}
foreach ($reviewEvidence in $reviewTestGameEvidence) {
    $destinationPath = Join-Path $stageRoot $reviewEvidence.Destination
    Copy-Item -LiteralPath $reviewEvidence.Source -Destination $destinationPath
    $stagedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destinationPath).Hash.ToLowerInvariant()
    if ($stagedHash -ne $reviewEvidence.Sha256) {
        throw "Review test-game evidence changed while staging: $($reviewEvidence.Destination)"
    }
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
$expectedStagedInstallers = @($installerFiles | Where-Object { $_.file -eq $expectedInstallerName })
if ($installerFiles.Count -ne 1 -or $expectedStagedInstallers.Count -ne 1) {
    throw "Package stage must contain exactly the current installer: $expectedInstallerName"
}
$unsigned = $installerFiles.Count -gt 0 -and @($installerFiles | Where-Object { $_.authenticode_status -ne 'NotSigned' }).Count -eq 0
$controlIdentity = Get-NpcExpectedNsisControlIdentity -Path $controlBinary
$manifest = [ordered]@{
    schema_version = 1
    operation_id = $packageOperationId
    build_started_utc = $buildStartedUtc.ToString('o')
    created_at_utc = [DateTime]::UtcNow.ToString('o')
    configuration = $Configuration
    distribution = 'local-review-only'
    application_identifier = $applicationIdentifier
    production_application_identifier = $productionIdentifier
    app_config_folder = $applicationIdentifier
    packaging_config = [ordered]@{
        path = $tauriConfig.Substring($repoRoot.Length).TrimStart('\').Replace('\', '/')
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $tauriConfig).Hash.ToLowerInvariant()
    }
    updater_enabled = $false
    unsigned = $unsigned
    evidence_class = $(if ($SkipChecks) { [string]$releasePolicy.skip_checks_classification } elseif ($sourceEvidence.dirty) { 'dirty-local-review' } else { 'pre-reconciliation-local-review' })
    # Pre-build staging proves intent, not extracted installer contents. Only
    # the opt-in installer smoke plus distribution reconciler can promote this
    # artifact after source freeze.
    immutable_release_candidate = $false
    reconciliation = [ordered]@{
        status = 'pending_installed_artifact_reconciliation'
        promotion_supported = $false
        installer_smoke_command = 'scripts/installer-smoke.ps1 -AcknowledgeLocalInstall'
        blocker = 'The source-frozen, state-changing two-install smoke and closed-world installed-distribution reconciliation have not yet been executed for this exact installer.'
        extracted_installer_reconciled = $false
        installed_legal_resources_reconciled = $false
        unclassified_installed_files_rejected = $false
    }
    source = $sourceEvidence
    acceptance_evidence = [ordered]@{
        status = [string]$acceptanceEvidence.status
        gap_map_path = 'docs/product-rework/original-brief-gap-map.json'
        gap_map_sha256 = [string]$acceptanceEvidence.gap_map_sha256
        authoritative_ledger_path = 'docs/product-rework/original-brief-acceptance.md'
        authoritative_ledger_sha256 = [string]$acceptanceEvidence.authoritative_ledger_sha256
        evidence_report_path = 'docs/requirements/local-review-evidence-report.md'
        evidence_report_sha256 = [string]$acceptanceEvidence.evidence_report_sha256
        requirement_rows = [int]$acceptanceEvidence.requirement_rows
        success_criteria_rows = [int]$acceptanceEvidence.success_criteria_rows
    }
    control_executable = [ordered]@{
        file_name = [System.IO.Path]::GetFileName($controlBinary)
        sha256 = [string]$controlIdentity.installed_sha256
        pre_bundle_sha256 = [string]$controlIdentity.pre_bundle_sha256
        bundle_patch = [ordered]@{
            type = 'nsis'
            source_marker = [string]$controlIdentity.source_marker
            installed_marker = [string]$controlIdentity.installed_marker
            marker_offset = [int64]$controlIdentity.marker_offset
            changed_bytes = [int]$controlIdentity.changed_bytes
        }
        pe_subsystem = 'windows_gui'
    }
    sidecars = $sidecarManifest
    product_resources = $productResourceManifest
    toolchain = Get-Content -LiteralPath $toolchainEvidencePath -Raw | ConvertFrom-Json
    webview2_offline_installer = [ordered]@{
        file_name = [string]$webViewInstallerIdentity.FileName
        file_version = [string]$webViewInstallerIdentity.FileVersion
        product_version = [string]$webViewInstallerIdentity.ProductVersion
        size_bytes = [long]$webViewInstallerIdentity.SizeBytes
        sha256 = [string]$webViewInstallerIdentity.Sha256
        authenticode_status = [string]$webViewInstallerIdentity.AuthenticodeStatus
        signer_subject = [string]$webViewInstallerIdentity.SignerSubject
        signer_thumbprint = [string]$webViewInstallerIdentity.SignerThumbprint
        tauri_config_mode = 'skip'
        package_contract = 'custom-pinned-offline-installer'
        package_time_network_acquisition = $false
        cache_path_recorded = $false
        generated_hook_sha256 = [string]$webViewHookStage.HookSha256
    }
    review_test_game = $reviewTestGame
    review_test_game_staged_evidence = @($reviewTestGameEvidence | ForEach-Object {
            [ordered]@{
                file = $_.Destination
                sha256 = $_.Sha256
                size_bytes = $_.SizeBytes
            }
        })
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
$manifestJson = ($manifest | ConvertTo-Json -Depth 10) + [Environment]::NewLine
[System.IO.File]::WriteAllText(
    (Join-Path $stageRoot 'package-manifest.json'),
    $manifestJson,
    (New-Object System.Text.UTF8Encoding($false))
)
Write-Host "Local package staged at: $stageRoot" -ForegroundColor Green
Write-Host 'Nothing was uploaded, signed, released, or added to an update feed.' -ForegroundColor Yellow
[ordered]@{
    schema_version = 1
    status = 'packaged'
    configuration = $Configuration
    application_identifier = $applicationIdentifier
    stage_root = $stageRoot
    package_manifest_path = Join-Path $stageRoot 'package-manifest.json'
    control_executable_path = $controlBinary
    installer_paths = @(Get-ChildItem -LiteralPath $stageRoot -File -Filter '*-setup.exe' |
        Sort-Object Name | Select-Object -ExpandProperty FullName)
    review_test_game_path = $(if ($null -ne $reviewTestGame) { [string]$reviewTestGame.executable_path } else { $null })
    publication_performed = $false
    signing_performed = $false
} | ConvertTo-Json -Compress -Depth 4 | Write-Output
exit 0
