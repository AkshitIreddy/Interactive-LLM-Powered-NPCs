Set-StrictMode -Version 2.0

function Get-NpcLowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Test-NpcLowerSha256 {
    param([AllowNull()][object]$Value)
    $null -ne $Value -and [string]$Value -cmatch '^[0-9a-f]{64}$'
}

function Test-NpcSemanticIdentifier {
    param([AllowNull()][object]$Value)
    $null -ne $Value -and [string]$Value -cmatch '^[a-z0-9](?:[a-z0-9.-]{0,94}[a-z0-9])?$' -and
        -not ([string]$Value).Contains('..')
}

function Assert-NpcJsonPropertySet {
    param(
        [Parameter(Mandatory = $true)][object]$Object,
        [Parameter(Mandatory = $true)][string[]]$Required,
        [string[]]$Optional = @(),
        [Parameter(Mandatory = $true)][string]$Label
    )
    $actual = @($Object.PSObject.Properties.Name | Sort-Object)
    $requiredSorted = @($Required | Sort-Object)
    $allowed = @(($Required + $Optional) | Sort-Object -Unique)
    $missing = @($requiredSorted | Where-Object { $_ -notin $actual })
    $unknown = @($actual | Where-Object { $_ -notin $allowed })
    if ($missing.Count -ne 0 -or $unknown.Count -ne 0) {
        throw "$Label has missing or unknown fields."
    }
}

function Assert-NpcReviewedMouthAtlasDocument {
    param(
        [Parameter(Mandatory = $true)][object]$Receipt,
        [Parameter(Mandatory = $true)][string]$ReceiptPath,
        [Parameter(Mandatory = $true)][string]$ArtifactRoot
    )

    Assert-NpcJsonPropertySet -Object $Receipt `
        -Required @('schema', 'artifact', 'atlasIdentity', 'review') `
        -Label 'Reviewed mouth-atlas receipt'
    if ([string]$Receipt.schema -ne 'interactive-npcs-reviewed-mouth-atlas/v1') {
        throw 'Reviewed mouth-atlas receipt schema is unsupported.'
    }
    Assert-NpcJsonPropertySet -Object $Receipt.artifact -Required @('atlas', 'texture') `
        -Label 'Reviewed mouth-atlas artifact'
    foreach ($binding in @($Receipt.artifact.atlas, $Receipt.artifact.texture)) {
        Assert-NpcJsonPropertySet -Object $binding -Required @('path', 'sizeBytes', 'sha256') `
            -Label 'Reviewed mouth-atlas file binding'
        $relative = [string]$binding.path
        if ([System.IO.Path]::IsPathRooted($relative) -or $relative.Contains('..') -or
            $relative.Contains('/') -or $relative.Contains('\') -or
            [string]$binding.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
            [int64]$binding.sizeBytes -le 0) {
            throw 'Reviewed mouth-atlas receipt contains an unsafe file binding.'
        }
        $path = Join-Path $ArtifactRoot $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-Item -LiteralPath $path).Length -ne [int64]$binding.sizeBytes -or
            (Get-NpcLowerSha256 -Path $path) -cne [string]$binding.sha256) {
            throw "Reviewed mouth-atlas artifact hash or size mismatch: $relative"
        }
    }
    if ([string]$Receipt.artifact.atlas.path -ne 'atlas.json' -or
        [string]$Receipt.artifact.texture.path -eq 'atlas.json' -or
        [System.IO.Path]::GetExtension([string]$Receipt.artifact.texture.path) -cne '.bin') {
        throw 'Reviewed mouth-atlas receipt file roles are invalid.'
    }

    $expectedFiles = @(
        [string]$Receipt.artifact.atlas.path,
        [string]$Receipt.artifact.texture.path
    )
    if ([string]::Equals(
            [System.IO.Path]::GetFullPath((Split-Path -Parent $ReceiptPath)).TrimEnd('\'),
            [System.IO.Path]::GetFullPath($ArtifactRoot).TrimEnd('\'),
            [System.StringComparison]::OrdinalIgnoreCase)) {
        $expectedFiles += [System.IO.Path]::GetFileName($ReceiptPath)
    }
    $expectedFiles = @($expectedFiles | Sort-Object)
    if ($expectedFiles.Count -ne @($expectedFiles | Select-Object -Unique).Count) {
        throw 'Reviewed mouth-atlas receipt file bindings are duplicated.'
    }
    $allItems = @((Get-Item -LiteralPath $ArtifactRoot -Force)) +
        @(Get-ChildItem -LiteralPath $ArtifactRoot -Force -Recurse)
    foreach ($item in $allItems) {
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw "Reviewed mouth-atlas artifact must not contain reparse points: $($item.FullName)"
        }
    }
    $actualFiles = @(Get-ChildItem -LiteralPath $ArtifactRoot -File -Force -Recurse |
        ForEach-Object { $_.FullName.Substring($ArtifactRoot.Length + 1).Replace('\', '/') } |
        Sort-Object)
    if (($expectedFiles -join "`n") -cne ($actualFiles -join "`n")) {
        throw 'Reviewed mouth-atlas artifact contains missing or unknown files.'
    }

    Assert-NpcJsonPropertySet -Object $Receipt.atlasIdentity `
        -Required @('schemaVersion', 'identityRevision', 'representation', 'gameProfileId', 'characterId') `
        -Optional @('enrollmentBindingSha256') -Label 'Reviewed mouth-atlas identity'
    $schemaVersion = [uint32]$Receipt.atlasIdentity.schemaVersion
    $expectedRepresentation = switch ($schemaVersion) {
        1 { 'full-lip-observation-v1' }
        2 { 'normalized-oral-interior-v1' }
        3 { 'photometric-full-lip-reference-v1' }
        4 { 'normalized-oral-strip-v1' }
        default { throw 'Reviewed mouth-atlas schema version is unsupported.' }
    }
    if ([string]$Receipt.atlasIdentity.identityRevision -cnotmatch '^[1-9][0-9]{0,19}$' -or
        [string]$Receipt.atlasIdentity.representation -ne $expectedRepresentation -or
        -not (Test-NpcSemanticIdentifier $Receipt.atlasIdentity.gameProfileId) -or
        -not (Test-NpcSemanticIdentifier $Receipt.atlasIdentity.characterId)) {
        throw 'Reviewed mouth-atlas semantic identity is invalid.'
    }

    Assert-NpcJsonPropertySet -Object $Receipt.review `
        -Required @(
            'classification', 'status', 'reviewEvidenceSha256', 'naturalQualityQualified',
            'ordinaryTargetsEnabled', 'qualification'
        ) -Label 'Reviewed mouth-atlas review boundary'
    if (-not (Test-NpcLowerSha256 $Receipt.review.reviewEvidenceSha256) -or
        [bool]$Receipt.review.ordinaryTargetsEnabled -or
        [string]::IsNullOrWhiteSpace([string]$Receipt.review.qualification)) {
        throw 'Reviewed mouth-atlas review evidence is invalid.'
    }

    $atlasPath = Join-Path $ArtifactRoot ([string]$Receipt.artifact.atlas.path)
    $atlas = Get-Content -LiteralPath $atlasPath -Raw | ConvertFrom-Json
    Assert-NpcJsonPropertySet -Object $atlas `
        -Required @('schemaVersion', 'identityRevision', 'texture', 'states') `
        -Optional @('enrollmentBinding', 'neutralStateIndex') -Label 'Mouth-atlas manifest'
    Assert-NpcJsonPropertySet -Object $atlas.texture `
        -Required @('file', 'sha256', 'width', 'height', 'strideBytes', 'stateCount', 'stateBytes') `
        -Optional @('representation') -Label 'Mouth-atlas texture'
    if ([uint32]$atlas.schemaVersion -ne $schemaVersion -or
        [string]$atlas.identityRevision -ne [string]$Receipt.atlasIdentity.identityRevision -or
        [string]$atlas.texture.file -ne [string]$Receipt.artifact.texture.path -or
        [string]$atlas.texture.sha256 -cne [string]$Receipt.artifact.texture.sha256) {
        throw 'Reviewed mouth-atlas receipt identity disagrees with atlas.json.'
    }
    $atlasRepresentation = if ($atlas.texture.PSObject.Properties.Name -contains 'representation') {
        [string]$atlas.texture.representation
    } elseif ($schemaVersion -eq 1) {
        'full-lip-observation-v1'
    } else {
        ''
    }
    $neutralStateValid = if ($schemaVersion -eq 3) {
        $atlas.PSObject.Properties.Name -contains 'neutralStateIndex' -and
            [uint32]$atlas.neutralStateIndex -eq 0
    } else {
        -not ($atlas.PSObject.Properties.Name -contains 'neutralStateIndex')
    }
    $width = [uint64]$atlas.texture.width
    $height = [uint64]$atlas.texture.height
    $stride = [uint64]$atlas.texture.strideBytes
    $stateCount = [uint64]$atlas.texture.stateCount
    $stateBytes = [uint64]$atlas.texture.stateBytes
    $expectedTextureBytes = $stateBytes * $stateCount
    if ($atlasRepresentation -ne $expectedRepresentation -or -not $neutralStateValid -or
        $width -lt 16 -or $height -lt 16 -or $width -gt 512 -or $height -gt 512 -or
        $stride -lt ($width * 4) -or $stateCount -lt 4 -or $stateCount -gt 16 -or
        $stateBytes -ne ($stride * $height) -or $expectedTextureBytes -gt (16 * 1024 * 1024) -or
        [int64]$Receipt.artifact.texture.sizeBytes -ne [int64]$expectedTextureBytes -or
        @($atlas.states).Count -ne [int]$stateCount) {
        throw 'Reviewed mouth-atlas dimensions or representation are invalid.'
    }

    $schema4Refine = $null
    for ($index = 0; $index -lt @($atlas.states).Count; $index += 1) {
        $state = @($atlas.states)[$index]
        Assert-NpcJsonPropertySet -Object $state `
            -Required @('index', 'coefficients', 'enrolledPose') `
            -Optional @('referenceContextMean', 'refineSourceEdges') `
            -Label 'Mouth-atlas state'
        $coefficients = @($state.coefficients)
        $pose = @($state.enrolledPose)
        if ([int]$state.index -ne $index -or $coefficients.Count -ne 8 -or $pose.Count -ne 3) {
            throw 'Reviewed mouth-atlas state shape is invalid.'
        }
        foreach ($value in $coefficients) {
            $number = [double]$value
            if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or
                $number -lt 0 -or $number -gt 1) {
                throw 'Reviewed mouth-atlas coefficients are invalid.'
            }
        }
        foreach ($value in $pose) {
            $number = [double]$value
            if ([double]::IsNaN($number) -or [double]::IsInfinity($number)) {
                throw 'Reviewed mouth-atlas enrolled pose is invalid.'
            }
        }
        $hasReferenceContext = $state.PSObject.Properties.Name -contains 'referenceContextMean'
        $hasRefine = $state.PSObject.Properties.Name -contains 'refineSourceEdges'
        if ($schemaVersion -eq 4) {
            if (-not $hasReferenceContext) {
                throw 'Schema 4 mouth-atlas state has no reference context.'
            }
            $context = [double]$state.referenceContextMean
            if ([double]::IsNaN($context) -or [double]::IsInfinity($context) -or
                $context -le 0 -or $context -gt 255) {
                throw 'Schema 4 mouth-atlas reference context is invalid.'
            }
            $refine = $hasRefine -and [bool]$state.refineSourceEdges
            if ($null -eq $schema4Refine) { $schema4Refine = $refine }
            if ($schema4Refine -ne $refine) {
                throw 'Schema 4 mouth-atlas edge-refinement policy changes between states.'
            }
        }
        elseif ($hasReferenceContext -or ($hasRefine -and [bool]$state.refineSourceEdges)) {
            throw 'Pre-schema-4 mouth-atlas state contains schema 4 metadata.'
        }
    }

    $hasEnrollment = $atlas.PSObject.Properties.Name -contains 'enrollmentBinding'
    $classification = [string]$Receipt.review.classification
    if ($classification -eq 'legacy-private-fixture') {
        if ($schemaVersion -ne 1 -or $hasEnrollment -or
            [string]$Receipt.review.status -ne 'legacy-compatibility-only' -or
            [bool]$Receipt.review.naturalQualityQualified -or
            [string]$Receipt.atlasIdentity.gameProfileId -ne 'eclipse-harbor' -or
            [string]$Receipt.atlasIdentity.characterId -ne 'mara-venn' -or
            [string]$Receipt.atlasIdentity.identityRevision -ne '14018431763358334153' -or
            [string]$Receipt.artifact.atlas.sha256 -ne
                'c4270c252f382502aa5218f5bb0c6b30b01f2db24988b0fde6b2f9d36ef65757' -or
            [string]$Receipt.artifact.texture.sha256 -ne
                '420e518d3a1552cdf6407a59e78f5d14225a2c461c624cac3049509a8bef5ad1') {
            throw 'Legacy mouth-atlas receipt is not the exact Mara private fixture.'
        }
    }
    elseif ($classification -eq 'reviewed-private-character-pack') {
        if (-not $hasEnrollment -or [string]$Receipt.review.status -ne 'accepted-for-private-review') {
            throw 'Reviewed character mouth-atlas has no accepted semantic enrollment.'
        }
        Assert-NpcJsonPropertySet -Object $atlas.enrollmentBinding `
            -Required @(
                'schemaVersion', 'gameProfileId', 'characterId', 'referenceProvenanceSha256',
                'reviewStatus', 'reviewEvidenceSha256'
            ) -Label 'Mouth-atlas enrollment binding'
        $references = @($atlas.enrollmentBinding.referenceProvenanceSha256)
        if ([uint32]$atlas.enrollmentBinding.schemaVersion -ne 1 -or
            [string]$atlas.enrollmentBinding.gameProfileId -ne
                [string]$Receipt.atlasIdentity.gameProfileId -or
            [string]$atlas.enrollmentBinding.characterId -ne [string]$Receipt.atlasIdentity.characterId -or
            [string]$atlas.enrollmentBinding.reviewStatus -ne 'reviewed-private' -or
            [string]$atlas.enrollmentBinding.reviewEvidenceSha256 -cne
                [string]$Receipt.review.reviewEvidenceSha256 -or
            $references.Count -lt 1 -or $references.Count -gt 16 -or
            @($references | Where-Object { -not (Test-NpcLowerSha256 $_) }).Count -ne 0 -or
            $references.Count -ne @($references | Select-Object -Unique).Count) {
            throw 'Reviewed character mouth-atlas semantic enrollment is invalid.'
        }
        $canonicalBinding = [ordered]@{
            schemaVersion = [uint32]$atlas.enrollmentBinding.schemaVersion
            gameProfileId = [string]$atlas.enrollmentBinding.gameProfileId
            characterId = [string]$atlas.enrollmentBinding.characterId
            referenceProvenanceSha256 = @($atlas.enrollmentBinding.referenceProvenanceSha256)
            reviewStatus = [string]$atlas.enrollmentBinding.reviewStatus
            reviewEvidenceSha256 = [string]$atlas.enrollmentBinding.reviewEvidenceSha256
        }
        $bindingBytes = [System.Text.Encoding]::UTF8.GetBytes(
            ($canonicalBinding | ConvertTo-Json -Compress -Depth 8))
        $bindingSha256 = [System.BitConverter]::ToString(
            [System.Security.Cryptography.SHA256]::HashData($bindingBytes)).Replace('-', '').ToLowerInvariant()
        if (-not (Test-NpcLowerSha256 $Receipt.atlasIdentity.enrollmentBindingSha256) -or
            [string]$Receipt.atlasIdentity.enrollmentBindingSha256 -cne $bindingSha256) {
            throw 'Reviewed character mouth-atlas enrollment digest disagrees with its receipt.'
        }
    }
    else {
        throw 'Reviewed mouth-atlas classification is unsupported.'
    }

    [pscustomobject]@{
        receipt_path = $ReceiptPath
        receipt_sha256 = Get-NpcLowerSha256 -Path $ReceiptPath
        artifact_root = $ArtifactRoot
        atlas_path = $atlasPath
        texture_path = Join-Path $ArtifactRoot ([string]$Receipt.artifact.texture.path)
        atlas_file_name = [string]$Receipt.artifact.atlas.path
        texture_file_name = [string]$Receipt.artifact.texture.path
        schema_version = $schemaVersion
        identity_revision = [string]$Receipt.atlasIdentity.identityRevision
        representation = $expectedRepresentation
        game_profile_id = [string]$Receipt.atlasIdentity.gameProfileId
        character_id = [string]$Receipt.atlasIdentity.characterId
        enrollment_binding_sha256 = if (
            $Receipt.atlasIdentity.PSObject.Properties.Name -contains 'enrollmentBindingSha256') {
            [string]$Receipt.atlasIdentity.enrollmentBindingSha256
        } else { $null }
        classification = $classification
        review_status = [string]$Receipt.review.status
        review_evidence_sha256 = [string]$Receipt.review.reviewEvidenceSha256
        natural_quality_qualified = [bool]$Receipt.review.naturalQualityQualified
        ordinary_targets_enabled = [bool]$Receipt.review.ordinaryTargetsEnabled
        qualification = [string]$Receipt.review.qualification
        atlas_sha256 = [string]$Receipt.artifact.atlas.sha256
        atlas_size_bytes = [int64]$Receipt.artifact.atlas.sizeBytes
        texture_sha256 = [string]$Receipt.artifact.texture.sha256
        texture_size_bytes = [int64]$Receipt.artifact.texture.sizeBytes
    }
}

function Get-NpcReviewedMouthAtlasReceipt {
    param(
        [Parameter(Mandatory = $true)][string]$ReceiptPath,
        [string]$ArtifactRoot = ''
    )
    $resolvedReceipt = [System.IO.Path]::GetFullPath($ReceiptPath)
    if (-not (Test-Path -LiteralPath $resolvedReceipt -PathType Leaf)) {
        throw "Reviewed mouth-atlas receipt is missing: $resolvedReceipt"
    }
    $resolvedArtifactRoot = if ([string]::IsNullOrWhiteSpace($ArtifactRoot)) {
        Split-Path -Parent $resolvedReceipt
    } else {
        [System.IO.Path]::GetFullPath($ArtifactRoot).TrimEnd('\')
    }
    $receipt = Get-Content -LiteralPath $resolvedReceipt -Raw | ConvertFrom-Json
    Assert-NpcReviewedMouthAtlasDocument -Receipt $receipt -ReceiptPath $resolvedReceipt `
        -ArtifactRoot $resolvedArtifactRoot
}
