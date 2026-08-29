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
$nativeCache = Join-Path $nativeBuild 'CMakeCache.txt'

function Get-CMakeCacheEntry {
    param(
        [Parameter(Mandatory = $true)][string]$CachePath,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $escapedName = [regex]::Escape($Name)
    $match = Select-String -LiteralPath $CachePath -Pattern "^$escapedName(?::[^=]+)?=(.*)$" |
        Select-Object -First 1
    if ($null -eq $match) { return $null }
    return $match.Matches[0].Groups[1].Value
}

function Normalize-ComparablePath {
    param([Parameter(Mandatory = $true)][string]$Path)

    return $Path.Replace('/', '\').TrimEnd('\')
}

if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        $cargoArguments = @('build', '-p', 'npc-runtime-host', '--locked')
        if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) { throw 'Runtime host build failed.' }

        $cmakeExecutable = (Get-Command cmake -CommandType Application -ErrorAction Stop |
            Select-Object -First 1).Source
        $needsNativeConfigure = -not (Test-Path -LiteralPath $nativeCache -PathType Leaf)
        if (-not $needsNativeConfigure) {
            $cachedCmake = Get-CMakeCacheEntry -CachePath $nativeCache -Name 'CMAKE_COMMAND'
            $cachedGenerator = Get-CMakeCacheEntry -CachePath $nativeCache -Name 'CMAKE_GENERATOR'
            $cachedPlatform = Get-CMakeCacheEntry -CachePath $nativeCache -Name 'CMAKE_GENERATOR_PLATFORM'
            $compatibleCache =
                -not [string]::IsNullOrWhiteSpace($cachedCmake) -and
                ([string]::Equals(
                    (Normalize-ComparablePath $cachedCmake),
                    (Normalize-ComparablePath $cmakeExecutable),
                    [System.StringComparison]::OrdinalIgnoreCase
                )) -and
                $cachedGenerator -eq 'Visual Studio 17 2022' -and
                $cachedPlatform -eq 'x64'
            if (-not $compatibleCache) {
                Write-Host 'Refreshing the disposable media-broker build cache because its CMake toolchain changed.' -ForegroundColor Yellow
                Remove-Item -LiteralPath $nativeBuild -Recurse -Force
                $needsNativeConfigure = $true
            }
        }

        if ($needsNativeConfigure) {
            & $cmakeExecutable -S native/media-broker -B $nativeBuild -G 'Visual Studio 17 2022' -A x64
            if ($LASTEXITCODE -ne 0) { throw 'Media broker configuration failed.' }
        }
        & $cmakeExecutable --build $nativeBuild --config $Configuration --parallel 4
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
