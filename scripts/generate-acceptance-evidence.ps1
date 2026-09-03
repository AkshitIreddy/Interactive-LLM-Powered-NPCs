[CmdletBinding()]
param(
    [ValidateSet('InProgress', 'Frozen')]
    [string]$Mode = 'InProgress',

    [string]$StatusManifestPath = 'docs/product-rework/original-brief-gap-map.json',

    [string]$OutputManifestPath = 'artifacts/acceptance/acceptance-evidence-run-v1.json',

    [string]$PackageManifestPath,

    [string]$InstallerPath,

    [string]$InstalledDistributionManifestPath,

    [string]$InstallerSmokeResultPath,

    [string[]]$EvidenceArtifactPath = @(),

    [switch]$UpdateDocuments
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$utf8NoBom = New-Object Text.UTF8Encoding($false)
$allowedStatuses = @('PASS', 'FAIL', 'NOT MEASURED', 'USER-APPROVED DEFERRAL')
$allowedResultScopes = @(
    'source_static',
    'workspace_matrix',
    'clean_clone_matrix',
    'product_e2e',
    'installed_e2e',
    'live_device',
    'live_provider',
    'rendered_acceptance',
    'performance_measurement',
    'privacy_os_enforced',
    'supply_chain_release'
)

function Assert-Condition {
    param(
        [bool]$Condition,
        [string]$Message
    )
    if (-not $Condition) { throw $Message }
}

function Resolve-TaskPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [switch]$RequireFile
    )
    $candidate = if ([IO.Path]::IsPathRooted($Path)) {
        [IO.Path]::GetFullPath($Path)
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $repoRoot $Path))
    }
    if ($RequireFile -and -not [IO.File]::Exists($candidate)) {
        throw "Required evidence file is missing: $candidate"
    }
    return $candidate
}

function Get-Sha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativeOrAbsolutePath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )
    $rootWithSeparator = $repoRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($Path.StartsWith($rootWithSeparator, [StringComparison]::OrdinalIgnoreCase)) {
        return $Path.Substring($rootWithSeparator.Length).Replace('\', '/')
    }
    return $Path
}

function Read-JsonFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Get-OptionalProperty {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Object,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Content
    )
    $parent = Split-Path -Parent $Path
    if ($parent) { [IO.Directory]::CreateDirectory($parent) | Out-Null }
    [IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Add-FileEvidence {
    param(
        [Parameter(Mandatory = $true)]
        [Collections.ArrayList]$Collection,
        [Parameter(Mandatory = $true)]
        [string]$Kind,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )
    $fullPath = Resolve-TaskPath -Path $Path -RequireFile
    [void]$Collection.Add([ordered]@{
        kind = $Kind
        path = Get-RelativeOrAbsolutePath -Path $fullPath
        size_bytes = (Get-Item -LiteralPath $fullPath).Length
        sha256 = Get-Sha256 -Path $fullPath
    })
}

function Sync-StatusDocument {
    param(
        [Parameter(Mandatory = $true)]
        [string]$DocumentPath,
        [Parameter(Mandatory = $true)]
        [Collections.IDictionary]$Statuses,
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string[]]$EvidenceBlock
    )
    $fullPath = Resolve-TaskPath -Path $DocumentPath -RequireFile
    $lines = [Collections.Generic.List[string]]::new()
    foreach ($line in [IO.File]::ReadAllLines($fullPath)) {
        if ($line -match '^\|\s*(R\d{2}|SC\d{2})\s*\|') {
            $id = $Matches[1]
            if ($Statuses.Contains($id)) {
                $cells = $line -split '\|'
                if ($cells.Count -ge 4) {
                    $cells[2] = " $($Statuses[$id]) "
                    [void]$lines.Add(($cells -join '|'))
                    continue
                }
            }
        }
        [void]$lines.Add($line)
    }

    $startMarker = '<!-- acceptance-evidence-run:start -->'
    $endMarker = '<!-- acceptance-evidence-run:end -->'
    $start = $lines.IndexOf($startMarker)
    $end = $lines.IndexOf($endMarker)
    if (($start -ge 0) -xor ($end -ge 0)) {
        throw "Acceptance evidence markers are incomplete in $DocumentPath"
    }
    if ($start -ge 0) {
        if ($end -le $start) { throw "Acceptance evidence markers are reversed in $DocumentPath" }
        for ($index = $end; $index -ge $start; $index--) { $lines.RemoveAt($index) }
        for ($index = $EvidenceBlock.Count - 1; $index -ge 0; $index--) {
            $lines.Insert($start, $EvidenceBlock[$index])
        }
    }
    else {
        $insertAt = 1
        while ($insertAt -lt $lines.Count -and [string]::IsNullOrWhiteSpace($lines[$insertAt])) {
            $insertAt++
        }
        for ($index = $EvidenceBlock.Count - 1; $index -ge 0; $index--) {
            $lines.Insert($insertAt, $EvidenceBlock[$index])
        }
        $lines.Insert($insertAt + $EvidenceBlock.Count, '')
    }
    Write-Utf8NoBom -Path $fullPath -Content (($lines -join "`n") + "`n")
}

$statusManifestFullPath = Resolve-TaskPath -Path $StatusManifestPath -RequireFile
$statusManifest = Read-JsonFile -Path $statusManifestFullPath
Assert-Condition ($statusManifest.schemaVersion -eq 1) 'Acceptance status manifest schemaVersion must be 1.'
Assert-Condition ($null -ne $statusManifest.evidencePolicy) 'Acceptance status manifest is missing evidencePolicy.'

$expectedIds = @()
1..40 | ForEach-Object { $expectedIds += ('R{0:D2}' -f $_) }
1..12 | ForEach-Object { $expectedIds += ('SC{0:D2}' -f $_) }
$rows = @($statusManifest.requirements)
Assert-Condition ($rows.Count -eq $expectedIds.Count) "Acceptance status manifest must contain exactly $($expectedIds.Count) rows."

$statuses = [ordered]@{}
foreach ($row in $rows) {
    $id = [string]$row.id
    $status = [string]$row.status
    Assert-Condition ($expectedIds -contains $id) "Unknown acceptance row: $id"
    Assert-Condition (-not $statuses.Contains($id)) "Duplicate acceptance row: $id"
    Assert-Condition ($allowedStatuses -contains $status) "Invalid status '$status' for $id"
    $statuses[$id] = $status
}
foreach ($id in $expectedIds) {
    Assert-Condition ($statuses.Contains($id)) "Acceptance row is missing: $id"
}

$calculatedSummary = [ordered]@{}
foreach ($status in $allowedStatuses) {
    $calculatedSummary[$status] = @($rows | Where-Object { $_.status -eq $status }).Count
    $declared = [int](Get-OptionalProperty -Object $statusManifest.summary -Name $status)
    Assert-Condition ($declared -eq $calculatedSummary[$status]) "Declared summary for '$status' is stale: expected $($calculatedSummary[$status]), found $declared."
}

$staticRows = @($statusManifest.evidencePolicy.staticEvidenceEligibleRows)
$scopeMap = $statusManifest.evidencePolicy.rowRequiredScopes
foreach ($row in $rows) {
    $scopeProperty = $scopeMap.PSObject.Properties[[string]$row.id]
    if ($staticRows -contains $row.id) {
        Assert-Condition ($null -eq $scopeProperty) "Static row $($row.id) must not declare live/E2E promotion scopes."
        continue
    }
    Assert-Condition ($null -ne $scopeProperty) "Non-static row $($row.id) has no explicit required evidence scopes."
    $requiredScopes = @($scopeProperty.Value)
    Assert-Condition ($requiredScopes.Count -gt 0) "Non-static row $($row.id) has an empty required evidence scope list."
    foreach ($requiredScope in $requiredScopes) {
        Assert-Condition ($allowedResultScopes -contains [string]$requiredScope) "Row $($row.id) declares invalid required scope '$requiredScope'."
    }
}
$recordById = @{}
foreach ($record in @($statusManifest.evidenceRecords)) {
    $recordId = [string]$record.id
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($recordId)) 'Evidence record id is empty.'
    Assert-Condition (-not $recordById.ContainsKey($recordId)) "Duplicate evidence record: $recordId"
    $scope = [string]$record.scope
    Assert-Condition ($allowedResultScopes -contains $scope) "Evidence record '$recordId' has invalid scope '$scope'."
    $recordPath = Resolve-TaskPath -Path ([string]$record.path) -RequireFile
    $expectedHash = ([string]$record.sha256).ToLowerInvariant()
    Assert-Condition ($expectedHash -match '^[0-9a-f]{64}$') "Evidence record '$recordId' has no exact SHA-256."
    Assert-Condition ((Get-Sha256 -Path $recordPath) -eq $expectedHash) "Evidence record '$recordId' hash does not match its file."
    $recordById[$recordId] = $record
}

if ($Mode -eq 'Frozen') {
    foreach ($row in $rows) {
        if ($row.status -ne 'PASS' -or $staticRows -contains $row.id) { continue }
        $recordIds = @(Get-OptionalProperty -Object $row -Name 'evidenceRecordIds')
        Assert-Condition ($recordIds.Count -gt 0) "Frozen non-static PASS row $($row.id) has no exact evidenceRecordIds."
        foreach ($recordIdValue in $recordIds) {
            $recordId = [string]$recordIdValue
            Assert-Condition ($recordById.ContainsKey($recordId)) "PASS row $($row.id) references missing evidence record '$recordId'."
            $scope = [string]$recordById[$recordId].scope
            Assert-Condition ($scope -ne 'source_static') "PASS row $($row.id) cannot be promoted by source-only or isolated-test evidence '$recordId'."
        }
        $recordScopes = @($recordIds | ForEach-Object { [string]$recordById[[string]$_].scope })
        foreach ($requiredScope in @($scopeMap.PSObject.Properties[[string]$row.id].Value)) {
            Assert-Condition ($recordScopes -contains [string]$requiredScope) "PASS row $($row.id) is missing exact '$requiredScope' evidence."
        }
    }
    if ($statuses['R40'] -eq 'PASS') {
        foreach ($criterionId in (1..12 | ForEach-Object { 'SC{0:D2}' -f $_ })) {
            Assert-Condition ($statuses[$criterionId] -in @('PASS', 'USER-APPROVED DEFERRAL')) "R40 cannot pass while $criterionId is $($statuses[$criterionId])."
        }
    }
}

$outputFullPath = Resolve-TaskPath -Path $OutputManifestPath
if ($UpdateDocuments) {
    $evidenceBlock = @(
        '<!-- acceptance-evidence-run:start -->',
        '',
        '## Synchronized evidence-run identity',
        '',
        "- Status map: ``$((Get-RelativeOrAbsolutePath -Path $statusManifestFullPath))``",
        "- Evidence run: ``$((Get-RelativeOrAbsolutePath -Path $outputFullPath))``",
        '',
        'The R01-R40 and SC01-SC12 status cells in this document are synchronized from the single status map above. The generated evidence run binds that map to the exact source-candidate, package, installed-distribution, and result-artifact hashes. Isolated crate, adapter, fixture, or simulation tests are supporting evidence only and cannot promote a product, installed, live, rendered, device, performance, privacy, or end-to-end row.',
        '',
        '<!-- acceptance-evidence-run:end -->'
    )
    foreach ($document in @($statusManifest.evidencePolicy.statusSourceFor)) {
        Sync-StatusDocument -DocumentPath ([string]$document) -Statuses $statuses -EvidenceBlock $evidenceBlock
    }
}

$outputDirectory = Split-Path -Parent $outputFullPath
[IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$sourceIdentityPath = Join-Path $outputDirectory 'source-identity-v1.json'
$sourceGenerator = Join-Path $repoRoot 'scripts/security/generate_source_evidence.py'
$pythonCommand = Get-Command python.exe -ErrorAction SilentlyContinue
if ($null -eq $pythonCommand) { $pythonCommand = Get-Command python3 -ErrorAction SilentlyContinue }
if ($null -eq $pythonCommand) { throw 'Python is required to generate exact source identity evidence.' }
$pythonExecutable = [string]$pythonCommand.Source
$pythonStdout = Join-Path $outputDirectory 'source-identity.stdout.log'
$pythonStderr = Join-Path $outputDirectory 'source-identity.stderr.log'
Remove-Item -LiteralPath $pythonStdout, $pythonStderr -Force -ErrorAction SilentlyContinue
$pythonProcess = Start-Process -FilePath $pythonExecutable -ArgumentList @(
    ('"{0}"' -f $sourceGenerator),
    '--root',
    ('"{0}"' -f $repoRoot),
    '--out',
    ('"{0}"' -f $sourceIdentityPath)
) -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $pythonStdout -RedirectStandardError $pythonStderr
if ($pythonProcess.ExitCode -ne 0) {
    $pythonError = if (Test-Path -LiteralPath $pythonStderr) {
        (Get-Content -LiteralPath $pythonStderr -Raw).Trim()
    }
    else { 'no stderr was captured' }
    throw "Source identity generation failed with exit code $($pythonProcess.ExitCode): $pythonError"
}
$sourceIdentity = Read-JsonFile -Path $sourceIdentityPath
$sourceIdentityHash = Get-Sha256 -Path $sourceIdentityPath
$sourceCandidateHash = [string]$sourceIdentity.source_candidate_digest.sha256
Assert-Condition ($sourceCandidateHash -match '^[0-9a-f]{64}$') 'Generated source candidate digest is malformed.'

$statusManifestHash = Get-Sha256 -Path $statusManifestFullPath
$artifacts = New-Object Collections.ArrayList
if (-not [string]::IsNullOrWhiteSpace($PackageManifestPath)) {
    Add-FileEvidence -Collection $artifacts -Kind 'package_manifest' -Path $PackageManifestPath
}
if (-not [string]::IsNullOrWhiteSpace($InstallerPath)) {
    Add-FileEvidence -Collection $artifacts -Kind 'installer' -Path $InstallerPath
}
if (-not [string]::IsNullOrWhiteSpace($InstalledDistributionManifestPath)) {
    Add-FileEvidence -Collection $artifacts -Kind 'installed_distribution_manifest' -Path $InstalledDistributionManifestPath
}
if (-not [string]::IsNullOrWhiteSpace($InstallerSmokeResultPath)) {
    Add-FileEvidence -Collection $artifacts -Kind 'installer_smoke_result' -Path $InstallerSmokeResultPath
}
foreach ($path in @($EvidenceArtifactPath)) {
    if ([string]::IsNullOrWhiteSpace($path)) { continue }
    Add-FileEvidence -Collection $artifacts -Kind 'result_artifact' -Path $path
}

if ($Mode -eq 'Frozen') {
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($PackageManifestPath)) 'Frozen evidence requires the exact package manifest.'
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($InstallerPath)) 'Frozen evidence requires the exact installer artifact.'
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($InstalledDistributionManifestPath)) 'Frozen evidence requires the exact installed-distribution manifest.'
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($InstallerSmokeResultPath)) 'Frozen evidence requires the exact passing installer-smoke result.'
    Assert-Condition (@($EvidenceArtifactPath).Count -gt 0) 'Frozen evidence requires at least one exact live, rendered, visual, provider, or matrix result artifact.'

    $packageManifestFullPath = Resolve-TaskPath -Path $PackageManifestPath -RequireFile
    $installerFullPath = Resolve-TaskPath -Path $InstallerPath -RequireFile
    $installedManifestFullPath = Resolve-TaskPath -Path $InstalledDistributionManifestPath -RequireFile
    $smokeResultFullPath = Resolve-TaskPath -Path $InstallerSmokeResultPath -RequireFile
    $package = Read-JsonFile -Path $packageManifestFullPath
    $installedManifest = Read-JsonFile -Path $installedManifestFullPath
    $smokeResult = Read-JsonFile -Path $smokeResultFullPath
    $packageHash = Get-Sha256 -Path $packageManifestFullPath
    $installedManifestHash = Get-Sha256 -Path $installedManifestFullPath

    Assert-Condition ($package.schema_version -eq 1 -and $package.distribution -eq 'local-review-only') 'Frozen evidence accepts only the schema-v1 local-review package contract.'
    Assert-Condition ($package.immutable_release_candidate -eq $false) 'The local-review package must remain explicitly non-immutable before installed reconciliation.'
    Assert-Condition ([string]$package.application_identifier -eq (([string]$package.production_application_identifier) + '.review')) 'Frozen local review requires the exact isolated .review application identifier.'
    Assert-Condition ($package.acceptance_evidence.status -eq 'consistent' -and $package.acceptance_evidence.requirement_rows -eq 40 -and $package.acceptance_evidence.success_criteria_rows -eq 12) 'Package acceptance-evidence metadata is missing or inconsistent.'
    $packageGapMapPath = Resolve-TaskPath -Path ([string]$package.acceptance_evidence.gap_map_path) -RequireFile
    $ledgerPath = Resolve-TaskPath -Path ([string]$package.acceptance_evidence.authoritative_ledger_path) -RequireFile
    $reportPath = Resolve-TaskPath -Path ([string]$package.acceptance_evidence.evidence_report_path) -RequireFile
    Assert-Condition ((Get-Sha256 -Path $packageGapMapPath) -eq ([string]$package.acceptance_evidence.gap_map_sha256).ToLowerInvariant()) 'Package binds a different canonical acceptance gap map.'
    Assert-Condition ((Get-Sha256 -Path $packageGapMapPath) -eq $statusManifestHash) 'Package acceptance gap map differs from this evidence run status map.'
    Assert-Condition ((Get-Sha256 -Path $ledgerPath) -eq ([string]$package.acceptance_evidence.authoritative_ledger_sha256).ToLowerInvariant()) 'Package binds a different authoritative acceptance ledger.'
    Assert-Condition ((Get-Sha256 -Path $reportPath) -eq ([string]$package.acceptance_evidence.evidence_report_sha256).ToLowerInvariant()) 'Package binds a different local-review evidence report.'
    Assert-Condition ([string]$package.source.source_candidate_digest.sha256 -eq $sourceCandidateHash) 'Package source candidate does not match the current synchronized source candidate.'

    $installerName = [IO.Path]::GetFileName($installerFullPath)
    $installerEntries = @($package.files | Where-Object { [string]$_.file -eq $installerName })
    Assert-Condition ($installerEntries.Count -eq 1) 'Package manifest does not bind exactly the supplied installer.'
    Assert-Condition (([string]$installerEntries[0].sha256).ToLowerInvariant() -eq (Get-Sha256 -Path $installerFullPath)) 'Supplied installer hash differs from the package manifest.'
    Assert-Condition ([long]$installerEntries[0].size_bytes -eq (Get-Item -LiteralPath $installerFullPath).Length) 'Supplied installer size differs from the package manifest.'

    Assert-Condition ($installedManifest.schema_version -eq 1 -and @($installedManifest.files).Count -gt 0) 'Installed-distribution manifest is malformed or empty.'
    Assert-Condition ($smokeResult.schema_version -eq 1 -and $smokeResult.status -eq 'passed' -and $null -eq $smokeResult.error) 'Installer-smoke result is not an exact passing schema-v1 result.'
    Assert-Condition (([string]$smokeResult.source_identity.package_manifest_sha256).ToLowerInvariant() -eq $packageHash) 'Installer smoke tested a different package manifest.'
    Assert-Condition (([string]$smokeResult.installed_distribution_manifest_sha256).ToLowerInvariant() -eq $installedManifestHash) 'Installer smoke reconciled a different installed-distribution manifest.'
    foreach ($requiredBoolean in @(
        'onboarding_auto_open_observed',
        'reinstall_cycle_verified',
        'production_namespace_unchanged',
        'installed_distribution_reconciled',
        'reinstalled_distribution_reconciled',
        'installed_legal_resources_reconciled',
        'unclassified_installed_files_rejected',
        'no_remaining_processes',
        'no_install_files',
        'no_shortcuts',
        'no_registry_entries'
    )) {
        Assert-Condition ((Get-OptionalProperty -Object $smokeResult -Name $requiredBoolean) -eq $true) "Installer-smoke result did not prove '$requiredBoolean'."
    }
}

$runManifest = [ordered]@{
    schema_version = 1
    state = if ($Mode -eq 'Frozen') { 'frozen' } else { 'in_progress_non_authoritative' }
    generated_at_utc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    status_source = [ordered]@{
        path = Get-RelativeOrAbsolutePath -Path $statusManifestFullPath
        sha256 = $statusManifestHash
        authoritative_ledger = [string]$statusManifest.source.authoritativeLedger
    }
    source_identity = [ordered]@{
        manifest_path = Get-RelativeOrAbsolutePath -Path $sourceIdentityPath
        manifest_sha256 = $sourceIdentityHash
        head_commit = [string]$sourceIdentity.head_commit
        dirty = [bool]$sourceIdentity.dirty
        source_candidate_sha256 = $sourceCandidateHash
        source_files = [int]$sourceIdentity.source_candidate_digest.files
        tracked_missing = [int]$sourceIdentity.source_candidate_digest.tracked_missing
    }
    result_policy = [ordered]@{
        isolated_tests_are_supporting_evidence_only = $true
        static_evidence_eligible_rows = $staticRows
        external_release_authorized = [bool]$statusManifest.direction.externalReleaseAuthorized
    }
    summary = $calculatedSummary
    rows = $rows
    artifacts = @($artifacts)
}
$encodedRun = ($runManifest | ConvertTo-Json -Depth 20) + "`n"
Write-Utf8NoBom -Path $outputFullPath -Content $encodedRun
$runManifestHash = Get-Sha256 -Path $outputFullPath

Write-Output ([ordered]@{
    mode = $Mode
    output = Get-RelativeOrAbsolutePath -Path $outputFullPath
    output_sha256 = $runManifestHash
    source_candidate_sha256 = $sourceCandidateHash
    status_manifest_sha256 = $statusManifestHash
    rows = $rows.Count
    artifacts = $artifacts.Count
} | ConvertTo-Json -Compress)
