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
$evidenceReports = New-Object System.Collections.Generic.List[object]

function Invoke-BenchmarkTarget {
    param(
        [string]$Name,
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )
    Write-Host "==> $Name" -ForegroundColor Cyan
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $quotedArguments = @($Arguments | ForEach-Object {
            if ($_ -match '[\s"]') {
                if ($_.Contains('"')) { throw "$Name has an unsupported quotation mark in its fixed argument list." }
                '"' + $_ + '"'
            }
            else { $_ }
        })
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $FilePath
    $startInfo.Arguments = $quotedArguments -join ' '
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) { throw "$Name did not start." }
        # Drain both pipes concurrently. Cargo commonly writes enough progress
        # to stderr to deadlock a sequential ReadToEnd/WaitForExit sequence.
        $standardOutputTask = $process.StandardOutput.ReadToEndAsync()
        $standardErrorTask = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $standardOutput = $standardOutputTask.GetAwaiter().GetResult()
        $standardError = $standardErrorTask.GetAwaiter().GetResult()
        $exitCode = $process.ExitCode
        if (-not [string]::IsNullOrWhiteSpace($standardOutput)) { Write-Host $standardOutput.TrimEnd() }
        if (-not [string]::IsNullOrWhiteSpace($standardError)) { Write-Host $standardError.TrimEnd() }
    }
    finally {
        $process.Dispose()
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
$boundedHarness = Join-Path $repoRoot 'scripts/benchmarks/run-bounded.ps1'
if (Test-Path -LiteralPath $boundedHarness -PathType Leaf) {
    $ran = $true
    $python = Get-Command python.exe, python, python3 -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') -Directory -ErrorAction SilentlyContinue |
            ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
    }
    if ($null -eq $python) {
        throw 'Python 3.10 or newer is required for canonical percentile evidence.'
    }
    $pythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
    $iterations = $(if ($Quick) { 25 } else { 100 })
    $evidenceOutput = Join-Path $OutputDirectory "pipeline-percentiles-simulated-$timestamp.json"
    Write-Host '==> deterministic-pipeline-percentiles' -ForegroundColor Cyan
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $global:LASTEXITCODE = 0
        & $boundedHarness `
            -Simulate `
            -OutputPath $evidenceOutput `
            -ReportId "canonical-simulation-$timestamp" `
            -Iterations $iterations `
            -TimeoutSeconds 30 `
            -PythonPath $pythonPath
        if ($LASTEXITCODE -ne 0) {
            throw "Deterministic pipeline-percentile harness failed with exit code $LASTEXITCODE."
        }
        if (-not (Test-Path -LiteralPath $evidenceOutput -PathType Leaf)) {
            throw 'Deterministic pipeline-percentile harness did not write its report.'
        }
        $evidence = Get-Content -LiteralPath $evidenceOutput -Raw | ConvertFrom-Json
        if ($evidence.schema_version -ne 'interactive-npcs-benchmark-report/v1' -or
            $evidence.classification.execution_mode -ne 'simulated' -or
            $evidence.classification.measurement_kind -ne 'measured' -or
            $evidence.classification.acceptance_eligible -ne $false -or
            @($evidence.metrics).Count -ne 12 -or
            @($evidence.metrics | Where-Object {
                    $null -eq $_.p50 -or $null -eq $_.p95 -or $null -eq $_.p99
                }).Count -ne 0) {
            throw 'Deterministic pipeline-percentile report failed its canonical classification/metric contract.'
        }
        $results.Add([pscustomobject]@{
            name = 'deterministic-pipeline-percentiles'
            status = 'passed'
            exit_code = 0
            duration_ms = $watch.ElapsedMilliseconds
        })
        $evidenceReports.Add([pscustomobject]@{
            path = $evidenceOutput
            schema_version = [string]$evidence.schema_version
            execution_mode = [string]$evidence.classification.execution_mode
            measurement_kind = [string]$evidence.classification.measurement_kind
            acceptance_eligible = [bool]$evidence.classification.acceptance_eligible
            metrics = @($evidence.metrics).Count
        })
    }
    finally {
        $watch.Stop()
    }
}
else {
    throw "The canonical bounded benchmark harness was not found: $boundedHarness"
}

$simRoot = Join-Path $repoRoot 'tools/sim'
if (Test-Path (Join-Path $simRoot 'src/cli.ts')) {
    $node = Get-Command node.exe, node -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $node) { throw 'Node.js 20+ is required for the deterministic simulation benchmark.' }
    $ran = $true
    $simOutput = Join-Path $OutputDirectory "simulation-benchmark-$timestamp.json"
    Invoke-BenchmarkTarget -Name 'deterministic-simulation' -FilePath $node.Source -Arguments @('src/cli.ts', 'benchmark', '--output', $simOutput) -WorkingDirectory $simRoot
    if (-not (Test-Path -LiteralPath $simOutput -PathType Leaf)) {
        throw 'Deterministic simulation command returned success without writing its benchmark report.'
    }
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
    $cargo = Get-Command cargo.exe, cargo -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $cargo) { throw 'cargo is required for Rust benchmarks.' }
    $ran = $true
    $cargoArgs = @('bench', '--workspace', '--locked')
    Invoke-BenchmarkTarget -Name 'rust-workspace' -FilePath $cargo.Source -Arguments $cargoArgs -WorkingDirectory $repoRoot
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
    evidence_reports = $evidenceReports.ToArray()
    note = 'The canonical simulated report exercises p50/p95/p99 aggregation but is not live acceptance evidence. Product latency/FPS acceptance requires a separate live + measured Windows report from instrumented product samples.'
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
