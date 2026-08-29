[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$launcher = Join-Path $PSScriptRoot 'synthetic-game-replay.ps1'
$raw = & $launcher -ValidateOnly
if ($LASTEXITCODE -ne 0) {
    throw "Synthetic replay validation exited with $LASTEXITCODE."
}

$result = $raw | ConvertFrom-Json
if ($result.status -ne 'passed') { throw 'Synthetic replay validation did not pass.' }
if ($result.fixture_kind -ne 'synthetic-original-video-replay') { throw 'Unexpected fixture classification.' }
if (-not $result.canonical_fixture) { throw 'The default fixture must be the canonical original Eclipse Harbor MP4.' }
if ($result.hardware_acceleration) { throw 'The replay decode probe must not request hardware acceleration.' }
if ($result.nvidia_compute_requested) { throw 'The replay decode probe must not request NVIDIA compute.' }
if ([int]$result.decoded_probe_frames -ne 1) { throw 'The CPU decode probe did not verify one video frame.' }
if ($result.executable_basename -ne 'interactive-npcs-synthetic-target.exe') { throw 'The fixture executable basename is not stable.' }
if (-not (Test-Path -LiteralPath $result.player_path -PathType Leaf)) { throw 'The fixture player was not compiled.' }

[ordered]@{
    status = 'passed'
    fixture_sha256 = [string]$result.input_sha256
    player_path = [string]$result.player_path
    decoded_probe_frames = [int]$result.decoded_probe_frames
    hardware_acceleration = [bool]$result.hardware_acceleration
    nvidia_compute_requested = [bool]$result.nvidia_compute_requested
} | ConvertTo-Json -Depth 3
