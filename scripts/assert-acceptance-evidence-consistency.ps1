[CmdletBinding()]
param(
    [string]$GapMapPath,
    [string]$AuthoritativeLedgerPath,
    [string]$EvidenceReportPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($GapMapPath)) {
    $GapMapPath = Join-Path $repoRoot 'docs/product-rework/original-brief-gap-map.json'
}
if ([string]::IsNullOrWhiteSpace($AuthoritativeLedgerPath)) {
    $AuthoritativeLedgerPath = Join-Path $repoRoot 'docs/product-rework/original-brief-acceptance.md'
}
if ([string]::IsNullOrWhiteSpace($EvidenceReportPath)) {
    $EvidenceReportPath = Join-Path $repoRoot 'docs/requirements/local-review-evidence-report.md'
}

function Get-AcceptanceStatuses {
    param([Parameter(Mandatory = $true)][string]$Path)

    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    $statuses = [ordered]@{}
    $lineNumber = 0
    foreach ($line in Get-Content -LiteralPath $resolved -ErrorAction Stop) {
        $lineNumber++
        if ($line -notmatch '^\|\s*(R\d{2}|SC\d{2})\s*\|\s*(PASS|FAIL|NOT MEASURED|USER-APPROVED DEFERRAL)\s*\|') {
            continue
        }
        $id = [string]$Matches[1]
        $status = [string]$Matches[2]
        if ($statuses.Contains($id)) {
            throw "Acceptance evidence contains duplicate row $id at ${resolved}:$lineNumber"
        }
        $statuses[$id] = $status
    }

    $expected = @(
        @(1..40 | ForEach-Object { 'R{0:D2}' -f $_ }) +
        @(1..12 | ForEach-Object { 'SC{0:D2}' -f $_ })
    )
    $missing = @($expected | Where-Object { -not $statuses.Contains($_) })
    $unexpected = @($statuses.Keys | Where-Object { $_ -notin $expected })
    if ($missing.Count -gt 0 -or $unexpected.Count -gt 0 -or $statuses.Count -ne $expected.Count) {
        throw "Acceptance evidence must contain exactly R01-R40 and SC01-SC12. Missing: $($missing -join ', '). Unexpected: $($unexpected -join ', ')."
    }
    return $statuses
}

function Get-GapMapStatuses {
    param([Parameter(Mandatory = $true)][string]$Path)

    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    $gapMap = Get-Content -LiteralPath $resolved -Raw -ErrorAction Stop | ConvertFrom-Json
    if ([int]$gapMap.schemaVersion -ne 1) {
        throw 'Canonical acceptance gap map must use schemaVersion 1.'
    }
    if ([int]$gapMap.source.briefSections -ne 40 -or [int]$gapMap.source.successCriteria -ne 12) {
        throw 'Canonical acceptance gap map source totals must remain 40 requirements and 12 success criteria.'
    }

    $allowedStatuses = @('PASS', 'FAIL', 'NOT MEASURED', 'USER-APPROVED DEFERRAL')
    $declaredStatuses = @($gapMap.statusVocabulary | ForEach-Object { [string]$_ } | Sort-Object)
    if (($declaredStatuses -join "`n") -cne (($allowedStatuses | Sort-Object) -join "`n")) {
        throw 'Canonical acceptance gap map declares an unexpected status vocabulary.'
    }

    $expectedRequirements = @(1..40 | ForEach-Object { 'R{0:D2}' -f $_ })
    $expectedSuccessCriteria = @(1..12 | ForEach-Object { 'SC{0:D2}' -f $_ })
    $rows = @($gapMap.requirements)
    if ($rows.Count -ne 52) {
        throw 'Canonical acceptance gap map must contain exactly 52 rows in requirements (R01-R40 and SC01-SC12).'
    }

    $statuses = [ordered]@{}
    foreach ($row in $rows) {
        $id = [string]$row.id
        $status = [string]$row.status
        if ($statuses.Contains($id)) {
            throw "Canonical acceptance gap map contains duplicate row $id."
        }
        if (($expectedRequirements + $expectedSuccessCriteria) -notcontains $id) {
            throw "Canonical acceptance gap map contains unexpected row $id."
        }
        if ($allowedStatuses -notcontains $status) {
            throw "Canonical acceptance gap map contains invalid status '$status' for $id."
        }
        $statuses[$id] = $status
    }
    $expected = @($expectedRequirements + $expectedSuccessCriteria)
    $missing = @($expected | Where-Object { -not $statuses.Contains($_) })
    if ($missing.Count -gt 0 -or $statuses.Count -ne 52) {
        throw "Canonical acceptance gap map is incomplete. Missing: $($missing -join ', ')."
    }

    foreach ($status in $allowedStatuses) {
        $summaryProperty = $gapMap.summary.PSObject.Properties[$status]
        if ($null -eq $summaryProperty) {
            throw "Canonical acceptance gap map summary omits '$status'."
        }
        $actual = @($statuses.Values | Where-Object { [string]$_ -ceq $status }).Count
        if ([int]$summaryProperty.Value -ne $actual) {
            throw "Canonical acceptance gap map summary for '$status' is $($summaryProperty.Value), expected $actual."
        }
    }
    $summaryTotal = @($allowedStatuses | ForEach-Object {
            [int]$gapMap.summary.PSObject.Properties[$_].Value
        } | Measure-Object -Sum).Sum
    if ([int]$summaryTotal -ne 52) {
        throw "Canonical acceptance gap map summary totals $summaryTotal rows, expected 52."
    }
    return $statuses
}

$gapMap = Get-GapMapStatuses -Path $GapMapPath
$authoritative = Get-AcceptanceStatuses -Path $AuthoritativeLedgerPath
$report = Get-AcceptanceStatuses -Path $EvidenceReportPath
$mismatches = @()
foreach ($id in $gapMap.Keys) {
    if ([string]$gapMap[$id] -cne [string]$authoritative[$id] -or
        [string]$gapMap[$id] -cne [string]$report[$id]) {
        $mismatches += "$id(map=$($gapMap[$id]),authoritative=$($authoritative[$id]),report=$($report[$id]))"
    }
}
if ($mismatches.Count -gt 0) {
    throw "Acceptance documents disagree with the canonical gap map: $($mismatches -join '; '). Regenerate current-source evidence before packaging."
}

[ordered]@{
    schema_version = 1
    status = 'consistent'
    requirement_rows = 40
    success_criteria_rows = 12
    gap_map_sha256 = (Get-FileHash -LiteralPath (Resolve-Path -LiteralPath $GapMapPath) -Algorithm SHA256).Hash.ToLowerInvariant()
    authoritative_ledger_sha256 = (Get-FileHash -LiteralPath (Resolve-Path -LiteralPath $AuthoritativeLedgerPath) -Algorithm SHA256).Hash.ToLowerInvariant()
    evidence_report_sha256 = (Get-FileHash -LiteralPath (Resolve-Path -LiteralPath $EvidenceReportPath) -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json -Compress | Write-Output
