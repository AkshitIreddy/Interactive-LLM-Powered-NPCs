[CmdletBinding()]
param(
    [string]$DestinationRoot = 'E:\temp\InteractiveNPCs\review-v21',
    [string]$ArtifactRoot = 'E:\temp\InteractiveNPCs',
    [string]$StableTestGameDirectory = 'E:\temp\InteractiveNPCs\review-game-v17-stable\local-app-data\test-game',
    [string]$MouthAtlasDirectory = 'E:\temp\InteractiveNPCs\review-mouth-atlas-v80-native-compatible',
    [string]$ReviewedMouthAtlasReceiptPath = '',
    [string[]]$ReviewedCharacterMouthPackReceiptPaths = @(),
    [string]$PrivateReviewModelCatalogDirectory = '',
    [switch]$PreflightOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$global:LASTEXITCODE = 0

. (Join-Path $PSScriptRoot 'node-tooling.ps1')
. (Join-Path $PSScriptRoot 'product-resource-paths.ps1')
. (Join-Path $PSScriptRoot 'reviewed-mouth-atlas-receipt.ps1')

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$destination = [System.IO.Path]::GetFullPath($DestinationRoot).TrimEnd('\')
$artifactRootFull = [System.IO.Path]::GetFullPath($ArtifactRoot).TrimEnd('\')
if (-not $destination.StartsWith("$artifactRootFull\", [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Portable review destination must be below $artifactRootFull."
}
if (Test-Path -LiteralPath $destination) {
    throw "Portable review destination already exists; refusing to replace it: $destination"
}
if ($env:OS -ne 'Windows_NT' -or -not [System.Environment]::Is64BitOperatingSystem) {
    throw 'Portable review preparation requires 64-bit Windows.'
}

$python = Get-Command python.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$cargo = Get-Command cargo.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$pwsh = Get-Command pwsh.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1
$pnpm = Get-NpcCorepackPnpmInvocation
$tauriConfig = Join-Path $repoRoot 'packaging/windows/tauri.review.conf.json'
$metadataProcess = Invoke-NpcHiddenProcess -FilePath $cargo.Source -ArgumentList @(
    'metadata', '--manifest-path', (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml'),
    '--format-version', '1', '--no-deps', '--locked', '--offline'
) -WorkingDirectory $repoRoot -NoReplayOutput
if ($metadataProcess.ExitCode -ne 0) { throw 'Could not resolve the nested Tauri target directory.' }
$targetDirectory = [System.IO.Path]::GetFullPath(
    [string](($metadataProcess.StandardOutput | ConvertFrom-Json).target_directory))
$controlBinary = Join-Path $targetDirectory 'debug/interactive-npcs-control.exe'
$stableTestGame = [System.IO.Path]::GetFullPath($StableTestGameDirectory).TrimEnd('\')
$engineeringReviewSource = Join-Path $repoRoot 'docs/product-rework/local-review-2026-09-05.md'
$privateModelCatalog = $null
$privateCatalogReceipt = $null
$privateCatalogReceiptHash = $null
$privateCatalogRuntimeFiles = @()

function Get-LowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Utf8NoBom {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Content)
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

function Assert-NormalClosedWorldDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string[]]$ExpectedFiles,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) { throw "$Label is missing: $Path" }
    $allItems = @((Get-Item -LiteralPath $Path -Force)) + @(Get-ChildItem -LiteralPath $Path -Force -Recurse)
    foreach ($item in $allItems) {
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw "$Label must not contain reparse points: $($item.FullName)"
        }
    }
    $actualFiles = @(Get-ChildItem -LiteralPath $Path -File -Force -Recurse |
        ForEach-Object { $_.FullName.Substring($Path.Length + 1).Replace('\', '/') } | Sort-Object)
    $expectedSorted = @($ExpectedFiles | Sort-Object)
    if (($actualFiles -join "`n") -cne ($expectedSorted -join "`n") -or
        $expectedSorted.Count -ne @($expectedSorted | Select-Object -Unique).Count) {
        throw "$Label contains missing, duplicated, or unknown files."
    }
}

function Remove-VerifiedReviewBuildDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Boundary,
        [Parameter(Mandatory = $true)][string]$ExpectedLeafPattern
    )
    $resolvedPath = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    $resolvedBoundary = [System.IO.Path]::GetFullPath($Boundary).TrimEnd('\')
    if (-not $resolvedPath.StartsWith("$resolvedBoundary\", [System.StringComparison]::OrdinalIgnoreCase) -or
        $resolvedPath.Equals($resolvedBoundary, [System.StringComparison]::OrdinalIgnoreCase) -or
        [System.IO.Path]::GetFileName($resolvedPath) -notlike $ExpectedLeafPattern) {
        throw "Refusing unsafe review-build cleanup target: $resolvedPath"
    }
    if (-not (Test-Path -LiteralPath $resolvedPath)) { return }
    $items = @((Get-Item -LiteralPath $resolvedPath -Force)) +
        @(Get-ChildItem -LiteralPath $resolvedPath -Force -Recurse)
    foreach ($item in $items) {
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw "Refusing review-build cleanup containing a reparse point: $($item.FullName)"
        }
    }
    [Console]::Error.WriteLine("Removing verified temporary directory: {0}; entries including root: {1}; reparse points: 0; Recycle Bin bypassed.", $resolvedPath, $items.Count)
    [System.IO.Directory]::Delete($resolvedPath, $true)
    if (Test-Path -LiteralPath $resolvedPath) {
        throw "Review-build cleanup did not remove the exact target: $resolvedPath"
    }
}

function Get-EngineeringReviewWithAbsoluteLinks {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath
    )
    $sourceDirectory = Split-Path -Parent $SourcePath
    $content = Get-Content -LiteralPath $SourcePath -Raw
    $rewritten = [System.Text.RegularExpressions.Regex]::Replace(
        $content,
        '\]\(([^)]+)\)',
        [System.Text.RegularExpressions.MatchEvaluator]{
            param($match)
            $target = [string]$match.Groups[1].Value
            if ($target.StartsWith('<') -and $target.EndsWith('>')) {
                $target = $target.Substring(1, $target.Length - 2)
            }
            if ($target -match '^(?i:https?|mailto|file):' -or $target.StartsWith('#') -or
                [System.IO.Path]::IsPathRooted($target)) {
                return $match.Value
            }
            $fragment = ''
            $pathPart = $target
            $fragmentIndex = $target.IndexOf('#')
            if ($fragmentIndex -ge 0) {
                $pathPart = $target.Substring(0, $fragmentIndex)
                $fragment = $target.Substring($fragmentIndex)
            }
            $resolved = [System.IO.Path]::GetFullPath((Join-Path $sourceDirectory $pathPart))
            if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
                throw "Engineering review contains a broken local link: $target"
            }
            return '](' + '<' + $resolved.Replace('\', '/') + $fragment + '>' + ')'
        })
    return $rewritten
}

if (-not [string]::IsNullOrWhiteSpace($ReviewedMouthAtlasReceiptPath) -and
    $PSBoundParameters.ContainsKey('MouthAtlasDirectory')) {
    throw 'Use ReviewedMouthAtlasReceiptPath or the legacy MouthAtlasDirectory lookup, not both.'
}
$reviewedMouthAtlasReceipt = if (-not [string]::IsNullOrWhiteSpace($ReviewedMouthAtlasReceiptPath)) {
    [System.IO.Path]::GetFullPath($ReviewedMouthAtlasReceiptPath)
} else {
    $legacyAtlasRoot = [System.IO.Path]::GetFullPath($MouthAtlasDirectory).TrimEnd('\')
    Join-Path $legacyAtlasRoot 'reviewed-artifact-receipt.v1.json'
}
$reviewedMouthAtlas = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $reviewedMouthAtlasReceipt
$mouthAtlas = [string]$reviewedMouthAtlas.artifact_root
$atlasPayloadFiles = @(
    [string]$reviewedMouthAtlas.atlas_file_name,
    [string]$reviewedMouthAtlas.texture_file_name
)
$reviewedCharacterMouthPacks = @()
$reviewedCharacterPackKeys = @{}
foreach ($characterReceiptPath in $ReviewedCharacterMouthPackReceiptPaths) {
    $characterPack = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $characterReceiptPath
    if ([string]$characterPack.classification -ne 'reviewed-private-character-pack') {
        throw 'Optional character mouth packs must have reviewed-private-character-pack receipts.'
    }
    $key = '{0}/{1}' -f $characterPack.game_profile_id, $characterPack.character_id
    if ($reviewedCharacterPackKeys.ContainsKey($key)) {
        throw "Optional character mouth-pack receipt is duplicated: $key"
    }
    if ($key -eq ('{0}/{1}' -f $reviewedMouthAtlas.game_profile_id, $reviewedMouthAtlas.character_id)) {
        throw "Optional character mouth pack duplicates the main review atlas: $key"
    }
    $reviewedCharacterPackKeys[$key] = $true
    $reviewedCharacterMouthPacks += $characterPack
}

if (-not [string]::IsNullOrWhiteSpace($PrivateReviewModelCatalogDirectory)) {
    $privateModelCatalog = [System.IO.Path]::GetFullPath($PrivateReviewModelCatalogDirectory).TrimEnd('\')
    $receiptPath = Join-Path $privateModelCatalog 'evidence/catalog-verification-receipt.json'
    $importContextPath = Join-Path $privateModelCatalog 'evidence/yunet-import-context.json'
    $liveVerification = & (Join-Path $PSScriptRoot 'verify-private-review-model-catalog.ps1') `
        -Directory $privateModelCatalog -ReceiptPath $receiptPath `
        -ImportContextPath $importContextPath | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $liveVerification.status -ne 'passed') {
        throw 'Private review model catalog failed live cryptographic verification.'
    }
    $privateCatalogReceiptHash = Get-LowerSha256 -Path $receiptPath
    if ($privateCatalogReceiptHash -cne [string]$liveVerification.receiptSha256) {
        throw 'Private review model catalog receipt changed after live verification.'
    }
    $privateCatalogReceipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    if ($privateCatalogReceipt.schema -ne 'interactive-npcs-private-catalog-verification/v1' -or
        -not [bool]$privateCatalogReceipt.verification.catalogBundlePublicApi -or
        -not [bool]$privateCatalogReceipt.verification.manifestFilesPublicApi -or
        -not [bool]$privateCatalogReceipt.verification.qualifiedEnvelopeFilesPublicApi -or
        -not [bool]$privateCatalogReceipt.verification.trustedPackSnapshotPublicApi -or
        $privateCatalogReceipt.catalog.trustScope -ne 'automated_local_review_bootstrap' -or
        [bool]$privateCatalogReceipt.catalog.productionTrust -or
        -not [bool]$privateCatalogReceipt.catalog.rotationRequiredBeforeRelease -or
        [bool]$privateCatalogReceipt.catalog.promotionSupported -or
        [bool]$privateCatalogReceipt.catalog.publicationSupported -or
        [int]$privateCatalogReceipt.catalog.signatureThreshold -ne 2 -or
        [int]$privateCatalogReceipt.catalog.keyCount -ne 2 -or
        $privateCatalogReceipt.pack.id -ne 'openseeface-yunet640-lm1-mouth-signal' -or
        $privateCatalogReceipt.pack.revision -ne '85aa70fc67582d046e771ea73625182a0d8f7475' -or
        -not [bool]$privateCatalogReceipt.pack.nativeInferenceQualified -or
        [bool]$privateCatalogReceipt.pack.providerLoadSelfTestAttested -or
        [bool]$privateCatalogReceipt.pack.installed -or [bool]$privateCatalogReceipt.pack.activated -or
        [bool]$privateCatalogReceipt.signerPrivateMaterialPersisted) {
        throw 'Private review catalog receipt violates the local-review trust or activation boundary.'
    }
    $privateCatalogRuntimeFiles = @($privateCatalogReceipt.runtimeFiles)
    if ($privateCatalogRuntimeFiles.Count -ne 4) {
        throw 'Private review catalog receipt must bind exactly four runtime metadata files.'
    }
    $runtimeRelativePaths = @($privateCatalogRuntimeFiles | ForEach-Object { [string]$_.path })
    $expectedFixedRuntimePaths = @(
        'model-catalog-root-v1.json',
        'model-catalog-v1.json',
        'openseeface-yunet640-lm1-mouth-signal.json'
    )
    foreach ($required in $expectedFixedRuntimePaths) {
        if ($required -notin $runtimeRelativePaths) {
            throw "Private review catalog receipt is missing runtime metadata: $required"
        }
    }
    $qualifiedPaths = @($runtimeRelativePaths | Where-Object {
            $_ -cmatch '^qual/[0-9a-f]{64}\.json$'
        })
    if ($qualifiedPaths.Count -ne 1 -or
        @($runtimeRelativePaths | Select-Object -Unique).Count -ne 4) {
        throw 'Private review catalog receipt must bind one digest-addressed qualified envelope.'
    }
    $privateCatalogExpectedFiles = @($runtimeRelativePaths) + @(
        'evidence/catalog-verification-receipt.json',
        'evidence/yunet-import-context.json'
    )
    Assert-NormalClosedWorldDirectory -Path $privateModelCatalog `
        -ExpectedFiles $privateCatalogExpectedFiles -Label 'Private signed review model catalog'
    foreach ($entry in $privateCatalogRuntimeFiles) {
        $relative = [string]$entry.path
        if ($relative -notin $runtimeRelativePaths -or
            [string]$entry.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
            (Get-LowerSha256 -Path (Join-Path $privateModelCatalog $relative)) -cne [string]$entry.sha256) {
            throw "Private review catalog runtime file disagrees with its verified receipt: $relative"
        }
    }
    if ([string]$privateCatalogReceipt.auditEvidence.path -ne 'evidence/yunet-import-context.json' -or
        (Get-LowerSha256 -Path (Join-Path $privateModelCatalog 'evidence/yunet-import-context.json')) -cne
            [string]$privateCatalogReceipt.auditEvidence.sha256) {
        throw 'Private review catalog audit evidence disagrees with its verified receipt.'
    }
}

if (-not (Test-Path -LiteralPath $engineeringReviewSource -PathType Leaf)) {
    throw "Engineering review source is missing: $engineeringReviewSource"
}
$preflightEngineeringReview = Get-EngineeringReviewWithAbsoluteLinks -SourcePath $engineeringReviewSource
$rewrittenEngineeringLinkCount = [System.Text.RegularExpressions.Regex]::Matches(
    $preflightEngineeringReview, '\]\(<[A-Za-z]:/').Count
$remainingRelativeEngineeringLinks = [System.Text.RegularExpressions.Regex]::Matches(
    $preflightEngineeringReview, '\]\((?![<#]|(?i:https?|mailto|file):)[^)]+\)').Count
if ($remainingRelativeEngineeringLinks -ne 0) {
    throw 'Engineering review link rewriting left a relative Markdown link.'
}
$stableGameJson = & (Join-Path $PSScriptRoot 'verify-review-test-game.ps1') -Directory $stableTestGame
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$stableGameVerification = $stableGameJson | ConvertFrom-Json
if ($stableGameVerification.status -ne 'passed' -or $stableGameVerification.file_count -ne 6 -or
    $stableGameVerification.visual_source -ne 'verified-sibling-project-owned-mara-camera-sequence-v1') {
    throw 'Stable synthetic review game prerequisite failed verification.'
}

$preflight = [ordered]@{
    schema = 'interactive-npcs-portable-review-preflight/v1'
    status = 'ready'
    repository = $repoRoot
    destination = $destination
    artifact_root = $artifactRootFull
    cargo_target = $targetDirectory
    control_binary = $controlBinary
    application_identifier = 'io.github.akshitireddy.interactive-npcs.review'
    layout = [ordered]@{
        application = 'interactive-npcs-control.exe'
        test_game = 'local-app-data/test-game/interactive-npcs-synthetic-target.exe'
        mouth_atlas = 'review-mouth-atlas'
        engineering_review = 'review-evidence/engineering-review.md'
    }
    stable_test_game_source = $stableTestGame
    stable_test_game_sha256 = [string]$stableGameVerification.executable_sha256
    mouth_atlas_source = $mouthAtlas
    mouth_atlas_receipt = [string]$reviewedMouthAtlas.receipt_path
    mouth_atlas_receipt_sha256 = [string]$reviewedMouthAtlas.receipt_sha256
    mouth_atlas_schema_version = [uint32]$reviewedMouthAtlas.schema_version
    mouth_atlas_identity_revision = [string]$reviewedMouthAtlas.identity_revision
    mouth_atlas_game_profile_id = [string]$reviewedMouthAtlas.game_profile_id
    mouth_atlas_character_id = [string]$reviewedMouthAtlas.character_id
    reviewed_character_mouth_pack_count = $reviewedCharacterMouthPacks.Count
    reviewed_character_mouth_packs = @($reviewedCharacterMouthPacks | ForEach-Object {
        '{0}/{1} schema {2}' -f $_.game_profile_id, $_.character_id, $_.schema_version
    })
    engineering_review_source = $engineeringReviewSource
    engineering_review_absolute_link_count = $rewrittenEngineeringLinkCount
    engineering_review_relative_link_count = $remainingRelativeEngineeringLinks
    private_review_model_catalog_source = $privateModelCatalog
    private_review_model_catalog_receipt_sha256 = if ($null -eq $privateCatalogReceipt) {
        $null
    } else {
        $privateCatalogReceiptHash
    }
    installer_used = $false
    desktop_launch_performed = $false
}
if ($PreflightOnly) { $preflight | ConvertTo-Json -Depth 5; exit 0 }

if ([string]::IsNullOrWhiteSpace($env:NPC_LARGE_ARTIFACT_ROOT)) {
    $env:NPC_LARGE_ARTIFACT_ROOT = $artifactRootFull
}
elseif (-not [string]::Equals([System.IO.Path]::GetFullPath($env:NPC_LARGE_ARTIFACT_ROOT).TrimEnd('\'),
        $artifactRootFull, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'NPC_LARGE_ARTIFACT_ROOT must match the requested portable artifact root.'
}

$stage = Join-Path $artifactRootFull ('.portable-review-stage-' + [Guid]::NewGuid().ToString('N'))
$evidenceWork = Join-Path $artifactRootFull ('.portable-review-evidence-' + [Guid]::NewGuid().ToString('N'))
$generatedResources = Join-Path $repoRoot 'apps/control/src-tauri/generated-resources'
if (Test-Path -LiteralPath $generatedResources) {
    throw "Generated Tauri resources already exist; another package operation may be active: $generatedResources"
}

try {
    New-Item -ItemType Directory -Path $stage,$evidenceWork -Force | Out-Null

    & (Join-Path $repoRoot 'scripts/security/check-source-hygiene.ps1') -Root $repoRoot | Out-Null
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $acceptance = (& (Join-Path $repoRoot 'scripts/assert-acceptance-evidence-consistency.ps1')) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $acceptance.status -ne 'consistent') { throw 'Acceptance evidence is inconsistent.' }
    & (Join-Path $repoRoot 'scripts/security/scan-secrets.ps1') -RepositoryRoot $repoRoot -IncludeUntracked -NoHistory
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & (Join-Path $repoRoot 'scripts/security/check-licenses.ps1') -Strict
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $sourceEvidencePath = Join-Path $evidenceWork 'source-evidence.json'
    $sourceProcess = Invoke-NpcHiddenProcess -FilePath $python.Source -ArgumentList @(
        (Join-Path $repoRoot 'scripts/security/generate_source_evidence.py'),
        '--root', $repoRoot, '--out', $sourceEvidencePath
    ) -WorkingDirectory $repoRoot -NoReplayOutput
    if ($sourceProcess.ExitCode -ne 0) { throw 'Initial source identity generation failed.' }
    $sourceBefore = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json

    $legalJson = & (Join-Path $repoRoot 'scripts/prepare-artifact-legal-resources.ps1') `
        -OutputDirectory (Join-Path $evidenceWork 'legal')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $legal = $legalJson | ConvertFrom-Json
    $sidecarJson = & (Join-Path $repoRoot 'scripts/prepare-sidecars.ps1') -Configuration Debug
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $sidecars = $sidecarJson | ConvertFrom-Json

    $resourceJson = & (Join-Path $repoRoot 'scripts/stage-product-resources.ps1') `
        -SbomPath ([string]$legal.sbom_path) `
        -SidecarManifestPath ([string]$sidecars.manifest_path) `
        -ArtifactScopePath ([string]$legal.artifact_scope_path) `
        -LicenseMaterialRoot ([string]$legal.license_root) `
        -DestinationRoot $generatedResources
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $resources = $resourceJson | ConvertFrom-Json

    Push-Location (Join-Path $repoRoot 'apps/control')
    try {
        Invoke-NpcCheckedCommand -FilePath $pnpm.FilePath `
            -ArgumentList @($pnpm.Prefix + @('run', 'build')) `
            -FailureMessage 'Pinned frontend build failed'
        $buildStarted = [DateTime]::UtcNow
        Invoke-NpcCheckedCommand -FilePath $pnpm.FilePath `
            -ArgumentList @($pnpm.Prefix + @(
                'run', 'tauri', 'build', '--debug', '--no-bundle', '--ci', '--config',
                $tauriConfig.Replace('\', '/'))) `
            -FailureMessage 'Installer-free Tauri build failed'
    }
    finally { Pop-Location }

    if (-not (Test-Path -LiteralPath $controlBinary -PathType Leaf) -or
        (Get-Item -LiteralPath $controlBinary).LastWriteTimeUtc -lt $buildStarted.AddSeconds(-2)) {
        throw 'Fresh installer-free control executable was not produced.'
    }
    & (Join-Path $repoRoot 'scripts/audit-pe-product-binary.ps1') `
        -Path $controlBinary -ExpectedSubsystem Gui | Out-Null

    $sourceProcess = Invoke-NpcHiddenProcess -FilePath $python.Source -ArgumentList @(
        (Join-Path $repoRoot 'scripts/security/generate_source_evidence.py'),
        '--root', $repoRoot, '--out', $sourceEvidencePath
    ) -WorkingDirectory $repoRoot -NoReplayOutput
    if ($sourceProcess.ExitCode -ne 0) { throw 'Final source identity generation failed.' }
    $sourceAfter = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
    if ([string]$sourceBefore.head_commit -ne [string]$sourceAfter.head_commit -or
        [string]$sourceBefore.source_candidate_digest.sha256 -ne [string]$sourceAfter.source_candidate_digest.sha256 -or
        [bool]$sourceBefore.dirty -ne [bool]$sourceAfter.dirty) {
        throw 'Source changed while the portable review was building.'
    }

    Copy-Item -LiteralPath $controlBinary -Destination (Join-Path $stage 'interactive-npcs-control.exe')
    foreach ($binary in @($sidecars.binaries)) {
        $source = Join-Path $repoRoot ('apps/control/src-tauri/binaries/' + [string]$binary.file_name)
        $destinationName = ([string]$binary.file_name).Replace('-x86_64-pc-windows-msvc', '')
        if ((Get-LowerSha256 -Path $source) -cne [string]$binary.sha256) { throw "Sidecar changed: $source" }
        Copy-Item -LiteralPath $source -Destination (Join-Path $stage $destinationName)
    }
    $repositoryMappings = @(
        @{ Source = 'profiles/games'; Destination = 'profiles/games' },
        @{ Source = 'profiles/content-packs/local-review'; Destination = 'profiles/content-packs/local-review' },
        @{ Source = 'catalog/v1'; Destination = 'catalog/v1' }
    )
    if ($null -eq $privateModelCatalog) {
        $repositoryMappings += @{ Source = 'packaging/model-packs'; Destination = 'packaging/model-packs' }
    }
    foreach ($mapping in $repositoryMappings) {
        $target = Join-Path $stage $mapping.Destination
        New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $repoRoot $mapping.Source) -Destination $target -Recurse
    }
    if ($null -ne $privateModelCatalog) {
        $modelCatalogTarget = Join-Path $stage 'packaging/model-packs'
        New-Item -ItemType Directory -Path $modelCatalogTarget -Force | Out-Null
        foreach ($entry in $privateCatalogRuntimeFiles) {
            $relative = [string]$entry.path
            $target = Join-Path $modelCatalogTarget $relative
            New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $privateModelCatalog $relative) -Destination $target
        }
    }
    Copy-Item -LiteralPath $generatedResources -Destination (Join-Path $stage 'product-audit') -Recurse

    $testGameDirectory = Join-Path $stage 'local-app-data/test-game'
    New-Item -ItemType Directory -Path $testGameDirectory -Force | Out-Null
    foreach ($fixtureFile in @(Get-ChildItem -LiteralPath $stableTestGame -File -Force)) {
        Copy-Item -LiteralPath $fixtureFile.FullName -Destination (Join-Path $testGameDirectory $fixtureFile.Name)
    }
    $testGameJson = & (Join-Path $PSScriptRoot 'verify-review-test-game.ps1') -Directory $testGameDirectory
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $testGame = $testGameJson | ConvertFrom-Json
    if ($testGame.status -ne 'passed') { throw 'Synthetic review game was not copied intact.' }
    $testGameManifestPath = Join-Path $testGameDirectory 'REVIEW-FIXTURE-MANIFEST.json'
    $testGameManifest = Get-Content -LiteralPath $testGameManifestPath -Raw | ConvertFrom-Json
    $sequenceEntries = @($testGameManifest.files | Where-Object {
            [System.IO.Path]::GetExtension([string]$_.path) -ieq '.inpcseq'
        })
    if (@($testGameManifest.expected_files).Count -ne 6 -or $sequenceEntries.Count -ne 1 -or
        [string]$testGameManifest.generated_media.visual_source -ne
            'verified-sibling-project-owned-mara-camera-sequence-v1') {
        throw 'Synthetic review game must contain the one verified 42-frame camera sequence.'
    }
    $sequenceEntry = $sequenceEntries[0]

    $atlasTarget = Join-Path $stage 'review-mouth-atlas'
    New-Item -ItemType Directory -Path $atlasTarget -Force | Out-Null
    foreach ($atlasFile in $atlasPayloadFiles) {
        Copy-Item -LiteralPath (Join-Path $mouthAtlas $atlasFile) -Destination (Join-Path $atlasTarget $atlasFile)
    }
    $stagedCharacterMouthPacks = @()
    foreach ($characterPack in $reviewedCharacterMouthPacks) {
        $relativeRoot = 'review-character-mouth-packs/{0}/{1}' -f
            $characterPack.game_profile_id, $characterPack.character_id
        $targetRoot = Join-Path $stage $relativeRoot
        New-Item -ItemType Directory -Path $targetRoot -Force | Out-Null
        foreach ($source in @(
                [string]$characterPack.receipt_path,
                [string]$characterPack.atlas_path,
                [string]$characterPack.texture_path
            )) {
            Copy-Item -LiteralPath $source -Destination (Join-Path $targetRoot ([System.IO.Path]::GetFileName($source)))
        }
        $stagedReceipt = Join-Path $targetRoot ([System.IO.Path]::GetFileName([string]$characterPack.receipt_path))
        $stagedValidation = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $stagedReceipt
        if ([string]$stagedValidation.receipt_sha256 -cne [string]$characterPack.receipt_sha256) {
            throw "Reviewed character mouth pack changed while staging: $relativeRoot"
        }
        $stagedCharacterMouthPacks += [pscustomobject]@{
            source = $characterPack
            relative_root = $relativeRoot
            root = $targetRoot
            receipt = $stagedReceipt
        }
    }

    $reviewEvidence = Join-Path $stage 'review-evidence'
    New-Item -ItemType Directory -Path $reviewEvidence -Force | Out-Null
    $engineeringReviewTarget = Join-Path $reviewEvidence 'engineering-review.md'
    $engineeringReviewSourceHash = Get-LowerSha256 -Path $engineeringReviewSource
    Write-Utf8NoBom -Path $engineeringReviewTarget `
        -Content (Get-EngineeringReviewWithAbsoluteLinks -SourcePath $engineeringReviewSource)
    if ((Get-LowerSha256 -Path $engineeringReviewSource) -cne $engineeringReviewSourceHash) {
        throw 'Engineering review source changed while its portable copy was generated.'
    }
    Copy-Item -LiteralPath $sourceEvidencePath -Destination (Join-Path $reviewEvidence 'source-evidence.json')
    Copy-Item -LiteralPath ([string]$sidecars.manifest_path) -Destination (Join-Path $reviewEvidence 'sidecar-manifest.v1.json')
    Copy-Item -LiteralPath ([string]$resources.manifest_path) -Destination (Join-Path $reviewEvidence 'resource-manifest.v1.json')
    $mouthAtlasReceiptTarget = Join-Path $reviewEvidence 'reviewed-mouth-atlas-receipt.v1.json'
    Copy-Item -LiteralPath ([string]$reviewedMouthAtlas.receipt_path) -Destination $mouthAtlasReceiptTarget
    if ((Get-LowerSha256 -Path $mouthAtlasReceiptTarget) -cne
        [string]$reviewedMouthAtlas.receipt_sha256) {
        throw 'Reviewed mouth-atlas receipt changed while it was staged.'
    }
    if ($null -ne $privateModelCatalog) {
        Copy-Item -LiteralPath (Join-Path $privateModelCatalog 'evidence/catalog-verification-receipt.json') `
            -Destination (Join-Path $reviewEvidence 'private-model-catalog-verification.json')
        Copy-Item -LiteralPath (Join-Path $privateModelCatalog 'evidence/yunet-import-context.json') `
            -Destination (Join-Path $reviewEvidence 'yunet-import-context.json')
    }
    & (Join-Path $repoRoot 'scripts/toolchain-evidence.ps1') -RequireNative `
        -OutputPath (Join-Path $reviewEvidence 'toolchain-evidence.json')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Write-Utf8NoBom -Path (Join-Path $reviewEvidence 'acceptance-consistency.json') `
        -Content (($acceptance | ConvertTo-Json -Depth 6) + [Environment]::NewLine)

    $modelCatalogReviewNote = if ($null -ne $privateModelCatalog) {
        @"
This review contains the independently verified, metadata-only YuNet local-review
catalog under packaging\model-packs. It has two-of-two ephemeral review signatures,
production trust disabled, and no model/runtime payload. The optional pack remains
inactive until the user explicitly installs it and the provider-load self-test plus
whole-loadout admission pass.
"@
    }
    else {
        'This review contains the checked repository model catalog metadata.'
    }
    $characterPackReviewNote = if ($stagedCharacterMouthPacks.Count -eq 0) {
        'No separately reviewed character mouth packs are staged in this review.'
    } else {
        $characterPackBoundaries = @($reviewedCharacterMouthPacks | ForEach-Object {
            "- $($_.game_profile_id)/$($_.character_id): schema $($_.schema_version); " +
                "$($_.classification); $($_.qualification); natural-quality qualified " +
                "$($_.natural_quality_qualified); ordinary targets enabled $($_.ordinary_targets_enabled)."
        }) -join [Environment]::NewLine
        "Reviewed character mouth packs are staged under review-character-mouth-packs. " +
            "They remain disabled until imported and enabled through the character workspace. " +
            "Packaging their reviewed receipts does not qualify or activate them." +
            [Environment]::NewLine + $characterPackBoundaries
    }

    $readme = @"
# Interactive NPCs local review

Review root: $destination

1. Start interactive-npcs-control.exe. Open Games and use **Launch & connect**
   on the Included practice game card. The app verifies and opens Eclipse Harbor
   from `local-app-data\test-game\interactive-npcs-synthetic-target.exe`, then
   connects the exact window. You do not need to find or start that file first.
2. The bundle contains no pre-activated model, provider, target, or character
   mouth-pack state. Existing settings in the isolated owner review profile
   persist separately from this directory. To review Cyberpunk 2077 instead,
   choose it in Games, confirm single-player use, scan for game windows, and
   connect the exact window you intend to review.
3. From Games choose Set up mouth tracking; the app opens Loadout > Local models.
   Choose Choose tracking model. In Model downloads, explicitly allow the local
   change, accept the displayed license when required, and choose Install exact
   optional pack for the staged YuNet catalog entry.
4. Select that exact local model in the prepared loadout and choose Test &
   activate. This checks that the provider can load and unload; it does not rate
   mouth-motion quality. Then, with the Cyberpunk window connected, choose Check
   and admit selected loadout. A blocked check is not a ready installation.
5. Before starting the short NPC-selection lease, open the matching character's
   mouth-pack workspace. Choose atlas.json and atlas-bgra8-premultiplied.bin from
   that character's directory under review-character-mouth-packs, review them,
   and import the pack. The receipt beside those files remains review evidence;
   it is not an import input. Keep that character selected, return to Games,
   choose Select NPC on screen, and then choose Apply to selected NPC. The sealed
   selection lease lasts at most 15 seconds; reselect the NPC before applying if
   it expires.
6. Provider credentials remain owner-local and separate from this package. On
   the already provisioned owner review profile, inspect the persisted connection
   and loadout status; rebuilding this package does not require re-entering keys.
   On another clean machine or profile, open Connect accounts and explicitly
   connect each hosted provider used by the voice and conversation loadout.
   Choose the model and stock voice you intend to review; charges and limits may
   apply.

This package contains no provider credential values and never imports them.
Owner-local review setup, when performed separately, stays in Windows Credential
Manager under interactive-npcs/v2/review and in the isolated review app state; it
is not part of this directory or REVIEW-MANIFEST.json.

The sibling review-mouth-atlas is bound by its reviewed-artifact receipt to
$($reviewedMouthAtlas.game_profile_id)/$($reviewedMouthAtlas.character_id). Its review classification is
$($reviewedMouthAtlas.classification); natural-quality qualification is $($reviewedMouthAtlas.natural_quality_qualified).
Its schema is $($reviewedMouthAtlas.schema_version). Its qualification boundary is:
$($reviewedMouthAtlas.qualification)
Ordinary game targets cannot select that atlas. Directly starting
local-app-data\test-game\interactive-npcs-synthetic-target.exe is an optional
standalone fixture check; it does not perform the review launch contract or
connect itself to the control app.

$modelCatalogReviewNote

$characterPackReviewNote

Build outcome: passed (fresh installer-free debug build). The packaging verifier
runs on the staged directory and again after its atomic move to the review root.
The generated REVIEW-MANIFEST.json binds every file, the full source identity
(clean in this build), the stable test game, the private atlas, and
review-evidence\engineering-review.md.

Still open: a permitted desktop/game-capture walkthrough, physical audio output,
mouth tracking performance under contention, and general mouth rendering for
ordinary moving targets. Headless synthesis and transport checks do not close
those gates. This private, unsigned review artifact was not launched or played
by the builder.
"@
    Write-Utf8NoBom -Path (Join-Path $stage 'REVIEW.md') -Content $readme

    $sourceProcess = Invoke-NpcHiddenProcess -FilePath $python.Source -ArgumentList @(
        (Join-Path $repoRoot 'scripts/security/generate_source_evidence.py'),
        '--root', $repoRoot, '--out', $sourceEvidencePath
    ) -WorkingDirectory $repoRoot -NoReplayOutput
    if ($sourceProcess.ExitCode -ne 0) { throw 'Staging-complete source identity generation failed.' }
    $sourceFinal = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
    if ([string]$sourceBefore.head_commit -ne [string]$sourceFinal.head_commit -or
        [string]$sourceBefore.source_candidate_digest.sha256 -ne [string]$sourceFinal.source_candidate_digest.sha256 -or
        [bool]$sourceBefore.dirty -ne [bool]$sourceFinal.dirty) {
        throw 'Source changed while the portable review was being built or staged.'
    }
    Copy-Item -LiteralPath $sourceEvidencePath `
        -Destination (Join-Path $reviewEvidence 'source-evidence.json') -Force

    $cyberpunkRuntimeProfileRelative = 'profiles/games/cyberpunk-2077/profile.json'
    $cyberpunkAuthoredPackRelative =
        'profiles/content-packs/local-review/cyberpunk-2077-authored-context-v1.pack.json'
    $cyberpunkRuntimeProfilePath = Join-Path $stage $cyberpunkRuntimeProfileRelative
    $cyberpunkAuthoredPackPath = Join-Path $stage $cyberpunkAuthoredPackRelative
    $cyberpunkRuntimeProfile = Get-Content -LiteralPath $cyberpunkRuntimeProfilePath -Raw | ConvertFrom-Json
    $cyberpunkAuthoredPack = Get-Content -LiteralPath $cyberpunkAuthoredPackPath -Raw | ConvertFrom-Json
    $runtimeCharacterIds = @($cyberpunkRuntimeProfile.characters | ForEach-Object { [string]$_.id })
    $packCharacterIds = @($cyberpunkAuthoredPack.profile.characters | ForEach-Object { [string]$_.id })
    $backgroundCharacterIds = @($runtimeCharacterIds | Where-Object { $_ -ceq 'night-city-resident' })
    if ([string]$cyberpunkRuntimeProfile.id -cne 'cyberpunk-2077' -or
        [string]$cyberpunkAuthoredPack.game_profile_id -cne 'cyberpunk-2077' -or
        [string]$cyberpunkAuthoredPack.version -cne '1.2.0' -or
        $runtimeCharacterIds.Count -ne 37 -or $packCharacterIds.Count -ne 37 -or
        ($runtimeCharacterIds -join "`n") -cne ($packCharacterIds -join "`n") -or
        $backgroundCharacterIds.Count -ne 1) {
        throw 'Packaged Cyberpunk runtime profile and authored corpus are not the reviewed 1.2.0 36+1 roster.'
    }

    $files = @(Get-ChildItem -LiteralPath $stage -File -Force -Recurse | Sort-Object FullName |
        ForEach-Object {
            [ordered]@{
                path = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
                size_bytes = [int64]$_.Length
                sha256 = Get-LowerSha256 -Path $_.FullName
            }
        })
    $directories = @(Get-ChildItem -LiteralPath $stage -Directory -Force -Recurse |
        ForEach-Object { $_.FullName.Substring($stage.Length + 1).Replace('\', '/') } |
        Sort-Object)
    $modelCatalogManifest = if ($null -ne $privateModelCatalog) {
        [ordered]@{
            scope = 'automated-local-review-bootstrap'
            runtime_root = 'packaging/model-packs'
            source_root = $privateModelCatalog
            verification_receipt = [ordered]@{
                relative_path = 'review-evidence/private-model-catalog-verification.json'
                sha256 = Get-LowerSha256 -Path (Join-Path $reviewEvidence 'private-model-catalog-verification.json')
            }
            import_context = [ordered]@{
                relative_path = 'review-evidence/yunet-import-context.json'
                sha256 = Get-LowerSha256 -Path (Join-Path $reviewEvidence 'yunet-import-context.json')
            }
            runtime_files = @($privateCatalogRuntimeFiles | ForEach-Object {
                $relative = [string]$_.path
                $path = Join-Path $stage ('packaging/model-packs/' + $relative)
                [ordered]@{
                    path = 'packaging/model-packs/' + $relative
                    size_bytes = [int64](Get-Item -LiteralPath $path).Length
                    sha256 = Get-LowerSha256 -Path $path
                }
            })
            production_trust = $false
            rotation_required_before_release = $true
            promotion_supported = $false
            publication_supported = $false
            model_or_runtime_payload_included = $false
            installed = $false
            activated = $false
            provider_load_self_test_attested = $false
        }
    }
    else {
        [ordered]@{
            scope = 'checked-repository-bootstrap'
            runtime_root = 'packaging/model-packs'
            source_root = Join-Path $repoRoot 'packaging/model-packs'
        }
    }
    $manifest = [ordered]@{
        schema = 'interactive-npcs-portable-review/v1'
        created_at_utc = [DateTime]::UtcNow.ToString('o')
        classification = 'source-identified-local-review'
        review_root = $destination
        source = $sourceFinal
        application = [ordered]@{
            relative_path = 'interactive-npcs-control.exe'
            absolute_path = Join-Path $destination 'interactive-npcs-control.exe'
            size_bytes = [int64](Get-Item -LiteralPath (Join-Path $stage 'interactive-npcs-control.exe')).Length
            sha256 = Get-LowerSha256 -Path (Join-Path $stage 'interactive-npcs-control.exe')
            build_configuration = 'Debug'
            bundle_created = $false
        }
        application_identifier = 'io.github.akshitireddy.interactive-npcs.review'
        installer_used = $false
        launched_by_builder = $false
        published = $false
        updater_enabled = $false
        signed = $false
        checks = [ordered]@{
            source_hygiene = $true
            current_source_and_untracked_secret_scan = $true
            reachable_history_secret_scan = $false
            strict_license_provenance = $true
            acceptance_evidence_consistent = $true
            gui_subsystem_audit = $true
        }
        test_game = [ordered]@{
            relative_path = 'local-app-data/test-game/interactive-npcs-synthetic-target.exe'
            absolute_path = Join-Path $destination 'local-app-data/test-game/interactive-npcs-synthetic-target.exe'
            sha256 = [string]$testGame.executable_sha256
            source_sha256 = [string]$testGame.source_sha256
            distribution_manifest_sha256 = Get-LowerSha256 -Path $testGameManifestPath
            visual_source = [string]$testGameManifest.generated_media.visual_source
            sequence = [ordered]@{
                relative_path = 'local-app-data/test-game/' + [string]$sequenceEntry.path
                sha256 = [string]$sequenceEntry.sha256
                size_bytes = [int64]$sequenceEntry.size_bytes
            }
        }
        mouth_atlas = [ordered]@{
            relative_root = 'review-mouth-atlas'
            scope = [string]$reviewedMouthAtlas.classification
            qualification = [string]$reviewedMouthAtlas.qualification
            review_status = [string]$reviewedMouthAtlas.review_status
            natural_quality_qualified = [bool]$reviewedMouthAtlas.natural_quality_qualified
            ordinary_targets_enabled = [bool]$reviewedMouthAtlas.ordinary_targets_enabled
            model_weights_included = $false
            schema_version = [uint32]$reviewedMouthAtlas.schema_version
            identity_revision = [string]$reviewedMouthAtlas.identity_revision
            representation = [string]$reviewedMouthAtlas.representation
            game_profile_id = [string]$reviewedMouthAtlas.game_profile_id
            character_id = [string]$reviewedMouthAtlas.character_id
            enrollment_binding_sha256 = $reviewedMouthAtlas.enrollment_binding_sha256
            review_evidence_sha256 = [string]$reviewedMouthAtlas.review_evidence_sha256
            receipt = [ordered]@{
                relative_path = 'review-evidence/reviewed-mouth-atlas-receipt.v1.json'
                sha256 = Get-LowerSha256 -Path $mouthAtlasReceiptTarget
            }
            files = @($atlasPayloadFiles | Sort-Object | ForEach-Object {
                $atlasPath = Join-Path $atlasTarget $_
                [ordered]@{
                    path = 'review-mouth-atlas/' + $_
                    size_bytes = [int64](Get-Item -LiteralPath $atlasPath).Length
                    sha256 = Get-LowerSha256 -Path $atlasPath
                }
            })
        }
        character_mouth_packs = @($stagedCharacterMouthPacks | ForEach-Object {
            $stagedPack = $_
            $sourcePack = $stagedPack.source
            [ordered]@{
                relative_root = [string]$stagedPack.relative_root
                receipt_path = ([string]$stagedPack.relative_root + '/' +
                    [System.IO.Path]::GetFileName([string]$stagedPack.receipt))
                receipt_sha256 = Get-LowerSha256 -Path ([string]$stagedPack.receipt)
                schema_version = [uint32]$sourcePack.schema_version
                identity_revision = [string]$sourcePack.identity_revision
                representation = [string]$sourcePack.representation
                game_profile_id = [string]$sourcePack.game_profile_id
                character_id = [string]$sourcePack.character_id
                enrollment_binding_sha256 = [string]$sourcePack.enrollment_binding_sha256
                review_evidence_sha256 = [string]$sourcePack.review_evidence_sha256
                natural_quality_qualified = [bool]$sourcePack.natural_quality_qualified
                enabled = $false
                files = @(Get-ChildItem -LiteralPath ([string]$stagedPack.root) -File -Force |
                    Sort-Object Name | ForEach-Object {
                        [ordered]@{
                            path = ([string]$stagedPack.relative_root + '/' + $_.Name)
                            size_bytes = [int64]$_.Length
                            sha256 = Get-LowerSha256 -Path $_.FullName
                        }
                    })
            }
        })
        content_corpus = [ordered]@{
            game_profile_id = 'cyberpunk-2077'
            version = [string]$cyberpunkAuthoredPack.version
            named_character_count = 36
            background_character_count = 1
            total_character_count = $runtimeCharacterIds.Count
            background_character_ids = @($backgroundCharacterIds)
            exact_character_id_order_match = $true
            runtime_load_mode = 'bundled-profile-resource'
            runtime_profile = [ordered]@{
                relative_path = $cyberpunkRuntimeProfileRelative
                sha256 = Get-LowerSha256 -Path $cyberpunkRuntimeProfilePath
            }
            authored_pack = [ordered]@{
                relative_path = $cyberpunkAuthoredPackRelative
                sha256 = Get-LowerSha256 -Path $cyberpunkAuthoredPackPath
            }
        }
        engineering_review = [ordered]@{
            relative_path = 'review-evidence/engineering-review.md'
            source_path = $engineeringReviewSource
            source_sha256 = $engineeringReviewSourceHash
            portable_sha256 = Get-LowerSha256 -Path $engineeringReviewTarget
            local_links_rewritten_to_absolute_source_paths = $true
        }
        model_catalog = $modelCatalogManifest
        outcomes = [ordered]@{
            fresh_installer_free_build = 'passed'
            stable_game_prerequisite_verification = 'passed'
            private_mouth_atlas_prerequisite_verification = 'passed'
            staged_closed_world_verification = 'pending'
            final_destination_verification = 'pending'
            desktop_or_game_capture = 'not-performed-prohibited'
            physical_audio_playback = 'not-performed-prohibited'
            generic_moving_target_mouth_gate = 'open'
            moving_tracking_under_contention_gate = 'open'
        }
        manifest_self = [ordered]@{ path = 'REVIEW-MANIFEST.json'; hash = 'excluded-to-avoid-circularity' }
        directories = $directories
        files = $files
    }
    Write-Utf8NoBom -Path (Join-Path $stage 'REVIEW-MANIFEST.json') `
        -Content (($manifest | ConvertTo-Json -Depth 12) + [Environment]::NewLine)

    $verificationJson = & (Join-Path $PSScriptRoot 'verify-portable-review.ps1') `
        -Directory $stage -ArtifactRoot $artifactRootFull -AllowPendingOutcome
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $verification = $verificationJson | ConvertFrom-Json
    if ($verification.status -ne 'passed') { throw 'Staged portable review verification failed.' }
    $manifest.outcomes.staged_closed_world_verification = 'passed'
    Write-Utf8NoBom -Path (Join-Path $stage 'REVIEW-MANIFEST.json') `
        -Content (($manifest | ConvertTo-Json -Depth 12) + [Environment]::NewLine)
    $verificationJson = & (Join-Path $PSScriptRoot 'verify-portable-review.ps1') `
        -Directory $stage -ArtifactRoot $artifactRootFull -AllowPendingOutcome
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $verification = $verificationJson | ConvertFrom-Json
    if ($verification.status -ne 'passed') { throw 'Receipt-bound staged verification failed.' }
    $freezeCheckPath = Join-Path $evidenceWork 'source-freeze-check.json'
    $sourceProcess = Invoke-NpcHiddenProcess -FilePath $python.Source -ArgumentList @(
        (Join-Path $repoRoot 'scripts/security/generate_source_evidence.py'),
        '--root', $repoRoot, '--out', $freezeCheckPath
    ) -WorkingDirectory $repoRoot -NoReplayOutput
    if ($sourceProcess.ExitCode -ne 0) { throw 'Pre-move source freeze verification failed.' }
    $freezeCheck = Get-Content -LiteralPath $freezeCheckPath -Raw | ConvertFrom-Json
    if ([string]$sourceFinal.head_commit -ne [string]$freezeCheck.head_commit -or
        [string]$sourceFinal.source_candidate_digest.sha256 -ne
            [string]$freezeCheck.source_candidate_digest.sha256 -or
        [bool]$sourceFinal.dirty -ne [bool]$freezeCheck.dirty) {
        throw 'Source changed after staging and before the portable review move.'
    }
    if (Test-Path -LiteralPath $destination) {
        throw "Portable review destination appeared during the build; refusing to merge into it: $destination"
    }
    Move-Item -LiteralPath $stage -Destination $destination
    $stage = $null
    $verificationJson = & (Join-Path $PSScriptRoot 'verify-portable-review.ps1') `
        -Directory $destination -ArtifactRoot $artifactRootFull -AllowPendingOutcome
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $verification = $verificationJson | ConvertFrom-Json
    if ($verification.status -ne 'passed') { throw 'Final portable review verification failed.' }
    $manifest.outcomes.final_destination_verification = 'passed'
    Write-Utf8NoBom -Path (Join-Path $destination 'REVIEW-MANIFEST.json') `
        -Content (($manifest | ConvertTo-Json -Depth 12) + [Environment]::NewLine)
    $verificationJson = & (Join-Path $PSScriptRoot 'verify-portable-review.ps1') `
        -Directory $destination -ArtifactRoot $artifactRootFull
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $verification = $verificationJson | ConvertFrom-Json
    if ($verification.status -ne 'passed') { throw 'Receipt-bound final verification failed.' }
    $freshVerificationProcess = Invoke-NpcHiddenProcess -FilePath $pwsh.Source -ArgumentList @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-File',
        (Join-Path $PSScriptRoot 'verify-portable-review.ps1'),
        '-Directory', $destination, '-ArtifactRoot', $artifactRootFull
    ) -WorkingDirectory $repoRoot -NoReplayOutput
    if ($freshVerificationProcess.ExitCode -ne 0) {
        throw "Fresh-process final verification failed: $($freshVerificationProcess.StandardError)"
    }
    try {
        $freshVerification = $freshVerificationProcess.StandardOutput | ConvertFrom-Json
    }
    catch {
        throw "Fresh-process verifier stdout was not one JSON document: $($_.Exception.Message)"
    }
    if ($freshVerification.status -ne 'passed' -or
        [string]$freshVerification.manifest_sha256 -ne [string]$verification.manifest_sha256) {
        throw 'Fresh-process final verification disagreed with in-process verification.'
    }
    $freshVerification | ConvertTo-Json -Depth 6
}
finally {
    if (Test-Path -LiteralPath $generatedResources) {
        Remove-VerifiedReviewBuildDirectory -Path $generatedResources `
            -Boundary $repoRoot -ExpectedLeafPattern 'generated-resources'
    }
    if ($null -ne $stage -and (Test-Path -LiteralPath $stage)) {
        Remove-VerifiedReviewBuildDirectory -Path $stage `
            -Boundary $artifactRootFull -ExpectedLeafPattern '.portable-review-stage-*'
    }
    if (Test-Path -LiteralPath $evidenceWork) {
        Remove-VerifiedReviewBuildDirectory -Path $evidenceWork `
            -Boundary $artifactRootFull -ExpectedLeafPattern '.portable-review-evidence-*'
    }
}
