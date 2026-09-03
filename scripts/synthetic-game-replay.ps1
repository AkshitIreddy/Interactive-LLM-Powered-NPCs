[CmdletBinding()]
param(
    [string]$MetadataPath,
    [string]$WindowTitle = 'Interactive NPCs Synthetic Game - Eclipse Harbor',
    [ValidateRange(320, 7680)]
    [int]$Width = 960,
    [ValidateRange(240, 4320)]
    [int]$Height = 600,
    [ValidateRange(1, 240)]
    [double]$FramesPerSecond = 15,
    [ValidateRange(0, 86400)]
    [int]$ExitAfterSeconds = 0,
    [switch]$NoLoop,
    [switch]$Mute,
    [switch]$PlaceOnSecondMonitor,
    [switch]$Wait,
    [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') { throw 'The synthetic review game can only be built and launched on Windows.' }

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')
$sourcePath = Join-Path $PSScriptRoot 'synthetic-game-replay\SyntheticGameReplay.cs'
$sourceManifestPath = Join-Path $PSScriptRoot 'synthetic-game-replay\SOURCE-MANIFEST.json'
$portraitPath = Join-Path $PSScriptRoot 'synthetic-game-replay\assets\mara-venn-portrait-v1.png'
$portraitProvenancePath = Join-Path $PSScriptRoot 'synthetic-game-replay\assets\PROVENANCE.md'
$portraitResourceName = 'InteractiveNpcs.SyntheticReplay.MaraVennPortraitV1.png'
$licensePath = Join-Path $repoRoot 'LICENSE'
if ([string]::IsNullOrWhiteSpace($MetadataPath)) {
    $MetadataPath = Join-Path $repoRoot 'artifacts\synthetic-replay\capture-target.json'
}
$MetadataPath = [System.IO.Path]::GetFullPath($MetadataPath)

function Get-LowerSha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-HexSha256 {
    param([Parameter(Mandatory)][string]$Value, [Parameter(Mandatory)][string]$Label)
    if ($Value -cnotmatch '^[0-9a-f]{64}$') { throw "$Label must be a lowercase SHA-256 value." }
}

function Get-CscEvidence {
    param([Parameter(Mandatory)]$SourceManifest)
    $contract = $SourceManifest.build_toolchain
    if ($null -eq $contract -or $contract.sdk_version -ne '9.0.302' -or
        @($contract.framework_references).Count -ne 6) {
        throw 'Synthetic source manifest does not declare the pinned Roslyn/reference toolchain.'
    }
    $hostPath = Join-Path $env:ProgramFiles 'dotnet\dotnet.exe'
    $compilerPath = Join-Path $env:ProgramFiles "dotnet\sdk\$($contract.sdk_version)\Roslyn\bincore\csc.dll"
    $referenceRoot = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319'
    foreach ($required in @($hostPath, $compilerPath)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Pinned synthetic compiler input is missing: $required" }
    }
    if ((Get-LowerSha256 -Path $hostPath) -cne [string]$contract.host_sha256 -or
        (Get-LowerSha256 -Path $compilerPath) -cne [string]$contract.compiler_sha256) {
        throw 'Pinned dotnet/Roslyn compiler hash mismatch.'
    }
    $references = foreach ($reference in $contract.framework_references) {
        $path = Join-Path $referenceRoot ([string]$reference.path)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-LowerSha256 -Path $path) -cne [string]$reference.sha256) {
            throw "Pinned .NET Framework reference hash mismatch: $path"
        }
        [pscustomobject][ordered]@{ path = $path; sha256 = [string]$reference.sha256 }
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $hostPath
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
        $null -eq $signature.SignerCertificate -or
        $signature.SignerCertificate.Subject -notmatch '(?i)O=Microsoft Corporation') {
        throw "The pinned dotnet compiler host is not signed by Microsoft with a currently valid Authenticode signature: $hostPath"
    }
    $version = (Get-Item -LiteralPath $hostPath).VersionInfo.FileVersion
    if ([string]::IsNullOrWhiteSpace($version)) { throw 'The pinned dotnet compiler host did not expose a file version.' }
    return [pscustomobject][ordered]@{
        host_path = $hostPath
        host_sha256 = Get-LowerSha256 -Path $hostPath
        path = $compilerPath
        sha256 = Get-LowerSha256 -Path $compilerPath
        sdk_version = [string]$contract.sdk_version
        file_version = $version
        authenticode_status = [string]$signature.Status
        signer_subject = [string]$signature.SignerCertificate.Subject
        framework_references = @($references)
    }
}

function Invoke-Compiler {
    param(
        [Parameter(Mandatory)][string]$HostPath,
        [Parameter(Mandatory)][string]$CompilerPath,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath
    )
    $process = Invoke-NpcHiddenProcess -FilePath $HostPath `
        -ArgumentList (@($CompilerPath) + @($ArgumentList)) -NoReplayOutput
    [System.IO.File]::WriteAllText($StdoutPath, $process.StandardOutput, (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::WriteAllText($StderrPath, $process.StandardError, (New-Object System.Text.UTF8Encoding($false)))
    if ($process.ExitCode -ne 0) {
        $stderr = if (Test-Path -LiteralPath $StderrPath) { Get-Content -LiteralPath $StderrPath -Raw } else { '' }
        throw "The project-owned synthetic target failed to compile (exit $($process.ExitCode)): $stderr"
    }
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

foreach ($required in @($sourcePath, $sourceManifestPath, $portraitPath, $portraitProvenancePath, $licensePath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Synthetic target source input is missing: $required" }
}
$sourceManifest = Get-Content -LiteralPath $sourceManifestPath -Raw | ConvertFrom-Json
$manifestProperties = @($sourceManifest.PSObject.Properties.Name)
foreach ($requiredProperty in @(
    'schema_version', 'component_id', 'component_version', 'source_path', 'source_sha256',
    'source_spdx_identifier', 'license_path', 'license_sha256', 'license_expression',
    'distribution_class', 'portrait_asset', 'build_toolchain', 'third_party_binaries', 'inbox_system_dependencies'
)) {
    if ($manifestProperties -notcontains $requiredProperty) { throw "Synthetic source manifest omits $requiredProperty." }
}
if ($sourceManifest.schema_version -ne 1 -or
    $sourceManifest.component_id -ne 'interactive-npcs-synthetic-review-game' -or
    $sourceManifest.source_path -ne 'scripts/synthetic-game-replay/SyntheticGameReplay.cs' -or
    $sourceManifest.license_path -ne 'LICENSE' -or
    $sourceManifest.source_spdx_identifier -ne 'MIT' -or
    $sourceManifest.license_expression -ne 'MIT' -or
    $sourceManifest.distribution_class -ne 'project-owned-source-built-review-fixture' -or
    $sourceManifest.portrait_asset.path -ne 'scripts/synthetic-game-replay/assets/mara-venn-portrait-v1.png' -or
    $sourceManifest.portrait_asset.resource_name -ne $portraitResourceName -or
    $sourceManifest.portrait_asset.provenance_path -ne 'scripts/synthetic-game-replay/assets/PROVENANCE.md' -or
    $sourceManifest.portrait_asset.visual_source -ne 'embedded-original-generated-photorealistic-portrait-v1' -or
    $sourceManifest.portrait_asset.source_mouth_motion -ne $false -or
    @($sourceManifest.third_party_binaries).Count -ne 0 -or
    @($sourceManifest.inbox_system_dependencies).Count -ne 3) {
    throw 'Synthetic source manifest does not match the fail-closed project-owned distribution policy.'
}
Assert-HexSha256 -Value ([string]$sourceManifest.source_sha256) -Label 'source_sha256'
Assert-HexSha256 -Value ([string]$sourceManifest.license_sha256) -Label 'license_sha256'
Assert-HexSha256 -Value ([string]$sourceManifest.portrait_asset.sha256) -Label 'portrait_asset.sha256'
Assert-HexSha256 -Value ([string]$sourceManifest.portrait_asset.provenance_sha256) -Label 'portrait_asset.provenance_sha256'
$sourceHash = Get-LowerSha256 -Path $sourcePath
$licenseHash = Get-LowerSha256 -Path $licensePath
$portraitHash = Get-LowerSha256 -Path $portraitPath
$portraitProvenanceHash = Get-LowerSha256 -Path $portraitProvenancePath
if ($sourceHash -cne [string]$sourceManifest.source_sha256) { throw "Synthetic target source hash mismatch. Expected $($sourceManifest.source_sha256), got $sourceHash." }
if ($licenseHash -cne [string]$sourceManifest.license_sha256) { throw "Repository license hash mismatch. Expected $($sourceManifest.license_sha256), got $licenseHash." }
if ($portraitHash -cne [string]$sourceManifest.portrait_asset.sha256) { throw "Synthetic target portrait hash mismatch. Expected $($sourceManifest.portrait_asset.sha256), got $portraitHash." }
if ($portraitProvenanceHash -cne [string]$sourceManifest.portrait_asset.provenance_sha256) { throw "Synthetic target portrait provenance hash mismatch. Expected $($sourceManifest.portrait_asset.provenance_sha256), got $portraitProvenanceHash." }
if ((Get-Content -LiteralPath $sourcePath -TotalCount 1) -cne '// SPDX-License-Identifier: MIT') {
    throw 'Synthetic target source must begin with the declared SPDX identifier.'
}

$compiler = Get-CscEvidence -SourceManifest $sourceManifest
$referenceIdentity = @($compiler.framework_references | ForEach-Object { "$($_.path)=$($_.sha256)" }) -join "`n"
$buildIdentityBytes = [System.Text.Encoding]::UTF8.GetBytes("$sourceHash`n$portraitHash`n$portraitProvenanceHash`n$($compiler.host_sha256)`n$($compiler.sha256)`n$referenceIdentity")
$sha = [System.Security.Cryptography.SHA256]::Create()
try { $buildIdentity = -join ($sha.ComputeHash($buildIdentityBytes) | ForEach-Object { $_.ToString('x2') }) }
finally { $sha.Dispose() }
$buildRoot = Join-Path $repoRoot "artifacts\synthetic-replay\bin\$($buildIdentity.Substring(0, 20))"
$playerPath = Join-Path $buildRoot 'interactive-npcs-synthetic-target.exe'
$buildReceiptPath = Join-Path $buildRoot 'BUILD-RECEIPT.json'

$mustCompile = -not (Test-Path -LiteralPath $playerPath -PathType Leaf) -or -not (Test-Path -LiteralPath $buildReceiptPath -PathType Leaf)
if (-not $mustCompile) {
    try {
        $receipt = Get-Content -LiteralPath $buildReceiptPath -Raw | ConvertFrom-Json
        $mustCompile = $receipt.schema_version -ne 1 -or
            $receipt.source_sha256 -cne $sourceHash -or
            $receipt.portrait_asset.sha256 -cne $portraitHash -or
            $receipt.portrait_asset.provenance_sha256 -cne $portraitProvenanceHash -or
            $receipt.source_manifest_sha256 -cne (Get-LowerSha256 -Path $sourceManifestPath) -or
            $receipt.license_sha256 -cne $licenseHash -or
            $receipt.compiler.host_sha256 -cne [string]$compiler.host_sha256 -or
            $receipt.compiler.sha256 -cne [string]$compiler.sha256 -or
            $receipt.reproducible_rebuild_verified -ne $true -or
            $receipt.executable_sha256 -cne (Get-LowerSha256 -Path $playerPath)
    }
    catch { $mustCompile = $true }
}
if ($mustCompile) {
    New-Item -ItemType Directory -Path $buildRoot -Force | Out-Null
    foreach ($owned in @($playerPath, $buildReceiptPath)) {
        if (Test-Path -LiteralPath $owned -PathType Leaf) { Remove-Item -LiteralPath $owned -Force }
    }
    $compileOut = Join-Path $buildRoot 'compile.stdout.log'
    $compileErr = Join-Path $buildRoot 'compile.stderr.log'
    $referenceArguments = @($compiler.framework_references | ForEach-Object { "/reference:$($_.path)" })
    $compilerArgs = @(
        '/noconfig', '/nostdlib+', '/nologo', '/target:winexe', '/deterministic+', '/optimize+', '/platform:anycpu',
        "/out:$playerPath", "/resource:$portraitPath,$portraitResourceName"
    )
    $compilerArgs += $referenceArguments
    $compilerArgs += $sourcePath
    Invoke-Compiler -HostPath $compiler.host_path -CompilerPath $compiler.path -ArgumentList $compilerArgs -StdoutPath $compileOut -StderrPath $compileErr
    $compileDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $playerPath -PathType Leaf) -and [DateTime]::UtcNow -lt $compileDeadline) {
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path -LiteralPath $playerPath -PathType Leaf)) { throw 'The compiler exited successfully without producing the synthetic target.' }
    $reproRoot = Join-Path $buildRoot 'repro-check'
    New-Item -ItemType Directory -Path $reproRoot -Force | Out-Null
    $reproPath = Join-Path $reproRoot 'interactive-npcs-synthetic-target.exe'
    $reproArgs = @($compilerArgs | ForEach-Object { if ($_ -eq "/out:$playerPath") { "/out:$reproPath" } else { $_ } })
    Invoke-Compiler -HostPath $compiler.host_path -CompilerPath $compiler.path -ArgumentList $reproArgs `
        -StdoutPath (Join-Path $reproRoot 'compile.stdout.log') -StderrPath (Join-Path $reproRoot 'compile.stderr.log')
    $reproDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $reproPath -PathType Leaf) -and [DateTime]::UtcNow -lt $reproDeadline) { Start-Sleep -Milliseconds 50 }
    if (-not (Test-Path -LiteralPath $reproPath -PathType Leaf) -or
        (Get-LowerSha256 -Path $reproPath) -cne (Get-LowerSha256 -Path $playerPath)) {
        throw 'Pinned Roslyn compilation did not reproduce the synthetic executable bit-for-bit.'
    }
    Remove-Item -LiteralPath $reproRoot -Recurse -Force
    $receipt = [ordered]@{
        schema_version = 1
        component_id = [string]$sourceManifest.component_id
        component_version = [string]$sourceManifest.component_version
        source_path = [string]$sourceManifest.source_path
        source_sha256 = $sourceHash
        portrait_asset = [ordered]@{
            path = [string]$sourceManifest.portrait_asset.path
            sha256 = $portraitHash
            provenance_path = [string]$sourceManifest.portrait_asset.provenance_path
            provenance_sha256 = $portraitProvenanceHash
            resource_name = $portraitResourceName
            visual_source = [string]$sourceManifest.portrait_asset.visual_source
            source_mouth_motion = $false
        }
        source_manifest_path = 'scripts/synthetic-game-replay/SOURCE-MANIFEST.json'
        source_manifest_sha256 = Get-LowerSha256 -Path $sourceManifestPath
        license_path = 'LICENSE'
        license_sha256 = $licenseHash
        license_expression = 'MIT'
        compiler = $compiler
        compile_arguments = @($compilerArgs | ForEach-Object { if ($_ -eq $sourcePath) { [string]$sourceManifest.source_path } elseif ($_ -like '/out:*') { '/out:interactive-npcs-synthetic-target.exe' } elseif ($_ -like '/resource:*') { '/resource:mara-venn-portrait-v1.png,' + $portraitResourceName } elseif ($_ -like '/reference:*') { '/reference:' + [System.IO.Path]::GetFileName($_.Substring(11)) } else { $_ } })
        reproducibility_class = 'bit-for-bit-deterministic-for-pinned-roslyn-and-framework-reference-inputs'
        reproducible_rebuild_verified = $true
        executable_sha256 = Get-LowerSha256 -Path $playerPath
        third_party_binaries_bundled = @()
    }
    [System.IO.File]::WriteAllText($buildReceiptPath, (($receipt | ConvertTo-Json -Depth 6) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
}

$validationRoot = Join-Path $repoRoot 'artifacts\synthetic-replay\validation'
New-Item -ItemType Directory -Path $validationRoot -Force | Out-Null
$selfTestPath = Join-Path $validationRoot 'self-test.json'
$selfTestFrames = Join-Path $validationRoot 'frames'
if (Test-Path -LiteralPath $selfTestPath -PathType Leaf) { Remove-Item -LiteralPath $selfTestPath -Force }
$selfTestArguments = @('--self-test-report', $selfTestPath, '--self-test-frame-directory', $selfTestFrames, '--width', [string]$Width, '--height', [string]$Height)
$selfTestProcess = Start-Process -FilePath $playerPath -ArgumentList (ConvertTo-NativeCommandLine -Arguments $selfTestArguments) -Wait -PassThru
if ($selfTestProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $selfTestPath -PathType Leaf)) {
    throw "Synthetic target self-test failed with exit code $($selfTestProcess.ExitCode)."
}
$selfTest = Get-Content -LiteralPath $selfTestPath -Raw | ConvertFrom-Json
if ($selfTest.status -ne 'passed' -or $selfTest.fixture_source -ne 'project-source-generated-native-v1' -or
    $selfTest.renderer -ne 'project-owned-gdi-generated-v1' -or $selfTest.generated_frame_count -ne 2 -or
    $selfTest.frames_differ -ne $true -or
    $selfTest.visual_source -ne 'embedded-original-generated-photorealistic-portrait-v1' -or
    $selfTest.portrait_sha256 -cne $portraitHash -or $selfTest.portrait_hash_verified -ne $true -or
    $selfTest.source_mouth_motion -ne $false -or $selfTest.mouth_region_invariant -ne $true -or
    $selfTest.audio_source -ne 'project-owned-generated-pcm-v1' -or
    $selfTest.audio_nonzero_samples -le 0 -or $selfTest.audio_peak -ge 32767 -or
    $selfTest.audio_clipped_samples -ne 0 -or $selfTest.audio_loop_boundary_delta -ge 256 -or
    $selfTest.hardware_acceleration -ne $false -or $selfTest.nvidia_compute_requested -ne $false -or
    $selfTest.third_party_binaries_loaded -ne $false) {
    throw 'Synthetic target self-test did not satisfy the deterministic frame/audio policy.'
}

if ($ValidateOnly) {
    [ordered]@{
        schema_version = 2
        status = 'passed'
        fixture_kind = 'synthetic-original-video-replay'
        fixture_source = 'project-source-generated-native-v1'
        source_path = $sourcePath
        source_sha256 = $sourceHash
        visual_source = [string]$selfTest.visual_source
        portrait_path = $portraitPath
        portrait_sha256 = $portraitHash
        portrait_provenance_path = $portraitProvenancePath
        portrait_provenance_sha256 = $portraitProvenanceHash
        source_mouth_motion = $false
        mouth_region_invariant = [bool]$selfTest.mouth_region_invariant
        source_manifest_path = $sourceManifestPath
        source_manifest_sha256 = Get-LowerSha256 -Path $sourceManifestPath
        license_path = $licensePath
        license_sha256 = $licenseHash
        license_expression = 'MIT'
        player_path = $playerPath
        player_sha256 = Get-LowerSha256 -Path $playerPath
        build_receipt_path = $buildReceiptPath
        compiler = $compiler
        renderer = 'project-owned-gdi-generated-v1'
        generated_probe_frames = [int]$selfTest.generated_frame_count
        frames_differ = [bool]$selfTest.frames_differ
        review_frame_path = [string]$selfTest.frame_a_path
        audio_source = 'project-owned-generated-pcm-v1'
        audio_sha256 = [string]$selfTest.audio_sha256
        audio_duration_seconds = [int]$selfTest.audio_duration_seconds
        audio_sample_rate = [int]$selfTest.audio_sample_rate
        audio_nonzero_samples = [int]$selfTest.audio_nonzero_samples
        audio_peak = [int]$selfTest.audio_peak
        audio_rms = [double]$selfTest.audio_rms
        audio_clipped_samples = [int]$selfTest.audio_clipped_samples
        audio_loop_boundary_delta = [int]$selfTest.audio_loop_boundary_delta
        hardware_acceleration = $false
        nvidia_compute_requested = $false
        third_party_binaries_bundled = @()
    } | ConvertTo-Json -Depth 6
    exit 0
}

$metadataDirectory = Split-Path -Parent $MetadataPath
if (-not [string]::IsNullOrWhiteSpace($metadataDirectory)) { New-Item -ItemType Directory -Path $metadataDirectory -Force | Out-Null }
if (Test-Path -LiteralPath $MetadataPath -PathType Leaf) { Remove-Item -LiteralPath $MetadataPath -Force }

$placementHelper = Join-Path $env:USERPROFILE '.codex\skills\prefer-second-monitor\scripts\place_process_windows.ps1'
$playerArguments = @(
    '--metadata', $MetadataPath, '--title', $WindowTitle,
    '--width', [string]$Width, '--height', [string]$Height,
    '--fps', $FramesPerSecond.ToString([System.Globalization.CultureInfo]::InvariantCulture),
    '--exit-after-seconds', [string]$ExitAfterSeconds
)
if ($NoLoop) { $playerArguments += '--no-loop' }
if ($Mute) { $playerArguments += '--mute' }
if ($PlaceOnSecondMonitor) {
    if (-not (Test-Path -LiteralPath $placementHelper -PathType Leaf)) { throw "Second-monitor placement helper is unavailable: $placementHelper" }
    $playerArguments += @('--place-on-second-monitor', '--placement-helper', $placementHelper)
}

$player = Start-Process -FilePath $playerPath -ArgumentList (ConvertTo-NativeCommandLine -Arguments $playerArguments) -PassThru
$deadline = [DateTime]::UtcNow.AddSeconds(15)
$metadata = $null
while ([DateTime]::UtcNow -lt $deadline) {
    if ($player.HasExited) { throw "Synthetic target exited before publishing capture metadata (exit code $($player.ExitCode))." }
    if (Test-Path -LiteralPath $MetadataPath -PathType Leaf) {
        try {
            $candidate = Get-Content -LiteralPath $MetadataPath -Raw | ConvertFrom-Json
            if ([int64]$candidate.process_id -eq [int64]$player.Id -and [int64]$candidate.window_handle -ne 0 -and
                [string]$candidate.state -eq 'playing' -and [int64]$candidate.generated_frames -ge 1 -and
                [string]$candidate.fixture_source -eq 'project-source-generated-native-v1' -and
                [string]$candidate.renderer -eq 'project-owned-gdi-generated-v1' -and
                [string]$candidate.visual_source -eq 'embedded-original-generated-photorealistic-portrait-v1' -and
                [string]$candidate.portrait_sha256 -ceq $portraitHash -and
                [bool]$candidate.source_mouth_motion -eq $false -and
                [bool]$candidate.third_party_binaries_loaded -eq $false) {
                $metadata = $candidate
                break
            }
        } catch { }
    }
    Start-Sleep -Milliseconds 100
}
if ($null -eq $metadata) {
    if (-not $player.HasExited) { $player.Kill() }
    throw 'Synthetic target did not publish valid generated-frame PID/HWND metadata within 15 seconds.'
}

[ordered]@{
    schema_version = 2
    status = 'launched'
    pid = [int]$player.Id
    player_pid = [int]$player.Id
    window_handle = [int64]$metadata.window_handle
    hwnd = [int64]$metadata.hwnd
    hwnd_hex = [string]$metadata.hwnd_hex
    executable_basename = [string]$metadata.executable_basename
    window_title = [string]$metadata.window_title
    metadata_path = $MetadataPath
    fixture_source = [string]$metadata.fixture_source
    renderer = [string]$metadata.renderer
    visual_source = [string]$metadata.visual_source
    portrait_sha256 = [string]$metadata.portrait_sha256
    source_mouth_motion = [bool]$metadata.source_mouth_motion
    generated_frames = [int64]$metadata.generated_frames
    audio_source = [string]$metadata.audio_source
    audio_output_started = [bool]$metadata.audio_output_started
    hardware_acceleration = $false
    nvidia_compute_requested = $false
    third_party_binaries_loaded = $false
} | ConvertTo-Json -Depth 4

if ($Wait) { $player.WaitForExit(); exit $player.ExitCode }
