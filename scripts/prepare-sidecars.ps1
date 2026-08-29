[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$destination = Join-Path $repoRoot 'apps/control/src-tauri/binaries'
$cargoProfile = if ($Configuration -eq 'Debug') { 'debug' } else { 'release' }
$nativeBuild = Join-Path $repoRoot 'artifacts/media-broker-build'

if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        $cargoArguments = @('build', '-p', 'npc-runtime-host', '--locked')
        if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) { throw 'Runtime host build failed.' }

        if (-not (Test-Path -LiteralPath (Join-Path $nativeBuild 'CMakeCache.txt'))) {
            & cmake -S native/media-broker -B $nativeBuild -G 'Visual Studio 17 2022' -A x64
            if ($LASTEXITCODE -ne 0) { throw 'Media broker configuration failed.' }
        }
        & cmake --build $nativeBuild --config $Configuration --parallel 4
        if ($LASTEXITCODE -ne 0) { throw 'Media broker build failed.' }
    }
    finally {
        Pop-Location
    }
}

$runtimeSource = Join-Path $repoRoot "target/$cargoProfile/npc-runtime.exe"
$brokerSource = Join-Path $nativeBuild "$Configuration/npc-media-broker.exe"
foreach ($source in @($runtimeSource, $brokerSource)) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Required sidecar was not built: $source"
    }
}

New-Item -ItemType Directory -Path $destination -Force | Out-Null
Copy-Item -LiteralPath $runtimeSource -Destination (Join-Path $destination 'npc-runtime-x86_64-pc-windows-msvc.exe') -Force
Copy-Item -LiteralPath $brokerSource -Destination (Join-Path $destination 'npc-media-broker-x86_64-pc-windows-msvc.exe') -Force

Write-Host "Prepared project sidecars in $destination" -ForegroundColor Green
Write-Host 'No third-party runtime or model payload was bundled.' -ForegroundColor Yellow
