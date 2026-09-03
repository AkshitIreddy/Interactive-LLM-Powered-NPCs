[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release',
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

if ($env:OS -ne 'Windows_NT') {
    throw 'Product sidecar preparation requires Windows x64.'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$destination = Join-Path $repoRoot 'apps/control/src-tauri/binaries'
$runtimeProfile = if ($Configuration -eq 'Debug') { 'debug' } else { 'release' }
$nativeConfiguration = 'Release'
$auditScript = Join-Path $PSScriptRoot 'audit-pe-product-binary.ps1'
if (-not (Test-Path -LiteralPath $auditScript -PathType Leaf)) {
    throw "Required PE product audit is missing: $auditScript"
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
    finally { $sha256.Dispose() }
    return (($digest | ForEach-Object { $_.ToString('x2') }) -join '').Substring(0, 16)
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

function Invoke-CheckedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentList,
        [Parameter(Mandatory = $true)][string]$FailureMessage,
        [switch]$WaitForDescendants
    )
    $process = Invoke-NpcHiddenProcess -FilePath $FilePath -ArgumentList $ArgumentList
    if ($process.ExitCode -ne 0) { throw "$FailureMessage (exit code $($process.ExitCode))." }
}

$largeArtifactRoot = $env:NPC_LARGE_ARTIFACT_ROOT
if ([string]::IsNullOrWhiteSpace($largeArtifactRoot)) {
    $localAppData = $env:LOCALAPPDATA
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        $localAppData = [System.Environment]::GetFolderPath(
            [System.Environment+SpecialFolder]::LocalApplicationData)
    }
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        throw 'LOCALAPPDATA could not be resolved for disposable native build caches.'
    }
    $nativeBuildBase = Join-Path $localAppData 'InteractiveNPCs/build'
}
else {
    if (-not [System.IO.Path]::IsPathRooted($largeArtifactRoot)) {
        throw 'NPC_LARGE_ARTIFACT_ROOT must be an absolute path.'
    }
    $nativeBuildBase = Join-Path $largeArtifactRoot 'native-builds/sidecars'
}
$nativeBuildBase = [System.IO.Path]::GetFullPath($nativeBuildBase).TrimEnd('\')
$buildKey = Get-RepositoryBuildKey -RepositoryRoot $repoRoot
$nativeComponents = @(
    [pscustomobject]@{
        Id = 'media-broker'; CacheSegment = 'mb'; Source = 'native/media-broker';
        Target = 'npc-media-broker'; Output = 'npc-media-broker.exe';
        Configure = @('-DBUILD_TESTING=OFF', '-DNPC_MEDIA_BROKER_BUILD_TESTS=OFF', '-DNPC_MEDIA_BROKER_WARNINGS_AS_ERRORS=ON')
    },
    [pscustomobject]@{
        Id = 'mouth-worker'; CacheSegment = 'mw'; Source = 'native/mouth-worker';
        Target = 'npc-mouth-worker'; Output = 'npc-mouth-worker.exe';
        Configure = @('-DBUILD_TESTING=OFF', '-DNPC_MOUTH_WORKER_BUILD_TESTS=OFF', '-DNPC_MOUTH_WORKER_BUILD_BENCHMARKS=OFF', '-DNPC_MOUTH_WORKER_BUILD_SYNTHETIC_PROOF=OFF', '-DNPC_MOUTH_WORKER_WARNINGS_AS_ERRORS=ON')
    },
    [pscustomobject]@{
        Id = 'subtitle-presenter'; CacheSegment = 'st'; Source = 'native/subtitle-renderer';
        Target = 'npc_subtitle_presenter'; Output = 'npc-subtitle-presenter.exe';
        Configure = @('-DBUILD_TESTING=OFF', '-DNPC_SUBTITLE_RENDERER_BUILD_TESTS=OFF', '-DNPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS=ON')
    }
)
foreach ($component in $nativeComponents) {
    $component | Add-Member -NotePropertyName SourcePath -NotePropertyValue (Join-Path $repoRoot $component.Source)
    $component | Add-Member -NotePropertyName BuildPath -NotePropertyValue `
        (Join-Path $nativeBuildBase "$($component.CacheSegment)/$buildKey")
}
$canonicalRepoRoot = [System.IO.Path]::GetFullPath($repoRoot).TrimEnd('\')
foreach ($component in $nativeComponents) {
    $canonicalBuild = [System.IO.Path]::GetFullPath($component.BuildPath).TrimEnd('\')
    if ($buildKey -notmatch '^[0-9a-f]{16}$' -or
        -not $canonicalBuild.StartsWith("$nativeBuildBase\", [System.StringComparison]::OrdinalIgnoreCase) -or
        $canonicalBuild.Equals($canonicalRepoRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
        $canonicalBuild.StartsWith("$canonicalRepoRoot\", [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing unsafe $($component.Id) product build path: $canonicalBuild"
    }
}

$cargoExecutable = (Get-Command cargo.exe -CommandType Application -ErrorAction Stop |
    Select-Object -First 1).Source
if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        $cargoArguments = @('build', '-p', 'npc-runtime-host', '--locked', '--no-default-features')
        if ($Configuration -eq 'Release') { $cargoArguments += '--release' }
        Invoke-CheckedProcess -FilePath $cargoExecutable -ArgumentList $cargoArguments `
            -FailureMessage 'Default-feature broker-audio runtime build failed' -WaitForDescendants

        $cmakeExecutable = (Get-Command cmake.exe -CommandType Application -ErrorAction Stop |
            Select-Object -First 1).Source
        $env:MSBUILDDISABLENODEREUSE = '1'
        foreach ($component in $nativeComponents) {
            $cachePath = Join-Path $component.BuildPath 'CMakeCache.txt'
            $needsConfigure = -not (Test-Path -LiteralPath $cachePath -PathType Leaf)
            if (-not $needsConfigure) {
                $cachedCmake = Get-CMakeCacheEntry -CachePath $cachePath -Name 'CMAKE_COMMAND'
                $cachedGenerator = Get-CMakeCacheEntry -CachePath $cachePath -Name 'CMAKE_GENERATOR'
                $cachedPlatform = Get-CMakeCacheEntry -CachePath $cachePath -Name 'CMAKE_GENERATOR_PLATFORM'
                $cachedSource = Get-CMakeCacheEntry -CachePath $cachePath -Name 'CMAKE_HOME_DIRECTORY'
                $compatible =
                    -not [string]::IsNullOrWhiteSpace($cachedCmake) -and
                    [string]::Equals((Normalize-ComparablePath $cachedCmake),
                        (Normalize-ComparablePath $cmakeExecutable),
                        [System.StringComparison]::OrdinalIgnoreCase) -and
                    $cachedGenerator -eq 'Visual Studio 17 2022' -and
                    $cachedPlatform -eq 'x64' -and
                    -not [string]::IsNullOrWhiteSpace($cachedSource) -and
                    [string]::Equals((Normalize-ComparablePath $cachedSource),
                        (Normalize-ComparablePath $component.SourcePath),
                        [System.StringComparison]::OrdinalIgnoreCase)
                foreach ($definition in $component.Configure) {
                    if ($definition -notmatch '^-D([^=]+)=(.*)$') {
                        throw "Invalid pinned CMake definition for $($component.Id): $definition"
                    }
                    $cachedValue = Get-CMakeCacheEntry -CachePath $cachePath -Name $Matches[1]
                    if ($cachedValue -ne $Matches[2]) { $compatible = $false }
                }
                if (-not $compatible) {
                    Write-Host "Refreshing incompatible $($component.Id) product build cache." -ForegroundColor Yellow
                    Remove-Item -LiteralPath $component.BuildPath -Recurse -Force
                    $needsConfigure = $true
                }
            }
            if ($needsConfigure) {
                $configureArguments = @(
                    '-S', $component.SourcePath, '-B', $component.BuildPath,
                    '-G', 'Visual Studio 17 2022', '-A', 'x64'
                ) + @($component.Configure)
                Invoke-CheckedProcess -FilePath $cmakeExecutable -ArgumentList $configureArguments `
                    -FailureMessage "$($component.Id) Release configuration failed" -WaitForDescendants
            }
            $buildArguments = @(
                '--build', $component.BuildPath, '--config', $nativeConfiguration,
                '--target', $component.Target, '--parallel', '4'
            )
            Invoke-CheckedProcess -FilePath $cmakeExecutable -ArgumentList $buildArguments `
                -FailureMessage "$($component.Id) Release product build failed" -WaitForDescendants
        }
    }
    finally { Pop-Location }
}

$metadataProcess = Invoke-NpcHiddenProcess -FilePath $cargoExecutable -ArgumentList @(
    'metadata', '--format-version', '1', '--no-deps', '--locked', '--offline'
) -WorkingDirectory $repoRoot -NoReplayOutput
if ($metadataProcess.ExitCode -ne 0) {
    if (-not $SkipBuild) {
        throw "Cargo metadata failed while resolving the configured target directory (exit code $($metadataProcess.ExitCode))."
    }
    # SkipBuild is also used by the isolated path-policy fixture, which has
    # audited PE stand-ins but deliberately no Cargo workspace metadata.
    $cargoTargetDirectory = Join-Path $repoRoot 'target'
}
else {
    $cargoMetadata = $metadataProcess.StandardOutput | ConvertFrom-Json
    $cargoTargetDirectory = [System.IO.Path]::GetFullPath([string]$cargoMetadata.target_directory)
}

$sidecars = @(
    [pscustomobject]@{
        Id = 'runtime'; Source = (Join-Path $cargoTargetDirectory "$runtimeProfile/npc-runtime.exe");
        Destination = 'npc-runtime-x86_64-pc-windows-msvc.exe'; BuildConfiguration = $Configuration;
        FeaturePolicy = 'default-features-only;broker-audio'
    }
)
foreach ($component in $nativeComponents) {
    $sidecars += [pscustomobject]@{
        Id = $component.Id
        Source = (Join-Path $component.BuildPath "$nativeConfiguration/$($component.Output)")
        Destination = ([System.IO.Path]::GetFileNameWithoutExtension($component.Output) + '-x86_64-pc-windows-msvc.exe')
        BuildConfiguration = $nativeConfiguration
        FeaturePolicy = 'project-owned-release-sidecar'
    }
}

New-Item -ItemType Directory -Path $destination -Force | Out-Null
$binaryReports = foreach ($sidecar in $sidecars | Sort-Object Destination) {
    if (-not (Test-Path -LiteralPath $sidecar.Source -PathType Leaf)) {
        throw "Required product sidecar was not built: $($sidecar.Source)"
    }
    $sourceAudit = (& $auditScript -Path $sidecar.Source -ExpectedSubsystem Gui -AsJson) |
        ConvertFrom-Json
    $destinationPath = Join-Path $destination $sidecar.Destination
    Copy-Item -LiteralPath $sidecar.Source -Destination $destinationPath -Force
    $stagedAudit = (& $auditScript -Path $destinationPath -ExpectedSubsystem Gui -AsJson) |
        ConvertFrom-Json
    if ($sourceAudit.sha256 -ne $stagedAudit.sha256) {
        throw "Sidecar changed while staging: $($sidecar.Destination)"
    }
    [ordered]@{
        id = $sidecar.Id
        file_name = $sidecar.Destination
        build_configuration = $sidecar.BuildConfiguration
        feature_policy = $sidecar.FeaturePolicy
        size_bytes = [int64]$stagedAudit.size_bytes
        sha256 = [string]$stagedAudit.sha256
        machine = [string]$stagedAudit.machine
        pe_subsystem = [string]$stagedAudit.pe_subsystem
        imports = @($stagedAudit.imports)
        delayed_imports = @($stagedAudit.delayed_imports)
        debug_crt_imports = @($stagedAudit.debug_crt_imports)
    }
}

$manifest = [ordered]@{
    schema = 'interactive-npcs-sidecars/v1'
    target_triple = 'x86_64-pc-windows-msvc'
    application_configuration = $Configuration
    native_configuration = $nativeConfiguration
    product_audio_route = 'media-broker'
    runtime_features = @()
    binaries = @($binaryReports)
}
$manifestPath = Join-Path $destination 'sidecar-manifest.v1.json'
$manifestJson = ($manifest | ConvertTo-Json -Depth 7) + [Environment]::NewLine
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, (New-Object System.Text.UTF8Encoding($false)))

$broker = $sidecars | Where-Object { $_.Id -eq 'media-broker' } | Select-Object -First 1
$env:NPC_MEDIA_BROKER_FIXTURE = $broker.Source

Write-Host "Prepared and audited four project sidecars in $destination" -ForegroundColor Green
Write-Host 'C++ children are Release-only; no third-party runtime or model payload was bundled.' -ForegroundColor Yellow
[ordered]@{
    schema = 'interactive-npcs-sidecar-stage/v1'
    status = 'prepared'
    manifest_path = $manifestPath
    manifest_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $manifestPath).Hash.ToLowerInvariant()
    binaries = @($binaryReports)
} | ConvertTo-Json -Compress -Depth 7 | Write-Output
