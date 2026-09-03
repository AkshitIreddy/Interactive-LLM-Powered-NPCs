[CmdletBinding()]
param(
    [string]$InstallerSmokePath,
    [string]$PreflightResultPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ([string]::IsNullOrWhiteSpace($InstallerSmokePath)) {
    $InstallerSmokePath = Join-Path $PSScriptRoot '../../scripts/installer-smoke.ps1'
}
$scriptPath = (Resolve-Path $InstallerSmokePath).Path
$tokens = $null
$parseErrors = $null
$scriptAst = [System.Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "installer-smoke.ps1 has $($parseErrors.Count) parse error(s)." }

$source = Get-Content -LiteralPath $scriptPath -Raw
$snapshotMarker = '$postUninstallSnapshot = Wait-InstallTargetsClean'
$evidenceAssignment = '$result.post_uninstall_snapshot = $postUninstallSnapshot'
$cleanupMarker = '# Emergency cleanup is deliberately after the evidence snapshot'
$snapshotIndex = $source.IndexOf($snapshotMarker, [System.StringComparison]::Ordinal)
$assignmentIndex = $source.IndexOf($evidenceAssignment, [System.StringComparison]::Ordinal)
$cleanupIndex = $source.IndexOf($cleanupMarker, [System.StringComparison]::Ordinal)
if ($snapshotIndex -lt 0 -or $assignmentIndex -le $snapshotIndex -or $cleanupIndex -le $assignmentIndex) {
    throw 'Post-uninstall evidence must be captured and assigned before emergency cleanup.'
}
$evidenceBlock = $source.Substring($assignmentIndex, $cleanupIndex - $assignmentIndex)
foreach ($field in @('no_remaining_processes', 'no_install_files', 'no_shortcuts', 'no_registry_entries')) {
    if ($evidenceBlock.IndexOf("`$result.$field", [System.StringComparison]::Ordinal) -lt 0) {
        throw "Evidence block does not compute $field."
    }
}
if ($evidenceBlock -match '(?m)^\s*(Remove-Item|Remove-ItemProperty|Stop-Process)\b') {
    throw 'Evidence block mutates installer targets before emergency cleanup.'
}
foreach ($identityField in @(
    'git_head',
    'git_dirty',
    'package_manifest_sha256',
    'cargo_lock_sha256',
    'pnpm_lock_sha256',
    'profile_corpus_sha256',
    'catalog_sha256',
    'model_manifest_sha256',
    'review_tauri_config_sha256',
    'release_tauri_config_sha256',
    'review_test_game_script_sha256',
    'review_test_game_verifier_sha256',
    'review_test_game_source_sha256',
    'review_test_game_source_manifest_sha256',
    'prepared_runtime_sha256',
    'prepared_broker_sha256',
    'prepared_mouth_worker_sha256',
    'prepared_subtitle_presenter_sha256',
    'composite_sha256'
)) {
    if ($source.IndexOf($identityField, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Source identity omits $identityField."
    }
}
foreach ($supervisionField in @(
    'runtime_child_observed',
    'broker_child_observed',
    'runtime_supervision_observed',
    'broker_supervision_observed',
    'no_orphans_after_parent_termination',
    'parent_matches_shell',
    'path_matches_install_target',
    'hash_matches_installed_file'
)) {
    if ($source.IndexOf($supervisionField, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Supervised process evidence omits $supervisionField."
    }
}
foreach ($aclField in @('app_data_acl_private', 'protected_dacl', 'inherited_ace_count', 'unexpected_principal_names', 'unresolved-principal')) {
    if ($source.IndexOf($aclField, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Private app-data ACL evidence omits $aclField."
    }
}
foreach ($healthField in @(
    'NPC2_INSTALLER_SMOKE',
    'installer-smoke-health-v1.json',
    'app_health_probe_observed',
    'runtime_authenticated',
    'runtime_connected',
    'broker_authenticated',
    'broker_connected',
    'app_config_acl_private',
    'health_probe_removed'
)) {
    if ($source.IndexOf($healthField, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Installed app health evidence omits $healthField."
    }
}
foreach ($reviewField in @(
    'application_identifier',
    'production_application_identifier',
    'clean_first_run_namespace',
    'onboarding_auto_open_observed',
    'onboarding_dom_evidence',
    'review_test_game_observed',
    'review_test_game_stopped',
    'review_test_game_evidence',
    'installed_control_sha256',
    'control_hash_verified',
    'mouth_worker_hash_verified',
    'subtitle_presenter_hash_verified',
    'installed_distribution_reconciled',
    'installed_legal_resources_reconciled',
    'unclassified_installed_files_rejected',
    'reinstalled_distribution_reconciled',
    'installed_model_catalog_promotion_policy_verified',
    'reinstalled_model_catalog_promotion_policy_verified',
    'model_catalog_promotion_blockers',
    'Test-NpcModelCatalogPromotionPolicy',
    'promotion_eligible',
    'webview2_offline_installer_contract_verified',
    'MicrosoftEdgeWebView2RuntimeInstallerX64.exe',
    '987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61',
    'package_time_network_acquisition',
    'reconcile-installed-product.ps1',
    'reinstall_cycle_verified',
    'production_namespace_unchanged',
    'review_namespace_removed',
    'review_local_namespace_removed',
    'review_webview_processes_observed',
    'no_review_webview_orphans',
    'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS',
    'remote-debugging-port',
    'InstallerSmokeRoot'
)) {
    if ($source.IndexOf($reviewField, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Isolated review/onboarding evidence omits $reviewField."
    }
}
if ($source.IndexOf('$reviewIdentifier -ne "$productionIdentifier.review"', [System.StringComparison]::Ordinal) -lt 0) {
    throw 'Installer smoke does not fail closed outside the isolated .review identifier.'
}
if ($source -match 'Invoke-CapturedProcess\s+-FilePath\s+\(Join-Path\s+\$installRoot\s+''npc-media-broker\.exe''\)') {
    throw 'Persistent media broker must never be invoked outside the Tauri supervision context.'
}
if ($source.IndexOf('$($_.Length)', [System.StringComparison]::Ordinal) -ge 0) {
    throw 'Directory metadata identity must not read Length from DirectoryInfo under strict mode.'
}
if ($source.IndexOf('$manifest.control_executable.path', [System.StringComparison]::Ordinal) -ge 0) {
    throw 'Installer smoke output must not read a control-executable path that the package manifest does not declare.'
}
if ($source.IndexOf("`$modelManifest.schema -ne 'npc.model-pack/v2'", [System.StringComparison]::Ordinal) -lt 0) {
    throw 'Installer smoke must validate the distributed canonical npc.model-pack/v2 example.'
}
if ($source.IndexOf('--remote-debugging-address=', [System.StringComparison]::Ordinal) -ge 0) {
    throw 'WebView2 inspection must use the documented port-only flag; the address flag prevents CDP discovery on the supported runtime.'
}
if ($source.IndexOf('"--remote-debugging-port=$webviewDebugPort"', [System.StringComparison]::Ordinal) -lt 0) {
    throw 'WebView2 inspection must bind its bounded random loopback port through WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS.'
}
if ($source.IndexOf("document.querySelector('.onboarding')", [System.StringComparison]::Ordinal) -ge 0) {
    throw 'Installed onboarding evidence must not target the obsolete pre-2.0 onboarding selector.'
}
if ($source.IndexOf('document.querySelector(''.setup-dialog[role="dialog"][aria-modal="true"]'')', [System.StringComparison]::Ordinal) -lt 0) {
    throw 'Installed onboarding evidence must target the rendered 2.0 semantic setup dialog.'
}
$cleanWaitCount = ([regex]::Matches($source, '\bWait-InstallTargetsClean\b')).Count
if ($cleanWaitCount -lt 3) {
    throw 'Installer smoke must define and use bounded full-target cleanup waits for both uninstall cycles.'
}

$privacyHook = '$result.installed_privacy_capture_completed = $true'
$privacyHookIndex = $source.IndexOf($privacyHook, [System.StringComparison]::Ordinal)
$finallyIndex = $source.LastIndexOf("`nfinally {", [System.StringComparison]::Ordinal)
if ($privacyHookIndex -lt 0 -or $finallyIndex -lt 0 -or $privacyHookIndex -gt $finallyIndex) {
    throw 'Installed privacy capture must complete against the exact reinstalled candidate before cleanup begins.'
}
foreach ($privacyContract in @(
    'capture-installed-privacy-proof.ps1',
    'reinstalledDistributionManifestEvidencePath',
    'AcknowledgeElevatedOsDenyAll',
    'installed_privacy_capture_requested',
    'installed_privacy_capture_evidence_path'
)) {
    if ($source.IndexOf($privacyContract, [System.StringComparison]::Ordinal) -lt 0) {
        throw "Installer smoke omits privacy capture contract: $privacyContract"
    }
}
if ($source.IndexOf('$result.installed_privacy_capture_completed -and', [System.StringComparison]::Ordinal) -lt 0) {
    throw 'Promotion eligibility must require completed installed privacy capture.'
}

$pathComparisonFunctionAst = $scriptAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Convert-ToComparableWindowsPath'
}, $true)
$supervisionFunctionAst = $scriptAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Get-SupervisedChildEvidence'
}, $true)
if ($null -eq $pathComparisonFunctionAst -or $null -eq $supervisionFunctionAst) {
    throw 'Installer smoke does not define its supervised-process path identity functions.'
}
Invoke-Expression $pathComparisonFunctionAst.Extent.Text
Invoke-Expression $supervisionFunctionAst.Extent.Text
function Get-OptionalFileSha256 {
    param([string]$Path)
    return 'synthetic-identical-sha256'
}
$expectedExecutablePath = 'E:\temp\npc2-smoke\operation\app\npc-media-broker.exe'
$extendedExecutablePath = '\\?\' + $expectedExecutablePath
$extendedPathEvidence = Get-SupervisedChildEvidence `
    -Process ([pscustomobject]@{
        ProcessId = 200
        ParentProcessId = 100
        Name = 'npc-media-broker.exe'
        ExecutablePath = $extendedExecutablePath
    }) `
    -ExpectedParentId 100 `
    -ExpectedExecutablePath $expectedExecutablePath `
    -OperationId 'operation'
if (-not $extendedPathEvidence.path_matches_install_target) {
    throw 'Supervised process identity must treat the Windows extended-length \\?\ path as the installed executable.'
}

$registrySnapshotFunctionAst = $scriptAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Get-RegistryValueSnapshot'
}, $true)
if ($null -eq $registrySnapshotFunctionAst) {
    throw 'Installer smoke does not define Get-RegistryValueSnapshot.'
}
Invoke-Expression $registrySnapshotFunctionAst.Extent.Text
function Test-Path {
    param([string]$LiteralPath)
    return $true
}
function Get-Item {
    param([string]$LiteralPath, $ErrorAction)
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic uninstall deleted the key after inspection.')
}
try {
    $missingRegistryValue = Get-RegistryValueSnapshot -Path 'HKCU:\Software\SyntheticUninstallRace' -Name 'InstallLocation'
    if ($missingRegistryValue.exists -ne $false -or $null -ne $missingRegistryValue.value) {
        throw 'A registry key removed during asynchronous uninstall must be recorded as absent.'
    }
}
finally {
    Remove-Item Function:\Test-Path -Force
    Remove-Item Function:\Get-Item -Force
}

if (-not [string]::IsNullOrWhiteSpace($PreflightResultPath)) {
    $preflight = Get-Content -LiteralPath (Resolve-Path $PreflightResultPath) -Raw | ConvertFrom-Json
    if ($preflight.status -ne 'preflight_passed' -or $null -eq $preflight.pre_install_snapshot -or $null -ne $preflight.post_uninstall_snapshot) {
        throw 'Preflight result has an invalid evidence lifecycle.'
    }
    if ([string]::IsNullOrWhiteSpace($preflight.source_identity.git_head) -or
        [string]::IsNullOrWhiteSpace($preflight.source_identity.composite_sha256) -or
        $preflight.source_identity.profile_count -ne 21) {
        throw 'Preflight result has incomplete source identity.'
    }
}

Write-Host 'Installer smoke evidence ordering and source identity checks passed.' -ForegroundColor Green
exit 0
