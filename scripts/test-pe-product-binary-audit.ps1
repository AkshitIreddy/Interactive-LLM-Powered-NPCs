[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: PE product-binary regression requires a Windows build fixture.'
    exit 0
}

function Assert-True {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw $Message }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$auditScript = Join-Path $PSScriptRoot 'audit-pe-product-binary.ps1'
. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')
$controlCandidates = [System.Collections.Generic.List[string]]::new()
$cargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
if ($null -ne $cargo) {
    try {
        $metadataProcess = Invoke-NpcHiddenProcess -FilePath $cargo.Source -ArgumentList @(
            'metadata', '--manifest-path', (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml'),
            '--format-version', '1', '--no-deps', '--locked', '--offline'
        ) -WorkingDirectory $repoRoot -NoReplayOutput
        if ($metadataProcess.ExitCode -eq 0) {
            $metadata = $metadataProcess.StandardOutput | ConvertFrom-Json
            if (-not [string]::IsNullOrWhiteSpace([string]$metadata.target_directory)) {
                $controlCandidates.Add((Join-Path ([string]$metadata.target_directory) `
                    'debug/interactive-npcs-control.exe'))
            }
        }
    }
    catch {
        # Retain the conventional-path fallbacks below when metadata is unavailable.
    }
}
@(
    (Join-Path $repoRoot 'apps/control/src-tauri/target/debug/interactive-npcs-control.exe'),
    (Join-Path $repoRoot 'target/debug/interactive-npcs-control.exe')
) | ForEach-Object { $controlCandidates.Add($_) }
$control = $controlCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($control)) {
    throw "A built GUI control executable is required for the PE audit regression: $($controlCandidates -join ', ')"
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) `
    "npc-pe-audit-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
try {
    $report = (& $auditScript -Path $control -ExpectedSubsystem Gui -AsJson) | ConvertFrom-Json
    Assert-True -Condition ($report.machine -eq 'x86_64') -Message 'PE audit did not report x86_64.'
    Assert-True -Condition ($report.pe_subsystem -eq 'windows_gui') -Message 'PE audit did not report GUI subsystem.'
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($report.sha256)) -Message 'PE audit omitted SHA-256.'
    Assert-True -Condition (@($report.imports).Count -gt 0) -Message 'PE import inventory is empty.'

    $consoleFixture = Join-Path $fixtureRoot 'console-fixture.exe'
    Copy-Item -LiteralPath $control -Destination $consoleFixture
    $consoleBytes = [System.IO.File]::ReadAllBytes($consoleFixture)
    $peOffset = [BitConverter]::ToInt32($consoleBytes, 0x3c)
    $subsystemOffset = $peOffset + 24 + 0x44
    $consoleBytes[$subsystemOffset] = 3
    $consoleBytes[$subsystemOffset + 1] = 0
    [System.IO.File]::WriteAllBytes($consoleFixture, $consoleBytes)
    $consoleRejected = $false
    try { & $auditScript -Path $consoleFixture -ExpectedSubsystem Gui | Out-Null }
    catch { $consoleRejected = $_.Exception.Message -match 'expected Windows Gui' }
    Assert-True -Condition $consoleRejected -Message 'PE audit did not reject a Console-subsystem child.'

    $debugFixture = Join-Path $fixtureRoot 'debug-crt-fixture.exe'
    Copy-Item -LiteralPath $control -Destination $debugFixture
    $debugBytes = [System.IO.File]::ReadAllBytes($debugFixture)
    $needle = [System.Text.Encoding]::ASCII.GetBytes('advapi32.dll')
    $replacement = [System.Text.Encoding]::ASCII.GetBytes("msvcrtd.dll`0")
    # Import names are ASCII. A single bounded string search avoids a slow
    # interpreted byte-by-byte scan across a large Debug executable.
    $matchOffset = [System.Text.Encoding]::ASCII.GetString($debugBytes).
        IndexOf('advapi32.dll', [System.StringComparison]::OrdinalIgnoreCase)
    if ($matchOffset -lt 0) { throw 'Could not locate an import name for the Debug CRT fixture.' }
    [Array]::Copy($replacement, 0, $debugBytes, $matchOffset, $replacement.Length)
    [System.IO.File]::WriteAllBytes($debugFixture, $debugBytes)
    $debugRejected = $false
    try { & $auditScript -Path $debugFixture -ExpectedSubsystem Gui | Out-Null }
    catch { $debugRejected = $_.Exception.Message -match 'imports Debug CRT libraries' }
    Assert-True -Condition $debugRejected -Message 'PE audit did not reject a Debug CRT import.'

    Write-Host 'PE product-binary audit regression checks passed.' -ForegroundColor Green
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
    }
}

exit 0
