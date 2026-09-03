[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$checker = Join-Path $PSScriptRoot 'assert-acceptance-evidence-consistency.ps1'
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("npc-acceptance-evidence-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot -Force | Out-Null

function Write-Fixture {
    param([string]$Path, [hashtable]$Overrides, [string]$Omit, [string]$Duplicate)
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add('| ID | Status | Evidence |')
    $lines.Add('| --- | --- | --- |')
    foreach ($id in @(
            @(1..40 | ForEach-Object { 'R{0:D2}' -f $_ }) +
            @(1..12 | ForEach-Object { 'SC{0:D2}' -f $_ })
        )) {
        if ($id -eq $Omit) { continue }
        $status = if ($Overrides.ContainsKey($id)) { [string]$Overrides[$id] } else { 'NOT MEASURED' }
        $lines.Add("| $id | $status | fixture |")
        if ($id -eq $Duplicate) { $lines.Add("| $id | $status | duplicate fixture |") }
    }
    [System.IO.File]::WriteAllText($Path, ($lines -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
}

function Write-GapMapFixture {
    param([string]$Path, [hashtable]$Overrides, [switch]$InvalidSummary)
    $requirements = @()
    $counts = [ordered]@{
        'PASS' = 0
        'FAIL' = 0
        'NOT MEASURED' = 0
        'USER-APPROVED DEFERRAL' = 0
    }
    foreach ($id in @(
            @(1..40 | ForEach-Object { 'R{0:D2}' -f $_ }) +
            @(1..12 | ForEach-Object { 'SC{0:D2}' -f $_ })
        )) {
        $status = if ($Overrides.ContainsKey($id)) { [string]$Overrides[$id] } else { 'NOT MEASURED' }
        $counts[$status] = [int]$counts[$status] + 1
        $requirements += [ordered]@{ id = $id; status = $status }
    }
    if ($InvalidSummary) { $counts['NOT MEASURED'] = [int]$counts['NOT MEASURED'] - 1 }
    $map = [ordered]@{
        schemaVersion = 1
        source = [ordered]@{ briefSections = 40; successCriteria = 12 }
        statusVocabulary = @('PASS', 'FAIL', 'NOT MEASURED', 'USER-APPROVED DEFERRAL')
        summary = $counts
        requirements = $requirements
    }
    [System.IO.File]::WriteAllText(
        $Path,
        ($map | ConvertTo-Json -Depth 5),
        (New-Object System.Text.UTF8Encoding($false))
    )
}

try {
    $gapMap = Join-Path $testRoot 'gap-map.json'
    $authoritative = Join-Path $testRoot 'authoritative.md'
    $report = Join-Path $testRoot 'report.md'
    Write-GapMapFixture -Path $gapMap -Overrides @{}
    Write-Fixture -Path $authoritative -Overrides @{} -Omit '' -Duplicate ''
    Write-Fixture -Path $report -Overrides @{} -Omit '' -Duplicate ''
    $pass = & $checker -GapMapPath $gapMap -AuthoritativeLedgerPath $authoritative -EvidenceReportPath $report | ConvertFrom-Json
    if ($pass.status -ne 'consistent' -or $pass.requirement_rows -ne 40 -or $pass.success_criteria_rows -ne 12) {
        throw 'Matching acceptance fixture did not pass with the exact row counts.'
    }
    if ([string]$pass.gap_map_sha256 -cne (Get-FileHash -LiteralPath $gapMap -Algorithm SHA256).Hash.ToLowerInvariant()) {
        throw 'Matching acceptance fixture did not bind the canonical gap-map hash.'
    }

    Write-Fixture -Path $authoritative -Overrides @{ R21 = 'PASS' } -Omit '' -Duplicate ''
    Write-Fixture -Path $report -Overrides @{ R21 = 'PASS' } -Omit '' -Duplicate ''
    $failed = $false
    try { & $checker -GapMapPath $gapMap -AuthoritativeLedgerPath $authoritative -EvidenceReportPath $report | Out-Null }
    catch { $failed = $_.Exception.Message -match 'R21\(map=NOT MEASURED,authoritative=PASS,report=PASS\)' }
    if (-not $failed) { throw 'Synchronized document drift away from the canonical gap map was not rejected.' }

    Write-Fixture -Path $authoritative -Overrides @{} -Omit '' -Duplicate ''
    $failed = $false
    try { & $checker -GapMapPath $gapMap -AuthoritativeLedgerPath $authoritative -EvidenceReportPath $report | Out-Null }
    catch { $failed = $_.Exception.Message -match 'R21\(map=NOT MEASURED,authoritative=NOT MEASURED,report=PASS\)' }
    if (-not $failed) { throw 'Conflicting acceptance document status was not rejected with exact map evidence.' }

    Write-Fixture -Path $report -Overrides @{} -Omit 'SC11' -Duplicate ''
    $failed = $false
    try { & $checker -GapMapPath $gapMap -AuthoritativeLedgerPath $authoritative -EvidenceReportPath $report | Out-Null }
    catch { $failed = $_.Exception.Message -match 'Missing: SC11' }
    if (-not $failed) { throw 'Missing success-criterion row was not rejected.' }

    Write-Fixture -Path $report -Overrides @{} -Omit '' -Duplicate 'R05'
    $failed = $false
    try { & $checker -GapMapPath $gapMap -AuthoritativeLedgerPath $authoritative -EvidenceReportPath $report | Out-Null }
    catch { $failed = $_.Exception.Message -match 'duplicate row R05' }
    if (-not $failed) { throw 'Duplicate requirement row was not rejected.' }

    Write-Fixture -Path $report -Overrides @{} -Omit '' -Duplicate ''
    Write-GapMapFixture -Path $gapMap -Overrides @{} -InvalidSummary
    $failed = $false
    try { & $checker -GapMapPath $gapMap -AuthoritativeLedgerPath $authoritative -EvidenceReportPath $report | Out-Null }
    catch { $failed = $_.Exception.Message -match "summary for 'NOT MEASURED'" }
    if (-not $failed) { throw 'Invalid canonical gap-map summary was not rejected.' }
}
finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}

Write-Host 'Acceptance evidence consistency regressions passed.' -ForegroundColor Green
