[CmdletBinding()]
param(
    [switch]$Quick,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot 'artifacts/benchmarks'
}
if (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot $OutputDirectory
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$timestamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')
$results = New-Object System.Collections.Generic.List[object]

function Invoke-BenchmarkTarget {
    param(
        [string]$Name,
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )
    Write-Host "==> $Name" -ForegroundColor Cyan
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    Push-Location $WorkingDirectory
    try {
        & $FilePath @Arguments
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    finally {
        Pop-Location
        $watch.Stop()
    }
    $results.Add([pscustomobject]@{
        name = $Name
        status = $(if ($exitCode -eq 0) { 'passed' } else { 'failed' })
        exit_code = $exitCode
        duration_ms = $watch.ElapsedMilliseconds
    })
    if ($exitCode -ne 0) { throw "$Name failed with exit code $exitCode." }
}

$ran = $false
$simRoot = Join-Path $repoRoot 'tools/sim'
if (Test-Path (Join-Path $simRoot 'src/cli.ts')) {
    if ($null -eq (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js 20+ is required for the deterministic simulation benchmark.' }
    $ran = $true
    $simOutput = Join-Path $OutputDirectory "simulation-benchmark-$timestamp.json"
    Invoke-BenchmarkTarget -Name 'deterministic-simulation' -FilePath 'node' -Arguments @('src/cli.ts', 'benchmark', '--output', $simOutput) -WorkingDirectory $simRoot
}

$packagePath = Join-Path $repoRoot 'package.json'
if (Test-Path $packagePath) {
    $package = Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json
    if ($null -ne $package.scripts -and $package.scripts.PSObject.Properties.Name -contains 'benchmark') {
        $ran = $true
        $pnpm = Get-Command pnpm -ErrorAction SilentlyContinue
        if ($null -eq $pnpm) { throw 'pnpm is required for the frontend benchmark target.' }
        Invoke-BenchmarkTarget -Name 'frontend' -FilePath 'pnpm' -Arguments @('run', 'benchmark') -WorkingDirectory $repoRoot
    }
}

if ((Test-Path (Join-Path $repoRoot 'Cargo.toml')) -and -not $Quick) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($null -eq $cargo) { throw 'cargo is required for Rust benchmarks.' }
    $ran = $true
    $cargoArgs = @('bench', '--workspace', '--locked')
    Invoke-BenchmarkTarget -Name 'rust-workspace' -FilePath 'cargo' -Arguments $cargoArgs -WorkingDirectory $repoRoot
}

if (-not $ran) {
    Write-Error 'No benchmark target was found. Add a package.json benchmark script or Cargo workspace benchmark.'
    exit 1
}

$report = [ordered]@{
    schema_version = 1
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
    machine = [ordered]@{
        os = [System.Environment]::OSVersion.VersionString
        processor_count = [System.Environment]::ProcessorCount
        dotnet_runtime = [System.Environment]::Version.ToString()
    }
    quick = [bool]$Quick
    results = $results.ToArray()
    note = 'This harness records command duration only. Product latency/FPS gates require the dedicated in-app benchmark suite.'
}

$outputPath = Join-Path $OutputDirectory "developer-benchmark-$timestamp.json"
$reportJson = ($report | ConvertTo-Json -Depth 6) + [Environment]::NewLine
[System.IO.File]::WriteAllText(
    $outputPath,
    $reportJson,
    (New-Object System.Text.UTF8Encoding($false))
)
Write-Host "Benchmark report: $outputPath" -ForegroundColor Green
exit 0
