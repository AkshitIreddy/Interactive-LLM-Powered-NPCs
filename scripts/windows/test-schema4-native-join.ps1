param(
    [Parameter(Mandatory = $true)][string]$MouthWorker,
    [Parameter(Mandatory = $true)][string]$NativeServiceSmoke,
    [Parameter(Mandatory = $true)][string]$TestGame,
    [Parameter(Mandatory = $true)][string]$Schema4AtlasRoot,
    [Parameter(Mandatory = $true)][string]$PrivateCatalogRoot,
    [Parameter(Mandatory = $true)][string]$ProviderArtifactRoot,
    [Parameter(Mandatory = $true)][string]$YunetLicense,
    [Parameter(Mandatory = $true)][string]$OpenSeeFaceLicense,
    [Parameter(Mandatory = $true)][string]$ProviderStateRoot,
    [Parameter(Mandatory = $true)][string]$JoinEvidenceRoot,
    [Parameter(Mandatory = $true)][string]$LogDirectory,
    [string]$CargoTargetDirectory = 'E:\temp\InteractiveNPCs\cargo-target-schema4-join'
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'

function Resolve-NpcExistingPath {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][ValidateSet('Leaf', 'Container')][string]$PathType,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not [System.IO.Path]::IsPathFullyQualified($Path) -or
        -not (Test-Path -LiteralPath $Path -PathType $PathType)) {
        throw "$Label must be an existing absolute $PathType path: $Path"
    }
    (Resolve-Path -LiteralPath $Path).Path
}

function Assert-NpcFreshAbsolutePath {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not [System.IO.Path]::IsPathFullyQualified($Path) -or
        (Test-Path -LiteralPath $Path)) {
        throw "$Label must be a fresh absolute path: $Path"
    }
}

function Invoke-NpcCargoProof {
    param(
        [Parameter(Mandatory = $true)][string]$ExactTest,
        [Parameter(Mandatory = $true)][string]$LogPath
    )
    & cargo test --manifest-path $script:ManifestPath --lib $ExactTest -- `
        --ignored --exact --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $LogPath
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo proof failed ($LASTEXITCODE): $ExactTest"
    }
}

$RepositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$script:ManifestPath = Join-Path $RepositoryRoot 'apps\control\src-tauri\Cargo.toml'
$MouthWorker = Resolve-NpcExistingPath $MouthWorker Leaf 'Mouth worker'
$NativeServiceSmoke = Resolve-NpcExistingPath $NativeServiceSmoke Leaf 'Native service smoke'
$TestGame = Resolve-NpcExistingPath $TestGame Leaf 'Test game'
$Schema4AtlasRoot = Resolve-NpcExistingPath $Schema4AtlasRoot Container 'Schema-four atlas root'
$PrivateCatalogRoot = Resolve-NpcExistingPath $PrivateCatalogRoot Container 'Private catalog root'
$ProviderArtifactRoot = Resolve-NpcExistingPath $ProviderArtifactRoot Container 'Provider artifact root'
$YunetLicense = Resolve-NpcExistingPath $YunetLicense Leaf 'YuNet license'
$OpenSeeFaceLicense = Resolve-NpcExistingPath $OpenSeeFaceLicense Leaf 'OpenSeeFace license'
Assert-NpcFreshAbsolutePath $ProviderStateRoot 'Provider state root'
Assert-NpcFreshAbsolutePath $JoinEvidenceRoot 'Join evidence root'
if (-not [System.IO.Path]::IsPathFullyQualified($LogDirectory)) {
    throw "Log directory must be absolute: $LogDirectory"
}

New-Item -ItemType Directory -Path $LogDirectory -Force | Out-Null
New-Item -ItemType Directory -Path $CargoTargetDirectory -Force | Out-Null
$ProviderLog = Join-Path $LogDirectory 'schema4-native-join-provider-command10.log'
$JoinLog = Join-Path $LogDirectory 'schema4-native-join-processes.log'

$env:CARGO_TARGET_DIR = $CargoTargetDirectory
$env:NPC_REAL_PROVIDER_CATALOG_ROOT = $PrivateCatalogRoot
$env:NPC_REAL_PROVIDER_ARTIFACT_ROOT = $ProviderArtifactRoot
$env:NPC_REAL_YUNET_LICENSE = $YunetLicense
$env:NPC_REAL_OPENSEEFACE_LICENSE = $OpenSeeFaceLicense
$env:NPC_REAL_MOUTH_WORKER = $MouthWorker
$env:NPC_REAL_PROVIDER_STATE_ROOT = $ProviderStateRoot

Push-Location $RepositoryRoot
try {
    Invoke-NpcCargoProof `
        -ExactTest 'local_resources::tests::real_private_catalog_provider_activation_is_hidden_retryable_and_inventory_bound' `
        -LogPath $ProviderLog

    $ProviderEvidence = Join-Path $ProviderStateRoot 'real-provider-activation-evidence.json'
    if (-not (Test-Path -LiteralPath $ProviderEvidence -PathType Leaf)) {
        throw 'Provider command-10 proof did not write its expected evidence.'
    }

    $env:NPC_REAL_SCHEMA4_MOUTH_WORKER = $MouthWorker
    $env:NPC_REAL_SCHEMA4_NATIVE_SERVICE_SMOKE = $NativeServiceSmoke
    $env:NPC_REAL_SCHEMA4_TEST_GAME = $TestGame
    $env:NPC_REAL_SCHEMA4_ATLAS_ROOT = $Schema4AtlasRoot
    $env:NPC_REAL_SCHEMA4_PROVIDER_EVIDENCE = $ProviderEvidence
    $env:NPC_REAL_SCHEMA4_JOIN_EVIDENCE_ROOT = $JoinEvidenceRoot
    Invoke-NpcCargoProof `
        -ExactTest 'visual_runtime::schema4_join_tests::real_schema_four_worker_join_is_headless_and_fail_closed' `
        -LogPath $JoinLog
}
finally {
    Pop-Location
}

$JoinReceipt = Join-Path $JoinEvidenceRoot 'schema4-native-join-receipt.json'
if (-not (Test-Path -LiteralPath $JoinReceipt -PathType Leaf)) {
    throw 'Schema-four process join did not write its expected receipt.'
}
$Receipt = Get-Content -LiteralPath $JoinReceipt -Raw | ConvertFrom-Json
if ([string]$Receipt.status -ne 'passed' -or
    -not [bool]$Receipt.providerActivation.command10LoadAndUnloadProven -or
    -not [bool]$Receipt.schema4Atlas.exactInstallAccepted -or
    -not [bool]$Receipt.nativeOwnedTextureSmoke.d3dSourceWasTestOwned) {
    throw 'Schema-four join receipt did not preserve the required evidence boundaries.'
}

$SummaryPath = Join-Path $JoinEvidenceRoot 'schema4-native-join-run.json'
$Summary = [ordered]@{
    schemaVersion = 1
    status = 'passed'
    repositoryHead = (& git -C $RepositoryRoot rev-parse HEAD).Trim()
    providerStateRoot = [System.IO.Path]::GetFullPath($ProviderStateRoot)
    joinEvidenceRoot = [System.IO.Path]::GetFullPath($JoinEvidenceRoot)
    joinReceipt = [ordered]@{
        path = [System.IO.Path]::GetFullPath($JoinReceipt)
        sha256 = (Get-FileHash -LiteralPath $JoinReceipt -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    logs = @(
        [ordered]@{
            path = [System.IO.Path]::GetFullPath($ProviderLog)
            sha256 = (Get-FileHash -LiteralPath $ProviderLog -Algorithm SHA256).Hash.ToLowerInvariant()
        },
        [ordered]@{
            path = [System.IO.Path]::GetFullPath($JoinLog)
            sha256 = (Get-FileHash -LiteralPath $JoinLog -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    )
    limitations = @(
        'No production actor-lock qualification was minted.',
        'No target-PID runtime loadout admission was minted.',
        'No desktop or commercial-game capture was used.',
        'No audio was played and no broker overlay was presented.',
        'The schema-four atlas enrollment is a mechanical test fixture and remains uninstalled and disabled.'
    )
}
[System.IO.File]::WriteAllText(
    $SummaryPath,
    (($Summary | ConvertTo-Json -Depth 8) + [Environment]::NewLine),
    [System.Text.UTF8Encoding]::new($false)
)
$Summary | ConvertTo-Json -Depth 8

