[CmdletBinding()]
param(
    [string]$ReceiptPath =
        'E:\temp\InteractiveNPCs\review-mouth-atlas-v80-native-compatible\reviewed-artifact-receipt.v1.json',
    [string]$ReviewedSchema4ReceiptPath = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'reviewed-mouth-atlas-receipt.ps1')

function Copy-JsonDocument {
    param([Parameter(Mandatory = $true)][object]$Value)
    ($Value | ConvertTo-Json -Depth 20) | ConvertFrom-Json
}

function Assert-Rejected {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$ExpectedMessage,
        [Parameter(Mandatory = $true)][string]$Name
    )
    try {
        & $Action
    }
    catch {
        if ($_.Exception.Message -ne $ExpectedMessage) {
            throw "$Name returned an unexpected error: $($_.Exception.Message)"
        }
        return
    }
    throw "$Name was accepted."
}

function Get-CanonicalEnrollmentBindingSha256 {
    param([Parameter(Mandatory = $true)][object]$Binding)
    $canonical = [ordered]@{
        schemaVersion = [uint32]$Binding.schemaVersion
        gameProfileId = [string]$Binding.gameProfileId
        characterId = [string]$Binding.characterId
        referenceProvenanceSha256 = @($Binding.referenceProvenanceSha256)
        reviewStatus = [string]$Binding.reviewStatus
        reviewEvidenceSha256 = [string]$Binding.reviewEvidenceSha256
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(
        ($canonical | ConvertTo-Json -Compress -Depth 8))
    [System.BitConverter]::ToString(
        [System.Security.Cryptography.SHA256]::HashData($bytes)).Replace('-', '').ToLowerInvariant()
}

function Write-Utf8JsonDocument {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][object]$Value
    )
    $json = $Value | ConvertTo-Json -Depth 20
    [System.IO.File]::WriteAllText(
        [System.IO.Path]::GetFullPath($Path), $json,
        [System.Text.UTF8Encoding]::new($false))
}

function Remove-OwnedSchema4ScratchDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedParent
    )
    $absolute = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    $parent = [System.IO.Path]::GetFullPath($ExpectedParent).TrimEnd('\')
    if (-not $absolute.StartsWith("$parent\", [System.StringComparison]::OrdinalIgnoreCase) -or
        [System.IO.Path]::GetFileName($absolute) -cnotmatch '^reviewed-mouth-atlas-receipt-test-[0-9]+-[a-f0-9]{32}$') {
        throw "Refusing to delete unexpected schema 4 receipt-test path: $absolute"
    }
    if (-not (Test-Path -LiteralPath $absolute)) { return }
    $rootItem = Get-Item -LiteralPath $absolute -Force
    if (-not $rootItem.PSIsContainer -or
        ($rootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing to recursively delete non-directory or reparse root: $absolute"
    }
    $items = @(Get-ChildItem -LiteralPath $absolute -Force -Recurse)
    $reparsePoints = @($items | Where-Object {
        $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint
    })
    if ($reparsePoints.Count -ne 0) {
        throw "Refusing to recursively delete schema 4 receipt-test scratch containing reparse points: $absolute"
    }
    [System.IO.Directory]::Delete($absolute, $true)
    if (Test-Path -LiteralPath $absolute) {
        throw "Schema 4 receipt-test scratch deletion could not be verified: $absolute"
    }
}

$resolvedReceipt = [System.IO.Path]::GetFullPath($ReceiptPath)
$artifactRoot = Split-Path -Parent $resolvedReceipt
$receipt = Get-Content -LiteralPath $resolvedReceipt -Raw | ConvertFrom-Json
$validated = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $resolvedReceipt
if ($validated.schema_version -ne 1 -or
    $validated.classification -ne 'legacy-private-fixture' -or
    $validated.game_profile_id -ne 'eclipse-harbor' -or
    $validated.character_id -ne 'mara-venn' -or
    $validated.natural_quality_qualified -or $validated.ordinary_targets_enabled) {
    throw 'Legacy reviewed mouth-atlas receipt returned the wrong boundary.'
}

$tamperedHash = Copy-JsonDocument $receipt
$tamperedHash.artifact.texture.sha256 = '0' * 64
Assert-Rejected -Name 'Texture hash tamper' `
    -ExpectedMessage 'Reviewed mouth-atlas artifact hash or size mismatch: atlas-bgra8-premultiplied.bin' `
    -Action {
        Assert-NpcReviewedMouthAtlasDocument -Receipt $tamperedHash `
            -ReceiptPath $resolvedReceipt -ArtifactRoot $artifactRoot | Out-Null
    }

$schemaMismatch = Copy-JsonDocument $receipt
$schemaMismatch.atlasIdentity.schemaVersion = 4
Assert-Rejected -Name 'Schema and representation mismatch' `
    -ExpectedMessage 'Reviewed mouth-atlas semantic identity is invalid.' `
    -Action {
        Assert-NpcReviewedMouthAtlasDocument -Receipt $schemaMismatch `
            -ReceiptPath $resolvedReceipt -ArtifactRoot $artifactRoot | Out-Null
    }

$semanticMismatch = Copy-JsonDocument $receipt
$semanticMismatch.atlasIdentity.characterId = 'misty'
Assert-Rejected -Name 'Legacy cross-character binding' `
    -ExpectedMessage 'Legacy mouth-atlas receipt is not the exact Mara private fixture.' `
    -Action {
        Assert-NpcReviewedMouthAtlasDocument -Receipt $semanticMismatch `
            -ReceiptPath $resolvedReceipt -ArtifactRoot $artifactRoot | Out-Null
    }

$unknownField = Copy-JsonDocument $receipt
$unknownField.atlasIdentity | Add-Member -NotePropertyName claimedQuality -NotePropertyValue 'qualified'
Assert-Rejected -Name 'Unknown receipt field' `
    -ExpectedMessage 'Reviewed mouth-atlas identity has missing or unknown fields.' `
    -Action {
        Assert-NpcReviewedMouthAtlasDocument -Receipt $unknownField `
            -ReceiptPath $resolvedReceipt -ArtifactRoot $artifactRoot | Out-Null
    }

$qualifiedLegacy = Copy-JsonDocument $receipt
$qualifiedLegacy.review.naturalQualityQualified = $true
Assert-Rejected -Name 'Legacy natural-quality claim' `
    -ExpectedMessage 'Legacy mouth-atlas receipt is not the exact Mara private fixture.' `
    -Action {
        Assert-NpcReviewedMouthAtlasDocument -Receipt $qualifiedLegacy `
            -ReceiptPath $resolvedReceipt -ArtifactRoot $artifactRoot | Out-Null
    }

$scratchParent = 'E:\temp\InteractiveNPCs'
if (-not (Test-Path -LiteralPath $scratchParent -PathType Container)) {
    throw "Schema 4 receipt-test parent is unavailable: $scratchParent"
}
$scratchName = "reviewed-mouth-atlas-receipt-test-$PID-$([guid]::NewGuid().ToString('N'))"
$schema4Root = Join-Path $scratchParent $scratchName
$schema4ReceiptPath = Join-Path $schema4Root 'reviewed-artifact-receipt.v1.json'
$schema4AtlasPath = Join-Path $schema4Root 'atlas.json'
$schema4TexturePath = Join-Path $schema4Root 'atlas-bgra8-premultiplied.bin'
$schema4FilesCreated = 0
$schema4Deleted = $false
$schema4Validated = $null
try {
    [System.IO.Directory]::CreateDirectory($schema4Root) | Out-Null
    $textureBytes = [byte[]]::new(4096)
    for ($index = 0; $index -lt $textureBytes.Length; $index += 1) {
        $textureBytes[$index] = [byte](($index * 37 + 11) % 256)
    }
    [System.IO.File]::WriteAllBytes($schema4TexturePath, $textureBytes)
    $schema4FilesCreated += 1
    $textureSha256 = Get-NpcLowerSha256 -Path $schema4TexturePath

    $binding = [ordered]@{
        schemaVersion = 1
        gameProfileId = 'test-profile'
        characterId = 'test-character'
        referenceProvenanceSha256 = @(('1' * 64), ('2' * 64))
        reviewStatus = 'reviewed-private'
        reviewEvidenceSha256 = '3' * 64
    }
    $schema4Atlas = [ordered]@{
        schemaVersion = 4
        identityRevision = 42
        enrollmentBinding = $binding
        texture = [ordered]@{
            file = 'atlas-bgra8-premultiplied.bin'
            sha256 = $textureSha256
            representation = 'normalized-oral-strip-v1'
            width = 16
            height = 16
            strideBytes = 64
            stateCount = 4
            stateBytes = 1024
        }
        states = @(
            [ordered]@{ index = 0; coefficients = @(0, 1, 0, 0, 0, 0, 0, 0); enrolledPose = @(0, 0, 0); referenceContextMean = 100; refineSourceEdges = $true },
            [ordered]@{ index = 1; coefficients = @(1, 0, 0, 0, 0, 0, 0, 0); enrolledPose = @(0, 0, 0); referenceContextMean = 100; refineSourceEdges = $true },
            [ordered]@{ index = 2; coefficients = @(0.75, 0, 1, 1, 0, 0, 0, 0); enrolledPose = @(0, 0, 0); referenceContextMean = 100; refineSourceEdges = $true },
            [ordered]@{ index = 3; coefficients = @(0.575, 0, 0, 0, 1, 1, 0, 0); enrolledPose = @(0, 0, 0); referenceContextMean = 100; refineSourceEdges = $true }
        )
    }
    Write-Utf8JsonDocument -Path $schema4AtlasPath -Value $schema4Atlas
    $schema4FilesCreated += 1
    $atlasBytes = [System.IO.File]::ReadAllBytes($schema4AtlasPath)

    $schema4Receipt = [ordered]@{
        schema = 'interactive-npcs-reviewed-mouth-atlas/v1'
        artifact = [ordered]@{
            atlas = [ordered]@{
                path = 'atlas.json'
                sizeBytes = [int64](Get-Item -LiteralPath $schema4AtlasPath).Length
                sha256 = Get-NpcLowerSha256 -Path $schema4AtlasPath
            }
            texture = [ordered]@{
                path = 'atlas-bgra8-premultiplied.bin'
                sizeBytes = [int64](Get-Item -LiteralPath $schema4TexturePath).Length
                sha256 = $textureSha256
            }
        }
        atlasIdentity = [ordered]@{
            schemaVersion = 4
            identityRevision = '42'
            representation = 'normalized-oral-strip-v1'
            gameProfileId = 'test-profile'
            characterId = 'test-character'
            enrollmentBindingSha256 = Get-CanonicalEnrollmentBindingSha256 -Binding $binding
        }
        review = [ordered]@{
            classification = 'reviewed-private-character-pack'
            status = 'accepted-for-private-review'
            reviewEvidenceSha256 = '3' * 64
            naturalQualityQualified = $false
            ordinaryTargetsEnabled = $false
            qualification = 'synthetic schema 4 validator fixture only'
        }
    }
    Write-Utf8JsonDocument -Path $schema4ReceiptPath -Value $schema4Receipt
    $schema4FilesCreated += 1
    $schema4Validated = Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $schema4ReceiptPath
    if ($schema4Validated.schema_version -ne 4 -or
        $schema4Validated.classification -ne 'reviewed-private-character-pack' -or
        $schema4Validated.review_status -ne 'accepted-for-private-review' -or
        $schema4Validated.natural_quality_qualified -or
        $schema4Validated.ordinary_targets_enabled) {
        throw 'Schema 4 reviewed receipt returned the wrong boundary.'
    }

    $schema4SemanticMismatch = Copy-JsonDocument $schema4Receipt
    $schema4SemanticMismatch.atlasIdentity.characterId = 'different-character'
    Assert-Rejected -Name 'Schema 4 semantic mismatch' `
        -ExpectedMessage 'Reviewed character mouth-atlas semantic enrollment is invalid.' `
        -Action {
            Assert-NpcReviewedMouthAtlasDocument -Receipt $schema4SemanticMismatch `
                -ReceiptPath $schema4ReceiptPath -ArtifactRoot $schema4Root | Out-Null
        }

    $schema4TextureHashMismatch = Copy-JsonDocument $schema4Receipt
    $schema4TextureHashMismatch.artifact.texture.sha256 = '0' * 64
    Assert-Rejected -Name 'Schema 4 texture hash mismatch' `
        -ExpectedMessage 'Reviewed mouth-atlas artifact hash or size mismatch: atlas-bgra8-premultiplied.bin' `
        -Action {
            Assert-NpcReviewedMouthAtlasDocument -Receipt $schema4TextureHashMismatch `
                -ReceiptPath $schema4ReceiptPath -ArtifactRoot $schema4Root | Out-Null
        }

    $schema4RefineMismatchAtlas = Copy-JsonDocument $schema4Atlas
    $schema4RefineMismatchAtlas.states[1].refineSourceEdges = $false
    Write-Utf8JsonDocument -Path $schema4AtlasPath -Value $schema4RefineMismatchAtlas
    $schema4RefineMismatchReceipt = Copy-JsonDocument $schema4Receipt
    $schema4RefineMismatchReceipt.artifact.atlas.sizeBytes =
        [int64](Get-Item -LiteralPath $schema4AtlasPath).Length
    $schema4RefineMismatchReceipt.artifact.atlas.sha256 = Get-NpcLowerSha256 -Path $schema4AtlasPath
    Assert-Rejected -Name 'Schema 4 refine policy mismatch' `
        -ExpectedMessage 'Schema 4 mouth-atlas edge-refinement policy changes between states.' `
        -Action {
            Assert-NpcReviewedMouthAtlasDocument -Receipt $schema4RefineMismatchReceipt `
                -ReceiptPath $schema4ReceiptPath -ArtifactRoot $schema4Root | Out-Null
        }

    $schema4ContextMismatchAtlas = Copy-JsonDocument $schema4Atlas
    $schema4ContextMismatchAtlas.states[2].referenceContextMean = 0
    Write-Utf8JsonDocument -Path $schema4AtlasPath -Value $schema4ContextMismatchAtlas
    $schema4ContextMismatchReceipt = Copy-JsonDocument $schema4Receipt
    $schema4ContextMismatchReceipt.artifact.atlas.sizeBytes =
        [int64](Get-Item -LiteralPath $schema4AtlasPath).Length
    $schema4ContextMismatchReceipt.artifact.atlas.sha256 = Get-NpcLowerSha256 -Path $schema4AtlasPath
    Assert-Rejected -Name 'Schema 4 reference context mismatch' `
        -ExpectedMessage 'Schema 4 mouth-atlas reference context is invalid.' `
        -Action {
            Assert-NpcReviewedMouthAtlasDocument -Receipt $schema4ContextMismatchReceipt `
                -ReceiptPath $schema4ReceiptPath -ArtifactRoot $schema4Root | Out-Null
        }

    [System.IO.File]::WriteAllBytes($schema4AtlasPath, $atlasBytes)
    Get-NpcReviewedMouthAtlasReceipt -ReceiptPath $schema4ReceiptPath | Out-Null
}
finally {
    if (Test-Path -LiteralPath $schema4Root) {
        Remove-OwnedSchema4ScratchDirectory -Path $schema4Root -ExpectedParent $scratchParent
    }
    $schema4Deleted = -not (Test-Path -LiteralPath $schema4Root)
}

$externalSchema4 = $null
if (-not [string]::IsNullOrWhiteSpace($ReviewedSchema4ReceiptPath)) {
    $externalSchema4 = Get-NpcReviewedMouthAtlasReceipt `
        -ReceiptPath ([System.IO.Path]::GetFullPath($ReviewedSchema4ReceiptPath))
    if ($externalSchema4.schema_version -ne 4 -or
        $externalSchema4.classification -ne 'reviewed-private-character-pack' -or
        $externalSchema4.review_status -ne 'accepted-for-private-review' -or
        $externalSchema4.natural_quality_qualified -or
        $externalSchema4.ordinary_targets_enabled) {
        throw 'External schema 4 receipt returned the wrong private-review boundary.'
    }
}

[ordered]@{
    schema = 'interactive-npcs-reviewed-mouth-atlas-receipt-tests/v1'
    status = 'passed'
    receipt_sha256 = $validated.receipt_sha256
    accepted_fixture = 'eclipse-harbor/mara-venn schema 1 legacy-private-fixture'
    rejected = @(
        'texture-hash-tamper',
        'schema-representation-mismatch',
        'legacy-cross-character-binding',
        'unknown-receipt-field',
        'legacy-natural-quality-claim',
        'schema4-semantic-mismatch',
        'schema4-texture-hash-mismatch',
        'schema4-refine-policy-mismatch',
        'schema4-reference-context-mismatch'
    )
    schema4_fixture = "$($schema4Validated.game_profile_id)/$($schema4Validated.character_id) schema 4 reviewed-private-character-pack"
    external_schema4_receipt_sha256 = if ($null -ne $externalSchema4) {
        $externalSchema4.receipt_sha256
    } else { $null }
    files_created = $schema4FilesCreated
    files_deleted = if ($schema4Deleted) { $schema4FilesCreated } else { 0 }
    scratch_directory_deleted = $schema4Deleted
} | ConvertTo-Json -Depth 5
