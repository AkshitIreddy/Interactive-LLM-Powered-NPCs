[CmdletBinding()]
param(
    [switch]$BuildNative
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: sidecar build-path regression requires Windows path semantics.'
    exit 0
}

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) { throw $Message }
}

function Get-RepositoryBuildKey {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $canonicalRoot = [System.IO.Path]::GetFullPath($RepositoryRoot).
        Replace('/', '\').
        TrimEnd('\').
        ToUpperInvariant()
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($canonicalRoot)
        $digest = $sha256.ComputeHash($bytes)
    }
    finally {
        $sha256.Dispose()
    }

    return (($digest | ForEach-Object { $_.ToString('x2') }) -join '').Substring(0, 16)
}

function New-DeepCheckoutFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$LocalAppDataRoot,
        [Parameter(Mandatory = $true)][string]$Marker
    )

    $deepParent = $Root
    foreach ($segment in @(
            'clean-source-archive-with-a-long-parent-name',
            'nested-automated-validation-workspace'
        )) {
        $deepParent = Join-Path $deepParent $segment
    }
    $repoRoot = Join-Path $deepParent 'sanitized-tree'
    $scriptsRoot = Join-Path $repoRoot 'scripts'
    $runtimeRoot = Join-Path $repoRoot 'target/debug'
    New-Item -ItemType Directory -Path $scriptsRoot, $runtimeRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'prepare-sidecars.ps1') -Destination (Join-Path $scriptsRoot 'prepare-sidecars.ps1')
    [System.IO.File]::WriteAllText((Join-Path $runtimeRoot 'npc-runtime.exe'), "runtime-$Marker")

    $buildKey = Get-RepositoryBuildKey -RepositoryRoot $repoRoot
    $nativeBuild = Join-Path $LocalAppDataRoot "InteractiveNPCs/build/mb/$buildKey"
    $brokerRoot = Join-Path $nativeBuild 'Debug'
    New-Item -ItemType Directory -Path $brokerRoot -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $brokerRoot 'npc-media-broker.exe'), "broker-$Marker")

    return [pscustomobject]@{
        RepoRoot = $repoRoot
        ScriptPath = Join-Path $scriptsRoot 'prepare-sidecars.ps1'
        BuildKey = $buildKey
        NativeBuild = $nativeBuild
        LegacyNativeBuild = Join-Path $repoRoot 'artifacts/media-broker-build'
        Destination = Join-Path $repoRoot 'apps/control/src-tauri/binaries'
        Marker = $Marker
    }
}

$caseRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-sidecar-path-$([Guid]::NewGuid().ToString('N'))"
$localAppDataRoot = Join-Path $caseRoot 'short-local-app-data'
$savedLocalAppData = $env:LOCALAPPDATA
New-Item -ItemType Directory -Path $caseRoot, $localAppDataRoot -Force | Out-Null

try {
    $first = New-DeepCheckoutFixture -Root (Join-Path $caseRoot 'checkout-a') -LocalAppDataRoot $localAppDataRoot -Marker 'a'
    $second = New-DeepCheckoutFixture -Root (Join-Path $caseRoot 'checkout-b') -LocalAppDataRoot $localAppDataRoot -Marker 'b'
    $env:LOCALAPPDATA = $localAppDataRoot

    Assert-True -Condition ($first.RepoRoot.Length -gt 160) -Message "Deep checkout fixture is not long enough: $($first.RepoRoot)"
    Assert-True -Condition ($first.NativeBuild.Length -lt $first.LegacyNativeBuild.Length) -Message 'Native build path did not become shorter than the repository-local legacy path.'
    Assert-True -Condition ($first.NativeBuild.Length -lt 180) -Message "Native build path still leaves too little FileTracker headroom: $($first.NativeBuild)"
    Assert-True -Condition ($first.BuildKey -ne $second.BuildKey) -Message 'Two different checkout roots resolved to the same native build key.'

    foreach ($fixture in @($first, $second)) {
        & $fixture.ScriptPath -Configuration Debug -SkipBuild

        $runtimeDestination = Join-Path $fixture.Destination 'npc-runtime-x86_64-pc-windows-msvc.exe'
        $brokerDestination = Join-Path $fixture.Destination 'npc-media-broker-x86_64-pc-windows-msvc.exe'
        Assert-True -Condition (([System.IO.File]::ReadAllText($runtimeDestination)) -eq "runtime-$($fixture.Marker)") -Message 'Runtime sidecar was not copied from the deep checkout.'
        Assert-True -Condition (([System.IO.File]::ReadAllText($brokerDestination)) -eq "broker-$($fixture.Marker)") -Message 'Native sidecar was not copied from the short repository-specific build path.'
        Assert-True -Condition ([string]::Equals(
                $env:NPC_MEDIA_BROKER_FIXTURE,
                (Join-Path $fixture.NativeBuild 'Debug/npc-media-broker.exe'),
                [System.StringComparison]::OrdinalIgnoreCase
            )) -Message "prepare-sidecars.ps1 did not publish the resolved short-cache broker fixture path. Expected beneath: $($fixture.NativeBuild); actual: $env:NPC_MEDIA_BROKER_FIXTURE"
        Assert-True -Condition (-not (Test-Path -LiteralPath $fixture.LegacyNativeBuild)) -Message 'prepare-sidecars.ps1 recreated the vulnerable repository-local native build path.'
    }

    if ($BuildNative) {
        $nativeParent = Join-Path $first.RepoRoot 'native'
        New-Item -ItemType Directory -Path $nativeParent -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../native/media-broker') -Destination $nativeParent -Recurse -Force
        Remove-Item -LiteralPath $first.NativeBuild -Recurse -Force

        $cmakeExecutable = (Get-Command cmake -CommandType Application -ErrorAction Stop |
            Select-Object -First 1).Source
        & $cmakeExecutable -S (Join-Path $first.RepoRoot 'native/media-broker') -B $first.NativeBuild -G 'Visual Studio 17 2022' -A x64 -DBUILD_TESTING=OFF -DNPC_MEDIA_BROKER_BUILD_TESTS=OFF
        if ($LASTEXITCODE -ne 0) { throw 'Deep-checkout media broker configuration failed.' }
        & $cmakeExecutable --build $first.NativeBuild --config Debug --target npc-media-broker --parallel 4
        if ($LASTEXITCODE -ne 0) { throw 'Deep-checkout media broker build failed.' }

        & $first.ScriptPath -Configuration Debug -SkipBuild
        $builtBroker = Join-Path $first.Destination 'npc-media-broker-x86_64-pc-windows-msvc.exe'
        Assert-True -Condition ((Get-Item -LiteralPath $builtBroker).Length -gt 0) -Message 'The real deep-checkout broker build was not copied into the Tauri sidecars.'
    }

    Write-Host 'Sidecar short-build-path regression checks passed.' -ForegroundColor Green
}
finally {
    [System.Environment]::SetEnvironmentVariable('LOCALAPPDATA', $savedLocalAppData, 'Process')
    Remove-Item Env:NPC_MEDIA_BROKER_FIXTURE -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $caseRoot) {
        Remove-Item -LiteralPath $caseRoot -Recurse -Force
    }
}

exit 0
