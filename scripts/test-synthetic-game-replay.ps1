[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$launcher = Join-Path $PSScriptRoot 'synthetic-game-replay.ps1'
$source = Join-Path $PSScriptRoot 'synthetic-game-replay\SyntheticGameReplay.cs'
$sourceManifest = Join-Path $PSScriptRoot 'synthetic-game-replay\SOURCE-MANIFEST.json'
$raw = & $launcher -ValidateOnly
if ($LASTEXITCODE -ne 0) { throw "Synthetic target validation exited with $LASTEXITCODE." }
$result = $raw | ConvertFrom-Json

if ($result.schema_version -ne 2 -or $result.status -ne 'passed') { throw 'Synthetic target validation did not pass schema v2.' }
if ($result.fixture_kind -ne 'synthetic-original-video-replay') { throw 'Debug bridge compatibility fixture kind changed.' }
if ($result.fixture_source -ne 'project-source-generated-native-v1') { throw 'Unexpected project-owned fixture source.' }
if ($result.renderer -ne 'project-owned-gdi-generated-v1' -or $result.generated_probe_frames -ne 2 -or
    $result.frames_differ -ne $true) { throw 'Generated-frame renderer did not produce two distinct real frames.' }
if ($result.audio_source -ne 'project-owned-generated-pcm-v1' -or $result.audio_duration_seconds -ne 12 -or
    $result.audio_sample_rate -ne 22050 -or $result.audio_nonzero_samples -le 0 -or
    $result.audio_peak -le 256 -or $result.audio_peak -ge 32767 -or $result.audio_rms -le 128 -or
    $result.audio_clipped_samples -ne 0 -or $result.audio_loop_boundary_delta -ge 256) {
    throw 'Generated PCM did not pass duration, non-silence, headroom, and loop-boundary checks.'
}
if ($result.hardware_acceleration -ne $false -or $result.nvidia_compute_requested -ne $false) {
    throw 'Synthetic target must not request hardware acceleration or NVIDIA compute.'
}
if (@($result.third_party_binaries_bundled).Count -ne 0) { throw 'Synthetic target validation unexpectedly lists a third-party binary.' }
if ($result.license_expression -ne 'MIT') { throw 'Synthetic target license expression must be MIT.' }
$unknownArgumentProcess = Start-Process -FilePath ([string]$result.player_path) `
    -ArgumentList @('--not-a-review-target-option', 'value') -Wait -PassThru
if ($unknownArgumentProcess.ExitCode -eq 0) { throw 'Synthetic target accepted an unknown command-line option.' }
foreach ($binding in @(
    @{ path = [string]$result.source_path; hash = [string]$result.source_sha256 },
    @{ path = [string]$result.source_manifest_path; hash = [string]$result.source_manifest_sha256 },
    @{ path = [string]$result.license_path; hash = [string]$result.license_sha256 },
    @{ path = [string]$result.player_path; hash = [string]$result.player_sha256 }
)) {
    if (-not (Test-Path -LiteralPath $binding.path -PathType Leaf) -or
        (Get-FileHash -LiteralPath $binding.path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $binding.hash) {
        throw "Synthetic target provenance binding failed: $($binding.path)"
    }
}
$receipt = Get-Content -LiteralPath ([string]$result.build_receipt_path) -Raw | ConvertFrom-Json
if ($receipt.reproducibility_class -ne 'bit-for-bit-deterministic-for-pinned-roslyn-and-framework-reference-inputs' -or
    $receipt.reproducible_rebuild_verified -ne $true -or $receipt.compiler.sdk_version -ne '9.0.302' -or
    $receipt.compiler.authenticode_status -ne 'Valid' -or @($receipt.compiler.framework_references).Count -ne 6) {
    throw 'Pinned compiler/reference or bit-for-bit reproducibility evidence is invalid.'
}
if ((Get-Content -LiteralPath $source -TotalCount 1) -cne '// SPDX-License-Identifier: MIT') {
    throw 'Synthetic target source SPDX header is missing.'
}
$launcherText = Get-Content -LiteralPath $launcher -Raw
$sourceText = Get-Content -LiteralPath $source -Raw
foreach ($forbidden in @('Get-Command ffmpeg', 'ffmpeg-static', 'C:\ffmpeg', '--ffmpeg')) {
    if ($launcherText.IndexOf($forbidden, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $sourceText.IndexOf($forbidden, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "Synthetic target reintroduced an ambient FFmpeg path: $forbidden"
    }
}
$manifest = Get-Content -LiteralPath $sourceManifest -Raw | ConvertFrom-Json
if (@($manifest.third_party_binaries).Count -ne 0 -or $manifest.build_toolchain.sdk_version -ne '9.0.302') {
    throw 'Synthetic source manifest does not retain the pinned no-third-party contract.'
}

[ordered]@{
    schema_version = 2
    status = 'passed'
    fixture_source = [string]$result.fixture_source
    player_path = [string]$result.player_path
    player_sha256 = [string]$result.player_sha256
    source_sha256 = [string]$result.source_sha256
    generated_probe_frames = [int]$result.generated_probe_frames
    frames_differ = [bool]$result.frames_differ
    audio_peak = [int]$result.audio_peak
    audio_rms = [double]$result.audio_rms
    audio_clipped_samples = [int]$result.audio_clipped_samples
    audio_loop_boundary_delta = [int]$result.audio_loop_boundary_delta
    reproducible_rebuild_verified = [bool]$receipt.reproducible_rebuild_verified
    third_party_binary_count = @($result.third_party_binaries_bundled).Count
    unknown_argument_rejected = $true
} | ConvertTo-Json -Depth 4
