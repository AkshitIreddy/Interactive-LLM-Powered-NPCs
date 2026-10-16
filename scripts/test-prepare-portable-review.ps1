[CmdletBinding()]
param(
    # An already prepared package to tamper-test (copied; never modified). Defaults to
    # <artifact root>\portable-review when it exists, otherwise that part is skipped.
    [string]$PackageDirectory = '',
    [string]$ArtifactRoot = ''
)

# Headless contract tests for scripts/windows/prepare-portable-review.ps1 and
# verify-portable-review.ps1. Nothing is built or launched. The negative cases use
# the real practice game, mouth atlas and (when present) prepared package from the
# local workspace; sections whose inputs are absent are reported as skipped, not
# silently passed.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') { throw 'The portable review test requires Windows.' }
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$workspaceRoot = Split-Path -Parent $repoRoot
if ([string]::IsNullOrWhiteSpace($ArtifactRoot)) {
    $ArtifactRoot = if (-not [string]::IsNullOrWhiteSpace($env:NPC_LARGE_ARTIFACT_ROOT)) {
        $env:NPC_LARGE_ARTIFACT_ROOT
    } else {
        Join-Path $workspaceRoot '.cache'
    }
}
$artifactRootFull = [System.IO.Path]::GetFullPath($ArtifactRoot).TrimEnd('\')
$prepareScript = Join-Path $PSScriptRoot 'windows/prepare-portable-review.ps1'
$verifyScript = Join-Path $PSScriptRoot 'windows/verify-portable-review.ps1'
$gameDirectory = Join-Path $workspaceRoot 'local-app-data/test-game'
$atlasReceipt = Join-Path $artifactRootFull 'mara-atlas/review-mouth-atlas/reviewed-artifact-receipt.v1.json'
if ([string]::IsNullOrWhiteSpace($PackageDirectory)) {
    $PackageDirectory = Join-Path $artifactRootFull 'portable-review'
}
$approval = 'test approval: procedural atlas'
# A destination that never exists, so preflight reaches the input checks even when a real package is present.
$freshDestination = Join-Path $artifactRootFull 'portable-review-test-nonexistent'
$passed = New-Object System.Collections.Generic.List[string]
$skipped = New-Object System.Collections.Generic.List[string]

function Assert-Throws {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][scriptblock]$Action,
        [Parameter(Mandatory)][string]$MessagePattern
    )
    $message = $null
    try { & $Action | Out-Null }
    catch { $message = $_.Exception.Message }
    if ($null -eq $message) { throw "$Name was accepted." }
    if ($message -notmatch $MessagePattern) { throw "$Name failed with an unexpected message: $message" }
    $passed.Add($Name)
}

function Remove-OwnedScratch {
    param([Parameter(Mandatory)][string]$Path)
    $absolute = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    if (-not $absolute.StartsWith("$artifactRootFull\", [System.StringComparison]::OrdinalIgnoreCase) -or
        [System.IO.Path]::GetFileName($absolute) -cnotmatch '^portable-review-test-[a-f0-9]{32}$') {
        throw "Refusing to delete unexpected scratch path: $absolute"
    }
    if (-not (Test-Path -LiteralPath $absolute)) { return }
    $reparse = @(Get-ChildItem -LiteralPath $absolute -Force -Recurse |
        Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint })
    if ($reparse.Count -ne 0) { throw "Refusing to delete scratch containing reparse points: $absolute" }
    [System.IO.Directory]::Delete($absolute, $true)
}

$scratch = Join-Path $artifactRootFull ('portable-review-test-' + [Guid]::NewGuid().ToString('N'))
try {
    # ---- Preflight contract ------------------------------------------------
    if (Test-Path -LiteralPath $PackageDirectory) {
        Assert-Throws -Name 'existing destination is never replaced' -MessagePattern 'already exists' -Action {
            & $prepareScript -PreflightOnly -ArtifactRoot $artifactRootFull -DestinationRoot $PackageDirectory
        }
    }
    Assert-Throws -Name 'receipt path and legacy atlas directory together' `
        -MessagePattern 'not both' -Action {
            & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ReviewedMouthAtlasReceiptPath 'x' -MouthAtlasDirectory 'y'
        }
    Assert-Throws -Name 'destination outside the artifact root' `
        -MessagePattern 'must be below' -Action {
            & $prepareScript -PreflightOnly -ArtifactRoot $artifactRootFull `
                -DestinationRoot (Join-Path (Split-Path -Parent $artifactRootFull) 'elsewhere')
        }
    Assert-Throws -Name 'missing mouth-atlas receipt' -MessagePattern 'receipt is missing' -Action {
        & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ArtifactRoot $artifactRootFull `
            -ReviewedMouthAtlasReceiptPath (Join-Path $scratch 'no-such-receipt.json')
    }
    Assert-Throws -Name 'reuse without an attested build commit' `
        -MessagePattern 'requires -ReusedBuildCommit' -Action {
            & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ArtifactRoot $artifactRootFull -MouthAtlasOwnerApproval $approval -ReuseExistingBuild
        }
    Assert-Throws -Name 'reuse with an unknown build commit' `
        -MessagePattern 'not a commit in this repository' -Action {
            & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ArtifactRoot $artifactRootFull -MouthAtlasOwnerApproval $approval -ReuseExistingBuild `
                -ReusedBuildCommit 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeef'
        }

    $haveGame = Test-Path -LiteralPath (Join-Path $gameDirectory 'REVIEW-FIXTURE-MANIFEST.json') -PathType Leaf
    $haveAtlas = Test-Path -LiteralPath $atlasReceipt -PathType Leaf
    if ($haveGame -and $haveAtlas) {
        $atlasText = Get-Content -LiteralPath $atlasReceipt -Raw
        $pending = $atlasText -match '(?i)\bpending\b'
        if ($pending) {
            Assert-Throws -Name 'pending atlas receipt without owner approval' `
                -MessagePattern 'visual review as pending' -Action {
                    & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ArtifactRoot $artifactRootFull `
                        -StableTestGameDirectory $gameDirectory -ReviewedMouthAtlasReceiptPath $atlasReceipt
                }
        }
        else { $skipped.Add('pending atlas receipt without owner approval (receipt is not pending)') }

        $preflightJson = & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ArtifactRoot $artifactRootFull `
            -StableTestGameDirectory $gameDirectory -ReviewedMouthAtlasReceiptPath $atlasReceipt `
            -MouthAtlasOwnerApproval $approval 3>$null
        $preflight = $preflightJson | ConvertFrom-Json
        if ($preflight.status -ne 'ready' -or
            @('static-five-file', 'moving-camera-sequence') -notcontains $preflight.stable_test_game_kind -or
            $preflight.mouth_atlas_receipt_review_pending -ne $pending -or
            $preflight.installer_used -ne $false -or $preflight.desktop_launch_performed -ne $false) {
            throw 'Preflight did not report the expected game kind, atlas boundary or safety flags.'
        }
        $passed.Add("preflight ready ($($preflight.stable_test_game_kind), schema $($preflight.mouth_atlas_schema_version) atlas)")

        # A tampered practice game must fail the prerequisite check, not be packaged.
        New-Item -ItemType Directory -Path $scratch -Force | Out-Null
        $tamperedGame = Join-Path $scratch 'test-game'
        New-Item -ItemType Directory -Path $tamperedGame -Force | Out-Null
        Get-ChildItem -LiteralPath $gameDirectory -File | ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $tamperedGame $_.Name)
        }
        [System.IO.File]::WriteAllText((Join-Path $tamperedGame 'unknown.dll'), 'not allowed')
        Assert-Throws -Name 'practice game with an unknown file' `
            -MessagePattern 'missing or unknown file' -Action {
                & $prepareScript -PreflightOnly -DestinationRoot $freshDestination -ArtifactRoot $artifactRootFull `
                    -StableTestGameDirectory $tamperedGame -ReviewedMouthAtlasReceiptPath $atlasReceipt `
                    -MouthAtlasOwnerApproval $approval 3>$null
            }
    }
    else {
        $skipped.Add('preflight and game tamper cases (practice game or mouth atlas not present)')
    }

    # ---- Verifier tamper cases against a copy of a prepared package -------------
    # The verifier pins a package to its recorded review root, so a relocated copy is checked with
    # -AllowPendingOutcome (which relaxes only that root pin and the pending-outcome states).
    $manifestPath = Join-Path $PackageDirectory 'REVIEW-MANIFEST.json'
    if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
        $packageCopy = $scratch
        New-Item -ItemType Directory -Path $packageCopy -Force | Out-Null
        $copyRoot = Join-Path $packageCopy 'pkg'
        Copy-Item -LiteralPath $PackageDirectory -Destination $copyRoot -Recurse
        $copiedManifest = Join-Path $copyRoot 'REVIEW-MANIFEST.json'
        $originalManifest = [System.IO.File]::ReadAllText($copiedManifest)
        $manifest = $originalManifest | ConvertFrom-Json

        $verified = (& $verifyScript -Directory $copyRoot -ArtifactRoot $artifactRootFull -AllowPendingOutcome) | ConvertFrom-Json
        if ($verified.status -ne 'passed') { throw 'The unmodified package copy did not verify.' }
        $passed.Add("unmodified package verifies (build origin $($verified.application_build_origin), game $($verified.test_game_kind))")

        function Assert-ManifestEditRejected {
            param([string]$Name, [string]$Pattern, [string]$Replacement, [string]$MessagePattern)
            $edited = [System.Text.RegularExpressions.Regex]::Replace($originalManifest, $Pattern, $Replacement)
            if ($edited -ceq $originalManifest) { $skipped.Add("$Name (field not present in this package)"); return }
            [System.IO.File]::WriteAllText($copiedManifest, $edited)
            try {
                Assert-Throws -Name $Name -MessagePattern $MessagePattern -Action {
                    & $verifyScript -Directory $copyRoot -ArtifactRoot $artifactRootFull -AllowPendingOutcome
                }
            }
            finally { [System.IO.File]::WriteAllText($copiedManifest, $originalManifest) }
        }

        if ($manifest.mouth_atlas.receipt_review_pending -eq $true) {
            Assert-ManifestEditRejected -Name 'verifier: pending atlas without owner approval' `
                -Pattern '("owner_approval_attestation":\s*)"[^"]*"' -Replacement '${1}null' `
                -MessagePattern 'pending and the manifest has no owner-approval'
        }
        if ($manifest.test_game.PSObject.Properties.Name -contains 'fixture_kind') {
            $flipped = if ($manifest.test_game.fixture_kind -ceq 'static-five-file') {
                'moving-camera-sequence'
            } else { 'static-five-file' }
            Assert-ManifestEditRejected -Name 'verifier: practice-game kind disagrees with the files' `
                -Pattern '("fixture_kind":\s*)"[a-z-]+"' -Replacement ('${1}"' + $flipped + '"') `
                -MessagePattern 'test game failed provenance|camera-sequence'
        }
        if ($manifest.application.build_origin -ceq 'reused-existing-debug-build') {
            Assert-ManifestEditRejected -Name 'verifier: reused build without a valid commit attestation' `
                -Pattern '("attested_build_commit":\s*)"[0-9a-f]{40}"' -Replacement '${1}"not-a-commit"' `
                -MessagePattern 'reused-build evidence'
            Assert-ManifestEditRejected -Name 'verifier: reused build recorded as a fresh build' `
                -Pattern '"build_origin":\s*"reused-existing-debug-build"' `
                -Replacement '"build_origin": "fresh-installer-free-build"' `
                -MessagePattern 'build origin or fresh-build outcome'
        }

        $exePath = Join-Path $copyRoot 'interactive-npcs-control.exe'
        $stream = [System.IO.File]::Open($exePath, 'Open', 'ReadWrite')
        try {
            $stream.Position = 4096
            $byte = $stream.ReadByte()
            $stream.Position = 4096
            $stream.WriteByte([byte](($byte + 1) % 256))
        }
        finally { $stream.Dispose() }
        Assert-Throws -Name 'verifier: modified application binary' `
            -MessagePattern 'hash or size mismatch: interactive-npcs-control.exe' -Action {
                & $verifyScript -Directory $copyRoot -ArtifactRoot $artifactRootFull -AllowPendingOutcome
            }
    }
    else { $skipped.Add("verifier tamper cases (no prepared package at $PackageDirectory)") }
}
finally {
    if (Test-Path -LiteralPath $scratch) { Remove-OwnedScratch -Path $scratch }
}

[ordered]@{
    schema = 'interactive-npcs-portable-review-tests/v1'
    status = 'passed'
    passed = @($passed)
    skipped = @($skipped)
    scratch_deleted = -not (Test-Path -LiteralPath $scratch)
} | ConvertTo-Json -Depth 4
