[CmdletBinding()]
param(
    [switch]$RequireAll
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$workspaceParent = Split-Path -Parent $repoRoot
$appPath = Join-Path $repoRoot 'apps/control/src-tauri/target/debug/interactive-npcs-control.exe'
$bundleRoot = Join-Path $repoRoot 'apps/control/src-tauri/target/debug/bundle/nsis'
$installer = @(Get-ChildItem -LiteralPath $bundleRoot -File -Filter '*-setup.exe' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1)
$installerPath = if ($installer.Count -eq 1) { $installer[0].FullName } else { $null }
$testGamePath = Join-Path $workspaceParent 'local-app-data/test-game/interactive-npcs-synthetic-target.exe'
$testGameDirectory = Split-Path -Parent $testGamePath
$testGameManifestPath = Join-Path $testGameDirectory 'REVIEW-FIXTURE-MANIFEST.json'
$testGameSbomPath = Join-Path $testGameDirectory 'review-test-game.cdx.json'
$testGameNoticesPath = Join-Path $testGameDirectory 'THIRD-PARTY-NOTICES.md'
$testGameLauncherPath = Join-Path $repoRoot 'scripts/synthetic-game-replay.ps1'
$testGamePreparePath = Join-Path $repoRoot 'scripts/windows/prepare-review-test-game.ps1'
$testGameVerifyPath = Join-Path $repoRoot 'scripts/windows/verify-review-test-game.ps1'
$packageScriptPath = Join-Path $repoRoot 'scripts/package.ps1'
$smokeScriptPath = Join-Path $repoRoot 'scripts/installer-smoke.ps1'

$missing = New-Object System.Collections.Generic.List[string]
foreach ($entry in @(
    @{ label = 'app'; path = $appPath },
    @{ label = 'installer'; path = $installerPath },
    @{ label = 'test_game'; path = $testGamePath },
    @{ label = 'test_game_manifest'; path = $testGameManifestPath },
    @{ label = 'test_game_sbom'; path = $testGameSbomPath },
    @{ label = 'test_game_notices'; path = $testGameNoticesPath },
    @{ label = 'test_game_launcher'; path = $testGameLauncherPath }
)) {
    if ([string]::IsNullOrWhiteSpace([string]$entry.path) -or
        -not (Test-Path -LiteralPath ([string]$entry.path) -PathType Leaf)) {
        $missing.Add([string]$entry.label)
    }
}

$result = [ordered]@{
    schema_version = 1
    status = if ($missing.Count -eq 0) { 'ready' } else { 'incomplete' }
    app_path = $appPath
    app_exists = Test-Path -LiteralPath $appPath -PathType Leaf
    installer_path = $installerPath
    installer_exists = -not [string]::IsNullOrWhiteSpace($installerPath) -and
        (Test-Path -LiteralPath $installerPath -PathType Leaf)
    test_game_path = $testGamePath
    test_game_exists = Test-Path -LiteralPath $testGamePath -PathType Leaf
    test_game_directory = $testGameDirectory
    test_game_manifest_path = $testGameManifestPath
    test_game_manifest_exists = Test-Path -LiteralPath $testGameManifestPath -PathType Leaf
    test_game_sbom_path = $testGameSbomPath
    test_game_sbom_exists = Test-Path -LiteralPath $testGameSbomPath -PathType Leaf
    test_game_notices_path = $testGameNoticesPath
    test_game_notices_exists = Test-Path -LiteralPath $testGameNoticesPath -PathType Leaf
    test_game_launcher_path = $testGameLauncherPath
    package_command = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$packageScriptPath`" -Configuration Debug -SkipChecks"
    test_game_prepare_command = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$testGamePreparePath`""
    test_game_verify_command = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$testGameVerifyPath`" -Directory `"$testGameDirectory`""
    test_game_direct_command = "& `"$testGamePath`""
    test_game_command = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$testGameLauncherPath`" -PlaceOnSecondMonitor"
    installer_smoke_script = $smokeScriptPath
    missing = @($missing)
}
$result | ConvertTo-Json -Compress -Depth 4 | Write-Output
if ($RequireAll -and $missing.Count -gt 0) { exit 1 }
exit 0
