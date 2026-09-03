[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [Parameter(Mandatory = $true)][string]$PackageManifestPath,
    [Parameter(Mandatory = $true)][string]$PackagePath,
    [Parameter(Mandatory = $true)][string]$InstalledManifestPath,
    [Parameter(Mandatory = $true)][string]$InstalledExecutablePath,
    [Parameter(Mandatory = $true)][string]$TelemetryInventoryPath,
    [Parameter(Mandatory = $true)][string]$ReleaseCandidateId,
    [Parameter(Mandatory = $true)][string]$ApplicationVersion,
    [Parameter(Mandatory = $true)][string]$ProofPath,
    [Parameter(Mandatory = $true)][string]$CaptureEvidencePath,
    [switch]$AcknowledgeElevatedOsDenyAll,
    [ValidateRange(5, 120)][int]$ScenarioTimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

# This script is deliberately post-package only. It does not build, install,
# sign, or promote anything. The installed candidate must emit a fresh native
# receipt for each requested scenario; a fixture file or test-harness receipt
# is rejected before a proof is written.
$requiredFiles = @(
    $PackageManifestPath,
    $PackagePath,
    $InstalledManifestPath,
    $InstalledExecutablePath
)
foreach ($candidatePath in $requiredFiles) {
    if (-not (Test-Path -LiteralPath $candidatePath -PathType Leaf)) {
        throw "Installed privacy capture requires an existing candidate artifact: $candidatePath"
    }
}
if (Test-Path -LiteralPath $ProofPath -or Test-Path -LiteralPath $CaptureEvidencePath) {
    throw 'Privacy capture refuses to overwrite an existing proof or capture.'
}
if (-not $AcknowledgeElevatedOsDenyAll) {
    throw 'OS deny-all capture requires -AcknowledgeElevatedOsDenyAll.'
}
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'OS deny-all capture requires an already-elevated Windows host.'
}
if ($ReleaseCandidateId -notmatch '^[A-Za-z0-9._:/-]{1,128}$') {
    throw 'ReleaseCandidateId is invalid.'
}
if ([string]::IsNullOrWhiteSpace($ApplicationVersion) -or $ApplicationVersion.Length -gt 64) {
    throw 'ApplicationVersion is invalid.'
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-JsonAtomic([object]$Value, [string]$Path, [int]$Depth = 12) {
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $parent = Split-Path -Parent $fullPath
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $temporary = Join-Path $parent ('.' + [System.IO.Path]::GetFileName($fullPath) + '.' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        $json = ($Value | ConvertTo-Json -Depth $Depth) + [Environment]::NewLine
        [System.IO.File]::WriteAllText($temporary, $json, (New-Object Text.UTF8Encoding($false)))
        Move-Item -LiteralPath $temporary -Destination $fullPath -ErrorAction Stop
    }
    finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Assert-ExactFields([object]$Value, [string[]]$Expected, [string]$Label) {
    $actual = @($Value.PSObject.Properties.Name | Sort-Object)
    $expectedSorted = @($Expected | Sort-Object)
    if (($actual -join "`n") -cne ($expectedSorted -join "`n")) {
        throw "$Label differs from its closed schema."
    }
}

function Test-ExactZeroInteger([object]$Value) {
    return ($Value -is [byte] -or $Value -is [int16] -or $Value -is [int32] -or $Value -is [int64] -or
        $Value -is [uint16] -or $Value -is [uint32] -or $Value -is [uint64]) -and [int64]$Value -eq 0
}

function Convert-EventProcessId([string]$Value) {
    if ($Value -match '^0[xX][0-9a-fA-F]+$') {
        return [int][Convert]::ToUInt32($Value.Substring(2), 16)
    }
    $parsed = 0
    if (-not [int]::TryParse($Value, [ref]$parsed)) { return -1 }
    return $parsed
}

function Get-LatestSecurityRecordId {
    $event = Get-WinEvent -LogName Security -MaxEvents 1 -ErrorAction Stop
    if ($null -eq $event -or [long]$event.RecordId -le 0) {
        throw 'Windows Security event-log record identity is unavailable.'
    }
    return [long]$event.RecordId
}

function Get-InstalledProcessObservations([int]$RootProcessId, [string]$InstallRoot) {
    $all = @(Get-CimInstance Win32_Process -ErrorAction Stop)
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'
    $null = $ids.Add($RootProcessId)
    do {
        $added = $false
        foreach ($process in $all) {
            if ($ids.Contains([int]$process.ParentProcessId) -and $ids.Add([int]$process.ProcessId)) {
                $added = $true
            }
        }
    } while ($added)
    $root = [System.IO.Path]::GetFullPath($InstallRoot).TrimEnd('\') + '\'
    $observations = @()
    foreach ($process in $all | Where-Object { $ids.Contains([int]$_.ProcessId) }) {
        if ([string]::IsNullOrWhiteSpace([string]$process.ExecutablePath)) { continue }
        $path = [System.IO.Path]::GetFullPath([string]$process.ExecutablePath)
        if (-not $path.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) { continue }
        $observations += [ordered]@{
            imageName = [System.IO.Path]::GetFileName($path)
            sha256 = Get-Sha256 $path
            processId = [int]$process.ProcessId
            parentProcessId = [int]$process.ParentProcessId
        }
    }
    if ($observations.Count -eq 0) { throw 'No installed candidate process was observable.' }
    return @($observations | Sort-Object processId)
}

function Read-WfpCounts([long]$FirstRecordId, [long]$LastRecordId, [int[]]$ProcessIds) {
    $processSet = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($processId in $ProcessIds) { $null = $processSet.Add([string]$processId) }
    $external = 0
    $loopback = 0
    $destinations = New-Object 'System.Collections.Generic.HashSet[string]'
    $filter = "*[System[(EventID=5152 or EventID=5157) and EventRecordID >= $FirstRecordId and EventRecordID <= $LastRecordId]]"
    foreach ($event in @(Get-WinEvent -LogName Security -FilterXPath $filter -ErrorAction Stop)) {
        [xml]$xml = $event.ToXml()
        $values = @{}
        foreach ($data in $xml.Event.EventData.Data) { $values[[string]$data.Name] = [string]$data.'#text' }
        if (-not $processSet.Contains([string](Convert-EventProcessId ([string]$values.ProcessID)))) { continue }
        $address = [string]$values.DestAddress
        if ($address -in @('127.0.0.1', '::1') -or $address.StartsWith('127.')) {
            $loopback += 1
        } else {
            $external += 1
            if (-not [string]::IsNullOrWhiteSpace($address)) { $null = $destinations.Add($address) }
        }
    }
    return [pscustomobject]@{
        external = $external
        loopback = $loopback
        destinations = $destinations.Count
    }
}

function Stop-CandidateTree([Diagnostics.Process]$Process) {
    if ($null -eq $Process) { return }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force -ErrorAction Stop }
    } catch { }
    try { $Process.WaitForExit(5000) | Out-Null } catch { }
}

function Invoke-InstalledScenario(
    [string]$Scenario,
    [string]$Executable,
    [string]$InstallRoot,
    [string]$ReceiptDirectory,
    [int]$TimeoutSeconds
) {
    $runId = 'privacy-' + $Scenario.Replace('_', '-') + '-' + [Guid]::NewGuid().ToString('N')
    $receiptPath = Join-Path $ReceiptDirectory ($runId + '.json')
    if (Test-Path -LiteralPath $receiptPath) { throw 'Fresh receipt path unexpectedly exists.' }
    $startUtc = [DateTime]::UtcNow
    $firstRecord = Get-LatestSecurityRecordId
    $previousRun = $env:NPC2_PRIVACY_PROOF_RUN_ID
    $previousScenario = $env:NPC2_PRIVACY_PROOF_SCENARIO
    $previousReceipt = $env:NPC2_PRIVACY_PROOF_RECEIPT_PATH
    $process = $null
    try {
        $env:NPC2_PRIVACY_PROOF_RUN_ID = $runId
        $env:NPC2_PRIVACY_PROOF_SCENARIO = $Scenario
        $env:NPC2_PRIVACY_PROOF_RECEIPT_PATH = $receiptPath
        $process = Start-Process -FilePath $Executable -WorkingDirectory $InstallRoot -WindowStyle Hidden -PassThru
    }
    finally {
        if ($null -eq $previousRun) { Remove-Item Env:\NPC2_PRIVACY_PROOF_RUN_ID -ErrorAction SilentlyContinue } else { $env:NPC2_PRIVACY_PROOF_RUN_ID = $previousRun }
        if ($null -eq $previousScenario) { Remove-Item Env:\NPC2_PRIVACY_PROOF_SCENARIO -ErrorAction SilentlyContinue } else { $env:NPC2_PRIVACY_PROOF_SCENARIO = $previousScenario }
        if ($null -eq $previousReceipt) { Remove-Item Env:\NPC2_PRIVACY_PROOF_RECEIPT_PATH -ErrorAction SilentlyContinue } else { $env:NPC2_PRIVACY_PROOF_RECEIPT_PATH = $previousReceipt }
    }
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        while (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf) -and [DateTime]::UtcNow -lt $deadline) {
            $process.Refresh()
            if ($process.HasExited) { throw "Installed candidate exited before $Scenario receipt creation." }
            Start-Sleep -Milliseconds 100
        }
        if (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
            throw "Installed candidate did not emit the required $Scenario native receipt."
        }
        $receiptFile = Get-Item -LiteralPath $receiptPath
        if ($receiptFile.Length -gt 65536 -or $receiptFile.LastWriteTimeUtc -lt $startUtc) {
            throw 'Native scenario receipt is stale or exceeds its bound.'
        }
        $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
        Assert-ExactFields $receipt @('schemaVersion','source','runId','scenario','processId','executableSha256','observedAtUtc','scenarioCompleted','providerRequestsStarted') 'native scenario receipt'
        if ($receipt.schemaVersion -ne '1.0.0' -or $receipt.source -ne 'packaged_executable' -or
            $receipt.runId -cne $runId -or $receipt.scenario -cne $Scenario -or
            [int]$receipt.processId -ne $process.Id -or $receipt.executableSha256 -cne (Get-Sha256 $Executable) -or
            $receipt.scenarioCompleted -ne $true -or -not (Test-ExactZeroInteger $receipt.providerRequestsStarted) -or
            (($receipt.processId -isnot [int]) -and ($receipt.processId -isnot [long]))) {
            throw 'Fixture, test-harness, stale, unbound, or incomplete scenario receipt was rejected.'
        }
        $processes = @(Get-InstalledProcessObservations $process.Id $InstallRoot)
        # WFP Security events are asynchronous. This bounded flush wait occurs
        # while deny rules remain effective and before the final record ID.
        Start-Sleep -Seconds 1
        $lastRecord = Get-LatestSecurityRecordId
        $wfp = Read-WfpCounts $firstRecord $lastRecord @($processes | ForEach-Object { [int]$_.processId })
        $tcpLoopback = @(Get-NetTCPConnection -ErrorAction SilentlyContinue | Where-Object {
            $_.OwningProcess -in @($processes.processId) -and ($_.RemoteAddress -eq '::1' -or $_.RemoteAddress.StartsWith('127.'))
        }).Count
        return [ordered]@{
            scenario = $Scenario
            runId = $runId
            receiptSource = 'packaged_executable'
            scenarioCompleted = $true
            observedAtUtc = [string]$receipt.observedAtUtc
            monitoredProcesses = $processes
            securityEventRecordStart = $firstRecord
            securityEventRecordEnd = $lastRecord
            observedConnectionAttempts = [int64]$wfp.external
            observedProviderRequests = [int64]$receipt.providerRequestsStarted
            externalDestinationCount = [int64]$wfp.destinations
            loopbackConnectionCount = [int64]($wfp.loopback + $tcpLoopback)
        }
    }
    finally {
        Stop-CandidateTree $process
    }
}

$packageManifestFull = (Resolve-Path -LiteralPath $PackageManifestPath).Path
$packageFull = (Resolve-Path -LiteralPath $PackagePath).Path
$installedManifestFull = (Resolve-Path -LiteralPath $InstalledManifestPath).Path
$executableFull = (Resolve-Path -LiteralPath $InstalledExecutablePath).Path
$installRoot = Split-Path -Parent $executableFull
$repositoryRoot = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path -LiteralPath $TelemetryInventoryPath -PathType Leaf)) {
    $python = Get-Command python -ErrorAction SilentlyContinue
    if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
    if ($null -eq $python) { throw 'Python 3 is required for packaged telemetry inventory.' }
    $pythonPath = if ($python -is [IO.FileInfo]) { $python.FullName } else { $python.Source }
    $inventoryProcess = Invoke-NpcHiddenProcess -FilePath $pythonPath -ArgumentList @(
        (Join-Path $PSScriptRoot 'security/measure_packaged_telemetry_inventory.py'),
        '--repository-root', $repositoryRoot,
        '--package-manifest', $packageManifestFull,
        '--installed-manifest', $installedManifestFull,
        '--install-root', $installRoot,
        '--output', ([System.IO.Path]::GetFullPath($TelemetryInventoryPath))
    ) -NoReplayOutput
    if ($inventoryProcess.ExitCode -ne 0) { throw 'Packaged telemetry inventory did not prove absence.' }
}
$packageManifest = Get-Content -LiteralPath $packageManifestFull -Raw | ConvertFrom-Json
$installedManifest = Get-Content -LiteralPath $installedManifestFull -Raw | ConvertFrom-Json
$inventory = Get-Content -LiteralPath (Resolve-Path -LiteralPath $TelemetryInventoryPath).Path -Raw | ConvertFrom-Json
Assert-ExactFields $inventory @('schemaVersion','provenance','evidenceSource','packageManifestSha256','installedDistributionManifestSha256','dependencyInventoryScanned','endpointInventoryScanned','automaticUploadEntryPoints','remoteTelemetryDestinations') 'telemetry inventory'
$packageManifestSha = Get-Sha256 $packageManifestFull
$packageSha = Get-Sha256 $packageFull
$installedManifestSha = Get-Sha256 $installedManifestFull
$executableSha = Get-Sha256 $executableFull
if ($packageManifest.schema_version -ne 1 -or $packageManifest.distribution -ne 'local-review-only' -or
    $installedManifest.schema_version -ne 1 -or $inventory.schemaVersion -ne '1.0.0' -or
    $inventory.provenance -ne 'measured' -or $inventory.evidenceSource -ne 'packaged_binary_inventory' -or
    $inventory.packageManifestSha256 -cne $packageManifestSha -or
    $inventory.installedDistributionManifestSha256 -cne $installedManifestSha -or
    $inventory.dependencyInventoryScanned -ne $true -or $inventory.endpointInventoryScanned -ne $true -or
    -not (Test-ExactZeroInteger $inventory.automaticUploadEntryPoints) -or
    -not (Test-ExactZeroInteger $inventory.remoteTelemetryDestinations)) {
    throw 'Fixture or incomplete telemetry inventory cannot authorize privacy proof capture.'
}

# Filtering Platform Connection: failure auditing must already be enabled. The
# runner will not silently weaken or mutate machine-wide audit policy.
$auditGuid = '{0CCE9226-69AE-11D9-BED3-505054503030}'
$auditCsv = (& "$env:SystemRoot\System32\auditpol.exe" /get /subcategory:$auditGuid /r | Out-String)
if ($LASTEXITCODE -ne 0 -or $auditCsv -notmatch '(?i)failure') {
    throw 'Windows Filtering Platform failure auditing must be enabled before capture.'
}

$installedExePaths = @()
$rootPrefix = [System.IO.Path]::GetFullPath($installRoot).TrimEnd('\') + '\'
foreach ($row in @($installedManifest.files)) {
    if ([string]::IsNullOrWhiteSpace([string]$row.path) -or [string]$row.path -notmatch '(?i)\.exe$') { continue }
    $path = [System.IO.Path]::GetFullPath((Join-Path $installRoot ([string]$row.path)))
    if (-not $path.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw 'Installed executable manifest entry escapes or is absent from the installed candidate.'
    }
    if ((Get-Sha256 $path) -cne [string]$row.sha256) { throw 'Installed executable differs from reconciliation evidence.' }
    $installedExePaths += $path
}
if ($installedExePaths.Count -eq 0 -or $executableFull -notin $installedExePaths) {
    throw 'Installed manifest does not bind the candidate executable.'
}

$groupId = 'npc-privacy-' + [Guid]::NewGuid().ToString('N')
$receiptRoot = Join-Path ([System.IO.Path]::GetTempPath()) $groupId
$rules = @()
$proofValidated = $false
try {
    New-Item -ItemType Directory -Path $receiptRoot -ErrorAction Stop | Out-Null
    if (-not $PSCmdlet.ShouldProcess($groupId, 'apply temporary per-program Windows Defender Firewall outbound deny rules and run installed privacy scenarios')) {
        throw 'Installed privacy capture was declined.'
    }
    foreach ($path in $installedExePaths) {
        $name = $groupId + '-' + [Guid]::NewGuid().ToString('N')
        $rules += New-NetFirewallRule -Name $name -DisplayName $name -Group $groupId -Direction Outbound -Action Block -Enabled True -Profile Any -Program $path -Protocol Any -PolicyStore PersistentStore -ErrorAction Stop
    }
    $effective = @(Get-NetFirewallRule -PolicyStore ActiveStore -Group $groupId -ErrorAction Stop | Where-Object {
        $_.Enabled -eq 'True' -and $_.Direction -eq 'Outbound' -and $_.Action -eq 'Block'
    })
    if ($effective.Count -ne $installedExePaths.Count) { throw 'Effective OS deny-all rule count differs from the installed executable set.' }
    if (@(Get-NetFirewallProfile -ErrorAction Stop | Where-Object { $_.Enabled -ne $true }).Count -ne 0) {
        throw 'Every Windows Defender Firewall profile must be enabled for deny-all capture.'
    }
    $effectivePrograms = @($effective | ForEach-Object {
        (Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $_ -ErrorAction Stop).Program
    })
    foreach ($path in $installedExePaths) {
        if (@($effectivePrograms | Where-Object { $_ -ieq $path }).Count -ne 1) {
            throw 'Effective OS deny-all application binding differs from the installed executable set.'
        }
    }

    $scenarios = @(
        Invoke-InstalledScenario 'offline_mode' $executableFull $installRoot $receiptRoot $ScenarioTimeoutSeconds
        Invoke-InstalledScenario 'local_lip_sync' $executableFull $installRoot $receiptRoot $ScenarioTimeoutSeconds
    )
    foreach ($scenario in $scenarios) {
        if ($scenario.observedConnectionAttempts -ne 0 -or $scenario.observedProviderRequests -ne 0 -or $scenario.externalDestinationCount -ne 0) {
            throw "OS deny-all scenario $($scenario.scenario) observed an egress attempt."
        }
    }
    $capture = [ordered]@{
        schemaVersion = '1.0.0'
        provenance = 'measured'
        evidenceSource = 'packaged_executable_observation'
        releaseCandidateId = $ReleaseCandidateId
        packageManifestSha256 = $packageManifestSha
        packageSha256 = $packageSha
        installedDistributionManifestSha256 = $installedManifestSha
        executableSha256 = $executableSha
        platform = 'windows'
        enforcementMechanism = 'windows_defender_firewall_program_deny'
        auditSource = 'windows_security_filtering_platform'
        firewallRuleGroupId = $groupId
        firewallRuleCount = $effective.Count
        auditPolicyVerified = $true
        telemetryInventory = [ordered]@{
            provenance = 'measured'
            evidenceSource = 'packaged_binary_inventory'
            dependencyInventoryScanned = $true
            endpointInventoryScanned = $true
            automaticUploadEntryPoints = 0
            remoteTelemetryDestinations = 0
        }
        scenarios = $scenarios
    }
    Write-JsonAtomic $capture $CaptureEvidencePath
    $captureSha = Get-Sha256 ([System.IO.Path]::GetFullPath($CaptureEvidencePath))
    $artifact = [ordered]@{
        applicationVersion = $ApplicationVersion
        releaseCandidateId = $ReleaseCandidateId
        packageManifestSha256 = $packageManifestSha
        packageSha256 = $packageSha
        installedDistributionManifestSha256 = $installedManifestSha
        executableSha256 = $executableSha
    }
    $header = [ordered]@{
        schemaVersion = '1.0.0'; artifact = $artifact; provenance = 'measured'
        evidenceSource = 'packaged_executable_observation'; observedAtUtc = [DateTime]::UtcNow.ToString('o').Replace('+00:00','Z'); outcome = 'passed'
    }
    $proofRows = foreach ($scenario in $scenarios) {
        [ordered]@{
            schemaVersion = $header.schemaVersion; artifact = $artifact; scenario = $scenario.scenario
            provenance = $header.provenance; evidenceSource = $header.evidenceSource; evidenceRunId = $scenario.runId
            observedAtUtc = $scenario.observedAtUtc; enforcement = 'os_deny_all'
            monitoredProcessCount = @($scenario.monitoredProcesses).Count
            observedConnectionAttempts = $scenario.observedConnectionAttempts
            observedProviderRequests = $scenario.observedProviderRequests
            externalDestinationCount = $scenario.externalDestinationCount; outcome = 'passed'
        }
    }
    $proof = [ordered]@{
        schemaVersion = '1.0.0'
        captureEvidenceSha256 = $captureSha
        remoteTelemetryAbsence = [ordered]@{
            schemaVersion = $header.schemaVersion; artifact = $artifact; provenance = $header.provenance
            evidenceSource = $header.evidenceSource; evidenceRunId = 'privacy-telemetry-' + [Guid]::NewGuid().ToString('N')
            observedAtUtc = $header.observedAtUtc; dependencyInventoryScanned = $true; endpointInventoryScanned = $true
            automaticUploadEntryPoints = 0; remoteTelemetryDestinations = 0; outcome = 'passed'
        }
        denyAllEgress = @($proofRows)
    }
    Write-JsonAtomic $proof $ProofPath
    $powershellPath = (Get-Command powershell.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $validatorProcess = Invoke-NpcHiddenProcess -FilePath $powershellPath -ArgumentList @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden',
        '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'validate-installed-privacy-proof.ps1'),
        '-ProofPath', ([System.IO.Path]::GetFullPath($ProofPath)),
        '-CaptureEvidencePath', ([System.IO.Path]::GetFullPath($CaptureEvidencePath)),
        '-PackageManifestPath', $packageManifestFull,
        '-PackagePath', $packageFull,
        '-InstalledManifestPath', $installedManifestFull,
        '-InstalledExecutablePath', $executableFull,
        '-ReleaseCandidateId', $ReleaseCandidateId
    ) -WorkingDirectory $repositoryRoot -TimeoutSeconds 120
    if ($validatorProcess.TimedOut -or $validatorProcess.ExitCode -ne 0) {
        throw 'Installed privacy proof failed independent schema and artifact validation.'
    }
    $proofValidated = $true
}
finally {
    $cleanupErrors = @()
    foreach ($rule in $rules) {
        try { Remove-NetFirewallRule -Name $rule.Name -PolicyStore PersistentStore -ErrorAction Stop }
        catch { $cleanupErrors += "firewall rule $($rule.Name): $($_.Exception.Message)" }
    }
    try {
        $remainingRules = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop | Where-Object {
            $_.Group -ceq $groupId
        })
        if ($remainingRules.Count -ne 0) {
            $cleanupErrors += "$($remainingRules.Count) task-owned firewall rule(s) remain"
        }
    }
    catch { $cleanupErrors += "firewall cleanup verification: $($_.Exception.Message)" }
    if (Test-Path -LiteralPath $receiptRoot) {
        try { Remove-Item -LiteralPath $receiptRoot -Recurse -Force -ErrorAction Stop }
        catch { $cleanupErrors += "receipt directory: $($_.Exception.Message)" }
    }

    # Both output paths were proven absent before capture. Remove only those
    # task-created files when validation or mandatory firewall cleanup fails so
    # a partial JSON document cannot be mistaken for retained proof.
    if (-not $proofValidated -or $cleanupErrors.Count -ne 0) {
        foreach ($createdPath in @($ProofPath, $CaptureEvidencePath)) {
            if (Test-Path -LiteralPath $createdPath) {
                try { Remove-Item -LiteralPath $createdPath -Force -ErrorAction Stop }
                catch { $cleanupErrors += "invalid output $createdPath`: $($_.Exception.Message)" }
            }
        }
    }
    if ($cleanupErrors.Count -ne 0) {
        throw ('Installed privacy capture cleanup failed closed: ' + ($cleanupErrors -join '; '))
    }
}

[ordered]@{
    schema_version = 1
    status = 'passed'
    proof_path = [System.IO.Path]::GetFullPath($ProofPath)
    capture_evidence_path = [System.IO.Path]::GetFullPath($CaptureEvidencePath)
    release_candidate_id = $ReleaseCandidateId
    packaging_performed = $false
    installation_performed = $false
    publication_performed = $false
} | ConvertTo-Json -Compress | Write-Output
