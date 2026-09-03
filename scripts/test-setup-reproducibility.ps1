[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: setup dispatch regression requires Windows command resolution.'
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
    $parent = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

function New-LoggedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [string]$Output
    )
    $outputLine = if ([string]::IsNullOrWhiteSpace($Output)) { '' } else { "ECHO $Output`r`n" }
    Write-Utf8NoBom -Path $Path -Content @"
@ECHO OFF
>>"%NPC_SETUP_LOG%" ECHO %NPC_SETUP_PHASE%^|%~n0^|%CD%^|%*
$outputLine`EXIT /B 0
"@
}

$caseRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-setup-repro-$([Guid]::NewGuid().ToString('N'))"
$repoRoot = Join-Path $caseRoot 'clean checkout with spaces'
$scriptsRoot = Join-Path $repoRoot 'scripts'
$toolRoot = Join-Path $caseRoot 'tools'
$logPath = Join-Path $caseRoot 'commands.log'
$savedPath = $env:PATH
$savedProgramFiles = $env:ProgramFiles
$savedProgramFilesX86 = ${env:ProgramFiles(x86)}
$savedUserProfile = $env:USERPROFILE
$savedLocalAppData = $env:LOCALAPPDATA
$savedLog = $env:NPC_SETUP_LOG
$savedPhase = $env:NPC_SETUP_PHASE

try {
    $windowsScriptsRoot = Join-Path $scriptsRoot 'windows'
    New-Item -ItemType Directory -Path $scriptsRoot, $windowsScriptsRoot, $toolRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'dev.ps1') -Destination $scriptsRoot
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'short-cmake-build-path.ps1') -Destination $scriptsRoot
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows/node-tooling.ps1') -Destination $windowsScriptsRoot
    Write-Utf8NoBom -Path (Join-Path $windowsScriptsRoot 'prepare-webview2-offline-installer.ps1') -Content @'
[CmdletBinding()]
param([switch]$Offline, [string]$RepositoryRoot)
Add-Content -LiteralPath $env:NPC_SETUP_LOG -Value "$($env:NPC_SETUP_PHASE)|webview|$RepositoryRoot|offline=$([bool]$Offline)"
exit 0
'@

    Write-Utf8NoBom -Path (Join-Path $repoRoot 'package.json') -Content @'
{"name":"setup-fixture","private":true,"packageManager":"pnpm@10.28.2","engines":{"node":"20.20.2","pnpm":"10.28.2"}}
'@
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'pnpm-lock.yaml') -Content "lockfileVersion: '9.0'`npackages: {}`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'Cargo.toml') -Content @'
[workspace]
resolver = "2"
members = []
'@
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'Cargo.lock') -Content "version = 4`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml') -Content @'
[package]
name = "tauri-fixture"
version = "0.0.0"
edition = "2021"

[workspace]
'@
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.lock') -Content "version = 4`n"
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'demo/readme/package.json') -Content '{"name":"demo-fixture","private":true}'
    Write-Utf8NoBom -Path (Join-Path $repoRoot 'demo/readme/package-lock.json') -Content '{"name":"demo-fixture","lockfileVersion":3,"packages":{"":{"name":"demo-fixture"}}}'

    New-LoggedCommand -Path (Join-Path $toolRoot 'node.cmd') -Output 'v20.20.2'
    New-LoggedCommand -Path (Join-Path $toolRoot 'pnpm.cmd') -Output '10.28.2'
    New-LoggedCommand -Path (Join-Path $toolRoot 'cargo.cmd')
    New-LoggedCommand -Path (Join-Path $toolRoot 'npm.cmd')

    $env:PATH = "$toolRoot;$savedPath"
    $env:ProgramFiles = Join-Path $caseRoot 'empty-program-files'
    ${env:ProgramFiles(x86)} = Join-Path $caseRoot 'empty-program-files-x86'
    $env:USERPROFILE = Join-Path $caseRoot 'empty-profile'
    $env:LOCALAPPDATA = Join-Path $caseRoot 'empty-local-app-data'
    $env:NPC_SETUP_LOG = $logPath

    $env:NPC_SETUP_PHASE = 'online'
    & (Join-Path $scriptsRoot 'dev.ps1') setup
    Assert-True -Condition ($LASTEXITCODE -eq 0) -Message "Online setup fixture failed with $LASTEXITCODE."

    $env:NPC_SETUP_PHASE = 'offline'
    & (Join-Path $scriptsRoot 'dev.ps1') setup -Offline
    Assert-True -Condition ($LASTEXITCODE -eq 0) -Message "Offline setup fixture failed with $LASTEXITCODE."

    $lines = @(Get-Content -LiteralPath $logPath)
    # The hidden cmd launcher quotes every token so cmd metacharacters remain
    # data. Normalize only the fixture receipt before asserting semantic argv.
    $normalizedLines = @($lines | ForEach-Object { $_.Replace('"', '') })
    foreach ($phase in @('online', 'offline')) {
        $phaseLines = @($normalizedLines | Where-Object { $_.StartsWith("$phase|", [System.StringComparison]::Ordinal) })
        $cargoLines = @($phaseLines | Where-Object { $_ -match '^.+\|cargo\|' })
        Assert-True -Condition ($cargoLines.Count -eq 2) -Message "$phase setup must fetch exactly the root and nested Tauri Cargo manifests. Calls: $($phaseLines -join '; ')"
        Assert-True -Condition (@($cargoLines | Where-Object { $_ -match [regex]::Escape((Join-Path $repoRoot 'Cargo.toml')) }).Count -eq 1) -Message "$phase setup omitted the root Cargo manifest."
        Assert-True -Condition (@($cargoLines | Where-Object { $_ -match [regex]::Escape((Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml')) }).Count -eq 1) -Message "$phase setup omitted the nested Tauri Cargo manifest."
        Assert-True -Condition (@($phaseLines | Where-Object { $_ -match '^.+\|pnpm\|.+\|install --frozen-lockfile' }).Count -eq 1) -Message "$phase setup omitted the frozen pnpm restore."
        Assert-True -Condition (@($phaseLines | Where-Object { $_ -match '^.+\|npm\|.+demo\\readme\|ci' }).Count -eq 1) -Message "$phase setup omitted the locked README demo restore."
        Assert-True -Condition (@($phaseLines | Where-Object { $_ -match '^.+\|webview\|' }).Count -eq 1) -Message "$phase setup omitted the pinned WebView2 cache preparation."
    }

    $onlineLines = @($normalizedLines | Where-Object { $_.StartsWith('online|', [System.StringComparison]::Ordinal) })
    $offlineLines = @($normalizedLines | Where-Object { $_.StartsWith('offline|', [System.StringComparison]::Ordinal) })
    Assert-True -Condition (@($onlineLines | Where-Object { $_ -match '(?:^| )--offline(?: |$)' }).Count -eq 0) -Message 'Online setup unexpectedly forced offline restore.'
    Assert-True -Condition (@($offlineLines | Where-Object { $_ -match '(?:^| )--offline(?: |$)' }).Count -eq 4) -Message "Offline setup must bind pnpm, both Cargo fetches, and npm to their offline modes. Calls: $($offlineLines -join '; ')"
    Assert-True -Condition (@($onlineLines | Where-Object { $_ -match '\|webview\|.*offline=False$' }).Count -eq 1) -Message 'Online setup did not permit exact immutable WebView cache warming.'
    Assert-True -Condition (@($offlineLines | Where-Object { $_ -match '\|webview\|.*offline=True$' }).Count -eq 1) -Message 'Offline setup did not require the already-warmed WebView cache.'

    Write-Host 'Setup online-warm/offline restore dispatch regression passed.' -ForegroundColor Green
}
finally {
    [System.Environment]::SetEnvironmentVariable('PATH', $savedPath, 'Process')
    [System.Environment]::SetEnvironmentVariable('ProgramFiles', $savedProgramFiles, 'Process')
    [System.Environment]::SetEnvironmentVariable('ProgramFiles(x86)', $savedProgramFilesX86, 'Process')
    [System.Environment]::SetEnvironmentVariable('USERPROFILE', $savedUserProfile, 'Process')
    [System.Environment]::SetEnvironmentVariable('LOCALAPPDATA', $savedLocalAppData, 'Process')
    [System.Environment]::SetEnvironmentVariable('NPC_SETUP_LOG', $savedLog, 'Process')
    [System.Environment]::SetEnvironmentVariable('NPC_SETUP_PHASE', $savedPhase, 'Process')
    if (Test-Path -LiteralPath $caseRoot) {
        Remove-Item -LiteralPath $caseRoot -Recurse -Force
    }
}

exit 0
