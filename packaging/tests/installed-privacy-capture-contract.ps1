[CmdletBinding()]
param(
    [string]$CaptureScriptPath,
    [string]$ReceiptSourcePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
if ([string]::IsNullOrWhiteSpace($CaptureScriptPath)) {
    $CaptureScriptPath = Join-Path $PSScriptRoot '../../scripts/capture-installed-privacy-proof.ps1'
}
if ([string]::IsNullOrWhiteSpace($ReceiptSourcePath)) {
    $ReceiptSourcePath = Join-Path $PSScriptRoot '../../apps/control/src-tauri/src/packaged_privacy_receipt.rs'
}
$path = (Resolve-Path -LiteralPath $CaptureScriptPath).Path
$tokens = $null
$parseErrors = $null
[Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$parseErrors) | Out-Null
if ($parseErrors.Count -gt 0) { throw "privacy capture runner has $($parseErrors.Count) parse error(s)." }
$source = Get-Content -LiteralPath $path -Raw
foreach ($required in @(
    'AcknowledgeElevatedOsDenyAll',
    'WindowsBuiltInRole]::Administrator',
    'windows_defender_firewall_program_deny',
    'windows_security_filtering_platform',
    'New-NetFirewallRule',
    'Get-NetFirewallRule -PolicyStore ActiveStore',
    'Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule',
    'Get-NetFirewallProfile',
    'EventID=5152 or EventID=5157',
    'NPC2_PRIVACY_PROOF_RUN_ID',
    'NPC2_PRIVACY_PROOF_SCENARIO',
    'NPC2_PRIVACY_PROOF_RECEIPT_PATH',
    "source -ne 'packaged_executable'",
    'scenarioCompleted -ne $true',
    'Test-ExactZeroInteger $receipt.providerRequestsStarted',
    'Get-InstalledProcessObservations',
    'validate-installed-privacy-proof.ps1',
    '$validatorProcess = Invoke-NpcHiddenProcess',
    'Remove-NetFirewallRule',
    '$proofValidated = $true',
    'Installed privacy capture cleanup failed closed'
)) {
    if ($source.IndexOf($required, [StringComparison]::Ordinal) -lt 0) {
        throw "Installed privacy capture omits fail-closed contract: $required"
    }
}
$ruleIndex = $source.IndexOf('New-NetFirewallRule', [StringComparison]::Ordinal)
$scenarioIndex = $source.IndexOf("Invoke-InstalledScenario 'offline_mode'", [StringComparison]::Ordinal)
$proofIndex = $source.IndexOf('Write-JsonAtomic $proof $ProofPath', [StringComparison]::Ordinal)
$removeIndex = $source.IndexOf('Remove-NetFirewallRule', [StringComparison]::Ordinal)
if ($ruleIndex -lt 0 -or $scenarioIndex -le $ruleIndex -or $proofIndex -le $scenarioIndex -or $removeIndex -le $proofIndex) {
    throw 'OS rules, installed scenarios, proof write, and rule cleanup are not ordered safely.'
}
if ($source -match '(?i)fixture_harness|test_harness') {
    throw 'Installed privacy capture runner must not contain a fixture or test-harness success path.'
}

$receiptPath = (Resolve-Path -LiteralPath $ReceiptSourcePath).Path
$receiptSource = Get-Content -LiteralPath $receiptPath -Raw
foreach ($required in @(
    'if request.scenario == LOCAL_LIP_SYNC',
    'return false;',
    'provider_loadouts.review(LoadoutContextV1::global(), true, local_resources)',
    'review.network_request_performed',
    'EgressClassV1::None',
    'write_receipt(request)'
)) {
    if ($receiptSource.IndexOf($required, [StringComparison]::Ordinal) -lt 0) {
        throw "Native packaged privacy probe omits fail-closed route-truth contract: $required"
    }
}
$lipGuardIndex = $receiptSource.IndexOf('if request.scenario == LOCAL_LIP_SYNC', [StringComparison]::Ordinal)
$offlineReviewIndex = $receiptSource.IndexOf('provider_loadouts.review(LoadoutContextV1::global(), true, local_resources)', [StringComparison]::Ordinal)
$authorizeCallIndex = $receiptSource.IndexOf('if !probe_authorized(', [StringComparison]::Ordinal)
$writeIndex = $receiptSource.IndexOf('write_receipt(request)', [StringComparison]::Ordinal)
if ($lipGuardIndex -lt 0 -or $offlineReviewIndex -le $lipGuardIndex -or
    $authorizeCallIndex -lt 0 -or $writeIndex -le $authorizeCallIndex) {
    throw 'Native receipt authorization must gate every write, reject local lip-sync, and prove offline native route truth.'
}

Write-Host 'Installed privacy capture contract passed.' -ForegroundColor Green
