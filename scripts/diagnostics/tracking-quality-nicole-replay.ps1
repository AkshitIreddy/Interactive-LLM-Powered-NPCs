[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$NativeProofExe,

    [Parameter(Mandatory = $true)]
    [string]$PackRoot,

    [Parameter(Mandatory = $true)]
    [string]$SourceFrames,

    [Parameter(Mandatory = $true)]
    [string]$AudioWav,

    [Parameter(Mandatory = $true)]
    [string]$AtlasRoot,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir,

    [ValidateSet(10, 15)]
    [int]$TrackingHz = 10
)

$ErrorActionPreference = 'Stop'

foreach ($requiredPath in @($NativeProofExe, $PackRoot, $SourceFrames, $AudioWav, $AtlasRoot)) {
    if (-not (Test-Path -LiteralPath $requiredPath)) {
        throw "Required path does not exist: $requiredPath"
    }
}

if (Test-Path -LiteralPath $OutputDir) {
    throw "Output directory already exists; choose a new immutable run path: $OutputDir"
}
New-Item -ItemType Directory -Path $OutputDir | Out-Null
$logPath = Join-Path $OutputDir 'tracking-quality-native-proof.log'
$evidencePath = Join-Path $OutputDir 'tracking-quality-evidence.json'
$proofPath = Join-Path $OutputDir 'headless-proof.json'

$stopwatch = [Diagnostics.Stopwatch]::StartNew()
& $NativeProofExe $PackRoot $SourceFrames $AudioWav $OutputDir $TrackingHz $AtlasRoot *> $logPath
$nativeExitCode = $LASTEXITCODE
$stopwatch.Stop()

if (-not (Test-Path -LiteralPath $proofPath)) {
    throw "Native proof produced no headless-proof.json; see $logPath"
}

$proof = Get-Content -Raw -LiteralPath $proofPath | ConvertFrom-Json
$logText = Get-Content -Raw -LiteralPath $logPath
$trackingAccepted =
    $nativeExitCode -eq 0 -and
    $proof.sourceFrameCount -gt 1 -and
    $proof.outputFrames -gt 0 -and
    $proof.residualFrames -eq $proof.outputFrames -and
    $logText -notmatch 'landmark adapter bypassed' -and
    $logText -notmatch 'provider inference failed'

$evidence = [ordered]@{
    schema = 'interactive-npcs-tracking-quality-replay/v1'
    trackingAccepted = $trackingAccepted
    nativeExitCode = $nativeExitCode
    nativeStatus = $proof.status
    sourceFrameCount = $proof.sourceFrameCount
    outputFrames = $proof.outputFrames
    residualFrames = $proof.residualFrames
    trackingRateHz = $proof.movingTrackingRateHz
    movingInferenceP50Ms = $proof.movingInferenceP50Ms
    movingInferenceP95Ms = $proof.movingInferenceP95Ms
    detectorConfidence = $proof.detectorConfidence
    landmarkConfidence = $proof.landmarkConfidence
    elapsedMilliseconds = [math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)
    nativeProof = $proofPath
    nativeLog = $logPath
    note = 'Tracking acceptance is independent of the renderer visual/performance status.'
}
$evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $evidencePath -Encoding utf8

if (-not $trackingAccepted) {
    Write-Error "Real moving-person tracking replay failed; see $evidencePath"
    exit 1
}

Write-Output $evidencePath
exit 0
