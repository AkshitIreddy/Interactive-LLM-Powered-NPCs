[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Directory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$root = [System.IO.Path]::GetFullPath($Directory)
if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw "Review fixture directory is missing: $root" }
$rootItem = Get-Item -LiteralPath $root -Force
if ($rootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Review fixture directory must not be a reparse point.' }

function Get-LowerSha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

$manifestPath = Join-Path $root 'REVIEW-FIXTURE-MANIFEST.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'Review fixture distribution manifest is missing.' }
$manifestItem = Get-Item -LiteralPath $manifestPath -Force
if ($manifestItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Review fixture manifest must not be a reparse point.' }
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.schema_version -ne 1 -or $manifest.component_id -ne 'project:synthetic-review-target' -or
    $manifest.fixture_source -ne 'project-source-generated-native-v2' -or
    $manifest.distribution_scope -ne 'review-test-game' -or
    $manifest.distribution_class -ne 'project-owned-source-built-review-fixture' -or
    @($manifest.third_party_binaries).Count -ne 0 -or
    $manifest.license.expression -ne 'MIT' -or $manifest.license.bundled_notice -ne 'THIRD-PARTY-NOTICES.md') {
    throw 'Review fixture distribution manifest violates the project-owned/no-third-party policy.'
}
if (@($manifest.manifest_self.PSObject.Properties.Name).Count -ne 2 -or
    $manifest.manifest_self.path -ne 'REVIEW-FIXTURE-MANIFEST.json' -or
    $manifest.manifest_self.hash -ne 'excluded-to-avoid-circularity') {
    throw 'Review fixture manifest self-hash policy is missing or invalid.'
}
$expectedFiles = @(
    'interactive-npcs-synthetic-target.exe',
    'mara-venn-camera-idle-v1.inpcseq',
    'README.md',
    'REVIEW-FIXTURE-MANIFEST.json',
    'THIRD-PARTY-NOTICES.md',
    'review-test-game.cdx.json'
) | Sort-Object
if ((@($manifest.expected_files | Sort-Object) -join "`n") -cne ($expectedFiles -join "`n")) {
    throw 'Review fixture expected-files allowlist does not match schema v1.'
}
$directories = @(Get-ChildItem -LiteralPath $root -Directory -Force)
if ($directories.Count -ne 0) { throw 'Review fixture contains an unexpected directory.' }
$actualFiles = @(Get-ChildItem -LiteralPath $root -File -Force | Sort-Object Name)
if ((@($actualFiles.Name | Sort-Object) -join "`n") -cne ($expectedFiles -join "`n")) {
    throw 'Review fixture contains a missing or unknown file.'
}
foreach ($file in $actualFiles) {
    if ($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw "Review fixture file must not be a reparse point: $($file.Name)" }
}

$entries = @($manifest.files)
if ($entries.Count -ne 5) { throw 'Review fixture manifest must hash exactly five non-manifest files.' }
$seen = @{}
foreach ($entry in $entries) {
    $path = [string]$entry.path
    if ([string]::IsNullOrWhiteSpace($path) -or $path -ne [System.IO.Path]::GetFileName($path) -or
        [System.IO.Path]::IsPathRooted($path) -or $path.IndexOfAny(@([char]'/', [char]'\')) -ge 0 -or
        $path -eq 'REVIEW-FIXTURE-MANIFEST.json') {
        throw "Review fixture manifest contains an unsafe or self-hashed path: $path"
    }
    if ($seen.ContainsKey($path)) { throw "Review fixture manifest contains a duplicate path: $path" }
    $seen[$path] = $true
    if ($expectedFiles -notcontains $path -or [string]$entry.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        [int64]$entry.size_bytes -le 0 -or @('project:synthetic-review-target', 'project:legal-resources') -notcontains $entry.component_id -or
        $entry.spdx -ne 'MIT' -or $entry.distribution_scope -ne 'review-test-game' -or
        [string]::IsNullOrWhiteSpace([string]$entry.distribution_class) -or
        [string]::IsNullOrWhiteSpace([string]$entry.source_reference) -or
        $entry.notice_reference -ne 'THIRD-PARTY-NOTICES.md') {
        throw "Review fixture manifest metadata is incomplete for: $path"
    }
    $expectedComponent = if (@('interactive-npcs-synthetic-target.exe', 'mara-venn-camera-idle-v1.inpcseq') -contains $path) { 'project:synthetic-review-target' } else { 'project:legal-resources' }
    if ($entry.component_id -ne $expectedComponent) { throw "Review fixture component ownership is invalid for: $path" }
    $filePath = Join-Path $root $path
    if ((Get-LowerSha256 -Path $filePath) -cne [string]$entry.sha256 -or
        (Get-Item -LiteralPath $filePath).Length -ne [int64]$entry.size_bytes) {
        throw "Review fixture file hash or size mismatch: $path"
    }
}
$seenNames = @(($seen.Keys | Sort-Object)) -join "`n"
$expectedEntryNames = @(($expectedFiles | Where-Object { $_ -ne 'REVIEW-FIXTURE-MANIFEST.json' })) -join "`n"
if ($seenNames -cne $expectedEntryNames) {
    throw 'Review fixture manifest file entries do not cover the complete non-manifest allowlist.'
}
if (@($manifest.inbox_system_dependencies).Count -ne 3 -or
    @($manifest.inbox_system_dependencies | Where-Object { $_.distribution_class -ne 'operating-system-inbox-not-bundled' }).Count -ne 0) {
    throw 'Review fixture inbox system dependencies are missing or misclassified.'
}
if ($manifest.generated_media.renderer -ne 'project-owned-gdi-source-pixel-idle-v2' -or
    $manifest.generated_media.visual_source -ne 'verified-sibling-project-owned-mara-camera-sequence-v1' -or
    $manifest.generated_media.visual_mode -ne 'moving-source-controlled-idle-v2' -or
    [string]$manifest.generated_media.portrait_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $manifest.generated_media.portrait_embedded_in_executable -ne $true -or
    $manifest.generated_media.moving_sequence_path -ne 'mara-venn-camera-idle-v1.inpcseq' -or
    [string]$manifest.generated_media.moving_sequence_file_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    [int64]$manifest.generated_media.moving_sequence_file_size_bytes -le 0 -or
    $manifest.generated_media.source_sequence_sha256 -cne '22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d' -or
    $manifest.generated_media.source_raw_pixels_sha256 -cne 'c58602f601d31b06cf1e006a1f65aea4e306e7983dc2b1d80d14bf4e5f094a0b' -or
    $manifest.generated_media.source_frame_count -ne 42 -or $manifest.generated_media.source_frame_width -ne 960 -or
    $manifest.generated_media.source_frame_height -ne 720 -or $manifest.generated_media.source_frame_rate -ne 30 -or
    $manifest.generated_media.source_frame_motion -ne $true -or
    $manifest.generated_media.source_actor_motion -ne $false -or
    $manifest.generated_media.source_camera_motion_only -ne $true -or
    $manifest.generated_media.rendered_actor_motion -ne $true -or
    $manifest.generated_media.rendered_blink_motion -ne $true -or
    $manifest.generated_media.rendered_breathing_motion -ne $true -or
    $manifest.generated_media.source_mouth_motion -ne $false -or
    $manifest.generated_media.source_mouth_articulation -ne $false -or
    $manifest.generated_media.product_lip_sync -ne $false -or
    $manifest.generated_media.renderer_mouth_pixels_preserved -ne $true -or
    $manifest.generated_media.deterministic_rendering -ne $true -or
    $manifest.generated_media.source_advance_passed -ne $true -or
    $manifest.generated_media.generated_probe_frames -ne 6 -or
    $manifest.generated_media.distinct_rendered_frame_hashes -ne 6 -or
    $manifest.generated_media.audio_source -ne 'project-owned-generated-pcm-v1' -or
    $manifest.generated_media.audio_clipped_samples -ne 0 -or
    $manifest.generated_media.audio_loop_boundary_delta -ge 256) {
    throw 'Review fixture moving-source, controlled-idle, mouth-truth, or audio evidence is invalid.'
}
$sequenceFile = Join-Path $root 'mara-venn-camera-idle-v1.inpcseq'
if ((Get-LowerSha256 -Path $sequenceFile) -cne [string]$manifest.generated_media.moving_sequence_file_sha256 -or
    (Get-Item -LiteralPath $sequenceFile).Length -ne [int64]$manifest.generated_media.moving_sequence_file_size_bytes) {
    throw 'Review fixture compressed moving sequence does not match generated-media evidence.'
}
if ($manifest.build.reproducibility_class -ne 'bit-for-bit-deterministic-for-pinned-roslyn-and-framework-reference-inputs' -or
    $manifest.build.reproducible_rebuild_verified -ne $true -or
    [string]$manifest.build.compiler_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    [string]$manifest.build.executable_sha256 -cnotmatch '^[0-9a-f]{64}$') {
    throw 'Review fixture deterministic build evidence is invalid.'
}

$sourcePath = Join-Path $repoRoot ([string]$manifest.source.path)
$sourceManifestPath = Join-Path $repoRoot ([string]$manifest.source.manifest_path)
$portraitPath = Join-Path $repoRoot ([string]$manifest.source.portrait_path)
$portraitProvenancePath = Join-Path $repoRoot ([string]$manifest.source.portrait_provenance_path)
$licensePath = Join-Path $repoRoot ([string]$manifest.license.repository_path)
foreach ($sourceInput in @(
    @{ path = $sourcePath; hash = [string]$manifest.source.sha256 },
    @{ path = $sourceManifestPath; hash = [string]$manifest.source.manifest_sha256 },
    @{ path = $portraitPath; hash = [string]$manifest.source.portrait_sha256 },
    @{ path = $portraitProvenancePath; hash = [string]$manifest.source.portrait_provenance_sha256 },
    @{ path = $licensePath; hash = [string]$manifest.license.sha256 }
)) {
    if (-not (Test-Path -LiteralPath $sourceInput.path -PathType Leaf) -or
        (Get-LowerSha256 -Path $sourceInput.path) -cne $sourceInput.hash) {
        throw "Review fixture source/license provenance mismatch: $($sourceInput.path)"
    }
}
$sourceManifest = Get-Content -LiteralPath $sourceManifestPath -Raw | ConvertFrom-Json
$movingContract = $sourceManifest.moving_sequence
if ($null -eq $movingContract -or
    $movingContract.packaged_path -ne 'mara-venn-camera-idle-v1.inpcseq' -or
    $movingContract.visual_source -ne [string]$manifest.generated_media.visual_source -or
    $movingContract.visual_mode -ne [string]$manifest.generated_media.visual_mode -or
    $movingContract.sequence_sha256 -cne [string]$manifest.generated_media.source_sequence_sha256 -or
    $movingContract.raw_pixels_sha256 -cne [string]$manifest.generated_media.source_raw_pixels_sha256 -or
    $movingContract.origin_portrait_sha256 -cne [string]$manifest.generated_media.portrait_sha256 -or
    $movingContract.source_actor_motion -ne $false -or $movingContract.source_mouth_articulation -ne $false -or
    $movingContract.renderer_adds_controlled_blink -ne $true -or $movingContract.renderer_adds_controlled_breathing -ne $true -or
    $movingContract.product_lip_sync -ne $false) {
    throw 'Review fixture moving sequence is not bound to the repository source manifest.'
}

$sbom = Get-Content -LiteralPath (Join-Path $root 'review-test-game.cdx.json') -Raw | ConvertFrom-Json
if ($sbom.bomFormat -ne 'CycloneDX' -or $sbom.specVersion -ne '1.5' -or
    $sbom.metadata.component.name -ne 'interactive-npcs-synthetic-review-game' -or
    @($sbom.metadata.component.licenses).Count -ne 1 -or
    $sbom.metadata.component.licenses[0].license.id -ne 'MIT' -or @($sbom.components).Count -ne 0) {
    throw 'Review fixture CycloneDX handoff is invalid or lists an unexpected bundled component.'
}

[ordered]@{
    schema_version = 1
    status = 'passed'
    directory = $root
    manifest_path = $manifestPath
    manifest_sha256 = Get-LowerSha256 -Path $manifestPath
    file_count = $actualFiles.Count
    executable_path = Join-Path $root 'interactive-npcs-synthetic-target.exe'
    executable_sha256 = [string]$manifest.build.executable_sha256
    source_sha256 = [string]$manifest.source.sha256
    visual_source = [string]$manifest.generated_media.visual_source
    visual_mode = [string]$manifest.generated_media.visual_mode
    portrait_sha256 = [string]$manifest.generated_media.portrait_sha256
    moving_sequence_path = $sequenceFile
    moving_sequence_sha256 = [string]$manifest.generated_media.moving_sequence_file_sha256
    moving_sequence_size_bytes = [int64]$manifest.generated_media.moving_sequence_file_size_bytes
    source_sequence_sha256 = [string]$manifest.generated_media.source_sequence_sha256
    source_frame_count = [int]$manifest.generated_media.source_frame_count
    source_frame_width = [int]$manifest.generated_media.source_frame_width
    source_frame_height = [int]$manifest.generated_media.source_frame_height
    source_frame_rate = [int]$manifest.generated_media.source_frame_rate
    source_frame_motion = [bool]$manifest.generated_media.source_frame_motion
    source_actor_motion = [bool]$manifest.generated_media.source_actor_motion
    rendered_actor_motion = [bool]$manifest.generated_media.rendered_actor_motion
    rendered_blink_motion = [bool]$manifest.generated_media.rendered_blink_motion
    rendered_breathing_motion = [bool]$manifest.generated_media.rendered_breathing_motion
    source_mouth_motion = [bool]$manifest.generated_media.source_mouth_motion
    source_mouth_articulation = [bool]$manifest.generated_media.source_mouth_articulation
    product_lip_sync = [bool]$manifest.generated_media.product_lip_sync
    renderer_mouth_pixels_preserved = [bool]$manifest.generated_media.renderer_mouth_pixels_preserved
    license_expression = [string]$manifest.license.expression
    third_party_binary_count = @($manifest.third_party_binaries).Count
} | ConvertTo-Json -Depth 4
