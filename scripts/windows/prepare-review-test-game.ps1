[CmdletBinding()]
param(
    [string]$DestinationDirectory,
    # The moving source footage is optional. When it is missing or empty (or -StaticOnly is passed) the script
    # builds the five-file static portrait fixture, which is what ships now.
    [string]$MovingSequenceDirectory = 'E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1',
    [switch]$StaticOnly
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

$staticOnly = [bool]$StaticOnly
if (-not $staticOnly) {
    $movingSourceAvailable = (Test-Path -LiteralPath $MovingSequenceDirectory -PathType Container) -and
        @(Get-ChildItem -LiteralPath $MovingSequenceDirectory -File -Filter 'frame-*.ppm' -Force -ErrorAction SilentlyContinue).Count -gt 0
    if (-not $movingSourceAvailable) {
        Write-Warning "Moving source frames are unavailable at '$MovingSequenceDirectory'; preparing the static portrait fixture."
        $staticOnly = $true
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

function ConvertTo-NativeCommandLine {
    param([Parameter(Mandatory)][string[]]$Arguments)
    return (($Arguments | ForEach-Object {
        $argument = [string]$_
        if ($argument.Length -eq 0) { return '""' }
        if ($argument -notmatch '[\s"]') { return $argument }
        $escaped = [System.Text.RegularExpressions.Regex]::Replace($argument, '(\\*)"', '$1$1\"')
        $escaped = [System.Text.RegularExpressions.Regex]::Replace($escaped, '(\\+)$', '$1$1')
        return '"' + $escaped + '"'
    }) -join ' ')
}

function New-VerifiedMovingSequence {
    param(
        [Parameter(Mandatory)][string]$SourceDirectory,
        [Parameter(Mandatory)]$Contract,
        [Parameter(Mandatory)][string]$OutputPath
    )
    $sourceRoot = [System.IO.Path]::GetFullPath($SourceDirectory)
    if (-not (Test-Path -LiteralPath $sourceRoot -PathType Container)) { throw "Moving source directory is missing: $sourceRoot" }
    $sourceRootItem = Get-Item -LiteralPath $sourceRoot -Force
    if ($sourceRootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Moving source directory must not be a reparse point.' }
    $expectedNames = @(1..([int]$Contract.frame_count) | ForEach-Object { 'frame-{0:D5}.ppm' -f $_ })
    $frames = @(Get-ChildItem -LiteralPath $sourceRoot -File -Filter 'frame-*.ppm' -Force | Where-Object Name -Match '^frame-\d{5}\.ppm$' | Sort-Object Name)
    if ([string]::Join([char]10, $frames.Name) -cne [string]::Join([char]10, $expectedNames)) { throw 'Moving source frame names/count do not match the pinned manifest.' }
    if (@($Contract.frame_sha256).Count -ne $frames.Count) { throw 'Moving source manifest frame hash count is invalid.' }
    $canonical = New-Object System.Text.StringBuilder
    $rawHasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        for ($index = 0; $index -lt $frames.Count; $index++) {
            $frame = $frames[$index]
            if ($frame.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw "Moving source frame must not be a reparse point: $($frame.Name)" }
            if ($frame.Length -ne [int64]$Contract.frame_file_size_bytes) { throw "Moving source frame size mismatch: $($frame.Name)" }
            $frameHash = Get-LowerSha256 -Path $frame.FullName
            if ($frameHash -cne [string]$Contract.frame_sha256[$index]) { throw "Moving source frame hash mismatch: $($frame.Name)" }
            [void]$canonical.Append($frame.Name).Append([char]10).Append($frameHash).Append([char]10).Append($frame.Length.ToString([Globalization.CultureInfo]::InvariantCulture)).Append([char]10)
            $bytes = [System.IO.File]::ReadAllBytes($frame.FullName)
            $headerBytes = [System.Text.Encoding]::ASCII.GetBytes([string]$Contract.ppm_header_ascii)
            if ($bytes.Length -le $headerBytes.Length) { throw "Moving source frame is truncated: $($frame.Name)" }
            for ($headerIndex = 0; $headerIndex -lt $headerBytes.Length; $headerIndex++) {
                if ($bytes[$headerIndex] -ne $headerBytes[$headerIndex]) { throw "Moving source PPM header mismatch: $($frame.Name)" }
            }
            [void]$rawHasher.TransformBlock($bytes, $headerBytes.Length, $bytes.Length - $headerBytes.Length, $null, 0)
        }
        [void]$rawHasher.TransformFinalBlock([byte[]]::new(0), 0, 0)
        $rawPixelsHash = -join ($rawHasher.Hash | ForEach-Object { $_.ToString('x2') })
    }
    finally { $rawHasher.Dispose() }
    $sequenceHasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $sequenceBytes = [System.Text.Encoding]::UTF8.GetBytes($canonical.ToString())
        $sequenceHash = -join ($sequenceHasher.ComputeHash($sequenceBytes) | ForEach-Object { $_.ToString('x2') })
    }
    finally { $sequenceHasher.Dispose() }
    if ($sequenceHash -cne [string]$Contract.sequence_sha256 -or $rawPixelsHash -cne [string]$Contract.raw_pixels_sha256) {
        throw 'Moving source aggregate identity does not match the pinned manifest.'
    }
    $stream = [System.IO.File]::Open($OutputPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        $writer = [System.IO.BinaryWriter]::new($stream, [System.Text.Encoding]::ASCII, $true)
        try {
            $writer.Write([System.Text.Encoding]::ASCII.GetBytes('INPCSEQ2'))
            $writer.Write([int]1)
            $writer.Write([int]$Contract.frame_count)
            $writer.Write([int]$Contract.frame_width)
            $writer.Write([int]$Contract.frame_height)
            $writer.Write([int]$Contract.frame_rate)
            $writer.Write([System.Text.Encoding]::ASCII.GetBytes($sequenceHash))
            $writer.Write([System.Text.Encoding]::ASCII.GetBytes($rawPixelsHash))
            $writer.Flush()
        }
        finally { $writer.Dispose() }
        $deflater = [System.IO.Compression.DeflateStream]::new($stream, [System.IO.Compression.CompressionLevel]::Optimal, $true)
        try {
            $headerLength = [System.Text.Encoding]::ASCII.GetByteCount([string]$Contract.ppm_header_ascii)
            foreach ($frame in $frames) {
                $bytes = [System.IO.File]::ReadAllBytes($frame.FullName)
                $deflater.Write($bytes, $headerLength, $bytes.Length - $headerLength)
            }
        }
        finally { $deflater.Dispose() }
    }
    finally { $stream.Dispose() }
    return [pscustomobject][ordered]@{
        path = $OutputPath
        sha256 = Get-LowerSha256 -Path $OutputPath
        size_bytes = (Get-Item -LiteralPath $OutputPath).Length
        source_sequence_sha256 = $sequenceHash
        raw_pixels_sha256 = $rawPixelsHash
        raw_pixels_bytes = [int64]$Contract.raw_pixels_bytes
        frame_count = [int]$Contract.frame_count
        frame_width = [int]$Contract.frame_width
        frame_height = [int]$Contract.frame_height
        frame_rate = [int]$Contract.frame_rate
    }
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
$sourceManifest = Get-Content -LiteralPath ([string]$validation.source_manifest_path) -Raw | ConvertFrom-Json
$movingContract = $sourceManifest.moving_sequence
if (-not $staticOnly -and ($null -eq $movingContract -or
    $movingContract.visual_source -ne 'verified-sibling-project-owned-mara-camera-sequence-v1' -or
    $movingContract.visual_mode -ne 'moving-source-controlled-idle-v2' -or
    $movingContract.frame_count -ne 42 -or $movingContract.frame_width -ne 960 -or
    $movingContract.frame_height -ne 720 -or $movingContract.frame_rate -ne 30 -or
    $movingContract.sequence_sha256 -cne '22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d' -or
    $movingContract.raw_pixels_sha256 -cne 'c58602f601d31b06cf1e006a1f65aea4e306e7983dc2b1d80d14bf4e5f094a0b' -or
    $movingContract.origin_portrait_sha256 -cne [string]$validation.portrait_sha256 -or
    $movingContract.source_frame_motion -ne $true -or $movingContract.source_actor_motion -ne $false -or
    $movingContract.source_camera_motion_only -ne $true -or $movingContract.source_mouth_articulation -ne $false -or
    $movingContract.renderer_adds_controlled_blink -ne $true -or $movingContract.renderer_adds_controlled_breathing -ne $true -or
    $movingContract.product_lip_sync -ne $false)) {
    throw 'The moving source contract is missing, drifted, or makes an unsupported actor/lip-sync claim.'
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
    if (-not $staticOnly) {
        $sequencePath = Join-Path $stage ([string]$movingContract.packaged_path)
        $sequenceEvidence = New-VerifiedMovingSequence -SourceDirectory $MovingSequenceDirectory -Contract $movingContract -OutputPath $sequencePath
        $sequenceReproPath = Join-Path $stage '.sequence-repro.tmp'
        try {
            $sequenceReproEvidence = New-VerifiedMovingSequence -SourceDirectory $MovingSequenceDirectory -Contract $movingContract -OutputPath $sequenceReproPath
            if ($sequenceReproEvidence.sha256 -cne $sequenceEvidence.sha256 -or $sequenceReproEvidence.size_bytes -ne $sequenceEvidence.size_bytes) {
                throw 'The compressed moving sequence did not reproduce bit-for-bit.'
            }
        }
        finally {
            if (Test-Path -LiteralPath $sequenceReproPath -PathType Leaf) { Remove-Item -LiteralPath $sequenceReproPath -Force }
        }

        $movingValidationRoot = Join-Path (Join-Path (Split-Path -Parent $repoRoot) '.cache\review-game-validation') ([string]$movingContract.sequence_sha256)
        $movingSelfTestPath = Join-Path $movingValidationRoot 'self-test.json'
        $movingSelfTestFrames = Join-Path $movingValidationRoot 'frames'
        New-Item -ItemType Directory -Path $movingSelfTestFrames -Force | Out-Null
        if (Test-Path -LiteralPath $movingSelfTestPath -PathType Leaf) { Remove-Item -LiteralPath $movingSelfTestPath -Force }
        $selfTestArguments = @(
            '--mode', 'moving', '--sequence', $sequencePath,
            '--self-test-report', $movingSelfTestPath, '--self-test-frame-directory', $movingSelfTestFrames,
            '--width', '960', '--height', '600', '--mute'
        )
        $movingSelfTestProcess = Start-Process -FilePath $executablePath -ArgumentList (ConvertTo-NativeCommandLine -Arguments $selfTestArguments) -WindowStyle Hidden -Wait -PassThru
        if ($movingSelfTestProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $movingSelfTestPath -PathType Leaf)) {
            throw "Moving synthetic target self-test failed with exit code $($movingSelfTestProcess.ExitCode)."
        }
        $movingSelfTest = Get-Content -LiteralPath $movingSelfTestPath -Raw | ConvertFrom-Json
        if ($movingSelfTest.schema_version -ne 2 -or $movingSelfTest.status -ne 'passed' -or
            $movingSelfTest.fixture_source -ne 'project-source-generated-native-v2' -or
            $movingSelfTest.renderer -ne 'project-owned-gdi-source-pixel-idle-v2' -or
            $movingSelfTest.visual_source -ne [string]$movingContract.visual_source -or
            $movingSelfTest.visual_mode -ne [string]$movingContract.visual_mode -or
            $movingSelfTest.source_sequence_sha256 -cne [string]$movingContract.sequence_sha256 -or
            $movingSelfTest.source_raw_pixels_sha256 -cne [string]$movingContract.raw_pixels_sha256 -or
            $movingSelfTest.source_frame_count -ne 42 -or $movingSelfTest.source_frame_width -ne 960 -or
            $movingSelfTest.source_frame_height -ne 720 -or $movingSelfTest.source_frame_rate -ne 30 -or
            $movingSelfTest.source_frame_motion -ne $true -or $movingSelfTest.source_actor_motion -ne $false -or
            $movingSelfTest.source_camera_motion_only -ne $true -or $movingSelfTest.rendered_actor_motion -ne $true -or
            $movingSelfTest.rendered_blink_motion -ne $true -or $movingSelfTest.rendered_breathing_motion -ne $true -or
            $movingSelfTest.source_mouth_articulation -ne $false -or $movingSelfTest.product_lip_sync -ne $false -or
            $movingSelfTest.renderer_mouth_pixels_preserved -ne $true -or $movingSelfTest.deterministic_rendering -ne $true -or
            $movingSelfTest.source_advance_passed -ne $true -or $movingSelfTest.distinct_source_indices -lt 5 -or
            $movingSelfTest.generated_probe_frames -ne 6 -or $movingSelfTest.distinct_rendered_frame_hashes -ne 6 -or
            $movingSelfTest.audio_clipped_samples -ne 0 -or $movingSelfTest.third_party_binaries_loaded -ne $false) {
            throw 'The moving synthetic target failed truthful source, controlled-idle, deterministic, or mouth-preservation validation.'
        }
    }

    $licenseText = Get-Content -LiteralPath ([string]$validation.license_path) -Raw
    if ($staticOnly) {
        $notices = @'
# Synthetic review fixture licensing and notices

The executable, documentation, original AI-generated fictional portrait, and
generated PCM in this folder are project-owned source-built material licensed
under MIT. The portrait is embedded into the executable; there is no loose
media payload. No third-party executable, codec, model, captured game media,
voice recording, or font file is bundled. Windows .NET Framework, Windows
Forms/GDI+, and Windows SoundPlayer/waveOut are operating-system inbox
dependencies and are not copied into this folder.

Source: `scripts/synthetic-game-replay/SyntheticGameReplay.cs`
Source manifest: `scripts/synthetic-game-replay/SOURCE-MANIFEST.json`

## MIT License

'@ + $licenseText
    }
    else {
        $notices = @'
# Synthetic review fixture licensing and notices

The executable, documentation, original AI-generated fictional portrait,
portrait-derived moving-frame sequence, and generated PCM in this folder are
project-owned source-built material licensed under MIT. The portrait is
embedded into the executable. The mara-venn-camera-idle-v1.inpcseq file is a
hash-verified Deflate-compressed sibling containing 42 RGB frames derived only
from that portrait. The source sequence itself contains camera zoom/pan motion;
the executable adds deterministic source-pixel blinking and breathing and does
not claim captured actor motion or source mouth articulation.

No third-party executable, codec, model, captured game media, voice recording,
or font file is bundled. Windows .NET Framework, Windows Forms/GDI+,
DeflateStream, and Windows SoundPlayer/waveOut are operating-system inbox
dependencies and are not copied into this folder.

Source: `scripts/synthetic-game-replay/SyntheticGameReplay.cs`
Source manifest: `scripts/synthetic-game-replay/SOURCE-MANIFEST.json`

## MIT License

'@ + $licenseText
    }
    Write-Utf8NoBom -Path $noticesPath -Content $notices

    if ($staticOnly) {
        $readme = @'
# Interactive NPCs synthetic review game

This is the standalone, project-owned Eclipse Harbor Windows capture target for
local Interactive NPCs 2.0 review. The executable embeds an original realistic
portrait of the fictional Mara Venn and keeps her source mouth pixel-static.
The portrait frame is rendered once and cached, so the game is very light on
CPU and GPU. The only thing that changes on screen is a tiny 6x6 pixel marker in
the bottom-right corner that alternates between two dark, low-contrast colours
30 times a second. Windows Graphics Capture only delivers a new frame when
window pixels change, and that marker keeps frames arriving fresh without any
bright or high-contrast flicker. There is no rain or other animation, and any
later mouth motion is attributable to the Interactive NPCs overlay. Playback is
muted by default; generated quiet ambience requires the explicit --audio option.

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
    }
    else {
        $readme = @'
# Interactive NPCs synthetic review game

This is the standalone, project-owned Eclipse Harbor Windows capture target for
local Interactive NPCs 2.0 review. It plays a verified 42-frame camera-motion
sequence derived from the original fictional Mara Venn portrait, then adds a
low-contrast deterministic blink and breathing deformation from source pixels.
The actor motion is synthetic test motion. The source mouth is unarticulated,
the renderer does not modify its mouth region, and this game is not lip-sync
evidence. Playback is muted by default; generated ambience requires the
explicit --audio option.

The runnable fixture needs only the executable and its verified
mara-venn-camera-idle-v1.inpcseq sibling. It does not load FFmpeg, Python, a
model, CUDA, NVIDIA compute, captured game content, or any third-party binary
from this folder or from PATH. The source sequence plays in a ping-pong order
to avoid a hard loop reset.

Double-click `interactive-npcs-synthetic-target.exe`, or use the repository
launcher for validated PID/HWND metadata and optional second-monitor placement:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File ".\scripts\synthetic-game-replay.ps1" -PlaceOnSecondMonitor
```

Use --mode static to run the lightweight static portrait control (a cached
portrait frame plus a tiny dark corner heartbeat marker).
REVIEW-FIXTURE-MANIFEST.json is the fail-closed file allowlist and provenance
record. review-test-game.cdx.json is the CycloneDX handoff. See
`THIRD-PARTY-NOTICES.md` for the complete no-third-party-binary notice and MIT
license text.
'@
    }
    Write-Utf8NoBom -Path $readmePath -Content $readme

    $componentVersion = [string]$sourceManifest.component_version
    $sbomVisualSource = if ($staticOnly) { [string]$validation.visual_source } else { [string]$movingSelfTest.visual_source }
    $sbomProperties = @(
        [ordered]@{ name = 'interactive-npcs:distribution-scope'; value = 'host-local-review-fixture' },
        [ordered]@{ name = 'interactive-npcs:distribution-class'; value = 'project-owned-source-built-review-fixture' },
        [ordered]@{ name = 'interactive-npcs:source-sha256'; value = [string]$validation.source_sha256 },
        [ordered]@{ name = 'interactive-npcs:source-manifest-sha256'; value = [string]$validation.source_manifest_sha256 },
        [ordered]@{ name = 'interactive-npcs:visual-source'; value = $sbomVisualSource },
        [ordered]@{ name = 'interactive-npcs:portrait-sha256'; value = [string]$validation.portrait_sha256 }
    )
    if ($staticOnly) {
        $sbomProperties += [ordered]@{ name = 'interactive-npcs:source-mouth-motion'; value = 'false' }
    }
    else {
        $sbomProperties += @(
            [ordered]@{ name = 'interactive-npcs:source-sequence-sha256'; value = [string]$movingSelfTest.source_sequence_sha256 },
            [ordered]@{ name = 'interactive-npcs:source-frame-motion'; value = 'true' },
            [ordered]@{ name = 'interactive-npcs:source-actor-motion'; value = 'false' },
            [ordered]@{ name = 'interactive-npcs:rendered-actor-motion'; value = 'true' },
            [ordered]@{ name = 'interactive-npcs:source-mouth-articulation'; value = 'false' },
            [ordered]@{ name = 'interactive-npcs:product-lip-sync'; value = 'false' }
        )
    }
    $sbomProperties += @(
        [ordered]@{ name = 'interactive-npcs:compiler-sha256'; value = [string]$validation.compiler.sha256 },
        [ordered]@{ name = 'interactive-npcs:third-party-binaries-bundled'; value = 'false' }
    )
    $sbom = [ordered]@{
        bomFormat = 'CycloneDX'
        specVersion = '1.5'
        serialNumber = 'urn:uuid:4b5bc0ae-a9f5-5c7d-b86e-25c06df78d9a'
        version = 1
        metadata = [ordered]@{
            component = [ordered]@{
                type = 'application'
                'bom-ref' = "pkg:generic/interactive-npcs-synthetic-review-game@$componentVersion"
                group = 'io.github.akshitireddy'
                name = 'interactive-npcs-synthetic-review-game'
                version = $componentVersion
                hashes = @([ordered]@{ alg = 'SHA-256'; content = Get-LowerSha256 -Path $executablePath })
                licenses = @([ordered]@{ license = [ordered]@{ id = 'MIT' } })
                properties = @($sbomProperties)
            }
        }
        components = @()
        dependencies = @([ordered]@{ ref = "pkg:generic/interactive-npcs-synthetic-review-game@$componentVersion"; dependsOn = @() })
    }
    Write-Utf8NoBom -Path $sbomPath -Content (($sbom | ConvertTo-Json -Depth 10) + [Environment]::NewLine)

    $fileMetadata = @{
        'interactive-npcs-synthetic-target.exe' = @{ component = 'project:synthetic-review-target'; class = 'project-owned-source-built-binary'; source = 'scripts/synthetic-game-replay/SyntheticGameReplay.cs' }
        'README.md' = @{ component = 'project:legal-resources'; class = 'project-owned-documentation'; source = 'scripts/windows/prepare-review-test-game.ps1' }
        'THIRD-PARTY-NOTICES.md' = @{ component = 'project:legal-resources'; class = 'project-owned-license-and-notices'; source = 'LICENSE' }
        'review-test-game.cdx.json' = @{ component = 'project:legal-resources'; class = 'project-owned-sbom'; source = 'scripts/windows/prepare-review-test-game.ps1' }
    }
    $entryNames = @('interactive-npcs-synthetic-target.exe', 'README.md', 'THIRD-PARTY-NOTICES.md', 'review-test-game.cdx.json')
    $expectedFileNames = @('interactive-npcs-synthetic-target.exe', 'README.md', 'REVIEW-FIXTURE-MANIFEST.json', 'THIRD-PARTY-NOTICES.md', 'review-test-game.cdx.json')
    if (-not $staticOnly) {
        $fileMetadata['mara-venn-camera-idle-v1.inpcseq'] = @{ component = 'project:synthetic-review-target'; class = 'project-owned-derived-compressed-media'; source = 'E:/temp/InteractiveNPCs/sources/mara-game-idle-source-v1' }
        $entryNames = @('interactive-npcs-synthetic-target.exe', 'mara-venn-camera-idle-v1.inpcseq', 'README.md', 'THIRD-PARTY-NOTICES.md', 'review-test-game.cdx.json')
        $expectedFileNames = @('interactive-npcs-synthetic-target.exe', 'mara-venn-camera-idle-v1.inpcseq', 'README.md', 'REVIEW-FIXTURE-MANIFEST.json', 'THIRD-PARTY-NOTICES.md', 'review-test-game.cdx.json')
    }
    $manifestEntries = foreach ($name in $entryNames) {
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
    $sourceRecord = [ordered]@{
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
    if ($staticOnly) {
        $generatedMedia = [ordered]@{
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
    }
    else {
        $sourceRecord['moving_sequence_source_path'] = [string]$movingContract.expected_source_path
        $sourceRecord['moving_sequence_packaged_path'] = [string]$movingContract.packaged_path
        $sourceRecord['moving_sequence_sha256'] = [string]$movingContract.sequence_sha256
        $sourceRecord['moving_sequence_raw_pixels_sha256'] = [string]$movingContract.raw_pixels_sha256
        $sourceRecord['moving_sequence_derivation_class'] = [string]$movingContract.derivation_class
        $sourceRecord['moving_sequence_rights_basis'] = [string]$movingContract.rights_basis
        $generatedMedia = [ordered]@{
            renderer = 'project-owned-gdi-source-pixel-idle-v2'
            visual_source = [string]$movingSelfTest.visual_source
            visual_mode = [string]$movingSelfTest.visual_mode
            portrait_sha256 = [string]$validation.portrait_sha256
            portrait_embedded_in_executable = $true
            moving_sequence_path = [string]$movingContract.packaged_path
            moving_sequence_file_sha256 = [string]$sequenceEvidence.sha256
            moving_sequence_file_size_bytes = [int64]$sequenceEvidence.size_bytes
            source_sequence_sha256 = [string]$movingSelfTest.source_sequence_sha256
            source_raw_pixels_sha256 = [string]$movingSelfTest.source_raw_pixels_sha256
            source_frame_count = [int]$movingSelfTest.source_frame_count
            source_frame_width = [int]$movingSelfTest.source_frame_width
            source_frame_height = [int]$movingSelfTest.source_frame_height
            source_frame_rate = [int]$movingSelfTest.source_frame_rate
            source_frame_motion = $true
            source_actor_motion = $false
            source_camera_motion_only = $true
            rendered_actor_motion = $true
            rendered_blink_motion = $true
            rendered_breathing_motion = $true
            source_mouth_motion = $false
            source_mouth_articulation = $false
            product_lip_sync = $false
            renderer_mouth_pixels_preserved = $true
            deterministic_rendering = $true
            source_advance_passed = $true
            generated_probe_frames = [int]$movingSelfTest.generated_probe_frames
            distinct_rendered_frame_hashes = [int]$movingSelfTest.distinct_rendered_frame_hashes
            audio_source = 'project-owned-generated-pcm-v1'
            audio_sha256 = [string]$validation.audio_sha256
            audio_duration_seconds = [int]$validation.audio_duration_seconds
            audio_sample_rate = [int]$validation.audio_sample_rate
            audio_peak = [int]$validation.audio_peak
            audio_rms = [double]$validation.audio_rms
            audio_clipped_samples = [int]$validation.audio_clipped_samples
            audio_loop_boundary_delta = [int]$validation.audio_loop_boundary_delta
        }
    }
    $distributionManifest = [ordered]@{
        schema_version = 1
        component_id = 'project:synthetic-review-target'
        component_version = $componentVersion
        fixture_kind_compatibility = 'synthetic-original-video-replay'
        fixture_source = if ($staticOnly) { 'project-source-generated-native-v1' } else { 'project-source-generated-native-v2' }
        distribution_scope = 'review-test-game'
        distribution_class = 'project-owned-source-built-review-fixture'
        expected_files = @($expectedFileNames)
        manifest_self = [ordered]@{
            path = 'REVIEW-FIXTURE-MANIFEST.json'
            hash = 'excluded-to-avoid-circularity'
        }
        files = @($manifestEntries)
        source = $sourceRecord
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
        generated_media = $generatedMedia
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
    if ($verification.status -ne 'passed' -or $verification.file_count -ne $expectedFileNames.Count -or $verification.third_party_binary_count -ne 0) {
        throw 'Prepared review fixture failed its final allowlist/provenance verification.'
    }

    $finalExecutable = Join-Path $destination 'interactive-npcs-synthetic-target.exe'
    $finalManifest = Join-Path $destination 'REVIEW-FIXTURE-MANIFEST.json'
    $finalSbom = Join-Path $destination 'review-test-game.cdx.json'
    $finalNotices = Join-Path $destination 'THIRD-PARTY-NOTICES.md'
    $summary = [ordered]@{
        schema_version = 2
        status = 'prepared'
        fixture_kind = 'synthetic-original-video-replay'
        fixture_source = if ($staticOnly) { 'project-source-generated-native-v1' } else { 'project-source-generated-native-v2' }
        directory = $destination
        executable_path = $finalExecutable
        executable_basename = [System.IO.Path]::GetFileName($finalExecutable)
        executable_sha256 = Get-LowerSha256 -Path $finalExecutable
        executable_size_bytes = (Get-Item -LiteralPath $finalExecutable).Length
        executable_subsystem = 'windows_gui'
    }
    if ($staticOnly) {
        $summary['renderer'] = 'project-owned-gdi-generated-v1'
        $summary['visual_source'] = [string]$validation.visual_source
        $summary['visual_mode'] = 'static-portrait-control-v1'
        $summary['portrait_sha256'] = [string]$validation.portrait_sha256
        $summary['portrait_embedded_in_executable'] = $true
        $summary['source_mouth_motion'] = $false
        $summary['mouth_region_invariant'] = [bool]$validation.mouth_region_invariant
        $summary['media_files_bundled'] = 0
    }
    else {
        $finalSequence = Join-Path $destination ([string]$movingContract.packaged_path)
        $summary['renderer'] = 'project-owned-gdi-source-pixel-idle-v2'
        $summary['visual_source'] = [string]$movingSelfTest.visual_source
        $summary['visual_mode'] = [string]$movingSelfTest.visual_mode
        $summary['portrait_sha256'] = [string]$validation.portrait_sha256
        $summary['portrait_embedded_in_executable'] = $true
        $summary['moving_sequence_path'] = $finalSequence
        $summary['moving_sequence_sha256'] = Get-LowerSha256 -Path $finalSequence
        $summary['moving_sequence_size_bytes'] = (Get-Item -LiteralPath $finalSequence).Length
        $summary['source_sequence_sha256'] = [string]$movingSelfTest.source_sequence_sha256
        $summary['source_raw_pixels_sha256'] = [string]$movingSelfTest.source_raw_pixels_sha256
        $summary['source_frame_count'] = [int]$movingSelfTest.source_frame_count
        $summary['source_frame_width'] = [int]$movingSelfTest.source_frame_width
        $summary['source_frame_height'] = [int]$movingSelfTest.source_frame_height
        $summary['source_frame_rate'] = [int]$movingSelfTest.source_frame_rate
        $summary['source_frame_motion'] = $true
        $summary['source_actor_motion'] = $false
        $summary['source_camera_motion_only'] = $true
        $summary['rendered_actor_motion'] = $true
        $summary['rendered_blink_motion'] = $true
        $summary['rendered_breathing_motion'] = $true
        $summary['source_mouth_motion'] = $false
        $summary['source_mouth_articulation'] = $false
        $summary['product_lip_sync'] = $false
        $summary['renderer_mouth_pixels_preserved'] = $true
        $summary['deterministic_rendering'] = $true
        $summary['source_advance_passed'] = $true
        $summary['media_files_bundled'] = 1
        $summary['headless_self_test_path'] = $movingSelfTestPath
    }
    $summary['generated_frame_rendering'] = $true
    $summary['audio_source'] = 'project-owned-generated-pcm-v1'
    $summary['audio_generated_pcm'] = $true
    $summary['hardware_acceleration'] = $false
    $summary['nvidia_compute_requested'] = $false
    $summary['third_party_binaries_bundled'] = @()
    $summary['distribution_manifest_path'] = $finalManifest
    $summary['distribution_manifest_sha256'] = Get-LowerSha256 -Path $finalManifest
    $summary['sbom_path'] = $finalSbom
    $summary['sbom_sha256'] = Get-LowerSha256 -Path $finalSbom
    $summary['notices_path'] = $finalNotices
    $summary['notices_sha256'] = Get-LowerSha256 -Path $finalNotices
    $summary['source_path'] = [string]$validation.source_path
    $summary['source_sha256'] = [string]$validation.source_sha256
    $summary['source_manifest_path'] = [string]$validation.source_manifest_path
    $summary['source_manifest_sha256'] = [string]$validation.source_manifest_sha256
    $summary['license_path'] = [string]$validation.license_path
    $summary['license_sha256'] = [string]$validation.license_sha256
    $summary['license_expression'] = 'MIT'
    $summary['build_receipt_path'] = [string]$validation.build_receipt_path
    $summary['build_receipt_sha256'] = Get-LowerSha256 -Path ([string]$validation.build_receipt_path)
    $summary['launcher_path'] = Join-Path $repoRoot 'scripts/synthetic-game-replay.ps1'
    $summary | ConvertTo-Json -Depth 6
}
finally {
    if ($null -ne $stage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
