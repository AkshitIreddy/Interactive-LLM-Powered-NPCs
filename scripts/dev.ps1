[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('setup', 'dev', 'test', 'lint', 'benchmark', 'package', 'environment')]
    [string]$Command = 'environment',

    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug',

    [switch]$Offline,
    [switch]$Quick,
    [switch]$SkipChecks,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$script:Failures = New-Object System.Collections.Generic.List[string]
$script:Warnings = New-Object System.Collections.Generic.List[string]

# Windows shells launched by IDEs, WSL, or desktop automation do not always
# inherit the standard developer-tool directories. Add only known existing
# locations; never guess a binary or download a tool implicitly.
$standardToolDirectories = @()
if ($env:OS -eq 'Windows_NT') {
    $requiredExecutableExtensions = @('.COM', '.EXE', '.BAT', '.CMD')
    $currentExecutableExtensions = @($env:PATHEXT -split ';')
    foreach ($extension in $requiredExecutableExtensions) {
        if ($currentExecutableExtensions -notcontains $extension) {
            $currentExecutableExtensions += $extension
        }
    }
    $env:PATHEXT = $currentExecutableExtensions -join ';'

    $standardToolDirectories += @(
        (Join-Path $env:ProgramFiles 'nodejs'),
        (Join-Path $env:USERPROFILE '.cargo\bin'),
        (Join-Path $env:ProgramFiles 'Git\cmd'),
        (Join-Path $env:ProgramFiles 'CMake\bin'),
        (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'),
        (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin')
    )
    if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $standardToolDirectories += @(
            (Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312'),
            (Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312\Scripts')
        )
    }
}
foreach ($directory in $standardToolDirectories) {
    if ((Test-Path -LiteralPath $directory -PathType Container) -and
        -not (($env:PATH -split ';') -contains $directory)) {
        $env:PATH = "$directory;$env:PATH"
    }
}

function Write-Step {
    param([string]$Message)
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Write-Skip {
    param([string]$Message)
    Write-Host "    SKIP  $Message" -ForegroundColor DarkGray
}

function Write-WarningMessage {
    param([string]$Message)
    $script:Warnings.Add($Message)
    Write-Host "    WARN  $Message" -ForegroundColor Yellow
}

function Add-Failure {
    param([string]$Message)
    $script:Failures.Add($Message)
    Write-Host "    FAIL  $Message" -ForegroundColor Red
}

function Test-CommandAvailable {
    param([string]$Name)
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Test-IsWindows {
    return $env:OS -eq 'Windows_NT'
}

function Invoke-External {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [string]$WorkingDirectory = $script:RepoRoot,
        [string]$DisplayArguments,
        [switch]$AllowFailure
    )

    $argumentSummary = if ([string]::IsNullOrWhiteSpace($DisplayArguments)) {
        $ArgumentList -join ' '
    } else {
        $DisplayArguments
    }
    Write-Host "    $FilePath $argumentSummary" -ForegroundColor DarkGray
    Push-Location $WorkingDirectory
    try {
        & $FilePath @ArgumentList
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
        if ($exitCode -ne 0 -and -not $AllowFailure) {
            throw "Command failed with exit code ${exitCode}: $FilePath $argumentSummary"
        }
        return $exitCode
    }
    finally {
        Pop-Location
    }
}

function Invoke-CheckBlock {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    try {
        & $Action
    }
    catch {
        Add-Failure "${Label}: $($_.Exception.Message)"
    }
}

function Get-NodePackageRoot {
    $candidates = @(
        (Join-Path $script:RepoRoot 'package.json'),
        (Join-Path $script:RepoRoot 'apps/control/package.json'),
        (Join-Path $script:RepoRoot 'apps/desktop/package.json'),
        (Join-Path $script:RepoRoot 'ui/package.json')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Split-Path -Parent $candidate)
        }
    }
    return $null
}

function Get-NodeInstallRoot {
    $packageRoot = Get-NodePackageRoot
    if ($null -eq $packageRoot) { return $null }
    if ((Test-Path (Join-Path $script:RepoRoot 'package.json')) -or (Test-Path (Join-Path $script:RepoRoot 'pnpm-workspace.yaml'))) {
        return $script:RepoRoot
    }
    return $packageRoot
}

function Get-NodeVersionConstraint {
    $packageRoot = Get-NodePackageRoot
    if ($null -eq $packageRoot) { return '20+' }
    $package = Get-Content -LiteralPath (Join-Path $packageRoot 'package.json') -Raw | ConvertFrom-Json
    if ($null -ne $package.engines -and -not [string]::IsNullOrWhiteSpace($package.engines.node)) {
        return [string]$package.engines.node
    }
    return '20+'
}

function Get-PackageScriptNames {
    param([string]$PackageRoot)
    if ([string]::IsNullOrWhiteSpace($PackageRoot)) { return @() }
    $packagePath = Join-Path $PackageRoot 'package.json'
    if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) { return @() }
    $package = Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json
    if ($null -eq $package.scripts) { return @() }
    return @($package.scripts.PSObject.Properties.Name)
}

function Get-CargoManifests {
    $workspaceManifest = Join-Path $script:RepoRoot 'Cargo.toml'
    if (Test-Path -LiteralPath $workspaceManifest -PathType Leaf) { return @($workspaceManifest) }
    $cratesRoot = Join-Path $script:RepoRoot 'crates'
    if (-not (Test-Path -LiteralPath $cratesRoot -PathType Container)) { return @() }
    return @(Get-ChildItem -LiteralPath $cratesRoot -Filter 'Cargo.toml' -File -Recurse | ForEach-Object { $_.FullName })
}

function Get-PnpmInvocation {
    if (Test-CommandAvailable 'pnpm') {
        return @{ FilePath = 'pnpm'; Prefix = @() }
    }
    if (Test-CommandAvailable 'corepack') {
        return @{ FilePath = 'corepack'; Prefix = @('pnpm') }
    }
    return $null
}

function Get-NpmCommand {
    if ((Test-IsWindows) -and (Test-CommandAvailable 'npm.cmd')) { return 'npm.cmd' }
    if (Test-CommandAvailable 'npm') { return 'npm' }
    return $null
}

function Get-PythonInvocation {
    if (Test-CommandAvailable 'python') {
        return @{ FilePath = 'python'; Prefix = @() }
    }
    if (Test-CommandAvailable 'python3') {
        return @{ FilePath = 'python3'; Prefix = @() }
    }
    if ((Test-IsWindows) -and (Test-CommandAvailable 'py.exe')) {
        return @{ FilePath = 'py.exe'; Prefix = @('-3.12') }
    }
    return $null
}

function Invoke-NativeProbe {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [string]$WorkingDirectory = $script:RepoRoot
    )

    $previousErrorActionPreference = $ErrorActionPreference
    $output = @()
    $exitCode = 1
    $locationPushed = $false
    try {
        # rustup selects a toolchain from the current directory. Probe from the
        # same repository root used by the real command so a caller outside the
        # checkout cannot make the probe see stable while lint sees the pin.
        Push-Location -LiteralPath $WorkingDirectory
        $locationPushed = $true

        # Windows PowerShell can promote a native process's stderr to a
        # terminating NativeCommandError when the caller uses Stop globally.
        # Probes need the real exit code and bounded output instead.
        $ErrorActionPreference = 'Continue'
        $output = @(& $FilePath @ArgumentList 2>&1)
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    catch {
        $output += $_.Exception.Message
        $exitCode = 1
    }
    finally {
        if ($locationPushed) { Pop-Location }
        $ErrorActionPreference = $previousErrorActionPreference
    }
    return @{ ExitCode = $exitCode; Output = $output }
}

function Get-RustcIdentity {
    param([string[]]$Prefix = @())

    $probe = Invoke-NativeProbe -FilePath 'rustc' -ArgumentList @($Prefix + @('-vV'))
    if ($probe.ExitCode -ne 0) { return $null }
    $release = $null
    $commitHash = $null
    foreach ($line in $probe.Output) {
        $text = [string]$line
        if ($text -match '^release:\s*(?<value>.+)$') { $release = $Matches.value.Trim() }
        if ($text -match '^commit-hash:\s*(?<value>.+)$') { $commitHash = $Matches.value.Trim() }
    }
    if ([string]::IsNullOrWhiteSpace($release) -or [string]::IsNullOrWhiteSpace($commitHash)) {
        return $null
    }
    return @{ Release = $release; CommitHash = $commitHash }
}

function Get-ClippyInvocation {
    $plainProbe = Invoke-NativeProbe -FilePath 'cargo' -ArgumentList @('clippy', '--version')
    if ($plainProbe.ExitCode -eq 0) {
        return @{ FilePath = 'cargo'; Prefix = @(); Note = $null }
    }

    $plainMessage = (($plainProbe.Output | ForEach-Object { [string]$_ }) -join ' ').Trim()
    if ($plainMessage.Length -gt 300) { $plainMessage = $plainMessage.Substring(0, 300) + '...' }
    if (-not (Test-IsWindows)) {
        throw "The pinned Clippy component is unavailable. Reinstall the clippy component for rust-toolchain.toml. cargo clippy reported: $plainMessage"
    }

    # Some Windows rustup installations can expose the pinned clippy component
    # only through the stable alias. The alias is safe solely when it resolves
    # to the exact same compiler release and commit as the active pinned toolchain.
    $active = Get-RustcIdentity
    $stable = Get-RustcIdentity -Prefix @('+stable')
    if ($null -ne $active -and $null -ne $stable -and
        $active.Release -eq $stable.Release -and
        $active.CommitHash -eq $stable.CommitHash) {
        return @{
            FilePath = 'cargo'
            Prefix = @('+stable')
            Note = "plain pinned cargo clippy was unavailable; verified +stable is identical ($($active.Release), $($active.CommitHash))"
        }
    }

    throw "The pinned Clippy component is unavailable and +stable was not proven identical to the active rustc release and commit. Reinstall the clippy component for rust-toolchain.toml. cargo clippy reported: $plainMessage"
}

function Prepare-TauriSidecarsForValidation {
    $prepareSidecars = Join-Path $PSScriptRoot 'prepare-sidecars.ps1'
    if (-not (Test-Path -LiteralPath $prepareSidecars -PathType Leaf)) {
        throw "Sidecar preparation script was not found: $prepareSidecars"
    }

    Write-Step 'Preparing project sidecars for nested Tauri validation'
    Push-Location -LiteralPath $script:RepoRoot
    try {
        $global:LASTEXITCODE = 0
        & $prepareSidecars -Configuration $Configuration
        $prepareExitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    if ($prepareExitCode -ne 0) {
        throw "Sidecar preparation exited with code $prepareExitCode."
    }

    $sidecarRoot = Join-Path $script:RepoRoot 'apps/control/src-tauri/binaries'
    foreach ($sidecarName in @(
            'npc-runtime-x86_64-pc-windows-msvc.exe',
            'npc-media-broker-x86_64-pc-windows-msvc.exe'
        )) {
        $sidecarPath = Join-Path $sidecarRoot $sidecarName
        if (-not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
            throw "Sidecar preparation completed without required output: $sidecarPath"
        }
    }
}

function Invoke-Pnpm {
    param(
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )
    $pnpm = Get-PnpmInvocation
    if ($null -eq $pnpm) {
        throw 'pnpm was not found. Install Node.js 20+ with Corepack, then run `corepack enable pnpm` or install the pinned pnpm version.'
    }
    Invoke-External -FilePath $pnpm.FilePath -ArgumentList @($pnpm.Prefix + $Arguments) -WorkingDirectory $WorkingDirectory
}

function Get-EnvironmentRows {
    $nodeRoot = Get-NodePackageRoot
    $cargoManifests = @(Get-CargoManifests)
    $mediaCmakeManifest = Join-Path $script:RepoRoot 'native/media-broker/CMakeLists.txt'
    $gameLoadCmakeManifest = Join-Path $script:RepoRoot 'tools/game-load/CMakeLists.txt'
    $pythonManifest = Join-Path $script:RepoRoot 'pyproject.toml'
    $workerTests = Join-Path $script:RepoRoot 'workers/tests'
    $demoPackage = Join-Path $script:RepoRoot 'demo/readme/package.json'
    $nodeConstraint = Get-NodeVersionConstraint

    $rows = @()
    $rows += [pscustomobject]@{ Tool = 'Git'; Required = $false; Found = (Test-CommandAvailable 'git'); Purpose = 'source and provenance checks' }
    $rows += [pscustomobject]@{ Tool = "Node.js $nodeConstraint"; Required = ($null -ne $nodeRoot); Found = (Test-CommandAvailable 'node'); Purpose = 'desktop control application' }
    $rows += [pscustomobject]@{ Tool = 'pnpm/Corepack'; Required = ($null -ne $nodeRoot); Found = ($null -ne (Get-PnpmInvocation)); Purpose = 'locked JavaScript workspace' }
    $rows += [pscustomobject]@{ Tool = 'npm'; Required = (Test-Path -LiteralPath $demoPackage); Found = ($null -ne (Get-NpmCommand)); Purpose = 'isolated deterministic README demo checks' }
    $rows += [pscustomobject]@{ Tool = 'Rust/Cargo'; Required = ($cargoManifests.Count -gt 0); Found = (Test-CommandAvailable 'cargo'); Purpose = 'runtime and Tauri shell' }
    $rows += [pscustomobject]@{ Tool = 'CMake/CTest'; Required = ((Test-IsWindows) -and ((Test-Path -LiteralPath $mediaCmakeManifest) -or (Test-Path -LiteralPath $gameLoadCmakeManifest))); Found = ((Test-CommandAvailable 'cmake') -and (Test-CommandAvailable 'ctest')); Purpose = 'Windows media broker and safe game-load harness tests' }
    $rows += [pscustomobject]@{ Tool = 'Python'; Required = (Test-Path -LiteralPath $workerTests); Found = ($null -ne (Get-PythonInvocation)); Purpose = 'deterministic worker protocol tests' }
    $rows += [pscustomobject]@{ Tool = 'uv'; Required = (Test-Path -LiteralPath $pythonManifest); Found = (Test-CommandAvailable 'uv'); Purpose = 'optional isolated inference packs' }
    $rows += [pscustomobject]@{ Tool = 'PowerShell'; Required = $true; Found = $true; Purpose = 'developer command surface' }

    if (Test-IsWindows) {
        $webViewFound = $false
        $webViewPaths = @(
            'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
            'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
        )
        foreach ($path in $webViewPaths) {
            if (Test-Path $path) { $webViewFound = $true; break }
        }
        $rows += [pscustomobject]@{ Tool = 'WebView2 Evergreen'; Required = ($Command -in @('dev', 'package')); Found = $webViewFound; Purpose = 'Tauri desktop rendering' }

        $vsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
        $nativeBuildRequired = $Command -in @('dev', 'test', 'package') -and
            (($cargoManifests.Count -gt 0) -or (Test-Path -LiteralPath $mediaCmakeManifest) -or (Test-Path -LiteralPath $gameLoadCmakeManifest))
        $rows += [pscustomobject]@{ Tool = 'MSVC Build Tools'; Required = $nativeBuildRequired; Found = (Test-Path $vsWhere); Purpose = 'Windows x64 native builds' }
    }

    return $rows
}

function Assert-Environment {
    Write-Step 'Validating developer environment'
    $rows = Get-EnvironmentRows
    $rows | Format-Table -AutoSize
    foreach ($row in $rows) {
        if ($row.Required -and -not $row.Found) {
            Add-Failure "$($row.Tool) is required for $($row.Purpose)."
        }
    }

    if (Test-CommandAvailable 'node') {
        $rawVersion = (& node --version 2>$null)
        if ($rawVersion -match '^v(?<major>\d+)') {
            $actualMajor = [int]$Matches.major
            $constraint = Get-NodeVersionConstraint
            if ($constraint -match '^\d+\.\d+\.\d+$' -and $rawVersion.TrimStart('v') -ne $constraint) {
                Add-Failure "Node.js $constraint is pinned by package.json; found $rawVersion."
            } elseif ($constraint -match '(?<minimumMajor>\d+)' -and [int]$Matches.minimumMajor -gt $actualMajor) {
                Add-Failure "Node.js $constraint is required; found $rawVersion."
            }
        }
    }

    if (-not (Test-IsWindows) -and $Command -eq 'package') {
        Add-Failure 'Windows installers must be packaged on Windows 10 22H2 or Windows 11 x64.'
    }
}

function Invoke-Setup {
    Assert-Environment
    if ($script:Failures.Count -gt 0) { return }

    $nodeRoot = Get-NodePackageRoot
    if ($null -ne $nodeRoot) {
        Write-Step 'Restoring the JavaScript workspace'
        $nodeInstallRoot = Get-NodeInstallRoot
        $lockPath = Join-Path $nodeInstallRoot 'pnpm-lock.yaml'
        if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
            Add-Failure "Missing pnpm lockfile: $lockPath. Refusing to create an unpinned dependency graph."
        } else {
            $pnpmArgs = @('install', '--frozen-lockfile')
            if ($Offline) { $pnpmArgs += '--offline' }
            Invoke-Pnpm -Arguments $pnpmArgs -WorkingDirectory $nodeInstallRoot
        }
    } else { Write-Skip 'No package.json was found.' }

    $cargoManifests = @(Get-CargoManifests)
    if ($cargoManifests.Count -gt 0) {
        Write-Step 'Fetching the Rust workspace'
        foreach ($cargoManifest in $cargoManifests) {
            $cargoArgs = @('fetch', '--manifest-path', $cargoManifest, '--locked')
            if ($Offline) { $cargoArgs += '--offline' }
            Invoke-External -FilePath 'cargo' -ArgumentList $cargoArgs
        }
    } else { Write-Skip 'No Cargo.toml was found.' }

    if (Test-Path (Join-Path $script:RepoRoot 'pyproject.toml')) {
        Write-Step 'Restoring the optional Python worker workspace'
        $uvArgs = @('sync', '--frozen')
        if ($Offline) { $uvArgs += '--offline' }
        Invoke-External -FilePath 'uv' -ArgumentList $uvArgs
    } else { Write-Skip 'No pyproject.toml was found; no Python runtime is installed.' }

    $cmakePresets = Join-Path $script:RepoRoot 'native/media-broker/CMakePresets.json'
    if (Test-Path $cmakePresets) {
        Write-Step 'Configuring the native media broker'
        $preset = $(if (Test-IsWindows) { 'windows-x64-dev' } else { 'host-dev' })
        Invoke-External -FilePath 'cmake' -ArgumentList @('--preset', $preset)
    } elseif (Test-Path (Join-Path $script:RepoRoot 'native/media-broker/CMakeLists.txt')) {
        Write-Step 'Configuring the native media broker in an untracked build directory'
        Invoke-External -FilePath 'cmake' -ArgumentList @(
            '-S', 'native/media-broker',
            '-B', 'out/build/native-media-broker',
            '-DBUILD_TESTING=ON',
            '-DNPC_MEDIA_BROKER_BUILD_TESTS=ON'
        )
    }
}

function Invoke-Dev {
    Assert-Environment
    if ($script:Failures.Count -gt 0) { return }
    $nodeRoot = Get-NodePackageRoot
    $scripts = Get-PackageScriptNames -PackageRoot $nodeRoot
    if ($scripts -contains 'tauri') {
        Write-Step 'Starting the Tauri development application'
        Invoke-Pnpm -Arguments @('run', 'tauri', 'dev') -WorkingDirectory $nodeRoot
    } elseif ($scripts -contains 'dev') {
        Write-Step 'Starting the desktop development application'
        Invoke-Pnpm -Arguments @('run', 'dev') -WorkingDirectory $nodeRoot
    } elseif ((Get-CargoManifests).Count -gt 0) {
        Write-Step 'Starting the Rust workspace'
        $cargoManifest = @(Get-CargoManifests)[0]
        Invoke-External -FilePath 'cargo' -ArgumentList @('run', '--manifest-path', $cargoManifest)
    } else {
        Add-Failure 'No runnable 2.0 workspace was found. Expected a package.json dev/tauri script or Cargo.toml.'
    }
}

function Invoke-Tests {
    Assert-Environment
    if ($script:Failures.Count -gt 0) { return }

    $clippyDispatchTest = Join-Path $PSScriptRoot 'test-dev-clippy-dispatch.ps1'
    if (Test-IsWindows) {
        Invoke-CheckBlock -Label 'Clippy dispatch regression' -Action {
            if (-not (Test-Path -LiteralPath $clippyDispatchTest -PathType Leaf)) {
                throw "Clippy dispatch regression script was not found: $clippyDispatchTest"
            }
            Write-Step 'Testing pinned Clippy fallback selection and identity refusal'
            & $clippyDispatchTest
            if ($LASTEXITCODE -ne 0) { throw "Clippy dispatch regression exited with code $LASTEXITCODE." }
        }
    } else {
        Write-Skip 'Clippy dispatch regression requires Windows rustup command semantics.'
    }

    $nodeRoot = Get-NodePackageRoot
    $scripts = Get-PackageScriptNames -PackageRoot $nodeRoot
    if ($scripts -contains 'test:frontend') {
        Invoke-CheckBlock -Label 'Frontend tests' -Action {
            Write-Step 'Running frontend tests once'
            Invoke-Pnpm -Arguments @('run', 'test:frontend') -WorkingDirectory $nodeRoot
        }
    } else {
        Add-Failure 'Frontend tests: package.json must define the canonical test:frontend script.'
    }

    if (($scripts -contains 'test:sim') -and ($scripts -contains 'sim:verify')) {
        Invoke-CheckBlock -Label 'Deterministic simulation' -Action {
            Write-Step 'Running deterministic simulation tests'
            Invoke-Pnpm -Arguments @('run', 'test:sim') -WorkingDirectory $nodeRoot
            Write-Step 'Verifying deterministic simulation fixtures'
            Invoke-Pnpm -Arguments @('run', 'sim:verify') -WorkingDirectory $nodeRoot
        }
    } else {
        Add-Failure 'Deterministic simulation: package.json must define test:sim and sim:verify.'
    }

    $rootCargoManifest = Join-Path $script:RepoRoot 'Cargo.toml'
    if (Test-Path -LiteralPath $rootCargoManifest -PathType Leaf) {
        Invoke-CheckBlock -Label 'Root Rust workspace tests' -Action {
            Write-Step 'Running Rust workspace tests'
            Invoke-External -FilePath 'cargo' -ArgumentList @(
                'test', '--workspace', '--all-targets', '--all-features', '--locked', '--offline'
            ) -WorkingDirectory $script:RepoRoot
        }
    } else {
        Add-Failure 'Root Rust workspace tests: Cargo.toml was not found.'
    }

    $tauriManifest = Join-Path $script:RepoRoot 'apps/control/src-tauri/Cargo.toml'
    if (Test-Path -LiteralPath $tauriManifest -PathType Leaf) {
        if (Test-IsWindows) {
            Invoke-CheckBlock -Label 'Tauri Rust tests' -Action {
                Write-Step 'Running nested Tauri Rust tests'
                Invoke-External -FilePath 'cargo' -ArgumentList @(
                    'test', '--manifest-path', $tauriManifest, '--all-targets', '--locked', '--offline'
                ) -WorkingDirectory $script:RepoRoot
            }
        } else {
            Write-Skip 'Nested Tauri Rust tests require the supported Windows target; this host is not Windows.'
        }
    } else {
        Add-Failure 'Tauri Rust tests: apps/control/src-tauri/Cargo.toml was not found.'
    }

    $profileFiles = @(Get-ChildItem -LiteralPath (Join-Path $script:RepoRoot 'profiles/games') -Filter 'profile.json' -File -Recurse | Sort-Object FullName)
    if ($profileFiles.Count -ne 20) {
        Add-Failure "Game profile validation: expected exactly 20 profile.json files; found $($profileFiles.Count)."
    } else {
        Invoke-CheckBlock -Label 'Game profile validation' -Action {
            Write-Step 'Validating the exact 20-profile corpus through the profile CLI'
            $profileArguments = @('run', '--locked', '--offline', '-p', 'npc-game-profile', '--bin', 'validate-profile', '--')
            $profileArguments += @($profileFiles | ForEach-Object { $_.FullName })
            Invoke-External -FilePath 'cargo' -ArgumentList $profileArguments -WorkingDirectory $script:RepoRoot -DisplayArguments 'run --locked --offline -p npc-game-profile --bin validate-profile -- <20 sorted profile.json paths>'
        }
    }

    $workerTests = Join-Path $script:RepoRoot 'workers/tests'
    if (Test-Path -LiteralPath $workerTests -PathType Container) {
        Invoke-CheckBlock -Label 'Worker protocol tests' -Action {
            $python = Get-PythonInvocation
            if ($null -eq $python) { throw 'Python was not found.' }
            Write-Step 'Running worker unittest discovery'
            $workerArguments = @($python.Prefix + @('-m', 'unittest', 'discover', '-s', 'workers/tests', '-p', 'test_*.py', '-v'))
            Invoke-External -FilePath $python.FilePath -ArgumentList $workerArguments -WorkingDirectory $script:RepoRoot
        }
    } else {
        Add-Failure 'Worker protocol tests: workers/tests was not found.'
    }

    $mediaSource = Join-Path $script:RepoRoot 'native/media-broker'
    $gameLoadSmoke = Join-Path $script:RepoRoot 'tools/game-load/scripts/smoke.ps1'
    if (Test-IsWindows) {
        if (Test-Path -LiteralPath (Join-Path $mediaSource 'CMakeLists.txt') -PathType Leaf) {
            Invoke-CheckBlock -Label 'Native media broker tests' -Action {
                $nativeBuild = Join-Path $script:RepoRoot 'out/build/native-media-broker-windows'
                Write-Step 'Configuring native media broker tests for Windows x64'
                Invoke-External -FilePath 'cmake' -ArgumentList @(
                    '-S', $mediaSource,
                    '-B', $nativeBuild,
                    '-G', 'Visual Studio 17 2022',
                    '-A', 'x64',
                    '-DBUILD_TESTING=ON',
                    '-DNPC_MEDIA_BROKER_BUILD_TESTS=ON'
                )
                Write-Step 'Building the native media broker tests'
                Invoke-External -FilePath 'cmake' -ArgumentList @('--build', $nativeBuild, '--config', 'Debug')
                Write-Step 'Running native media broker tests'
                Invoke-External -FilePath 'ctest' -ArgumentList @('--test-dir', $nativeBuild, '-C', 'Debug', '--output-on-failure')
            }
        } else {
            Add-Failure 'Native media broker tests: native/media-broker/CMakeLists.txt was not found.'
        }

        if (Test-Path -LiteralPath $gameLoadSmoke -PathType Leaf) {
            Invoke-CheckBlock -Label 'Game-load harness tests' -Action {
                Write-Step 'Running game-load CMake tests and inert dry-run smoke'
                & $gameLoadSmoke
                if ($LASTEXITCODE -ne 0) { throw "Game-load smoke exited with code $LASTEXITCODE." }
            }
        } else {
            Add-Failure 'Game-load harness tests: tools/game-load/scripts/smoke.ps1 was not found.'
        }
    } else {
        Write-Skip 'Native media-broker tests require Windows 10/11 and MSVC; this host is not Windows.'
        Write-Skip 'Game-load CMake and dry-run smoke require Windows 10/11; no live load was attempted.'
    }

    $demoRoot = Join-Path $script:RepoRoot 'demo/readme'
    if (Test-Path -LiteralPath (Join-Path $demoRoot 'package.json') -PathType Leaf) {
        Invoke-CheckBlock -Label 'README demo tests' -Action {
            $npm = Get-NpmCommand
            if ($null -eq $npm) { throw 'npm was not found.' }
            Write-Step 'Running deterministic README demo unit tests'
            Invoke-External -FilePath $npm -ArgumentList @('test') -WorkingDirectory $demoRoot
            Write-Step 'Running README demo dry-run (no GUI capture)'
            Invoke-External -FilePath $npm -ArgumentList @('run', 'dry-run') -WorkingDirectory $demoRoot
            Write-Step 'Verifying checked-in README demo artifacts'
            Invoke-External -FilePath $npm -ArgumentList @('run', 'verify') -WorkingDirectory $demoRoot
        }
    } else {
        Add-Failure 'README demo tests: demo/readme/package.json was not found.'
    }
}

function Invoke-Lint {
    Assert-Environment
    if ($script:Failures.Count -gt 0) { return }

    $nodeRoot = Get-NodePackageRoot
    $scripts = Get-PackageScriptNames -PackageRoot $nodeRoot
    if ($scripts -contains 'typecheck') {
        Invoke-CheckBlock -Label 'Frontend typecheck' -Action {
            Write-Step 'Running frontend typecheck'
            Invoke-Pnpm -Arguments @('run', 'typecheck') -WorkingDirectory $nodeRoot
        }
    } else {
        Add-Failure 'Frontend typecheck: package.json must define typecheck.'
    }
    if ($scripts -contains 'format:check') {
        Invoke-CheckBlock -Label 'Frontend Prettier check' -Action {
            Write-Step 'Checking frontend formatting with Prettier'
            Invoke-Pnpm -Arguments @('run', 'format:check') -WorkingDirectory $nodeRoot
        }
    } else {
        Add-Failure 'Frontend Prettier check: package.json must define format:check.'
    }

    $rootCargoManifest = Join-Path $script:RepoRoot 'Cargo.toml'
    $tauriManifest = Join-Path $script:RepoRoot 'apps/control/src-tauri/Cargo.toml'
    if ((Test-Path -LiteralPath $rootCargoManifest -PathType Leaf) -and
        (Test-Path -LiteralPath $tauriManifest -PathType Leaf)) {
        Invoke-CheckBlock -Label 'Root Rust formatting' -Action {
            Write-Step 'Checking root Rust workspace formatting'
            Invoke-External -FilePath 'cargo' -ArgumentList @('fmt', '--all', '--', '--check') -WorkingDirectory $script:RepoRoot
        }
        Invoke-CheckBlock -Label 'Tauri Rust formatting' -Action {
            Write-Step 'Checking nested Tauri Rust formatting'
            Invoke-External -FilePath 'cargo' -ArgumentList @('fmt', '--manifest-path', $tauriManifest, '--all', '--', '--check') -WorkingDirectory $script:RepoRoot
        }

        $clippy = $null
        try {
            $clippy = Get-ClippyInvocation
            if (-not [string]::IsNullOrWhiteSpace($clippy.Note)) {
                Write-WarningMessage $clippy.Note
            }
        }
        catch {
            Add-Failure "Rust Clippy toolchain: $($_.Exception.Message)"
        }
        if ($null -ne $clippy) {
            Invoke-CheckBlock -Label 'Root Rust Clippy' -Action {
            Write-Step 'Running root Rust workspace Clippy with the locked dependency graph'
            $rootClippyArguments = @($clippy.Prefix + @(
                'clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--offline', '--', '-D', 'warnings'
            ))
            Invoke-External -FilePath $clippy.FilePath -ArgumentList $rootClippyArguments -WorkingDirectory $script:RepoRoot
            }
            if (Test-IsWindows) {
                $failureCountBeforeSidecars = $script:Failures.Count
                Invoke-CheckBlock -Label 'Tauri sidecar preparation' -Action {
                    Prepare-TauriSidecarsForValidation
                }
                if ($script:Failures.Count -eq $failureCountBeforeSidecars) {
                    Invoke-CheckBlock -Label 'Tauri Rust Clippy' -Action {
                        Write-Step 'Running nested Tauri Rust Clippy with the locked dependency graph'
                        $tauriClippyArguments = @($clippy.Prefix + @(
                            'clippy', '--manifest-path', $tauriManifest, '--all-targets', '--all-features', '--locked', '--offline', '--', '-D', 'warnings'
                        ))
                        Invoke-External -FilePath $clippy.FilePath -ArgumentList $tauriClippyArguments -WorkingDirectory $script:RepoRoot
                    }
                } else {
                    Write-Skip 'Nested Tauri Rust Clippy was not run because its required project sidecars were not prepared.'
                }
            } else {
                Write-Skip 'Nested Tauri Rust Clippy requires the supported Windows target; this host is not Windows.'
            }
        }
    } else {
        Add-Failure 'Rust static checks require both Cargo.toml and apps/control/src-tauri/Cargo.toml.'
    }

    Invoke-CheckBlock -Label 'Repository JSON validation' -Action {
        Write-Step 'Validating repository JSON files'
        $jsonValidator = Join-Path $PSScriptRoot 'validate-json.cjs'
        $jsonValidatorTest = Join-Path $PSScriptRoot 'test-json-validator.cjs'
        if (-not (Test-Path -LiteralPath $jsonValidator -PathType Leaf)) {
            throw "Repository JSON validator was not found: $jsonValidator"
        }
        if (-not (Test-Path -LiteralPath $jsonValidatorTest -PathType Leaf)) {
            throw "Repository JSON validator regression test was not found: $jsonValidatorTest"
        }

        Invoke-External -FilePath 'node' -ArgumentList @($jsonValidatorTest) -WorkingDirectory $script:RepoRoot -DisplayArguments '<repository JSON validator regression test>'

        $excludedDirectories = @('.git', '.secrets', '.pnpm-store', '.render-work', 'node_modules', 'target', 'out', 'dist', 'coverage')
        $pendingDirectories = New-Object 'System.Collections.Generic.Stack[string]'
        $pendingDirectories.Push($script:RepoRoot)
        $jsonFiles = New-Object 'System.Collections.Generic.List[System.IO.FileInfo]'
        while ($pendingDirectories.Count -gt 0) {
            $directory = $pendingDirectories.Pop()
            foreach ($childDirectory in @(Get-ChildItem -LiteralPath $directory -Directory -Force -ErrorAction Stop | Sort-Object Name -Descending)) {
                if ($excludedDirectories -notcontains $childDirectory.Name -and
                    -not ($childDirectory.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
                    $pendingDirectories.Push($childDirectory.FullName)
                }
            }
            foreach ($jsonFile in @(Get-ChildItem -LiteralPath $directory -Filter '*.json' -File -Force -ErrorAction Stop | Sort-Object FullName)) {
                $jsonFiles.Add($jsonFile)
            }
        }
        $jsonPaths = @($jsonFiles | Sort-Object FullName | ForEach-Object { $_.FullName })
        $jsonManifestPath = Join-Path ([System.IO.Path]::GetTempPath()) "npc-json-paths-$([Guid]::NewGuid().ToString('N')).json"
        try {
            $jsonManifest = [ordered]@{
                version = 1
                paths = @($jsonPaths)
            } | ConvertTo-Json -Depth 3 -Compress
            [System.IO.File]::WriteAllText(
                $jsonManifestPath,
                $jsonManifest,
                (New-Object System.Text.UTF8Encoding($false))
            )
            Invoke-External -FilePath 'node' -ArgumentList @($jsonValidator, '--paths-file', $jsonManifestPath) -WorkingDirectory $script:RepoRoot -DisplayArguments "<local JSON parser> --paths-file <temporary manifest with $($jsonFiles.Count) sorted JSON paths>"
        }
        finally {
            Remove-Item -LiteralPath $jsonManifestPath -Force -ErrorAction SilentlyContinue
        }
        Write-Host "    Validated $($jsonFiles.Count) JSON files." -ForegroundColor DarkGray
    }

    $linkChecker = Join-Path $PSScriptRoot 'check-doc-links.ps1'
    if (Test-Path -LiteralPath $linkChecker -PathType Leaf) {
        Invoke-CheckBlock -Label 'Markdown link validation' -Action {
            Write-Step 'Validating local Markdown links without network access'
            & $linkChecker -RepositoryRoot $script:RepoRoot
        }
    } else {
        Add-Failure "Markdown link validation: missing $linkChecker."
    }

    $analyzer = Get-Module -ListAvailable -Name PSScriptAnalyzer
    if ($null -ne $analyzer) {
        Write-Step 'Running PSScriptAnalyzer'
        Import-Module PSScriptAnalyzer
        $issues = @(Invoke-ScriptAnalyzer -Path $PSScriptRoot -Recurse -Severity Error, Warning)
        if ($issues.Count -gt 0) {
            $issues | Format-Table -AutoSize
            Add-Failure "PSScriptAnalyzer reported $($issues.Count) issue(s)."
        }
    } else {
        Write-WarningMessage 'PSScriptAnalyzer is not installed; PowerShell static analysis was skipped.'
    }
}

function Invoke-BenchmarkCommand {
    Assert-Environment
    if ($script:Failures.Count -gt 0) { return }
    $parameters = @{}
    if ($Quick) { $parameters.Quick = $true }
    if (-not [string]::IsNullOrWhiteSpace($OutputDirectory)) {
        $parameters.OutputDirectory = $OutputDirectory
    }
    & (Join-Path $PSScriptRoot 'benchmark.ps1') @parameters
    if ($LASTEXITCODE -ne 0) { throw 'Benchmark harness failed.' }
}

function Invoke-PackageCommand {
    Assert-Environment
    if ($script:Failures.Count -gt 0) { return }
    $parameters = @{ Configuration = $Configuration }
    if ($SkipChecks) { $parameters.SkipChecks = $true }
    if (-not [string]::IsNullOrWhiteSpace($OutputDirectory)) {
        $parameters.OutputDirectory = $OutputDirectory
    }
    & (Join-Path $PSScriptRoot 'package.ps1') @parameters
    if ($LASTEXITCODE -ne 0) { throw 'Packaging harness failed.' }
}

try {
    switch ($Command) {
        'environment' { Assert-Environment }
        'setup' { Invoke-Setup }
        'dev' { Invoke-Dev }
        'test' { Invoke-Tests }
        'lint' { Invoke-Lint }
        'benchmark' { Invoke-BenchmarkCommand }
        'package' { Invoke-PackageCommand }
    }
}
catch {
    Add-Failure $_.Exception.Message
}

if ($script:Warnings.Count -gt 0) {
    Write-Host "`nWarnings:" -ForegroundColor Yellow
    foreach ($warning in $script:Warnings) { Write-Host "  - $warning" }
}

if ($script:Failures.Count -gt 0) {
    Write-Host "`n$($script:Failures.Count) blocking problem(s):" -ForegroundColor Red
    foreach ($failure in $script:Failures) { Write-Host "  - $failure" }
    exit 1
}

Write-Host "`nDone: $Command" -ForegroundColor Green
exit 0
