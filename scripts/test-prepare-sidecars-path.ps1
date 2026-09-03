[CmdletBinding()]
param(
    [switch]$BuildNative
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: sidecar build-path regression requires Windows PE/path semantics.'
    exit 0
}

function Assert-True {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw $Message }
}

function Get-RepositoryBuildKey {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)
    $canonical = [System.IO.Path]::GetFullPath($RepositoryRoot).Replace('/', '\').TrimEnd('\').ToUpperInvariant()
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try { $digest = $sha256.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($canonical)) }
    finally { $sha256.Dispose() }
    return (($digest | ForEach-Object { $_.ToString('x2') }) -join '').Substring(0, 16)
}

function New-GuiPeFixture {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][byte]$Marker)
    $bytes = New-Object byte[] 512
    $bytes[0] = 0x4d; $bytes[1] = 0x5a
    [Array]::Copy([BitConverter]::GetBytes([int32]0x80), 0, $bytes, 0x3c, 4)
    $bytes[0x60] = $Marker
    $bytes[0x80] = 0x50; $bytes[0x81] = 0x45
    [Array]::Copy([BitConverter]::GetBytes([uint16]0x8664), 0, $bytes, 0x84, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]0), 0, $bytes, 0x86, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]0xf0), 0, $bytes, 0x94, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]0x20b), 0, $bytes, 0x98, 2)
    [Array]::Copy([BitConverter]::GetBytes([uint16]2), 0, $bytes, (0x98 + 0x44), 2)
    [Array]::Copy([BitConverter]::GetBytes([uint32]0), 0, $bytes, (0x98 + 0x6c), 4)
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    [System.IO.File]::WriteAllBytes($Path, $bytes)
}

function New-CheckoutFixture {
    param([string]$Root, [string]$LocalAppDataRoot, [byte]$Marker)
    $repoRoot = Join-Path $Root 'clean-source-archive-with-a-long-parent-name/nested-validation/sanitized-tree'
    $scriptsRoot = Join-Path $repoRoot 'scripts'
    New-Item -ItemType Directory -Path $scriptsRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'prepare-sidecars.ps1') -Destination $scriptsRoot
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'audit-pe-product-binary.ps1') -Destination $scriptsRoot
    $windowsRoot = Join-Path $scriptsRoot 'windows'
    New-Item -ItemType Directory -Path $windowsRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows/node-tooling.ps1') -Destination $windowsRoot

    $key = Get-RepositoryBuildKey -RepositoryRoot $repoRoot
    $sources = [ordered]@{
        'runtime' = Join-Path $repoRoot 'target/debug/npc-runtime.exe'
        'media-broker' = Join-Path $LocalAppDataRoot "InteractiveNPCs/build/mb/$key/Release/npc-media-broker.exe"
        'mouth-worker' = Join-Path $LocalAppDataRoot "InteractiveNPCs/build/mw/$key/Release/npc-mouth-worker.exe"
        'subtitle-presenter' = Join-Path $LocalAppDataRoot "InteractiveNPCs/build/st/$key/Release/npc-subtitle-presenter.exe"
    }
    $offset = [byte]0
    foreach ($source in $sources.Values) {
        New-GuiPeFixture -Path $source -Marker ([byte]($Marker + $offset))
        $offset++
    }
    return [pscustomobject]@{
        RepoRoot = $repoRoot
        ScriptPath = Join-Path $scriptsRoot 'prepare-sidecars.ps1'
        BuildKey = $key
        Sources = $sources
        Destination = Join-Path $repoRoot 'apps/control/src-tauri/binaries'
    }
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-sidecars-$([Guid]::NewGuid().ToString('N'))"
$localAppDataRoot = Join-Path $fixtureRoot 'short-local-app-data'
$savedLocalAppData = $env:LOCALAPPDATA
$savedLargeArtifactRoot = [System.Environment]::GetEnvironmentVariable(
    'NPC_LARGE_ARTIFACT_ROOT', 'Process')
New-Item -ItemType Directory -Path $fixtureRoot, $localAppDataRoot -Force | Out-Null
try {
    $first = New-CheckoutFixture -Root (Join-Path $fixtureRoot 'checkout-a') -LocalAppDataRoot $localAppDataRoot -Marker 10
    $second = New-CheckoutFixture -Root (Join-Path $fixtureRoot 'checkout-b') -LocalAppDataRoot $localAppDataRoot -Marker 30
    $env:LOCALAPPDATA = $localAppDataRoot
    # This isolated fixture deliberately validates the default LocalAppData
    # cache layout. Do not let the caller's external large-artifact override
    # redirect its synthetic binaries outside the fixture tree.
    Remove-Item Env:NPC_LARGE_ARTIFACT_ROOT -ErrorAction SilentlyContinue
    Assert-True -Condition ($first.BuildKey -ne $second.BuildKey) -Message 'Distinct checkouts collided in native cache keys.'

    foreach ($fixture in @($first, $second)) {
        $result = (& $fixture.ScriptPath -Configuration Debug -SkipBuild) | ConvertFrom-Json
        Assert-True -Condition ($result.status -eq 'prepared') -Message 'Sidecar staging did not report prepared.'
        Assert-True -Condition (@($result.binaries).Count -eq 4) -Message 'Sidecar staging did not emit four binaries.'
        $manifest = Get-Content -LiteralPath $result.manifest_path -Raw | ConvertFrom-Json
        Assert-True -Condition ($manifest.application_configuration -eq 'Debug') -Message 'Application profile was not Debug.'
        Assert-True -Condition ($manifest.native_configuration -eq 'Release') -Message 'C++ profile was not forced to Release.'
        Assert-True -Condition ($manifest.product_audio_route -eq 'media-broker') -Message 'Broker audio is not the product route.'
        Assert-True -Condition (@($manifest.runtime_features).Count -eq 0) -Message 'Packaged runtime enabled a developer feature.'
        foreach ($binary in $manifest.binaries) {
            Assert-True -Condition ($binary.pe_subsystem -eq 'windows_gui') -Message "$($binary.id) is not GUI subsystem."
            Assert-True -Condition (@($binary.debug_crt_imports).Count -eq 0) -Message "$($binary.id) imports Debug CRT."
            $staged = Join-Path $fixture.Destination $binary.file_name
            Assert-True -Condition (Test-Path -LiteralPath $staged -PathType Leaf) -Message "Missing staged $($binary.file_name)."
            Assert-True -Condition ((Get-FileHash -Algorithm SHA256 $staged).Hash.ToLowerInvariant() -eq $binary.sha256) -Message "Hash mismatch for $($binary.file_name)."
            if ($binary.id -ne 'runtime') {
                Assert-True -Condition ($binary.build_configuration -eq 'Release') -Message "$($binary.id) was not a Release C++ binary."
            }
        }
        Assert-True -Condition ([string]::Equals($env:NPC_MEDIA_BROKER_FIXTURE,
                $fixture.Sources['media-broker'], [System.StringComparison]::OrdinalIgnoreCase)) `
            -Message 'Media-broker fixture did not resolve to the Release cache output.'
    }

    if ($BuildNative) {
        [System.Environment]::SetEnvironmentVariable(
            'NPC_LARGE_ARTIFACT_ROOT', $savedLargeArtifactRoot, 'Process')
        Write-Host 'Building and auditing the real four-sidecar product set.' -ForegroundColor Cyan
        & (Join-Path $PSScriptRoot 'prepare-sidecars.ps1') -Configuration Release
        if ($LASTEXITCODE -ne 0) { throw "Real sidecar preparation exited with code $LASTEXITCODE." }
    }
    Write-Host 'Four-sidecar Release/product path regression checks passed.' -ForegroundColor Green
}
finally {
    [System.Environment]::SetEnvironmentVariable('LOCALAPPDATA', $savedLocalAppData, 'Process')
    [System.Environment]::SetEnvironmentVariable(
        'NPC_LARGE_ARTIFACT_ROOT', $savedLargeArtifactRoot, 'Process')
    Remove-Item Env:NPC_MEDIA_BROKER_FIXTURE -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $fixtureRoot) { Remove-Item -LiteralPath $fixtureRoot -Recurse -Force }
}

exit 0
