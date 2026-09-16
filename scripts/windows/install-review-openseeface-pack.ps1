[CmdletBinding()]
param(
    [string]$PackSourceDirectory = 'E:\temp\InteractiveNPCs\review-openseeface-mnv3-pack\openseeface-mnv3-lm1-mouth-signal\85aa70fc67582d046e771ea73625182a0d8f7475',
    [string]$ConfigDirectory = (Join-Path $env:APPDATA 'io.github.akshitireddy.interactive-npcs.review')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

# Installs the review-only OpenSeeFace mnv3+lm1 CPU signal pack that the
# debug review lip-sync path requires. The unsigned review bundle deliberately
# cannot carry model weights or native runtime DLLs, so this pack lives in
# the isolated owner review profile instead of the package.
#
# Every file is hash-pinned. The model pins mirror
# packaging/model-packs/openseeface-mnv3-lm1-mouth-signal.json; the runtime
# pins mirror the official Microsoft ONNX Runtime 1.22.1 release archive
# recorded there and the REVIEW_ORT_* constants in
# apps/control/src-tauri/src/visual_runtime.rs.

$packId = 'openseeface-mnv3-lm1-mouth-signal'
$packRevision = '85aa70fc67582d046e771ea73625182a0d8f7475'
$expectedFiles = @(
    @{ path = 'models/mnv3_detection_opt.onnx'; size = 568302;
       sha256 = '0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809' },
    @{ path = 'models/lm_model1_opt.onnx'; size = 4842329;
       sha256 = '5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f' },
    @{ path = 'runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime.dll'; size = 12416032;
       sha256 = '7788f3f38e9a339003f7d7e1bf47f928287cf409bb5273273149e1282fbf503f' },
    @{ path = 'runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime_providers_shared.dll'; size = 22048;
       sha256 = '7a71dbee513692aeb0bd2346a50ae0edd28c8b34defc17254605a29e81c60569' },
    @{ path = 'LICENSE'; size = 1364;
       sha256 = '28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146' }
)

$applicationNamespace = 'io.github.akshitireddy.interactive-npcs.review'
$configRoot = [System.IO.Path]::GetFullPath($ConfigDirectory)
if ([System.IO.Path]::GetFileName($configRoot) -cne $applicationNamespace) {
    throw "This review pack can only be installed into the isolated $applicationNamespace directory."
}
$source = [System.IO.Path]::GetFullPath($PackSourceDirectory).TrimEnd('\')
$destination = Join-Path $configRoot ("model-packs\$packId\$packRevision")

function Get-LowerSha256([string]$path) {
    (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
}

$actualFiles = @(Get-ChildItem -LiteralPath $source -File -Force -Recurse |
    ForEach-Object { $_.FullName.Substring($source.Length + 1).Replace('\', '/') } |
    Where-Object { $_ -notin @('LICENSE', 'models/mnv3_detection_opt.onnx', 'models/lm_model1_opt.onnx') -and $_ -notlike 'runtime/*' } |
    Sort-Object)
foreach ($entry in $expectedFiles) {
    $path = Join-Path $source ([string]$entry.path)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Pack source is missing: $($entry.path)" }
    if ((Get-Item -LiteralPath $path).Length -ne [int64]$entry.size) { throw "Pack source size mismatch: $($entry.path)" }
    if ((Get-LowerSha256 -Path $path) -cne [string]$entry.sha256) { throw "Pack source hash mismatch: $($entry.path)" }
}
if (Test-Path -LiteralPath $destination) {
    try {
        foreach ($entry in $expectedFiles) {
            $path = Join-Path $destination ([string]$entry.path)
            if ((Get-Item -LiteralPath $path).Length -ne [int64]$entry.size -or
                (Get-LowerSha256 -Path $path) -cne [string]$entry.sha256) {
                throw "Existing install fails verification: $($entry.path). Remove $destination and retry."
            }
        }
        [ordered]@{ schema_version = 1; status = 'already_installed'; destination = $destination } |
            ConvertTo-Json -Depth 3
        exit 0
    } catch {
        throw
    }
}
$stage = "$destination.installing"
if (Test-Path -LiteralPath $stage) { throw "A previous install did not finish: $stage" }
New-Item -ItemType Directory -Path $stage -Force | Out-Null
try {
    foreach ($entry in $expectedFiles) {
        $target = Join-Path $stage ([string]$entry.path)
        New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $source ([string]$entry.path)) -Destination $target
    }
    foreach ($aux in @('runtime/onnxruntime-1.22.1-cpu/LICENSE',
            'runtime/onnxruntime-1.22.1-cpu/ThirdPartyNotices.txt',
            'runtime/onnxruntime-1.22.1-cpu/VERSION_NUMBER',
            'runtime/onnxruntime-1.22.1-cpu/GIT_COMMIT_ID')) {
        $from = Join-Path $source $aux
        if (-not (Test-Path -LiteralPath $from -PathType Leaf)) { throw "Pack source is missing license file: $aux" }
        $target = Join-Path $stage $aux
        New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
        Copy-Item -LiteralPath $from -Destination $target
    }
    foreach ($entry in $expectedFiles) {
        $path = Join-Path $stage ([string]$entry.path)
        if ((Get-LowerSha256 -Path $path) -cne [string]$entry.sha256) { throw "Staged file changed: $($entry.path)" }
    }
    [System.IO.Directory]::Move($stage, $destination)
} catch {
    if (Test-Path -LiteralPath $stage) { [System.IO.Directory]::Delete($stage, $true) }
    throw
}
[ordered]@{
    schema_version = 1
    status = 'installed'
    destination = $destination
    pack_id = $packId
    pack_revision = $packRevision
    files_verified = $expectedFiles.Count + 4
} | ConvertTo-Json -Depth 3
