[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: Clippy dispatch regression requires Windows rustup command semantics.'
    exit 0
}

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) { throw $Message }
}

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )

    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

function New-ClippyDispatchFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][bool]$MismatchStableIdentity
    )

    $repoRoot = Join-Path $Root 'sanitized-tree'
    $binRoot = Join-Path $Root 'command-shims'
    $outsideRoot = Join-Path $Root 'caller-outside-repository'
    foreach ($directory in @(
            $repoRoot,
            $binRoot,
            $outsideRoot,
            (Join-Path $repoRoot 'scripts'),
            (Join-Path $repoRoot 'scripts/windows'),
            (Join-Path $repoRoot 'apps/control/src-tauri'),
            (Join-Path $repoRoot 'apps/control/src-tauri/binaries')
        )) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }

    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'dev.ps1') -Destination (Join-Path $repoRoot 'scripts/dev.ps1')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'short-cmake-build-path.ps1') -Destination (Join-Path $repoRoot 'scripts/short-cmake-build-path.ps1')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows/node-tooling.ps1') -Destination (Join-Path $repoRoot 'scripts/windows/node-tooling.ps1')
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'package.json') -Content @'
{
  "packageManager": "pnpm@10.28.2",
  "engines": {
    "node": "20.20.2",
    "pnpm": "10.28.2"
  },
  "scripts": {
    "typecheck": "fixture",
    "format:check": "fixture"
  }
}
'@
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'Cargo.toml') -Content "[workspace]`nmembers = []`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot '.pinned-clippy-unavailable') -Content "fixture`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml') -Content "[package]`nname = 'fixture'`nversion = '0.0.0'`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'scripts/validate-json.cjs') -Content "process.exit(0);`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'scripts/test-json-validator.cjs') -Content "process.exit(0);`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'scripts/check-doc-links.ps1') -Content "exit 0`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'scripts/prepare-sidecars.ps1') -Content @'
param([string]$Configuration)
Add-Content -LiteralPath $env:NPC_CLIPPY_TEST_LOG -Value "prepare|$PWD|$Configuration"
$sidecarRoot = Join-Path $PSScriptRoot '../apps/control/src-tauri/binaries'
Set-Content -LiteralPath (Join-Path $sidecarRoot 'npc-runtime-x86_64-pc-windows-msvc.exe') -Value 'fixture'
Set-Content -LiteralPath (Join-Path $sidecarRoot 'npc-media-broker-x86_64-pc-windows-msvc.exe') -Value 'fixture'
Set-Content -LiteralPath (Join-Path $sidecarRoot 'npc-mouth-worker-x86_64-pc-windows-msvc.exe') -Value 'fixture'
Set-Content -LiteralPath (Join-Path $sidecarRoot 'npc-subtitle-presenter-x86_64-pc-windows-msvc.exe') -Value 'fixture'
'@

    $cargoShim = @'
@echo off
setlocal
>>"%NPC_CLIPPY_TEST_LOG%" echo cargo^|%CD%^|%*
if /I "%~1"=="clippy" goto plain_clippy
if /I "%~1"=="+stable" goto stable_clippy
exit /b 0

:plain_clippy
if /I not "%~2"=="--version" exit /b 0
if exist ".pinned-clippy-unavailable" goto pinned_unavailable
echo clippy 0.1.fixture-outside
exit /b 0

:pinned_unavailable
echo error: pinned Clippy is unavailable 1>&2
exit /b 1

:stable_clippy
echo clippy 0.1.fixture-stable
exit /b 0
'@
    Write-Utf8NoBom -Path (Join-Path $binRoot 'cargo.cmd') -Content $cargoShim

    $stableCommit = if ($MismatchStableIdentity) { 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' } else { 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }
    $rustcShim = @"
@echo off
setlocal
>>"%NPC_CLIPPY_TEST_LOG%" echo rustc^|%CD%^|%*
if /I "%~1"=="+stable" (
  echo release: 1.96.1
  echo commit-hash: $stableCommit
  exit /b 0
)
echo release: 1.96.1
echo commit-hash: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
exit /b 0
"@
    Write-Utf8NoBom -Path (Join-Path $binRoot 'rustc.cmd') -Content $rustcShim

    foreach ($commandName in @('corepack', 'git', 'node')) {
        Write-Utf8NoBom -Path (Join-Path $binRoot "$commandName.cmd") -Content "@echo off`r`nexit /b 0`r`n"
    }
    Write-Utf8NoBom -Path (Join-Path $binRoot 'pnpm.cmd') -Content "@echo off`r`necho 10.28.2`r`nexit /b 0`r`n"

    return [pscustomobject]@{
        RepoRoot = $repoRoot
        BinRoot = $binRoot
        OutsideRoot = $outsideRoot
        LogPath = Join-Path $Root 'commands.log'
    }
}

function Invoke-ClippyDispatchCase {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$MismatchStableIdentity
    )

    $caseRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-clippy-dispatch-$Name-$([Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $caseRoot -Force | Out-Null
    $fixture = New-ClippyDispatchFixture -Root $caseRoot -MismatchStableIdentity $MismatchStableIdentity
    $powershellPath = (Get-Command powershell.exe -ErrorAction Stop).Source

    $savedEnvironment = @{
        LOCALAPPDATA = $env:LOCALAPPDATA
        PATH = $env:PATH
        PATHEXT = $env:PATHEXT
        ProgramFiles = $env:ProgramFiles
        USERPROFILE = $env:USERPROFILE
    }
    try {
        $env:LOCALAPPDATA = Join-Path $caseRoot 'local-app-data'
        $env:ProgramFiles = Join-Path $caseRoot 'program-files'
        $env:USERPROFILE = Join-Path $caseRoot 'user-profile'
        $env:PATHEXT = '.COM;.EXE;.BAT;.CMD'
        $env:PATH = $fixture.BinRoot
        $env:NPC_CLIPPY_TEST_LOG = $fixture.LogPath
        $env:NPC_CLIPPY_TEST_REPO = $fixture.RepoRoot

        $process = Invoke-NpcHiddenProcess -FilePath $powershellPath -ArgumentList @(
            '-NoLogo',
            '-NoProfile',
            '-NonInteractive',
            '-WindowStyle', 'Hidden',
            '-ExecutionPolicy', 'Bypass',
            '-File', (Join-Path $fixture.RepoRoot 'scripts/dev.ps1'),
            'lint'
        ) -WorkingDirectory $fixture.OutsideRoot -NoReplayOutput
        $output = @($process.StandardOutput, $process.StandardError)
        $exitCode = $process.ExitCode

        $text = ($output | ForEach-Object { [string]$_ }) -join "`n"
        $calls = if (Test-Path -LiteralPath $fixture.LogPath -PathType Leaf) {
            @(Get-Content -LiteralPath $fixture.LogPath)
        } else {
            @()
        }
        $repoPrefix = "cargo|$($fixture.RepoRoot)|"
        $prepareCall = "prepare|$($fixture.RepoRoot)|Debug"
        $plainClippyProbe = "${repoPrefix}`"clippy`" `"--version`""
        $outsideClippyProbe = "cargo|$($fixture.OutsideRoot)|`"clippy`" `"--version`""

        Assert-True -Condition ($calls -contains $plainClippyProbe) -Message "$Name did not probe plain Clippy from the sanitized repository root. Calls: $($calls -join '; '). Output: $text"
        Assert-True -Condition (-not ($calls -contains $outsideClippyProbe)) -Message "$Name incorrectly probed Clippy from the caller's directory."

        if ($MismatchStableIdentity) {
            Assert-True -Condition ($exitCode -ne 0) -Message 'Identity mismatch unexpectedly passed lint.'
            Assert-True -Condition ($text -match 'not proven identical') -Message "Identity mismatch did not report the safe refusal. Output: $text"
            Assert-True -Condition (-not ($calls | Where-Object { $_ -like "${repoPrefix}`"+stable`" `"clippy`" `"--workspace`"*" })) -Message 'Identity mismatch executed the unverified stable Clippy fallback.'
        } else {
            Assert-True -Condition ($exitCode -eq 0) -Message "Matching identity fixture failed lint with exit $exitCode. Output: $text"
            Assert-True -Condition ($text -match 'verified \+stable is identical') -Message "Matching identity fixture did not report the verified fallback. Calls: $($calls -join '; '). Output: $text"
            Assert-True -Condition (@($calls | Where-Object { $_ -like "${repoPrefix}`"+stable`" `"clippy`"*" }).Count -eq 2) -Message "Matching identity fixture did not run both Clippy gates through +stable. Calls: $($calls -join '; ')"
            Assert-True -Condition (-not ($calls | Where-Object { $_ -like "${repoPrefix}`"clippy`" `"--workspace`"*" })) -Message 'Matching identity fixture bypassed the selected +stable invocation.'
            Assert-True -Condition ($calls -contains $prepareCall) -Message "Matching identity fixture did not prepare clean-checkout sidecars. Calls: $($calls -join '; ')"
            $prepareIndex = [array]::IndexOf($calls, $prepareCall)
            $tauriClippyCall = @($calls | Where-Object { $_ -like "${repoPrefix}`"+stable`" `"clippy`" `"--manifest-path`"*" }) | Select-Object -First 1
            $tauriClippyIndex = [array]::IndexOf($calls, $tauriClippyCall)
            Assert-True -Condition ($prepareIndex -ge 0 -and $tauriClippyIndex -gt $prepareIndex) -Message "Matching identity fixture did not prepare sidecars before nested Tauri Clippy. Calls: $($calls -join '; ')"
        }
    }
    finally {
        foreach ($name in $savedEnvironment.Keys) {
            [System.Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process')
        }
        Remove-Item Env:NPC_CLIPPY_TEST_LOG -ErrorAction SilentlyContinue
        Remove-Item Env:NPC_CLIPPY_TEST_REPO -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $caseRoot) {
            Remove-Item -LiteralPath $caseRoot -Recurse -Force
        }
    }
}

Invoke-ClippyDispatchCase -Name 'matching' -MismatchStableIdentity $false
Invoke-ClippyDispatchCase -Name 'mismatch' -MismatchStableIdentity $true
Write-Host 'Clippy dispatch regression checks passed.' -ForegroundColor Green
exit 0
