[CmdletBinding()]
param(
    [string]$InputPath,
    [string]$MetadataPath,
    [string]$WindowTitle = 'Interactive NPCs Synthetic Game - Eclipse Harbor',
    [ValidateRange(64, 7680)]
    [int]$Width = 960,
    [ValidateRange(64, 4320)]
    [int]$Height = 600,
    [ValidateRange(1, 240)]
    [double]$FramesPerSecond = 15,
    [ValidateRange(0, 86400)]
    [int]$ExitAfterSeconds = 0,
    [switch]$NoLoop,
    [switch]$PlaceOnSecondMonitor,
    [switch]$Wait,
    [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$canonicalInput = Join-Path $repoRoot 'docs\assets\demo\demo.mp4'
$canonicalManifest = Join-Path $repoRoot 'docs\assets\demo\render-manifest.json'
$canonicalProvenance = Join-Path $repoRoot 'docs\assets\demo\PROVENANCE.md'

if ([string]::IsNullOrWhiteSpace($InputPath)) {
    $InputPath = $canonicalInput
}
$InputPath = (Resolve-Path -LiteralPath $InputPath).Path

if ([string]::IsNullOrWhiteSpace($MetadataPath)) {
    $MetadataPath = Join-Path $repoRoot 'artifacts\synthetic-replay\capture-target.json'
}
$MetadataPath = [System.IO.Path]::GetFullPath($MetadataPath)

function Resolve-ToolPath {
    param([Parameter(Mandatory)][string]$Name)

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -ne $command -and -not [string]::IsNullOrWhiteSpace($command.Source)) {
        return $command.Source
    }

    $candidates = if ($Name -eq 'ffmpeg.exe') {
        @(
            (Join-Path $repoRoot 'demo\readme\node_modules\ffmpeg-static\ffmpeg.exe'),
            'C:\ffmpeg\bin\ffmpeg.exe'
        )
    } else {
        @()
    }

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    throw "$Name is required. Run the checked-in demo setup or place FFmpeg on PATH."
}

function Get-CscPath {
    $candidates = @(
        (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'),
        (Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return $candidate
        }
    }
    throw 'The Windows .NET Framework C# compiler was not found.'
}

function Invoke-HiddenProcess {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath,
        [int]$TimeoutSeconds = 30
    )

    # This script itself is run headlessly by automation. Invoking console tools
    # in-process avoids PowerShell 5.1's Start-Process/redirected ExitCode bug
    # and cannot create another console window.
    $global:LASTEXITCODE = 0
    & $FilePath @ArgumentList 1> $StdoutPath 2> $StderrPath
    $exitCode = $global:LASTEXITCODE
    if ($exitCode -ne 0) {
        $stderr = if (Test-Path -LiteralPath $StderrPath) { Get-Content -LiteralPath $StderrPath -Raw } else { '' }
        throw "$([System.IO.Path]::GetFileName($FilePath)) failed with exit code ${exitCode}: $stderr"
    }
}

function ConvertTo-NativeCommandLine {
    param([Parameter(Mandatory)][string[]]$Arguments)

    return (($Arguments | ForEach-Object {
        $argument = [string]$_
        if ($argument.Length -eq 0) {
            return '""'
        }
        if ($argument -notmatch '[\s"]') {
            return $argument
        }
        # CommandLineToArgvW-compatible quoting: double backslashes before a
        # quote, and double trailing backslashes before the closing quote.
        $escaped = [System.Text.RegularExpressions.Regex]::Replace($argument, '(\\*)"', '$1$1\"')
        $escaped = [System.Text.RegularExpressions.Regex]::Replace($escaped, '(\\+)$', '$1$1')
        return '"' + $escaped + '"'
    }) -join ' ')
}

$ffmpegPath = Resolve-ToolPath -Name 'ffmpeg.exe'
$sourcePath = Join-Path $PSScriptRoot 'synthetic-game-replay\SyntheticGameReplay.cs'
$sourceHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
$buildRoot = Join-Path $repoRoot "artifacts\synthetic-replay\bin\$($sourceHash.Substring(0, 16))"
$playerPath = Join-Path $buildRoot 'interactive-npcs-synthetic-target.exe'

if (-not (Test-Path -LiteralPath $playerPath -PathType Leaf)) {
    New-Item -ItemType Directory -Path $buildRoot -Force | Out-Null
    $compileOut = Join-Path $buildRoot 'compile.stdout.log'
    $compileErr = Join-Path $buildRoot 'compile.stderr.log'
    $compilerArgs = @(
        '/nologo', '/target:winexe', '/optimize+', '/platform:anycpu',
        "/out:$playerPath",
        '/reference:System.dll', '/reference:System.Core.dll', '/reference:System.Drawing.dll',
        '/reference:System.Windows.Forms.dll', '/reference:System.Web.Extensions.dll',
        $sourcePath
    )
    Invoke-HiddenProcess -FilePath (Get-CscPath) -ArgumentList $compilerArgs -StdoutPath $compileOut -StderrPath $compileErr
    $compileDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path -LiteralPath $playerPath -PathType Leaf) -and [DateTime]::UtcNow -lt $compileDeadline) {
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path -LiteralPath $playerPath -PathType Leaf)) {
        throw 'The synthetic replay compiler succeeded but did not produce its target executable.'
    }
}

$inputHash = (Get-FileHash -LiteralPath $InputPath -Algorithm SHA256).Hash.ToLowerInvariant()
$isCanonicalFixture = [string]::Equals($InputPath, $canonicalInput, [System.StringComparison]::OrdinalIgnoreCase)
if ($isCanonicalFixture) {
    $manifest = Get-Content -LiteralPath $canonicalManifest -Raw | ConvertFrom-Json
    $expectedHash = [string]$manifest.media.'demo.mp4'.sha256
    if (-not [string]::Equals($inputHash, $expectedHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "The canonical synthetic replay hash does not match its provenance manifest. Expected $expectedHash, got $inputHash."
    }
}

if ($ValidateOnly) {
    $validationRoot = Join-Path $repoRoot 'artifacts\synthetic-replay\validation'
    New-Item -ItemType Directory -Path $validationRoot -Force | Out-Null
    $decodeOut = Join-Path $validationRoot 'decode.stdout.log'
    $decodeErr = Join-Path $validationRoot 'decode.stderr.log'
    $decodeArgs = @(
        '-hide_banner', '-loglevel', 'error', '-nostdin', '-hwaccel', 'none',
        '-i', $InputPath, '-map', '0:v:0', '-frames:v', '1', '-an', '-sn', '-dn',
        '-threads', '2', '-f', 'null', '-'
    )
    Invoke-HiddenProcess -FilePath $ffmpegPath -ArgumentList $decodeArgs -StdoutPath $decodeOut -StderrPath $decodeErr

    [ordered]@{
        schema_version = 1
        status = 'passed'
        fixture_kind = 'synthetic-original-video-replay'
        input_path = $InputPath
        input_sha256 = $inputHash
        canonical_fixture = $isCanonicalFixture
        provenance_path = if ($isCanonicalFixture) { $canonicalProvenance } else { $null }
        manifest_path = if ($isCanonicalFixture) { $canonicalManifest } else { $null }
        player_path = $playerPath
        executable_basename = [System.IO.Path]::GetFileName($playerPath)
        exe_basename = [System.IO.Path]::GetFileName($playerPath)
        ffmpeg_path = $ffmpegPath
        decoder = 'ffmpeg-software'
        hardware_acceleration = $false
        nvidia_compute_requested = $false
        decoded_probe_frames = 1
    } | ConvertTo-Json -Depth 4
    exit 0
}

$metadataDirectory = Split-Path -Parent $MetadataPath
if (-not [string]::IsNullOrWhiteSpace($metadataDirectory)) {
    New-Item -ItemType Directory -Path $metadataDirectory -Force | Out-Null
}
if (Test-Path -LiteralPath $MetadataPath -PathType Leaf) {
    # This exact file is task-owned launch metadata; removing it prevents a
    # prior bounded run from being mistaken for the new HWND.
    Remove-Item -LiteralPath $MetadataPath -Force
}

$placementHelper = Join-Path $env:USERPROFILE '.codex\skills\prefer-second-monitor\scripts\place_process_windows.ps1'
$playerArguments = @(
    '--input', $InputPath,
    '--metadata', $MetadataPath,
    '--ffmpeg', $ffmpegPath,
    '--title', $WindowTitle,
    '--width', [string]$Width,
    '--height', [string]$Height,
    '--fps', $FramesPerSecond.ToString([System.Globalization.CultureInfo]::InvariantCulture),
    '--exit-after-seconds', [string]$ExitAfterSeconds
)
if ($NoLoop) { $playerArguments += '--no-loop' }
if ($PlaceOnSecondMonitor) {
    if (-not (Test-Path -LiteralPath $placementHelper -PathType Leaf)) {
        throw "Second-monitor placement was requested, but the helper is unavailable: $placementHelper"
    }
    $playerArguments += @('--place-on-second-monitor', '--placement-helper', $placementHelper)
}

$playerCommandLine = ConvertTo-NativeCommandLine -Arguments $playerArguments
$player = Start-Process -FilePath $playerPath -ArgumentList $playerCommandLine -PassThru
$deadline = [DateTime]::UtcNow.AddSeconds(15)
$metadata = $null
while ([DateTime]::UtcNow -lt $deadline) {
    if ($player.HasExited) {
        throw "Synthetic replay exited before publishing capture metadata (exit code $($player.ExitCode))."
    }
    if (Test-Path -LiteralPath $MetadataPath -PathType Leaf) {
        try {
            $candidate = Get-Content -LiteralPath $MetadataPath -Raw | ConvertFrom-Json
            if ([int64]$candidate.process_id -eq [int64]$player.Id -and
                [int64]$candidate.window_handle -ne 0 -and
                [string]$candidate.state -eq 'playing' -and
                [int64]$candidate.decoded_frames -ge 1) {
                $metadata = $candidate
                break
            }
        } catch {
            # Atomic metadata replacement should make this rare; retry until
            # the bounded deadline if an antivirus filter races the read.
        }
    }
    Start-Sleep -Milliseconds 100
}
if ($null -eq $metadata) {
    if (-not $player.HasExited) { $player.Kill() }
    throw "Synthetic replay did not decode a frame and publish valid PID/HWND metadata within 15 seconds."
}

[ordered]@{
    schema_version = 1
    status = 'launched'
    pid = [int]$player.Id
    player_pid = [int]$player.Id
    window_handle = [int64]$metadata.window_handle
    hwnd = [int64]$metadata.hwnd
    hwnd_hex = [string]$metadata.hwnd_hex
    executable_basename = [string]$metadata.executable_basename
    exe_basename = [string]$metadata.exe_basename
    window_title = [string]$metadata.window_title
    metadata_path = $MetadataPath
    input_sha256 = $inputHash
    cpu_software_decode = $true
    nvidia_compute_requested = $false
} | ConvertTo-Json -Depth 3

if ($Wait) {
    $player.WaitForExit()
    exit $player.ExitCode
}
