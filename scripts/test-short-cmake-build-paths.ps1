[CmdletBinding()]
param(
    [switch]$BuildNative
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: short CMake build-path regression requires Windows path semantics.'
    exit 0
}

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) { throw $Message }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$resolverScript = Join-Path $PSScriptRoot 'short-cmake-build-path.ps1'
if (-not (Test-Path -LiteralPath $resolverScript -PathType Leaf)) {
    throw "Short CMake build-path resolver was not found: $resolverScript"
}
. $resolverScript

$caseRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-cmake-path-$([Guid]::NewGuid().ToString('N'))"
$localAppDataRoot = Join-Path $caseRoot 'short-local-app-data'
$deepSegments = @(
    'clean-source-archive-with-a-long-parent-name',
    'nested-automated-validation-workspace'
)
$savedLocalAppData = $env:LOCALAPPDATA

try {
    New-Item -ItemType Directory -Path $caseRoot, $localAppDataRoot -Force | Out-Null
    $env:LOCALAPPDATA = $localAppDataRoot

    $deepParent = Join-Path $caseRoot 'checkout-a'
    foreach ($segment in $deepSegments) { $deepParent = Join-Path $deepParent $segment }
    $firstRepo = Join-Path $deepParent 'sanitized-tree'
    $secondRepo = Join-Path $caseRoot 'checkout-b/sanitized-tree'
    New-Item -ItemType Directory -Path $firstRepo, $secondRepo -Force | Out-Null

    $mediaBuild = Get-NpcShortCMakeBuildPath -RepositoryRoot $firstRepo -Component 'mb-tests'
    $gameLoadBuild = Get-NpcShortCMakeBuildPath -RepositoryRoot $firstRepo -Component 'gl'
    $secondMediaBuild = Get-NpcShortCMakeBuildPath -RepositoryRoot $secondRepo -Component 'mb-tests'

    Assert-True -Condition ($firstRepo.Length -gt 160) -Message "Deep checkout fixture is not long enough: $firstRepo"
    Assert-True -Condition ((Join-Path $firstRepo 'tools/game-load/CMakeLists.txt').Length -lt 248) -Message 'Fixture source path exceeds CMake input limits instead of isolating FileTracker intermediate expansion.'
    Assert-True -Condition ($mediaBuild.Length -lt 180) -Message "Media-broker build path leaves too little FileTracker headroom: $mediaBuild"
    Assert-True -Condition ($gameLoadBuild.Length -lt 180) -Message "Game-load build path leaves too little FileTracker headroom: $gameLoadBuild"
    Assert-True -Condition (-not $mediaBuild.StartsWith($firstRepo, [System.StringComparison]::OrdinalIgnoreCase)) -Message 'Media-broker build path remained inside the deep checkout.'
    Assert-True -Condition (-not $gameLoadBuild.StartsWith($firstRepo, [System.StringComparison]::OrdinalIgnoreCase)) -Message 'Game-load build path remained inside the deep checkout.'
    Assert-True -Condition ($mediaBuild -ne $secondMediaBuild) -Message 'Different checkout roots resolved to the same media-broker build path.'
    Assert-True -Condition ($mediaBuild -ne $gameLoadBuild) -Message 'Different native components resolved to the same build path.'

    $devScript = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'dev.ps1') -Raw
    Assert-True -Condition ($devScript -notmatch "out/build/native-media-broker-windows") -Message 'dev.ps1 still contains the vulnerable repository-local media-broker test path.'
    Assert-True -Condition ($devScript -match "Get-NpcShortCMakeBuildPath.+mb-tests") -Message 'dev.ps1 does not resolve media-broker tests through the shared short-path policy.'

    $gameLoadSmoke = Get-Content -LiteralPath (Join-Path $repoRoot 'tools/game-load/scripts/smoke.ps1') -Raw
    Assert-True -Condition ($gameLoadSmoke -notmatch "out/build/windows-msvc") -Message 'Game-load smoke still contains the vulnerable repository-local build path.'
    Assert-True -Condition ($gameLoadSmoke -match "Get-NpcShortCMakeBuildPath.+gl") -Message 'Game-load smoke does not resolve builds through the shared short-path policy.'

    $presets = Get-Content -LiteralPath (Join-Path $repoRoot 'tools/game-load/CMakePresets.json') -Raw | ConvertFrom-Json
    $windowsPreset = @($presets.configurePresets | Where-Object { $_.name -eq 'windows-msvc' })
    Assert-True -Condition ($windowsPreset.Count -eq 0) -Message 'The source-local Windows preset remains available and can recreate the FileTracker failure; use the keyed smoke wrapper.'

    if ($BuildNative) {
        $fixtureScripts = Join-Path $firstRepo 'scripts'
        $fixtureNative = Join-Path $firstRepo 'native'
        $fixtureTools = Join-Path $firstRepo 'tools'
        New-Item -ItemType Directory -Path $fixtureScripts, $fixtureNative, $fixtureTools -Force | Out-Null
        Copy-Item -LiteralPath $resolverScript -Destination $fixtureScripts
        Copy-Item -LiteralPath (Join-Path $repoRoot 'native/media-broker') -Destination $fixtureNative -Recurse
        $fixtureGameLoad = Join-Path $fixtureTools 'game-load'
        New-Item -ItemType Directory -Path $fixtureGameLoad -Force | Out-Null
        foreach ($entry in @('include', 'scripts', 'shaders', 'src', 'tests', 'CMakeLists.txt', 'CMakePresets.json')) {
            Copy-Item -LiteralPath (Join-Path $repoRoot "tools/game-load/$entry") -Destination $fixtureGameLoad -Recurse
        }

        $cmake = (Get-Command cmake.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
        $ctest = Join-Path (Split-Path -Parent $cmake) 'ctest.exe'
        $fixtureMediaSource = Join-Path $firstRepo 'native/media-broker'
        $global:LASTEXITCODE = 0
        & $cmake -S $fixtureMediaSource -B $mediaBuild -G 'Visual Studio 17 2022' -A x64 -DBUILD_TESTING=ON -DNPC_MEDIA_BROKER_BUILD_TESTS=ON -DNPC_MEDIA_BROKER_REGISTER_INTERACTIVE_WINDOWS_TESTS=OFF
        if ($LASTEXITCODE -ne 0) { throw 'Deep-checkout media-broker configuration failed.' }
        & $cmake --build $mediaBuild --config Debug --parallel 4
        if ($LASTEXITCODE -ne 0) { throw 'Deep-checkout media-broker build failed.' }
        & $ctest --test-dir $mediaBuild -C Debug --output-on-failure
        if ($LASTEXITCODE -ne 0) { throw 'Deep-checkout media-broker tests failed.' }

        $global:LASTEXITCODE = 0
        & (Join-Path $firstRepo 'tools/game-load/scripts/smoke.ps1')
        if ($LASTEXITCODE -ne 0) { throw 'Deep-checkout game-load smoke failed.' }
    }

    Write-Host 'Short CMake build-path regression checks passed.' -ForegroundColor Green
}
finally {
    [System.Environment]::SetEnvironmentVariable('LOCALAPPDATA', $savedLocalAppData, 'Process')
    for ($attempt = 0; $attempt -lt 5 -and (Test-Path -LiteralPath $caseRoot); $attempt++) {
        if ($attempt -gt 0) { Start-Sleep -Milliseconds 250 }
        Remove-Item -LiteralPath $caseRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $caseRoot) {
        Write-Warning "Temporary deep-checkout fixture could not be removed immediately: $caseRoot"
    }
}

exit 0
