[CmdletBinding()]
param(
    [string]$DestinationDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') { throw 'The review test game can only be prepared on Windows.' }

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if ([string]::IsNullOrWhiteSpace($DestinationDirectory)) {
    $DestinationDirectory = Join-Path (Split-Path -Parent $repoRoot) 'local-app-data/test-game'
}
if (-not [System.IO.Path]::IsPathRooted($DestinationDirectory)) { $DestinationDirectory = Join-Path $repoRoot $DestinationDirectory }
$destination = [System.IO.Path]::GetFullPath($DestinationDirectory)
$destinationLeaf = Split-Path -Leaf $destination
$destinationParentLeaf = Split-Path -Leaf (Split-Path -Parent $destination)
if ($destinationLeaf -ne 'test-game' -or $destinationParentLeaf -ne 'local-app-data') {
    throw "Review test-game destination must end in local-app-data\test-game: $destination"
}
if (Test-Path -LiteralPath $destination) {
    $existing = Get-Item -LiteralPath $destination -Force
    if (-not $existing.PSIsContainer -or ($existing.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "Review test-game destination must be a normal directory, not a file or reparse point: $destination"
    }
}

function Get-LowerSha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Utf8NoBom {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Content)
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

$rawValidation = & (Join-Path $repoRoot 'scripts/synthetic-game-replay.ps1') -ValidateOnly
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$validation = $rawValidation | ConvertFrom-Json
if ($validation.status -ne 'passed' -or $validation.fixture_source -ne 'project-source-generated-native-v1' -or
    $validation.renderer -ne 'project-owned-gdi-generated-v1' -or $validation.generated_probe_frames -ne 2 -or
    $validation.frames_differ -ne $true -or
    $validation.visual_source -ne 'embedded-original-generated-photorealistic-portrait-v1' -or
    [string]$validation.portrait_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $validation.source_mouth_motion -ne $false -or $validation.mouth_region_invariant -ne $true -or
    $validation.audio_source -ne 'project-owned-generated-pcm-v1' -or
    $validation.audio_nonzero_samples -le 0 -or $validation.audio_clipped_samples -ne 0 -or
    $validation.hardware_acceleration -ne $false -or $validation.nvidia_compute_requested -ne $false -or
    @($validation.third_party_binaries_bundled).Count -ne 0 -or $validation.license_expression -ne 'MIT') {
    throw 'The project-owned synthetic target did not pass generated-frame, PCM, provenance, and no-third-party validation.'
}

foreach ($source in @(
    [string]$validation.player_path,
    [string]$validation.source_path,
    [string]$validation.source_manifest_path,
    [string]$validation.portrait_path,
    [string]$validation.portrait_provenance_path,
    [string]$validation.license_path,
    [string]$validation.build_receipt_path
)) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Review test-game source input is missing: $source" }
}
if ((Get-LowerSha256 -Path ([string]$validation.player_path)) -cne [string]$validation.player_sha256 -or
    (Get-LowerSha256 -Path ([string]$validation.source_path)) -cne [string]$validation.source_sha256 -or
    (Get-LowerSha256 -Path ([string]$validation.portrait_path)) -cne [string]$validation.portrait_sha256 -or
    (Get-LowerSha256 -Path ([string]$validation.portrait_provenance_path)) -cne [string]$validation.portrait_provenance_sha256 -or
    (Get-LowerSha256 -Path ([string]$validation.source_manifest_path)) -cne [string]$validation.source_manifest_sha256 -or
    (Get-LowerSha256 -Path ([string]$validation.license_path)) -cne [string]$validation.license_sha256) {
    throw 'Synthetic target source, executable, manifest, or license changed after validation.'
}

$parent = Split-Path -Parent $destination
New-Item -ItemType Directory -Path $parent -Force | Out-Null
$stage = Join-Path $parent ("test-game.stage-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    $executablePath = Join-Path $stage 'interactive-npcs-synthetic-target.exe'
    $readmePath = Join-Path $stage 'README.md'
    $noticesPath = Join-Path $stage 'THIRD-PARTY-NOTICES.md'
    $sbomPath = Join-Path $stage 'review-test-game.cdx.json'
    $distributionManifestPath = Join-Path $stage 'REVIEW-FIXTURE-MANIFEST.json'
    Copy-Item -LiteralPath ([string]$validation.player_path) -Destination $executablePath
    if ((Get-LowerSha256 -Path $executablePath) -cne [string]$validation.player_sha256) { throw 'Copied synthetic executable hash mismatch.' }
    & (Join-Path $repoRoot 'scripts/assert-pe-subsystem.ps1') -Path $executablePath -Expected Gui | Out-Null
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $licenseText = Get-Content -LiteralPath ([string]$validation.license_path) -Raw
    $notices = @'
# Synthetic review fixture licensing and notices

The executable, documentation, original AI-generated fictional portrait, and
generated PCM in this folder are project-owned source-built material licensed
under MIT. The portrait and its provenance are embedded into the executable;
there is no loose media payload. No third-party executable, codec, model,
captured game media, voice recording, or font file is bundled. Windows .NET
Framework, Windows Forms/GDI+, and Windows
SoundPlayer/waveOut are operating-system inbox dependencies and are not copied
into this folder.

Source: `scripts/synthetic-game-replay/SyntheticGameReplay.cs`
Source manifest: `scripts/synthetic-game-replay/SOURCE-MANIFEST.json`

## MIT License

'@ + $licenseText
    Write-Utf8NoBom -Path $noticesPath -Content $notices

    $readme = @'
# Interactive NPCs synthetic review game

This is the standalone, project-owned Eclipse Harbor Windows capture target for
local Interactive NPCs 2.0 review. The executable embeds an original realistic
portrait of the fictional Mara Venn, keeps her source mouth pixel-static, adds
only peripheral GDI motion, and generates quiet looping PCM ambience in memory.
This makes any later mouth motion attributable to the Interactive NPCs overlay.
It does not load FFmpeg, a video file, a model, CUDA, NVIDIA compute, captured
game content, or any other third-party binary from this folder or from PATH.

Double-click `interactive-npcs-synthetic-target.exe`, or use the repository
launcher for validated PID/HWND metadata and optional second-monitor placement:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File ".\scripts\synthetic-game-replay.ps1" -PlaceOnSecondMonitor
```

`REVIEW-FIXTURE-MANIFEST.json` is the fail-closed file allowlist and provenance
record. `review-test-game.cdx.json` is the CycloneDX handoff. See
`THIRD-PARTY-NOTICES.md` for the complete no-third-party-binary notice and MIT
license text.
'@
    Write-Utf8NoBom -Path $readmePath -Content $readme

    $sbom = [ordered]@{
        bomFormat = 'CycloneDX'
        specVersion = '1.5'
        serialNumber = 'urn:uuid:4b5bc0ae-a9f5-5c7d-b86e-25c06df78d9a'
        version = 1
        metadata = [ordered]@{
            component = [ordered]@{
                type = 'application'
                'bom-ref' = 'pkg:generic/interactive-npcs-synthetic-review-game@1.1.0'
                group = 'io.github.akshitireddy'
                name = 'interactive-npcs-synthetic-review-game'
                version = '1.1.0'
                hashes = @([ordered]@{ alg = 'SHA-256'; content = Get-LowerSha256 -Path $executablePath })
                licenses = @([ordered]@{ license = [ordered]@{ id = 'MIT' } })
                properties = @(
                    [ordered]@{ name = 'interactive-npcs:distribution-scope'; value = 'host-local-review-fixture' },
                    [ordered]@{ name = 'interactive-npcs:distribution-class'; value = 'project-owned-source-built-review-fixture' },
                    [ordered]@{ name = 'interactive-npcs:source-sha256'; value = [string]$validation.source_sha256 },
                    [ordered]@{ name = 'interactive-npcs:source-manifest-sha256'; value = [string]$validation.source_manifest_sha256 },
                    [ordered]@{ name = 'interactive-npcs:visual-source'; value = [string]$validation.visual_source },
                    [ordered]@{ name = 'interactive-npcs:portrait-sha256'; value = [string]$validation.portrait_sha256 },
                    [ordered]@{ name = 'interactive-npcs:source-mouth-motion'; value = 'false' },
                    [ordered]@{ name = 'interactive-npcs:compiler-sha256'; value = [string]$validation.compiler.sha256 },
                    [ordered]@{ name = 'interactive-npcs:third-party-binaries-bundled'; value = 'false' }
                )
            }
        }
        components = @()
        dependencies = @([ordered]@{ ref = 'pkg:generic/interactive-npcs-synthetic-review-game@1.1.0'; dependsOn = @() })
    }
    Write-Utf8NoBom -Path $sbomPath -Content (($sbom | ConvertTo-Json -Depth 10) + [Environment]::NewLine)

    $fileMetadata = @{
        'interactive-npcs-synthetic-target.exe' = @{ component = 'project:synthetic-review-target'; class = 'project-owned-source-built-binary'; source = 'scripts/synthetic-game-replay/SyntheticGameReplay.cs' }
        'README.md' = @{ component = 'project:legal-resources'; class = 'project-owned-documentation'; source = 'scripts/windows/prepare-review-test-game.ps1' }
        'THIRD-PARTY-NOTICES.md' = @{ component = 'project:legal-resources'; class = 'project-owned-license-and-notices'; source = 'LICENSE' }
        'review-test-game.cdx.json' = @{ component = 'project:legal-resources'; class = 'project-owned-sbom'; source = 'scripts/windows/prepare-review-test-game.ps1' }
    }
    $manifestEntries = foreach ($name in @('interactive-npcs-synthetic-target.exe', 'README.md', 'THIRD-PARTY-NOTICES.md', 'review-test-game.cdx.json')) {
        $path = Join-Path $stage $name
        [ordered]@{
            path = $name
            sha256 = Get-LowerSha256 -Path $path
            size_bytes = (Get-Item -LiteralPath $path).Length
            component_id = [string]$fileMetadata[$name].component
            spdx = 'MIT'
            distribution_scope = 'review-test-game'
            distribution_class = [string]$fileMetadata[$name].class
            source_reference = [string]$fileMetadata[$name].source
            notice_reference = 'THIRD-PARTY-NOTICES.md'
        }
    }
    $distributionManifest = [ordered]@{
        schema_version = 1
        component_id = 'project:synthetic-review-target'
        component_version = '1.1.0'
        fixture_kind_compatibility = 'synthetic-original-video-replay'
        fixture_source = 'project-source-generated-native-v1'
        distribution_scope = 'review-test-game'
        distribution_class = 'project-owned-source-built-review-fixture'
        expected_files = @(
            'interactive-npcs-synthetic-target.exe',
            'README.md',
            'REVIEW-FIXTURE-MANIFEST.json',
            'THIRD-PARTY-NOTICES.md',
            'review-test-game.cdx.json'
        )
        manifest_self = [ordered]@{
            path = 'REVIEW-FIXTURE-MANIFEST.json'
            hash = 'excluded-to-avoid-circularity'
        }
        files = @($manifestEntries)
        source = [ordered]@{
            path = 'scripts/synthetic-game-replay/SyntheticGameReplay.cs'
            sha256 = [string]$validation.source_sha256
            manifest_path = 'scripts/synthetic-game-replay/SOURCE-MANIFEST.json'
            manifest_sha256 = [string]$validation.source_manifest_sha256
            build_receipt_sha256 = Get-LowerSha256 -Path ([string]$validation.build_receipt_path)
            portrait_path = 'scripts/synthetic-game-replay/assets/mara-venn-portrait-v1.png'
            portrait_sha256 = [string]$validation.portrait_sha256
            portrait_provenance_path = 'scripts/synthetic-game-replay/assets/PROVENANCE.md'
            portrait_provenance_sha256 = [string]$validation.portrait_provenance_sha256
        }
        license = [ordered]@{
            expression = 'MIT'
            repository_path = 'LICENSE'
            sha256 = [string]$validation.license_sha256
            bundled_notice = 'THIRD-PARTY-NOTICES.md'
        }
        build = [ordered]@{
            compiler_path_class = 'windows-inbox-dotnet-framework-csc'
            compiler_sha256 = [string]$validation.compiler.sha256
            compiler_file_version = [string]$validation.compiler.file_version
            compiler_authenticode_status = [string]$validation.compiler.authenticode_status
            compiler_signer_subject = [string]$validation.compiler.signer_subject
            executable_sha256 = Get-LowerSha256 -Path $executablePath
            reproducibility_class = 'bit-for-bit-deterministic-for-pinned-roslyn-and-framework-reference-inputs'
            reproducible_rebuild_verified = $true
        }
        generated_media = [ordered]@{
            renderer = 'project-owned-gdi-generated-v1'
            visual_source = [string]$validation.visual_source
            portrait_sha256 = [string]$validation.portrait_sha256
            portrait_embedded_in_executable = $true
            source_mouth_motion = $false
            mouth_region_invariant = [bool]$validation.mouth_region_invariant
            generated_probe_frames = [int]$validation.generated_probe_frames
            frames_differ = [bool]$validation.frames_differ
            audio_source = 'project-owned-generated-pcm-v1'
            audio_sha256 = [string]$validation.audio_sha256
            audio_duration_seconds = [int]$validation.audio_duration_seconds
            audio_sample_rate = [int]$validation.audio_sample_rate
            audio_peak = [int]$validation.audio_peak
            audio_rms = [double]$validation.audio_rms
            audio_clipped_samples = [int]$validation.audio_clipped_samples
            audio_loop_boundary_delta = [int]$validation.audio_loop_boundary_delta
        }
        third_party_binaries = @()
        inbox_system_dependencies = @(
            [ordered]@{ component_id = 'windows-dotnet-framework-4'; distribution_class = 'operating-system-inbox-not-bundled' },
            [ordered]@{ component_id = 'windows-forms-gdi-plus'; distribution_class = 'operating-system-inbox-not-bundled' },
            [ordered]@{ component_id = 'windows-soundplayer-waveout'; distribution_class = 'operating-system-inbox-not-bundled' }
        )
    }
    Write-Utf8NoBom -Path $distributionManifestPath -Content (($distributionManifest | ConvertTo-Json -Depth 10) + [Environment]::NewLine)

    $expectedNames = @($distributionManifest.expected_files | Sort-Object)
    $actualFiles = @(Get-ChildItem -LiteralPath $stage -File -Force | Sort-Object Name)
    $actualNames = @($actualFiles.Name | Sort-Object)
    if (($actualNames -join "`n") -cne ($expectedNames -join "`n")) { throw 'Staged review fixture contains a missing or unknown file.' }
    if (@(Get-ChildItem -LiteralPath $stage -Directory -Force).Count -ne 0) { throw 'Staged review fixture must not contain directories.' }
    foreach ($entry in $manifestEntries) {
        $file = Join-Path $stage ([string]$entry.path)
        if ((Get-LowerSha256 -Path $file) -cne [string]$entry.sha256 -or (Get-Item -LiteralPath $file).Length -ne [int64]$entry.size_bytes) {
            throw "Staged review fixture hash or size mismatch: $($entry.path)"
        }
    }

    if (Test-Path -LiteralPath $destination) { Remove-Item -LiteralPath $destination -Recurse -Force }
    Move-Item -LiteralPath $stage -Destination $destination
    $stage = $null

    $verificationJson = & (Join-Path $PSScriptRoot 'verify-review-test-game.ps1') -Directory $destination
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $verification = $verificationJson | ConvertFrom-Json
    if ($verification.status -ne 'passed' -or $verification.file_count -ne 5 -or $verification.third_party_binary_count -ne 0) {
        throw 'Prepared review fixture failed its final allowlist/provenance verification.'
    }

    $finalExecutable = Join-Path $destination 'interactive-npcs-synthetic-target.exe'
    $finalManifest = Join-Path $destination 'REVIEW-FIXTURE-MANIFEST.json'
    $finalSbom = Join-Path $destination 'review-test-game.cdx.json'
    $finalNotices = Join-Path $destination 'THIRD-PARTY-NOTICES.md'
    [ordered]@{
        schema_version = 2
        status = 'prepared'
        fixture_kind = 'synthetic-original-video-replay'
        fixture_source = 'project-source-generated-native-v1'
        directory = $destination
        executable_path = $finalExecutable
        executable_basename = [System.IO.Path]::GetFileName($finalExecutable)
        executable_sha256 = Get-LowerSha256 -Path $finalExecutable
        executable_subsystem = 'windows_gui'
        renderer = 'project-owned-gdi-generated-v1'
        visual_source = [string]$validation.visual_source
        portrait_sha256 = [string]$validation.portrait_sha256
        portrait_embedded_in_executable = $true
        source_mouth_motion = $false
        mouth_region_invariant = [bool]$validation.mouth_region_invariant
        generated_frame_rendering = $true
        audio_source = 'project-owned-generated-pcm-v1'
        audio_generated_pcm = $true
        hardware_acceleration = $false
        nvidia_compute_requested = $false
        media_files_bundled = 0
        third_party_binaries_bundled = @()
        distribution_manifest_path = $finalManifest
        distribution_manifest_sha256 = Get-LowerSha256 -Path $finalManifest
        sbom_path = $finalSbom
        sbom_sha256 = Get-LowerSha256 -Path $finalSbom
        notices_path = $finalNotices
        notices_sha256 = Get-LowerSha256 -Path $finalNotices
        source_path = [string]$validation.source_path
        source_sha256 = [string]$validation.source_sha256
        source_manifest_path = [string]$validation.source_manifest_path
        source_manifest_sha256 = [string]$validation.source_manifest_sha256
        license_path = [string]$validation.license_path
        license_sha256 = [string]$validation.license_sha256
        license_expression = 'MIT'
        build_receipt_path = [string]$validation.build_receipt_path
        build_receipt_sha256 = Get-LowerSha256 -Path ([string]$validation.build_receipt_path)
        launcher_path = Join-Path $repoRoot 'scripts/synthetic-game-replay.ps1'
    } | ConvertTo-Json -Depth 6
}
finally {
    if ($null -ne $stage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
