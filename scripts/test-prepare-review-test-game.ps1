[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') { throw 'The review fixture packaging test requires Windows.' }
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$operationRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("InteractiveNpcsReviewFixture-" + [guid]::NewGuid().ToString('N'))
$destination = Join-Path $operationRoot 'local-app-data\test-game'
$tamperRoot = Join-Path $operationRoot 'tamper\local-app-data\test-game'
$prepareScript = Join-Path $PSScriptRoot 'windows\prepare-review-test-game.ps1'
$verifyScript = Join-Path $PSScriptRoot 'windows\verify-review-test-game.ps1'

function Assert-VerificationFails {
    param([Parameter(Mandatory)][string]$Directory, [Parameter(Mandatory)][string]$Label)
    $failed = $false
    try { & $verifyScript -Directory $Directory | Out-Null }
    catch { $failed = $true }
    if (-not $failed) { throw "Review fixture verification did not fail for $Label." }
}

try {
    $preparedJson = & $prepareScript -DestinationDirectory $destination
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $prepared = $preparedJson | ConvertFrom-Json
    if ($prepared.schema_version -ne 2 -or $prepared.status -ne 'prepared' -or
        $prepared.fixture_source -ne 'project-source-generated-native-v1' -or
        $prepared.renderer -ne 'project-owned-gdi-generated-v1' -or
        $prepared.generated_frame_rendering -ne $true -or $prepared.audio_generated_pcm -ne $true -or
        $prepared.media_files_bundled -ne 0 -or @($prepared.third_party_binaries_bundled).Count -ne 0 -or
        $prepared.license_expression -ne 'MIT') {
        throw 'Prepared review fixture summary violates the project-owned standalone contract.'
    }
    $verified = (& $verifyScript -Directory $destination) | ConvertFrom-Json
    if ($verified.status -ne 'passed' -or $verified.file_count -ne 5 -or $verified.third_party_binary_count -ne 0) {
        throw 'Prepared review fixture did not pass its independent verifier.'
    }
    $expected = @(
        'interactive-npcs-synthetic-target.exe', 'README.md', 'REVIEW-FIXTURE-MANIFEST.json',
        'THIRD-PARTY-NOTICES.md', 'review-test-game.cdx.json'
    ) | Sort-Object
    $actual = @(Get-ChildItem -LiteralPath $destination -File -Force | Select-Object -ExpandProperty Name | Sort-Object)
    if (($actual -join "`n") -cne ($expected -join "`n") -or
        @(Get-ChildItem -LiteralPath $destination -Directory -Force).Count -ne 0) {
        throw 'Prepared review fixture exact file allowlist is invalid.'
    }
    foreach ($forbidden in @('ffmpeg.exe', 'ffprobe.exe', 'eclipse-harbor-synthetic-game.mp4')) {
        if (Test-Path -LiteralPath (Join-Path $destination $forbidden)) { throw "Forbidden redistributed media dependency exists: $forbidden" }
    }

    New-Item -ItemType Directory -Path $tamperRoot -Force | Out-Null
    Copy-Item -Path (Join-Path $destination '*') -Destination $tamperRoot -Force
    [System.IO.File]::WriteAllText((Join-Path $tamperRoot 'unknown.dll'), 'not allowed', (New-Object System.Text.UTF8Encoding($false)))
    Assert-VerificationFails -Directory $tamperRoot -Label 'unknown file injection'
    Remove-Item -LiteralPath (Join-Path $tamperRoot 'unknown.dll') -Force
    Add-Content -LiteralPath (Join-Path $tamperRoot 'README.md') -Value 'tampered'
    Assert-VerificationFails -Directory $tamperRoot -Label 'manifested file mutation'

    $manifestPath = Join-Path $destination 'REVIEW-FIXTURE-MANIFEST.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if (@($manifest.third_party_binaries).Count -ne 0 -or
        $manifest.manifest_self.path -ne 'REVIEW-FIXTURE-MANIFEST.json' -or
        $manifest.manifest_self.hash -ne 'excluded-to-avoid-circularity' -or
        $manifest.build.reproducible_rebuild_verified -ne $true) {
        throw 'Prepared review fixture manifest omits no-third-party, self-hash, or reproducibility evidence.'
    }

    [ordered]@{
        schema_version = 1
        status = 'passed'
        directory = $destination
        file_count = $actual.Count
        executable_sha256 = [string]$prepared.executable_sha256
        distribution_manifest_sha256 = [string]$prepared.distribution_manifest_sha256
        sbom_sha256 = [string]$prepared.sbom_sha256
        notices_sha256 = [string]$prepared.notices_sha256
        unknown_file_rejected = $true
        content_tamper_rejected = $true
        reproducible_rebuild_verified = [bool]$manifest.build.reproducible_rebuild_verified
        third_party_binary_count = @($manifest.third_party_binaries).Count
    } | ConvertTo-Json -Depth 4
}
finally {
    if (Test-Path -LiteralPath $operationRoot) { Remove-Item -LiteralPath $operationRoot -Recurse -Force }
}
