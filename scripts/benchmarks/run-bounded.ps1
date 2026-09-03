[CmdletBinding(DefaultParameterSetName = 'Summarize')]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Summarize')]
    [string]$InputPath,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]{0,95}$')]
    [string]$ReportId,

    [Parameter(Mandatory = $true, ParameterSetName = 'Simulate')]
    [switch]$Simulate,

    [ValidateRange(1, 100000)]
    [int]$Iterations = 100,

    [ValidateRange(1, 1000000)]
    [int]$MaxRecords = 100000,

    [ValidateRange(1, 268435456)]
    [int]$MaxInputBytes = 16777216,

    [ValidateRange(1, 900)]
    [int]$TimeoutSeconds = 60,

    [string]$PythonPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$harness = Join-Path $PSScriptRoot 'benchmark_harness.py'
if (-not (Test-Path -LiteralPath $harness -PathType Leaf)) {
    throw 'The benchmark harness is missing.'
}

$pythonPrefix = @()
if (-not [string]::IsNullOrWhiteSpace($PythonPath)) {
    if (-not (Test-Path -LiteralPath $PythonPath -PathType Leaf)) {
        throw 'PythonPath must identify an existing Python executable.'
    }
    $pythonExecutable = (Resolve-Path -LiteralPath $PythonPath).Path
}
else {
    $python = Get-Command py -ErrorAction SilentlyContinue
    $pythonPrefix = @('-3')
    if ($null -eq $python) {
        $python = Get-Command python -ErrorAction SilentlyContinue
        $pythonPrefix = @()
    }
    if ($null -eq $python) {
        throw 'Python 3.10 or newer is required. Supply -PythonPath when it is not on PATH.'
    }
    $pythonExecutable = $python.Source
}

$arguments = @($pythonPrefix)
if ($Simulate) {
    $arguments += @(
        $harness, 'simulate',
        '--output', $OutputPath,
        '--report-id', $ReportId,
        '--iterations', $Iterations.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--timeout-seconds', $TimeoutSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--max-records', $MaxRecords.ToString([Globalization.CultureInfo]::InvariantCulture)
    )
}
else {
    if (-not (Test-Path -LiteralPath $InputPath -PathType Leaf)) {
        throw 'InputPath must identify an existing JSONL file.'
    }
    $arguments += @(
        $harness, 'summarize',
        '--input', $InputPath,
        '--output', $OutputPath,
        '--report-id', $ReportId,
        '--timeout-seconds', $TimeoutSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--max-records', $MaxRecords.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--max-input-bytes', $MaxInputBytes.ToString([Globalization.CultureInfo]::InvariantCulture)
    )
}

# The child receives only fixed harness switches and paths. Provider credentials,
# prompts, audio, and arbitrary command lines are intentionally unsupported.
function ConvertTo-ProcessArgument {
    param([string]$Value)
    if ($Value.Contains('"')) {
        throw 'Benchmark arguments must not contain quotation marks.'
    }
    if ($Value.Length -eq 0 -or $Value -match '\s') {
        return '"' + $Value + '"'
    }
    return $Value
}

$processArguments = @($arguments | ForEach-Object { ConvertTo-ProcessArgument -Value $_ })
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = $pythonExecutable
$startInfo.Arguments = $processArguments -join ' '
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$process = New-Object System.Diagnostics.Process
$process.StartInfo = $startInfo
if (-not $process.Start()) {
    throw 'Benchmark harness process could not be started.'
}
if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    try { $process.Kill() } catch { }
    try { $process.WaitForExit() } catch { }
    throw "Benchmark harness exceeded the $TimeoutSeconds-second Windows process limit."
}
$process.WaitForExit()
$exitCode = $process.ExitCode
if ($exitCode -ne 0) {
    throw "Benchmark harness failed with exit code $exitCode."
}

Write-Host "Bounded benchmark completed: $OutputPath" -ForegroundColor Green
