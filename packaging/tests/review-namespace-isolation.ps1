[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$basePath = Join-Path $repoRoot 'apps/control/src-tauri/tauri.conf.json'
$releasePath = Join-Path $repoRoot 'packaging/windows/tauri.release.conf.json'
$reviewPath = Join-Path $repoRoot 'packaging/windows/tauri.review.conf.json'
$devPath = Join-Path $repoRoot 'packaging/windows/tauri.dev.conf.json'
$packageScriptPath = Join-Path $repoRoot 'scripts/package.ps1'
$devScriptPath = Join-Path $repoRoot 'scripts/dev.ps1'
$tauriLibPath = Join-Path $repoRoot 'apps/control/src-tauri/src/lib.rs'
$providerLoadoutsPath = Join-Path $repoRoot 'apps/control/src-tauri/src/provider_loadouts.rs'
$testGameScriptPath = Join-Path $repoRoot 'scripts/windows/prepare-review-test-game.ps1'
$reviewPathsScriptPath = Join-Path $repoRoot 'scripts/windows/review-paths.ps1'
$priorCleanupScriptPath = Join-Path $repoRoot 'scripts/windows/remove-prior-hands-on-install.ps1'

$base = Get-Content -LiteralPath $basePath -Raw | ConvertFrom-Json
$release = Get-Content -LiteralPath $releasePath -Raw | ConvertFrom-Json
$review = Get-Content -LiteralPath $reviewPath -Raw | ConvertFrom-Json
$dev = Get-Content -LiteralPath $devPath -Raw | ConvertFrom-Json
$productionIdentifier = [string]$base.identifier
$reviewIdentifier = [string]$review.identifier
$devIdentifier = [string]$dev.identifier

if ([string]::IsNullOrWhiteSpace($productionIdentifier)) { throw 'Base Tauri identifier is missing.' }
if ($reviewIdentifier -ne "$productionIdentifier.review") {
    throw 'Debug review identifier must be the production identifier plus .review.'
}
if ($devIdentifier -ne "$productionIdentifier.debug") {
    throw 'Tauri dev identifier must be the production identifier plus .debug.'
}
if ($dev.bundle.active -ne $false) {
    throw 'Tauri dev namespace overlay must never enable bundling.'
}
if (@($release.PSObject.Properties.Name) -contains 'identifier') {
    throw 'Release overlay must not override or leak the local-review identifier.'
}
if ($review.bundle.createUpdaterArtifacts -ne $false -or $release.bundle.createUpdaterArtifacts -ne $false) {
    throw 'Neither packaging overlay may create updater artifacts.'
}
if ($review.bundle.windows.nsis.installMode -ne 'currentUser' -or
    $release.bundle.windows.nsis.installMode -ne 'currentUser') {
    throw 'Both package configurations must remain current-user installs.'
}
$expectedHooks = 'generated-installer-inputs/installer-hooks.generated.nsh'
if ($review.bundle.windows.nsis.installerHooks -ne $expectedHooks -or
    $release.bundle.windows.nsis.installerHooks -ne $expectedHooks) {
    throw 'Debug and Release packaging must require the generated pinned-offline NSIS hook.'
}
if (@($review.bundle.windows.nsis.languages).Count -ne 1 -or
    @($review.bundle.windows.nsis.languages)[0] -ne 'English') {
    throw 'Debug review installer language selection must remain deterministic.'
}

$packageSource = Get-Content -LiteralPath $packageScriptPath -Raw
$devSource = Get-Content -LiteralPath $devScriptPath -Raw
$tauriLibSource = Get-Content -LiteralPath $tauriLibPath -Raw
$providerLoadoutsSource = Get-Content -LiteralPath $providerLoadoutsPath -Raw
$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($packageScriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "package.ps1 has $($parseErrors.Count) parse error(s)." }
$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($devScriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "dev.ps1 has $($parseErrors.Count) parse error(s)." }
$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($testGameScriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "prepare-review-test-game.ps1 has $($parseErrors.Count) parse error(s)." }
$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($reviewPathsScriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "review-paths.ps1 has $($parseErrors.Count) parse error(s)." }
$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($priorCleanupScriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "remove-prior-hands-on-install.ps1 has $($parseErrors.Count) parse error(s)." }
foreach ($requiredText in @(
    "tauri.review.conf.json",
    "tauri.release.conf.json",
    'production_application_identifier',
    'application_identifier',
    'app_config_folder',
    'review_test_game',
    'control_executable_path',
    'installer_paths',
    'review_test_game_path',
    'publication_performed',
    'signing_performed'
)) {
    if ($packageSource.IndexOf($requiredText, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Package manifest/config selection omits: $requiredText"
    }
}
foreach ($requiredText in @('tauri.dev.conf.json', "'--config'", 'isolated .debug namespace')) {
    if ($devSource.IndexOf($requiredText, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Canonical dev command does not enforce isolated config: $requiredText"
    }
}
foreach ($requiredText in @(
    'app.config().identifier.clone()',
    'AppState::new_for_distribution('
)) {
    if ($tauriLibSource.IndexOf($requiredText, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Tauri bootstrap does not pass the actual application identifier into native state: $requiredText"
    }
}
foreach ($requiredText in @(
    ('const PRODUCTION_APPLICATION_NAMESPACE: &str = "{0}"' -f $productionIdentifier),
    ('"{0}"' -f $reviewIdentifier),
    ('"{0}"' -f $devIdentifier),
    'REVIEW_APPLICATION_NAMESPACE | DEBUG_APPLICATION_NAMESPACE',
    'stored.acknowledgement.application_namespace == expected_application_namespace',
    'production namespace must not read a review acknowledgement even if pointed at the same test directory'
)) {
    if ($providerLoadoutsSource.IndexOf($requiredText, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Native private-evaluation namespace gate omits: $requiredText"
    }
}
foreach ($forbiddenText in @(
    'cfg!(debug_assertions) || app.config().identifier',
    'cfg!(debug_assertions) || application_namespace',
    'cfg!(debug_assertions) && application_namespace'
)) {
    if ($tauriLibSource.IndexOf($forbiddenText, [System.StringComparison]::Ordinal) -ge 0 -or
        $providerLoadoutsSource.IndexOf($forbiddenText, [System.StringComparison]::Ordinal) -ge 0) {
        throw "Debug build profile still participates in private-evaluation authority: $forbiddenText"
    }
}

[ordered]@{
    schema_version = 1
    status = 'passed'
    production_identifier = $productionIdentifier
    dev_identifier = $devIdentifier
    review_identifier = $reviewIdentifier
    release_identifier_source = 'apps/control/src-tauri/tauri.conf.json'
    review_isolated = $true
    dev_isolated = $true
    updater_artifacts_disabled = $true
} | ConvertTo-Json -Compress
