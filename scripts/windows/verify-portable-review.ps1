[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Directory,
    [string]$ArtifactRoot = 'E:\temp\InteractiveNPCs',
    [switch]$AllowPendingOutcome
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
. (Join-Path $PSScriptRoot 'reviewed-mouth-atlas-receipt.ps1')
$root = [System.IO.Path]::GetFullPath($Directory).TrimEnd('\')
$allowedRoot = [System.IO.Path]::GetFullPath($ArtifactRoot).TrimEnd('\')
if (-not $root.StartsWith("$allowedRoot\", [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Portable review root must be below $allowedRoot."
}
if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw "Portable review root is missing: $root" }

function Get-LowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

$items = @(Get-ChildItem -LiteralPath $root -Force -Recurse)
foreach ($item in @((Get-Item -LiteralPath $root -Force)) + $items) {
    if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw "Portable review must not contain reparse points: $($item.FullName)"
    }
}

$manifestPath = Join-Path $root 'REVIEW-MANIFEST.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'REVIEW-MANIFEST.json is missing.' }
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.schema -ne 'interactive-npcs-portable-review/v1' -or
    $manifest.classification -ne 'source-identified-local-review' -or
    $manifest.installer_used -ne $false -or $manifest.launched_by_builder -ne $false -or
    $manifest.published -ne $false -or $manifest.updater_enabled -ne $false) {
    throw 'Portable review manifest boundary is invalid.'
}
if ($manifest.manifest_self.path -ne 'REVIEW-MANIFEST.json' -or
    $manifest.manifest_self.hash -ne 'excluded-to-avoid-circularity') {
    throw 'Portable review self-hash policy is invalid.'
}
$recordedReviewRoot = [System.IO.Path]::GetFullPath([string]$manifest.review_root).TrimEnd('\')
if (-not $recordedReviewRoot.StartsWith("$allowedRoot\", [System.StringComparison]::OrdinalIgnoreCase) -or
    (-not $AllowPendingOutcome -and
        -not [string]::Equals($recordedReviewRoot, $root, [System.StringComparison]::OrdinalIgnoreCase))) {
    throw 'Portable review recorded destination is invalid.'
}
if ($manifest.outcomes.fresh_installer_free_build -ne 'passed' -or
    $manifest.outcomes.stable_game_prerequisite_verification -ne 'passed' -or
    $manifest.outcomes.private_mouth_atlas_prerequisite_verification -ne 'passed') {
    throw 'Portable review build or prerequisite outcome is invalid.'
}
if ($AllowPendingOutcome) {
    if (@('pending', 'passed') -notcontains [string]$manifest.outcomes.staged_closed_world_verification -or
        @('pending', 'passed') -notcontains [string]$manifest.outcomes.final_destination_verification) {
        throw 'Portable review pending verification outcome is invalid.'
    }
}
elseif ($manifest.outcomes.staged_closed_world_verification -ne 'passed' -or
    $manifest.outcomes.final_destination_verification -ne 'passed') {
    throw 'Portable review has not recorded both closed-world verification passes.'
}

$entries = @($manifest.files)
$expected = @($entries | ForEach-Object { [string]$_.path } | Sort-Object)
$actual = @(Get-ChildItem -LiteralPath $root -File -Force -Recurse |
        ForEach-Object { $_.FullName.Substring($root.Length + 1).Replace('\', '/') } |
        Where-Object { $_ -ne 'REVIEW-MANIFEST.json' } | Sort-Object)
if (($expected -join "`n") -cne ($actual -join "`n") -or $expected.Count -ne (@($expected | Select-Object -Unique)).Count) {
    throw 'Portable review file allowlist is incomplete, duplicated, or contains unknown files.'
}
if ($manifest.PSObject.Properties.Name -contains 'directories') {
    $expectedDirectories = @($manifest.directories | ForEach-Object { [string]$_ } | Sort-Object)
    $actualDirectories = @(Get-ChildItem -LiteralPath $root -Directory -Force -Recurse |
            ForEach-Object { $_.FullName.Substring($root.Length + 1).Replace('\', '/') } | Sort-Object)
    if (($expectedDirectories -join "`n") -cne ($actualDirectories -join "`n") -or
        $expectedDirectories.Count -ne @($expectedDirectories | Select-Object -Unique).Count) {
        throw 'Portable review directory allowlist contains a missing, duplicated, or unknown directory.'
    }
    foreach ($relativeDirectory in $expectedDirectories) {
        if ([string]::IsNullOrWhiteSpace($relativeDirectory) -or
            [System.IO.Path]::IsPathRooted($relativeDirectory) -or
            $relativeDirectory.Contains('..')) {
            throw "Portable review contains an unsafe directory entry: $relativeDirectory"
        }
    }
}
foreach ($entry in $entries) {
    $relative = [string]$entry.path
    if ([System.IO.Path]::IsPathRooted($relative) -or $relative.Contains('..') -or
        [string]$entry.sha256 -cnotmatch '^[0-9a-f]{64}$' -or [int64]$entry.size_bytes -lt 0) {
        throw "Portable review contains an unsafe manifest entry: $relative"
    }
    $path = Join-Path $root $relative
    if ((Get-LowerSha256 -Path $path) -cne [string]$entry.sha256 -or
        (Get-Item -LiteralPath $path).Length -ne [int64]$entry.size_bytes) {
        throw "Portable review hash or size mismatch: $relative"
    }
}
foreach ($file in @($items | Where-Object { -not $_.PSIsContainer })) {
    $alternateStreams = @(Get-Item -LiteralPath $file.FullName -Stream * |
            Where-Object { $_.Stream -ne ':$DATA' })
    if ($alternateStreams.Count -ne 0) {
        throw "Portable review contains an alternate data stream: $($file.FullName)"
    }
}

$requiredExecutables = @(
    'interactive-npcs-control.exe', 'npc-runtime.exe', 'npc-media-broker.exe',
    'npc-mouth-worker.exe', 'npc-subtitle-presenter.exe',
    'local-app-data/test-game/interactive-npcs-synthetic-target.exe'
) | Sort-Object
$actualExecutables = @($actual | Where-Object { [System.IO.Path]::GetExtension($_) -ieq '.exe' } | Sort-Object)
if (($requiredExecutables -join "`n") -cne ($actualExecutables -join "`n")) {
    throw 'Portable review executable allowlist is invalid.'
}
$applicationPath = Join-Path $root 'interactive-npcs-control.exe'
if ([string]$manifest.application.relative_path -ne 'interactive-npcs-control.exe' -or
    [string]$manifest.application.absolute_path -ne (Join-Path $recordedReviewRoot 'interactive-npcs-control.exe') -or
    [string]$manifest.application.build_configuration -ne 'Debug' -or
    [bool]$manifest.application.bundle_created -ne $false -or
    (Get-LowerSha256 -Path $applicationPath) -cne [string]$manifest.application.sha256 -or
    (Get-Item -LiteralPath $applicationPath).Length -ne [int64]$manifest.application.size_bytes) {
    throw 'Portable review application identity is invalid.'
}
if ([string]$manifest.test_game.absolute_path -ne
        (Join-Path $recordedReviewRoot 'local-app-data/test-game/interactive-npcs-synthetic-target.exe')) {
    throw 'Portable review test-game absolute path is invalid.'
}
$blockedExtensions = @('.onnx', '.pt', '.pth', '.safetensors', '.gguf', '.bin')
$permittedPrivateAtlasBinaries = @($manifest.mouth_atlas.files |
    ForEach-Object { [string]$_.path } |
    Where-Object {
        $_.StartsWith('review-mouth-atlas/', [System.StringComparison]::Ordinal) -and
        [System.IO.Path]::GetExtension($_) -ceq '.bin'
    })
$characterMouthPackEntries = @(if (
        $manifest.PSObject.Properties.Name -contains 'character_mouth_packs') {
        @($manifest.character_mouth_packs)
    })
foreach ($pack in $characterMouthPackEntries) {
    $packBinaries = @($pack.files | ForEach-Object { [string]$_.path } | Where-Object {
            $_.StartsWith('review-character-mouth-packs/', [System.StringComparison]::Ordinal) -and
            [System.IO.Path]::GetExtension($_) -ceq '.bin'
        })
    if ($packBinaries.Count -ne 1) {
        throw 'Each reviewed character mouth pack must bind exactly one texture binary.'
    }
    $permittedPrivateAtlasBinaries += $packBinaries
}
if ($permittedPrivateAtlasBinaries.Count -ne (1 + $characterMouthPackEntries.Count)) {
    throw 'Portable review private mouth-atlas binary allowlist is invalid.'
}
if (@($actual | Where-Object {
            $blockedExtensions -contains [System.IO.Path]::GetExtension($_).ToLowerInvariant() -and
            $_ -cnotin $permittedPrivateAtlasBinaries
        }).Count -ne 0) {
    throw 'Portable base review contains model weights or binary model payloads.'
}
if (@($actual | Where-Object { [System.IO.Path]::GetFileName($_) -match '^(?i:python|ffmpeg|ffprobe|nvcc)(?:\.exe)?$' }).Count -ne 0) {
    throw 'Portable base review contains a blocked runtime or codec executable.'
}
$blockedRuntimeExtensions = @(
    '.dll', '.pyd', '.so', '.dylib', '.lib', '.a', '.py', '.pyc', '.pyo',
    '.whl', '.zip', '.7z', '.tar', '.gz'
)
if (@($actual | Where-Object {
            $blockedRuntimeExtensions -contains [System.IO.Path]::GetExtension($_).ToLowerInvariant()
        }).Count -ne 0) {
    throw 'Portable base review contains a blocked runtime, native library, script, or archive payload.'
}

if ($manifest.PSObject.Properties.Name -contains 'model_catalog') {
    if ([string]$manifest.model_catalog.runtime_root -ne 'packaging/model-packs') {
        throw 'Portable review model-catalog runtime root is invalid.'
    }
    if ([string]$manifest.model_catalog.scope -eq 'automated-local-review-bootstrap') {
        if ([bool]$manifest.model_catalog.production_trust -or
            -not [bool]$manifest.model_catalog.rotation_required_before_release -or
            [bool]$manifest.model_catalog.promotion_supported -or
            [bool]$manifest.model_catalog.publication_supported -or
            [bool]$manifest.model_catalog.model_or_runtime_payload_included -or
            [bool]$manifest.model_catalog.installed -or [bool]$manifest.model_catalog.activated -or
            [bool]$manifest.model_catalog.provider_load_self_test_attested) {
            throw 'Portable review private model-catalog boundary is invalid.'
        }
        $catalogRoot = Join-Path $root 'packaging/model-packs'
        $recordedCatalogFiles = @($manifest.model_catalog.runtime_files)
        if ($recordedCatalogFiles.Count -ne 4) {
            throw 'Portable review private model-catalog manifest must bind four runtime files.'
        }
        $expectedCatalogFiles = @($recordedCatalogFiles | ForEach-Object {
                $relative = [string]$_.path
                if (-not $relative.StartsWith('packaging/model-packs/',
                        [System.StringComparison]::Ordinal)) {
                    throw "Portable review private model-catalog path escaped its runtime root: $relative"
                }
                $relative.Substring('packaging/model-packs/'.Length)
            } | Sort-Object)
        foreach ($required in @(
                'model-catalog-root-v1.json',
                'model-catalog-v1.json',
                'openseeface-yunet640-lm1-mouth-signal.json'
            )) {
            if ($required -notin $expectedCatalogFiles) {
                throw "Portable review private model-catalog is missing runtime metadata: $required"
            }
        }
        $qualifiedCatalogFiles = @($expectedCatalogFiles | Where-Object {
                $_ -cmatch '^qual/[0-9a-f]{64}\.json$'
            })
        if ($qualifiedCatalogFiles.Count -ne 1 -or
            @($expectedCatalogFiles | Select-Object -Unique).Count -ne 4) {
            throw 'Portable review private model-catalog must bind one digest-addressed envelope.'
        }
        $actualCatalogFiles = @(Get-ChildItem -LiteralPath $catalogRoot -File -Force -Recurse |
                ForEach-Object { $_.FullName.Substring($catalogRoot.Length + 1).Replace('\', '/') } |
                Sort-Object)
        if (($expectedCatalogFiles -join "`n") -cne ($actualCatalogFiles -join "`n")) {
            throw 'Portable review private model-catalog loader root is not the four-file closed world.'
        }
        foreach ($entry in $recordedCatalogFiles) {
            $path = Join-Path $root ([string]$entry.path)
            if ((Get-LowerSha256 -Path $path) -cne [string]$entry.sha256 -or
                (Get-Item -LiteralPath $path).Length -ne [int64]$entry.size_bytes) {
                throw "Portable review private model-catalog file disagrees with manifest: $($entry.path)"
            }
        }
        $receiptPath = Join-Path $root ([string]$manifest.model_catalog.verification_receipt.relative_path)
        if ([string]$manifest.model_catalog.verification_receipt.relative_path -ne
                'review-evidence/private-model-catalog-verification.json' -or
            (Get-LowerSha256 -Path $receiptPath) -cne
                [string]$manifest.model_catalog.verification_receipt.sha256) {
            throw 'Portable review private model-catalog verification receipt is invalid.'
        }
        $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
        if ($receipt.schema -ne 'interactive-npcs-private-catalog-verification/v1' -or
            -not [bool]$receipt.verification.catalogBundlePublicApi -or
            -not [bool]$receipt.verification.manifestFilesPublicApi -or
            -not [bool]$receipt.verification.qualifiedEnvelopeFilesPublicApi -or
            -not [bool]$receipt.verification.trustedPackSnapshotPublicApi -or
            $receipt.catalog.trustScope -ne 'automated_local_review_bootstrap' -or
            [int]$receipt.catalog.signatureThreshold -ne 2 -or [int]$receipt.catalog.keyCount -ne 2 -or
            [bool]$receipt.catalog.productionTrust -or
            -not [bool]$receipt.catalog.rotationRequiredBeforeRelease -or
            [bool]$receipt.catalog.promotionSupported -or [bool]$receipt.catalog.publicationSupported -or
            $receipt.pack.id -ne 'openseeface-yunet640-lm1-mouth-signal' -or
            $receipt.pack.revision -ne '85aa70fc67582d046e771ea73625182a0d8f7475' -or
            -not [bool]$receipt.pack.nativeInferenceQualified -or
            [bool]$receipt.pack.providerLoadSelfTestAttested -or
            [bool]$receipt.pack.installed -or [bool]$receipt.pack.activated -or
            [bool]$receipt.signerPrivateMaterialPersisted) {
            throw 'Portable review private model-catalog verified state is invalid.'
        }
        foreach ($entry in @($receipt.runtimeFiles)) {
            $path = Join-Path $catalogRoot ([string]$entry.path)
            if ((Get-LowerSha256 -Path $path) -cne [string]$entry.sha256) {
                throw "Portable review private model-catalog disagrees with verified receipt: $($entry.path)"
            }
        }
        $importContextPath = Join-Path $root ([string]$manifest.model_catalog.import_context.relative_path)
        if ([string]$manifest.model_catalog.import_context.relative_path -ne
                'review-evidence/yunet-import-context.json' -or
            (Get-LowerSha256 -Path $importContextPath) -cne [string]$receipt.auditEvidence.sha256 -or
            (Get-LowerSha256 -Path $importContextPath) -cne
                [string]$manifest.model_catalog.import_context.sha256) {
            throw 'Portable review private model-catalog import context is invalid.'
        }
        $liveVerification = & (Join-Path $repoRoot `
                'scripts/windows/verify-private-review-model-catalog.ps1') `
            -Directory $catalogRoot -ReceiptPath $receiptPath `
            -ImportContextPath $importContextPath | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or $liveVerification.status -ne 'passed' -or
            [string]$liveVerification.receiptSha256 -cne
                [string]$manifest.model_catalog.verification_receipt.sha256) {
            throw 'Portable review private model-catalog failed live cryptographic verification.'
        }
        $catalogTrustRoot = Get-Content -LiteralPath (Join-Path $catalogRoot 'model-catalog-root-v1.json') -Raw |
            ConvertFrom-Json
        $catalog = Get-Content -LiteralPath (Join-Path $catalogRoot 'model-catalog-v1.json') -Raw |
            ConvertFrom-Json
        $qualifiedEnvelope = Get-Content -LiteralPath (Join-Path $catalogRoot `
                $qualifiedCatalogFiles[0]) -Raw |
            ConvertFrom-Json
        if ($catalogTrustRoot.schema -ne 'npc.model-catalog-root/v1' -or
            $catalogTrustRoot.trustScope -ne 'automated_local_review_bootstrap' -or
            [bool]$catalogTrustRoot.productionTrust -or
            -not [bool]$catalogTrustRoot.rotationRequiredBeforeRelease -or
            [bool]$catalogTrustRoot.promotionSupported -or [bool]$catalogTrustRoot.publicationSupported -or
            [int]$catalogTrustRoot.signatureThreshold -ne 2 -or @($catalogTrustRoot.keys).Count -ne 2 -or
            $catalog.signed.schema -ne 'npc.model-catalog/v1' -or @($catalog.signed.entries).Count -ne 1 -or
            [int64]$catalog.signed.expires_unix_seconds -le [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() -or
            @($catalog.signatures).Count -ne 2 -or @($catalog.source_inventory_signatures).Count -ne 2 -or
            $qualifiedEnvelope.signed.schema -ne 'npc.measured-resource-envelope/v1' -or
            $qualifiedEnvelope.signed.identity.pack_id -ne 'openseeface-yunet640-lm1-mouth-signal' -or
            $qualifiedEnvelope.signed.identity.revision -ne '85aa70fc67582d046e771ea73625182a0d8f7475' -or
            [int]$qualifiedEnvelope.signed.sample_count -lt 20 -or
            [int64]$qualifiedEnvelope.signed.expires_unix_seconds -le
                [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() -or @($qualifiedEnvelope.signatures).Count -ne 2) {
            throw 'Portable review private model-catalog runtime metadata is stale or violates its trust contract.'
        }
    }
    elseif ([string]$manifest.model_catalog.scope -ne 'checked-repository-bootstrap') {
        throw 'Portable review model-catalog scope is unknown.'
    }
}

foreach ($relative in @($requiredExecutables | Where-Object { $_ -notlike 'local-app-data/*' })) {
    & (Join-Path $repoRoot 'scripts/audit-pe-product-binary.ps1') `
        -Path (Join-Path $root $relative) -ExpectedSubsystem Gui | Out-Null
}
& (Join-Path $repoRoot 'scripts/assert-pe-subsystem.ps1') `
    -Path (Join-Path $root 'local-app-data/test-game/interactive-npcs-synthetic-target.exe') `
    -Expected Gui | Out-Null

$sidecarManifest = Get-Content -LiteralPath (Join-Path $root 'review-evidence/sidecar-manifest.v1.json') -Raw |
    ConvertFrom-Json
if ($sidecarManifest.schema -ne 'interactive-npcs-sidecars/v1' -or @($sidecarManifest.binaries).Count -ne 4) {
    throw 'Portable review sidecar evidence is invalid.'
}
foreach ($binary in @($sidecarManifest.binaries)) {
    $portableName = ([string]$binary.file_name).Replace('-x86_64-pc-windows-msvc', '')
    if ((Get-LowerSha256 -Path (Join-Path $root $portableName)) -cne [string]$binary.sha256) {
        throw "Portable review sidecar disagrees with build evidence: $portableName"
    }
}

$resourceManifestPath = Join-Path $root 'review-evidence/resource-manifest.v1.json'
$resourceManifest = Get-Content -LiteralPath $resourceManifestPath -Raw | ConvertFrom-Json
if ($resourceManifest.schema -ne 'interactive-npcs-product-resources/v1' -or
    $resourceManifest.install_root -ne 'product-audit') {
    throw 'Portable review product-resource evidence is invalid.'
}
$productAuditRoot = Join-Path $root 'product-audit'
$expectedResources = @($resourceManifest.files | ForEach-Object { [string]$_.destination } | Sort-Object)
$actualResources = @(Get-ChildItem -LiteralPath $productAuditRoot -File -Force -Recurse |
        ForEach-Object { $_.FullName.Substring($productAuditRoot.Length + 1).Replace('\', '/') } |
        Where-Object { $_ -ne 'audit/resource-manifest.v1.json' } | Sort-Object)
if (($expectedResources -join "`n") -cne ($actualResources -join "`n")) {
    throw 'Portable review product-audit resource allowlist is invalid.'
}
foreach ($resource in @($resourceManifest.files)) {
    $path = Join-Path $productAuditRoot ([string]$resource.destination)
    if ((Get-LowerSha256 -Path $path) -cne [string]$resource.sha256 -or
        (Get-Item -LiteralPath $path).Length -ne [int64]$resource.size_bytes) {
        throw "Portable review product resource disagrees with build evidence: $($resource.destination)"
    }
}
$installedResourceManifest = Join-Path $productAuditRoot 'audit/resource-manifest.v1.json'
if ((Get-LowerSha256 -Path $installedResourceManifest) -cne (Get-LowerSha256 -Path $resourceManifestPath)) {
    throw 'Portable review installed resource manifest disagrees with review evidence.'
}

$sourceEvidencePath = Join-Path $root 'review-evidence/source-evidence.json'
$recordedSource = Get-Content -LiteralPath $sourceEvidencePath -Raw | ConvertFrom-Json
if ([string]$recordedSource.source_candidate_digest.sha256 -ne [string]$manifest.source.source_candidate_digest.sha256 -or
    [string]$recordedSource.head_commit -ne [string]$manifest.source.head_commit -or
    [bool]$recordedSource.dirty -ne [bool]$manifest.source.dirty -or
    @($recordedSource.working_tree_changes).Count -ne @($manifest.source.working_tree_changes).Count) {
    throw 'Portable review source identity disagrees with its evidence file.'
}

$testGameRoot = Join-Path $root 'local-app-data/test-game'
$testGameJson = & (Join-Path $PSScriptRoot 'verify-review-test-game.ps1') -Directory $testGameRoot
$testGame = $testGameJson | ConvertFrom-Json
if ($testGame.status -ne 'passed' -or $testGame.file_count -ne 6 -or
    $testGame.third_party_binary_count -ne 0 -or
    $testGame.visual_source -ne 'verified-sibling-project-owned-mara-camera-sequence-v1') {
    throw 'Portable review test game failed provenance verification.'
}
$fixtureManifestPath = Join-Path $testGameRoot 'REVIEW-FIXTURE-MANIFEST.json'
$fixtureManifest = Get-Content -LiteralPath $fixtureManifestPath -Raw | ConvertFrom-Json
$sequenceEntries = @($fixtureManifest.files | Where-Object {
        [System.IO.Path]::GetExtension([string]$_.path) -ieq '.inpcseq'
    })
if ($sequenceEntries.Count -ne 1 -or
    (Get-LowerSha256 -Path $fixtureManifestPath) -cne [string]$manifest.test_game.distribution_manifest_sha256 -or
    [string]$manifest.test_game.visual_source -ne [string]$testGame.visual_source) {
    throw 'Portable review camera-sequence manifest binding is invalid.'
}
$sequenceEntry = $sequenceEntries[0]
$sequencePath = Join-Path $root ([string]$manifest.test_game.sequence.relative_path)
if ([string]$manifest.test_game.sequence.relative_path -ne
        ('local-app-data/test-game/' + [string]$sequenceEntry.path) -or
    [string]$manifest.test_game.sequence.sha256 -ne [string]$sequenceEntry.sha256 -or
    [int64]$manifest.test_game.sequence.size_bytes -ne [int64]$sequenceEntry.size_bytes -or
    (Get-LowerSha256 -Path $sequencePath) -cne [string]$sequenceEntry.sha256) {
    throw 'Portable review camera-sequence file binding is invalid.'
}
if (@($actual | Where-Object { [System.IO.Path]::GetExtension($_) -ieq '.inpcseq' }).Count -ne 1) {
    throw 'Portable review must contain exactly one verified camera-sequence payload.'
}

$atlasRoot = Join-Path $root 'review-mouth-atlas'
$actualAtlasFiles = @(Get-ChildItem -LiteralPath $atlasRoot -File -Force -Recurse |
    ForEach-Object { $_.FullName.Substring($atlasRoot.Length + 1).Replace('\', '/') } | Sort-Object)
$recordedAtlasFiles = @($manifest.mouth_atlas.files)
if ($recordedAtlasFiles.Count -ne 2) { throw 'Portable review mouth-atlas file allowlist is invalid.' }
foreach ($entry in $recordedAtlasFiles) {
    $path = Join-Path $root ([string]$entry.path)
    if ((Get-LowerSha256 -Path $path) -cne [string]$entry.sha256 -or
        (Get-Item -LiteralPath $path).Length -ne [int64]$entry.size_bytes) {
        throw "Portable review mouth atlas disagrees with manifest: $($entry.path)"
    }
}
if ($manifest.mouth_atlas.PSObject.Properties.Name -contains 'receipt') {
    $receiptPath = Join-Path $root ([string]$manifest.mouth_atlas.receipt.relative_path)
    if ([string]$manifest.mouth_atlas.receipt.relative_path -ne
            'review-evidence/reviewed-mouth-atlas-receipt.v1.json' -or
        (Get-LowerSha256 -Path $receiptPath) -cne [string]$manifest.mouth_atlas.receipt.sha256) {
        throw 'Portable review mouth-atlas receipt binding is invalid.'
    }
    $reviewedAtlas = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $receiptPath `
        -ArtifactRoot $atlasRoot
    $expectedAtlasFiles = @(
        [string]$reviewedAtlas.atlas_file_name,
        [string]$reviewedAtlas.texture_file_name
    ) | Sort-Object
    if (($actualAtlasFiles -join "`n") -cne ($expectedAtlasFiles -join "`n") -or
        [string]$manifest.mouth_atlas.scope -ne [string]$reviewedAtlas.classification -or
        [string]$manifest.mouth_atlas.qualification -ne [string]$reviewedAtlas.qualification -or
        [string]$manifest.mouth_atlas.review_status -ne [string]$reviewedAtlas.review_status -or
        [bool]$manifest.mouth_atlas.natural_quality_qualified -ne
            [bool]$reviewedAtlas.natural_quality_qualified -or
        [bool]$manifest.mouth_atlas.ordinary_targets_enabled -ne
            [bool]$reviewedAtlas.ordinary_targets_enabled -or
        [bool]$manifest.mouth_atlas.model_weights_included -or
        [uint32]$manifest.mouth_atlas.schema_version -ne [uint32]$reviewedAtlas.schema_version -or
        [string]$manifest.mouth_atlas.identity_revision -ne [string]$reviewedAtlas.identity_revision -or
        [string]$manifest.mouth_atlas.representation -ne [string]$reviewedAtlas.representation -or
        [string]$manifest.mouth_atlas.game_profile_id -ne [string]$reviewedAtlas.game_profile_id -or
        [string]$manifest.mouth_atlas.character_id -ne [string]$reviewedAtlas.character_id -or
        [string]$manifest.mouth_atlas.enrollment_binding_sha256 -ne
            [string]$reviewedAtlas.enrollment_binding_sha256 -or
        [string]$manifest.mouth_atlas.review_evidence_sha256 -ne
            [string]$reviewedAtlas.review_evidence_sha256) {
        throw 'Portable review mouth-atlas disagrees with its reviewed-artifact receipt.'
    }
}
elseif (($actualAtlasFiles -join "`n") -cne
        ((@('atlas-bgra8-premultiplied.bin', 'atlas.json') | Sort-Object) -join "`n") -or
    [string]$manifest.mouth_atlas.scope -ne 'private-synthetic-mara-only' -or
    @(
        'visual improvement demonstrated; moving tracking performance gate open',
        'experimental private fixture; natural-quality and moving-tracking gates open'
    ) -notcontains [string]$manifest.mouth_atlas.qualification -or
    [bool]$manifest.mouth_atlas.ordinary_targets_enabled -or
    [bool]$manifest.mouth_atlas.model_weights_included -or
    [string]$manifest.mouth_atlas.identity_revision -ne '14018431763358334153' -or
    (Get-LowerSha256 -Path (Join-Path $atlasRoot 'atlas.json')) -cne
        'c4270c252f382502aa5218f5bb0c6b30b01f2db24988b0fde6b2f9d36ef65757' -or
    (Get-LowerSha256 -Path (Join-Path $atlasRoot 'atlas-bgra8-premultiplied.bin')) -cne
        '420e518d3a1552cdf6407a59e78f5d14225a2c461c624cac3049509a8bef5ad1') {
    throw 'Portable review legacy mouth atlas is not the exact v80 Mara fixture.'
}

$characterPackKeys = @{}
foreach ($pack in $characterMouthPackEntries) {
    $key = '{0}/{1}' -f [string]$pack.game_profile_id, [string]$pack.character_id
    $expectedRoot = 'review-character-mouth-packs/{0}/{1}' -f
        [string]$pack.game_profile_id, [string]$pack.character_id
    if ($characterPackKeys.ContainsKey($key) -or
        $key -eq ('{0}/{1}' -f [string]$manifest.mouth_atlas.game_profile_id,
            [string]$manifest.mouth_atlas.character_id) -or
        [string]$pack.relative_root -ne $expectedRoot -or [bool]$pack.enabled -or
        @($pack.files).Count -ne 3) {
        throw "Portable review character mouth-pack identity is invalid: $key"
    }
    $characterPackKeys[$key] = $true
    foreach ($entry in @($pack.files)) {
        $path = Join-Path $root ([string]$entry.path)
        if (-not ([string]$entry.path).StartsWith("$expectedRoot/", [System.StringComparison]::Ordinal) -or
            (Get-LowerSha256 -Path $path) -cne [string]$entry.sha256 -or
            (Get-Item -LiteralPath $path).Length -ne [int64]$entry.size_bytes) {
            throw "Portable review character mouth-pack file binding is invalid: $key"
        }
    }
    $packRoot = Join-Path $root $expectedRoot
    $receiptPath = Join-Path $root ([string]$pack.receipt_path)
    if (-not ([string]$pack.receipt_path).StartsWith("$expectedRoot/", [System.StringComparison]::Ordinal) -or
        (Get-LowerSha256 -Path $receiptPath) -cne [string]$pack.receipt_sha256) {
        throw "Portable review character mouth-pack receipt binding is invalid: $key"
    }
    $reviewedPack = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $receiptPath
    if ([string]$reviewedPack.classification -ne 'reviewed-private-character-pack' -or
        [uint32]$pack.schema_version -ne [uint32]$reviewedPack.schema_version -or
        [string]$pack.identity_revision -ne [string]$reviewedPack.identity_revision -or
        [string]$pack.representation -ne [string]$reviewedPack.representation -or
        [string]$pack.game_profile_id -ne [string]$reviewedPack.game_profile_id -or
        [string]$pack.character_id -ne [string]$reviewedPack.character_id -or
        [string]$pack.enrollment_binding_sha256 -ne
            [string]$reviewedPack.enrollment_binding_sha256 -or
        [string]$pack.review_evidence_sha256 -ne [string]$reviewedPack.review_evidence_sha256 -or
        [bool]$pack.natural_quality_qualified -ne [bool]$reviewedPack.natural_quality_qualified) {
        throw "Portable review character mouth pack disagrees with its reviewed receipt: $key"
    }
}

$engineeringReviewPath = Join-Path $root 'review-evidence/engineering-review.md'
if ([string]$manifest.engineering_review.relative_path -ne 'review-evidence/engineering-review.md' -or
    [bool]$manifest.engineering_review.local_links_rewritten_to_absolute_source_paths -ne $true -or
    (Get-LowerSha256 -Path $engineeringReviewPath) -cne [string]$manifest.engineering_review.portable_sha256) {
    throw 'Portable review engineering-review evidence binding is invalid.'
}
$engineeringReviewText = Get-Content -LiteralPath $engineeringReviewPath -Raw
if ($engineeringReviewText -match '\]\((?![<#]|(?i:https?|mailto|file):)[^)]+\)') {
    throw 'Portable engineering review still contains a relative Markdown link.'
}

[ordered]@{
    schema = 'interactive-npcs-portable-review-verification/v1'
    status = 'passed'
    directory = $root
    manifest_path = $manifestPath
    manifest_sha256 = Get-LowerSha256 -Path $manifestPath
    source_head = [string]$manifest.source.head_commit
    source_dirty = [bool]$manifest.source.dirty
    source_candidate_sha256 = [string]$manifest.source.source_candidate_digest.sha256
    source_change_count = @($manifest.source.working_tree_changes).Count
    file_count = @($actual).Count + 1
    executable_count = $actualExecutables.Count
    application_sha256 = [string]$manifest.application.sha256
    test_game_sha256 = [string]$testGame.executable_sha256
    mouth_atlas_schema_version = if (
        $manifest.mouth_atlas.PSObject.Properties.Name -contains 'schema_version') {
        [uint32]$manifest.mouth_atlas.schema_version
    } else { 1 }
    mouth_atlas_identity_revision = [string]$manifest.mouth_atlas.identity_revision
    mouth_atlas_game_profile_id = if (
        $manifest.mouth_atlas.PSObject.Properties.Name -contains 'game_profile_id') {
        [string]$manifest.mouth_atlas.game_profile_id
    } else { 'eclipse-harbor' }
    mouth_atlas_character_id = if (
        $manifest.mouth_atlas.PSObject.Properties.Name -contains 'character_id') {
        [string]$manifest.mouth_atlas.character_id
    } else { 'mara-venn' }
    mouth_atlas_receipt_sha256 = if (
        $manifest.mouth_atlas.PSObject.Properties.Name -contains 'receipt') {
        [string]$manifest.mouth_atlas.receipt.sha256
    } else { $null }
    reviewed_character_mouth_pack_count = $characterMouthPackEntries.Count
    model_catalog_scope = if ($manifest.PSObject.Properties.Name -contains 'model_catalog') {
        [string]$manifest.model_catalog.scope
    } else {
        'legacy-manifest-unspecified'
    }
    private_model_catalog_receipt_sha256 = if (
        $manifest.PSObject.Properties.Name -contains 'model_catalog' -and
        [string]$manifest.model_catalog.scope -eq 'automated-local-review-bootstrap') {
        [string]$manifest.model_catalog.verification_receipt.sha256
    } else {
        $null
    }
    engineering_review_sha256 = [string]$manifest.engineering_review.portable_sha256
    staged_closed_world_verification = [string]$manifest.outcomes.staged_closed_world_verification
    final_destination_verification = [string]$manifest.outcomes.final_destination_verification
    installer_used = $false
    launched = $false
} | ConvertTo-Json -Depth 5
