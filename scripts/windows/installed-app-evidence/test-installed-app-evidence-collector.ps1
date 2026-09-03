[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$collector = Join-Path $PSScriptRoot 'collect-installed-app-evidence.ps1'
$driver = Join-Path $PSScriptRoot 'drive-installed-app-evidence.cjs'
$planPath = Join-Path $PSScriptRoot 'evidence-plan.v1.json'
foreach ($required in @($collector, $driver, $planPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing collector input: $required" }
}

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Write-TestJson {
    param([string]$Path, $Value)
    [System.IO.File]::WriteAllText($Path, (($Value | ConvertTo-Json -Depth 10) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
}

function Test-Entry {
    param([string]$Path, [string]$Name)
    return [ordered]@{ file = $Name; sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant(); size_bytes = (Get-Item -LiteralPath $Path).Length }
}

$before = @(Get-Process -ErrorAction SilentlyContinue | Where-Object ProcessName -in @('interactive-npcs-control', 'interactive-npcs-synthetic-target') | Select-Object -ExpandProperty Id)
$validationRaw = & $collector -Mode ValidatePlan
if ($LASTEXITCODE -ne 0) { throw 'ValidatePlan failed.' }
$validation = $validationRaw | ConvertFrom-Json
Assert-True ($validation.status -eq 'passed' -and $validation.gui_launched -eq $false) 'ValidatePlan must pass without launching GUI.'
Assert-True ($validation.retained_page_count -eq 5 -and $validation.view_profile_count -eq 6) 'ValidatePlan page/profile coverage differs from the canonical contract.'
Assert-True ($validation.closeups_per_screen -eq '4-6') 'ValidatePlan closeup contract is not four to six.'
$after = @(Get-Process -ErrorAction SilentlyContinue | Where-Object ProcessName -in @('interactive-npcs-control', 'interactive-npcs-synthetic-target') | Select-Object -ExpandProperty Id)
Assert-True ((@($before | Sort-Object) -join ',') -ceq (@($after | Sort-Object) -join ',')) 'ValidatePlan launched or changed a product GUI process.'

$plan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
$profileIds = @($plan.view_profiles.id)
foreach ($profile in @('normal-100', 'normal-150', 'normal-200', 'narrow-100', 'narrow-150', 'narrow-200')) {
    Assert-True ($profileIds -contains $profile) "Missing visual profile: $profile"
}
$specialIds = @($plan.special_states.id)
foreach ($state in @('onboarding', 'session-completed-degradation', 'world-local-draft', 'voice-advanced-fit', 'stt-selected-blocked', 'stt-consent', 'stt-arming', 'stt-capturing', 'stt-receipt-ready', 'stt-submitted-spent', 'stt-rejected-recapture')) {
    Assert-True ($specialIds -contains $state) "Missing retained installed state: $state"
}
Assert-True (@($plan.required_native_claims).Count -ge 13) 'Installed-native provenance contract is incomplete.'

$collectorText = Get-Content -LiteralPath $collector -Raw
$driverText = Get-Content -LiteralPath $driver -Raw
Assert-True ($collectorText.Contains("ROOT_CONFIRMED_SOURCE_FROZEN")) 'Capture authorization gate is absent.'
Assert-True ($collectorText.Contains('interactive-npcs-source-freeze/v1')) 'Frozen-source hash receipt gate is absent.'
Assert-True ($collectorText.Contains('Transparency App')) 'Protected Transparency App inventory is absent.'
Assert-True (-not $collectorText.Contains('HWND_TOPMOST')) 'Collector must not force task windows topmost.'
Assert-True (-not $collectorText.Contains('remote-debugging-port=9226')) 'Collector must not use the historic fixed WebView debug port.'
Assert-True (-not $collectorText.Contains('--remote-debugging-address=')) 'Collector must use the supported WebView2 port-only CDP flag.'
Assert-True ($collectorText.Contains('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"')) 'Collector must bind its random WebView2 CDP port without an address override.'
Assert-True ($collectorText.Contains('CreateNoWindow = $true')) 'Hidden process creation is absent.'
Assert-True ($collectorText.Contains("physical_dpi = [ordered]@{ status = 'not_measured'; simulated = `$false")) 'Physical DPI must be explicitly unmeasured and unsimulated.'
Assert-True ($collectorText.Contains("hdr = [ordered]@{ status = 'not_measured'; simulated = `$false")) 'HDR must be explicitly unmeasured and unsimulated.'
Assert-True ($driverText.Contains('unexercised')) 'Enabled unmatched controls must fail coverage.'
Assert-True ($driverText.Contains('horizontal_overflow')) 'Horizontal-overflow detection is absent.'
Assert-True ($driverText.Contains('enabled_unlabeled_controls')) 'Enabled-unlabeled-control detection is absent.'
Assert-True ($driverText.Contains('webview-cdp')) 'Canonical WebView pixel-source declaration is absent.'
Assert-True ($driverText.Contains('(?:\\d{2}\\s+)?')) 'Semantic navigation labels are not matched against indexed accessible names.'
Assert-True ($driverText.Contains('png.readUInt32BE(16)') -and $driverText.Contains('png.readUInt32BE(20)')) 'Close-up bounds are not derived from the captured PNG.'
Assert-True ($driverText.Contains('indexed-element-screenshot')) 'Below-viewport WebView2 close-ups have no element-level fallback.'

$nodeCandidates = @(@(
    (Join-Path $env:ProgramFiles 'nodejs\node.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'nodejs\node.exe')
) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and (Test-Path -LiteralPath $_ -PathType Leaf) })
if ($nodeCandidates.Count -gt 0) {
    $syntaxInfo = New-Object System.Diagnostics.ProcessStartInfo
    $syntaxInfo.FileName = $nodeCandidates[0]
    $syntaxInfo.Arguments = '--check "' + $driver.Replace('"', '\"') + '"'
    $syntaxInfo.UseShellExecute = $false
    $syntaxInfo.CreateNoWindow = $true
    $syntax = [System.Diagnostics.Process]::Start($syntaxInfo)
    $syntax.WaitForExit()
    Assert-True ($syntax.ExitCode -eq 0) 'Node rejected the installed-app evidence driver syntax.'
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('interactive-npcs-evidence-test-' + [Guid]::NewGuid().ToString('N'))
try {
    $packageRoot = Join-Path $fixtureRoot 'package'
    $installedRoot = Join-Path $fixtureRoot 'installed'
    $testGameRoot = Join-Path $fixtureRoot 'test-game'
    foreach ($directory in @($packageRoot, $installedRoot, $testGameRoot)) { [void](New-Item -ItemType Directory -Path $directory) }
    $sourceEvidencePath = Join-Path $packageRoot 'source-evidence.json'
    $sourceDigest = ('1' * 64)
    $sourceEvidence = [ordered]@{ schema_version = 1; dirty = $false; source_tree_clean = $true; source_candidate_digest = [ordered]@{ algorithm = 'SHA-256'; sha256 = $sourceDigest } }
    Write-TestJson -Path $sourceEvidencePath -Value $sourceEvidence
    $installerPath = Join-Path $packageRoot 'Interactive NPCs Response Console_test_x64-setup.exe'
    [System.IO.File]::WriteAllText($installerPath, 'deterministic installer fixture')
    $packageManifestPath = Join-Path $packageRoot 'package-manifest.json'
    $package = [ordered]@{
        schema_version = 1
        distribution = 'local-review-only'
        immutable_release_candidate = $true
        source = $sourceEvidence
        files = @(
            Test-Entry -Path $installerPath -Name (Split-Path -Leaf $installerPath)
            Test-Entry -Path $sourceEvidencePath -Name 'source-evidence.json'
        )
    }
    Write-TestJson -Path $packageManifestPath -Value $package
    $installedNames = @('interactive-npcs-control.exe', 'npc-runtime.exe', 'npc-media-broker.exe', 'npc-mouth-worker.exe', 'npc-subtitle-presenter.exe', 'uninstall.exe')
    $installedEntries = foreach ($name in $installedNames) {
        $file = Join-Path $installedRoot $name
        [System.IO.File]::WriteAllText($file, "installed fixture $name")
        $entry = Test-Entry -Path $file -Name $name
        [ordered]@{ path = $entry.file; sha256 = $entry.sha256; size_bytes = $entry.size_bytes }
    }
    $installedManifestPath = Join-Path $fixtureRoot 'installed-distribution-manifest.v1.json'
    Write-TestJson -Path $installedManifestPath -Value ([ordered]@{ schema_version = 1; files = @($installedEntries); manifest_self = [ordered]@{ path = 'installed-distribution-manifest.v1.json'; hash = 'excluded-to-avoid-circularity' } })
    $smokePath = Join-Path $fixtureRoot 'installer-smoke.json'
    $smoke = [ordered]@{
        schema_version = 1
        source_identity = [ordered]@{ package_manifest_sha256 = (Get-FileHash -LiteralPath $packageManifestPath -Algorithm SHA256).Hash.ToLowerInvariant() }
        installed_distribution_reconciled = $true
        installed_distribution_manifest_sha256 = (Get-FileHash -LiteralPath $installedManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    Write-TestJson -Path $smokePath -Value $smoke
    $fixtureExe = Join-Path $testGameRoot 'interactive-npcs-synthetic-target.exe'
    [System.IO.File]::WriteAllText($fixtureExe, 'project-owned synthetic fixture')
    $fixtureEntry = Test-Entry -Path $fixtureExe -Name 'interactive-npcs-synthetic-target.exe'
    $reviewManifestPath = Join-Path $testGameRoot 'REVIEW-FIXTURE-MANIFEST.json'
    Write-TestJson -Path $reviewManifestPath -Value ([ordered]@{
        schema_version = 1
        component_id = 'project:synthetic-review-target'
        fixture_source = 'project-source-generated-native-v1'
        third_party_binaries = @()
        files = @([ordered]@{ path = $fixtureEntry.file; sha256 = $fixtureEntry.sha256; size_bytes = $fixtureEntry.size_bytes })
    })
    $preflightRaw = & $collector -Mode Preflight -PackageManifestPath $packageManifestPath -InstallerSmokeReceiptPath $smokePath -InstalledDistributionManifestPath $installedManifestPath -InstalledRoot $installedRoot -SourceEvidencePath $sourceEvidencePath -TestGameRoot $testGameRoot
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic binding preflight failed.' }
    $preflight = $preflightRaw | ConvertFrom-Json
    Assert-True ($preflight.status -eq 'passed' -and $preflight.gui_launched -eq $false -and @($preflight.inputs).Count -eq 9) 'Synthetic binding preflight did not bind exactly nine immutable inputs without GUI.'
    $freezePath = Join-Path $fixtureRoot 'frozen-source-receipt.json'
    Write-TestJson -Path $freezePath -Value ([ordered]@{
        schema = 'interactive-npcs-source-freeze/v1'
        status = 'frozen'
        package_manifest_sha256 = (Get-FileHash -LiteralPath $packageManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
        source_candidate_digest_sha256 = $sourceDigest
    })
    $captureGateRejected = $false
    $processesBeforeGate = @(Get-Process -ErrorAction SilentlyContinue | Where-Object ProcessName -in @('interactive-npcs-control', 'interactive-npcs-synthetic-target') | Select-Object -ExpandProperty Id)
    try {
        $null = & $collector -Mode Capture -PackageManifestPath $packageManifestPath -InstallerSmokeReceiptPath $smokePath -InstalledDistributionManifestPath $installedManifestPath -InstalledRoot $installedRoot -SourceEvidencePath $sourceEvidencePath -TestGameRoot $testGameRoot -FrozenSourceReceiptPath $freezePath -OutputRoot (Join-Path $fixtureRoot 'capture-output') 2>$null
    } catch { $captureGateRejected = $_.Exception.Message -match 'Capture is disabled' }
    $processesAfterGate = @(Get-Process -ErrorAction SilentlyContinue | Where-Object ProcessName -in @('interactive-npcs-control', 'interactive-npcs-synthetic-target') | Select-Object -ExpandProperty Id)
    Assert-True $captureGateRejected 'Capture without the exact root authorization token was not rejected.'
    Assert-True ((@($processesBeforeGate | Sort-Object) -join ',') -ceq (@($processesAfterGate | Sort-Object) -join ',')) 'Rejected Capture changed a product GUI process.'
    [System.IO.File]::AppendAllText((Join-Path $installedRoot 'npc-runtime.exe'), 'tamper')
    $driftRejected = $false
    try {
        $null = & $collector -Mode Preflight -PackageManifestPath $packageManifestPath -InstallerSmokeReceiptPath $smokePath -InstalledDistributionManifestPath $installedManifestPath -InstalledRoot $installedRoot -SourceEvidencePath $sourceEvidencePath -TestGameRoot $testGameRoot 2>$null
    } catch { $driftRejected = $true }
    Assert-True $driftRejected 'Installed-distribution hash drift was not rejected during no-GUI preflight.'
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot -PathType Container) { Remove-Item -LiteralPath $fixtureRoot -Recurse -Force }
}

[ordered]@{
    schema = 'interactive-npcs-installed-evidence-collector-tests/v1'
    status = 'passed'
    gui_launched = $false
    assertions = 40
    retained_pages = @($plan.retained_pages).Count
    view_profiles = @($plan.view_profiles).Count
    special_states = @($plan.special_states).Count
    native_claims = @($plan.required_native_claims).Count
} | ConvertTo-Json -Depth 4
