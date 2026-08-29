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
[System.Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "installer-smoke.ps1 has $($parseErrors.Count) parse error(s)." }

$source = Get-Content -LiteralPath $scriptPath -Raw
$snapshotMarker = '$postUninstallSnapshot = Get-InstallTargetSnapshot'
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
    'prepared_runtime_sha256',
    'prepared_broker_sha256',
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
if ($source -match 'Invoke-CapturedProcess\s+-FilePath\s+\(Join-Path\s+\$installRoot\s+''npc-media-broker\.exe''\)') {
    throw 'Persistent media broker must never be invoked outside the Tauri supervision context.'
}

if (-not [string]::IsNullOrWhiteSpace($PreflightResultPath)) {
    $preflight = Get-Content -LiteralPath (Resolve-Path $PreflightResultPath) -Raw | ConvertFrom-Json
    if ($preflight.status -ne 'preflight_passed' -or $null -eq $preflight.pre_install_snapshot -or $null -ne $preflight.post_uninstall_snapshot) {
        throw 'Preflight result has an invalid evidence lifecycle.'
    }
    if ([string]::IsNullOrWhiteSpace($preflight.source_identity.git_head) -or
        [string]::IsNullOrWhiteSpace($preflight.source_identity.composite_sha256) -or
        $preflight.source_identity.profile_count -ne 20) {
        throw 'Preflight result has incomplete source identity.'
    }
}

Write-Host 'Installer smoke evidence ordering and source identity checks passed.' -ForegroundColor Green
exit 0
