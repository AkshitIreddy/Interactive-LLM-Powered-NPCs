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
$nativeSource = Join-Path $repoRoot 'native/media-broker'

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

$localAppData = $env:LOCALAPPDATA
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    $localAppData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
}
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'LOCALAPPDATA could not be resolved for the disposable native build cache.'
}

$nativeBuildBase = Join-Path $localAppData 'InteractiveNPCs/build/mb'
$nativeBuildKey = Get-RepositoryBuildKey -RepositoryRoot $repoRoot
$nativeBuild = Join-Path $nativeBuildBase $nativeBuildKey
$nativeCache = Join-Path $nativeBuild 'CMakeCache.txt'

$canonicalRepoRoot = [System.IO.Path]::GetFullPath($repoRoot).TrimEnd('\')
$canonicalNativeBuildBase = [System.IO.Path]::GetFullPath($nativeBuildBase).TrimEnd('\')
$canonicalNativeBuild = [System.IO.Path]::GetFullPath($nativeBuild).TrimEnd('\')
if (-not $canonicalNativeBuild.StartsWith(
        "$canonicalNativeBuildBase\",
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or $nativeBuildKey -notmatch '^[0-9a-f]{16}$') {
    throw "Refusing unsafe native build path: $nativeBuild"
}
if ($canonicalNativeBuild.Equals($canonicalRepoRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
    $canonicalNativeBuild.StartsWith("$canonicalRepoRoot\", [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "The disposable native build path must remain outside the repository: $nativeBuild"
}

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
            $cachedSource = Get-CMakeCacheEntry -CachePath $nativeCache -Name 'CMAKE_HOME_DIRECTORY'
            $compatibleCache =
                -not [string]::IsNullOrWhiteSpace($cachedCmake) -and
                ([string]::Equals(
                    (Normalize-ComparablePath $cachedCmake),
                    (Normalize-ComparablePath $cmakeExecutable),
                    [System.StringComparison]::OrdinalIgnoreCase
                )) -and
                $cachedGenerator -eq 'Visual Studio 17 2022' -and
                $cachedPlatform -eq 'x64' -and
                -not [string]::IsNullOrWhiteSpace($cachedSource) -and
                ([string]::Equals(
                    (Normalize-ComparablePath $cachedSource),
                    (Normalize-ComparablePath $nativeSource),
                    [System.StringComparison]::OrdinalIgnoreCase
                ))
            if (-not $compatibleCache) {
                Write-Host 'Refreshing the disposable media-broker build cache because its source or CMake toolchain changed.' -ForegroundColor Yellow
                Remove-Item -LiteralPath $nativeBuild -Recurse -Force
                $needsNativeConfigure = $true
            }
        }

        if ($needsNativeConfigure) {
            & $cmakeExecutable -S $nativeSource -B $nativeBuild -G 'Visual Studio 17 2022' -A x64
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
$runtimeDestination = Join-Path $destination 'npc-runtime-x86_64-pc-windows-msvc.exe'
$brokerDestination = Join-Path $destination 'npc-media-broker-x86_64-pc-windows-msvc.exe'
Copy-Item -LiteralPath $runtimeSource -Destination $runtimeDestination -Force
Copy-Item -LiteralPath $brokerSource -Destination $brokerDestination -Force

# The integration test launches the native binary directly, whose fixed-name
# validation intentionally rejects Tauri's target-triple filename. Publish the
# resolved short-cache source so Rust does not duplicate this path algorithm.
$env:NPC_MEDIA_BROKER_FIXTURE = $brokerSource

Write-Host "Prepared project sidecars in $destination" -ForegroundColor Green
Write-Host 'No third-party runtime or model payload was bundled.' -ForegroundColor Yellow
