[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    throw 'Windows installer artifact-scope generation requires Windows.'
}

. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot $OutputDirectory
}
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$graphRoot = Join-Path $OutputDirectory 'graphs'
New-Item -ItemType Directory -Path $graphRoot -Force | Out-Null

function Invoke-ProcessToFiles {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentList,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$StandardOutputPath,
        [Parameter(Mandatory = $true)][string]$FailureMessage
    )

    Remove-Item -LiteralPath $StandardOutputPath -Force -ErrorAction SilentlyContinue
    $process = Invoke-NpcHiddenProcess -FilePath $FilePath -ArgumentList $ArgumentList `
        -WorkingDirectory $WorkingDirectory -NoReplayOutput
    if ($process.ExitCode -ne 0) {
        throw "$FailureMessage (exit code $($process.ExitCode)): $($process.StandardError)"
    }
    [System.IO.File]::WriteAllText(
        $StandardOutputPath,
        $process.StandardOutput,
        (New-Object System.Text.UTF8Encoding($false)))
    if (-not (Test-Path -LiteralPath $StandardOutputPath -PathType Leaf) -or
        (Get-Item -LiteralPath $StandardOutputPath).Length -eq 0) {
        throw "$FailureMessage produced no output: $StandardOutputPath"
    }
}

function Resolve-PythonExecutable {
    foreach ($name in @('python.exe', 'python3.exe', 'python', 'python3')) {
        $command = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($null -ne $command) { return $command.Source }
    }
    if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $candidate = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') `
            -Directory -ErrorAction SilentlyContinue |
            ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue } |
            Sort-Object FullName -Descending | Select-Object -First 1
        if ($null -ne $candidate) { return $candidate.FullName }
    }
    throw 'Python 3 is required for artifact-scoped legal evidence.'
}

$cargo = (Get-Command cargo.exe -CommandType Application -ErrorAction Stop |
    Select-Object -First 1).Source
$python = Resolve-PythonExecutable
$pnpm = Get-NpcCorepackPnpmInvocation
$tauriManifest = Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml'

$rootTree = Join-Path $graphRoot 'root.cargo-tree.txt'
$tauriTree = Join-Path $graphRoot 'tauri.cargo-tree.txt'
$pnpmGraph = Join-Path $graphRoot 'pnpm-production.json'
$rootMetadata = Join-Path $graphRoot 'root.cargo-metadata.json'
$tauriMetadata = Join-Path $graphRoot 'tauri.cargo-metadata.json'

Invoke-ProcessToFiles -FilePath $cargo -WorkingDirectory $repoRoot -StandardOutputPath $rootTree `
    -ArgumentList @('tree', '--workspace', '--locked', '--offline', '--edges', 'normal', '--prefix', 'none', '--format', '{p}') `
    -FailureMessage 'Locked/offline root Cargo artifact graph failed'
Invoke-ProcessToFiles -FilePath $cargo -WorkingDirectory $repoRoot -StandardOutputPath $tauriTree `
    -ArgumentList @('tree', '--manifest-path', $tauriManifest, '--workspace', '--locked', '--offline', '--edges', 'normal', '--prefix', 'none', '--format', '{p}') `
    -FailureMessage 'Locked/offline nested Tauri Cargo artifact graph failed'
Invoke-ProcessToFiles -FilePath $pnpm.FilePath -WorkingDirectory $repoRoot -StandardOutputPath $pnpmGraph `
    -ArgumentList @($pnpm.Prefix + @('list', '--prod', '--recursive', '--depth', 'Infinity', '--json')) `
    -FailureMessage 'Locked production pnpm artifact graph failed'
Invoke-ProcessToFiles -FilePath $cargo -WorkingDirectory $repoRoot -StandardOutputPath $rootMetadata `
    -ArgumentList @('metadata', '--locked', '--offline', '--format-version', '1') `
    -FailureMessage 'Locked/offline root Cargo metadata failed'
Invoke-ProcessToFiles -FilePath $cargo -WorkingDirectory $repoRoot -StandardOutputPath $tauriMetadata `
    -ArgumentList @('metadata', '--manifest-path', $tauriManifest, '--locked', '--offline', '--format-version', '1') `
    -FailureMessage 'Locked/offline nested Tauri Cargo metadata failed'

$artifactScope = Join-Path $OutputDirectory 'windows-artifact-scope.json'
$scopeOutput = Join-Path $graphRoot 'scope-command.json'
Invoke-ProcessToFiles -FilePath $python -WorkingDirectory $repoRoot -StandardOutputPath $scopeOutput `
    -ArgumentList @(
        (Join-Path $PSScriptRoot 'security/generate_artifact_scope.py'),
        '--root', $repoRoot,
        '--artifact-id', 'windows-review-installer',
        '--cargo-tree', $rootTree,
        '--cargo-tree', $tauriTree,
        '--pnpm-list-json', $pnpmGraph,
        '--out', $artifactScope
    ) -FailureMessage 'Artifact-scope generation failed'

$licenseRoot = Join-Path $OutputDirectory 'legal/packages'
if (Test-Path -LiteralPath $licenseRoot) {
    Remove-Item -LiteralPath $licenseRoot -Recurse -Force
}
$licenseOutput = Join-Path $graphRoot 'license-command.json'
Invoke-ProcessToFiles -FilePath $python -WorkingDirectory $repoRoot -StandardOutputPath $licenseOutput `
    -ArgumentList @(
        (Join-Path $PSScriptRoot 'security/collect_artifact_licenses.py'),
        '--root', $repoRoot,
        '--scope', $artifactScope,
        '--cargo-metadata', $rootMetadata,
        '--cargo-metadata', $tauriMetadata,
        '--node-modules', (Join-Path $repoRoot 'node_modules'),
        '--out', $licenseRoot
    ) -FailureMessage 'Artifact license-material collection failed'
$licenseIndex = Join-Path $licenseRoot 'THIRD-PARTY-LICENSE-FILES.json'

$legalRoot = Join-Path $OutputDirectory 'legal'
$sbomPath = Join-Path $legalRoot 'lockfiles.cdx.json'
$sbomOutput = Join-Path $graphRoot 'sbom-command.json'
Invoke-ProcessToFiles -FilePath $python -WorkingDirectory $repoRoot -StandardOutputPath $sbomOutput `
    -ArgumentList @(
        (Join-Path $PSScriptRoot 'security/generate_lock_sbom.py'),
        '--strict',
        '--distribution-profile', 'installer',
        '--require-artifact-scope',
        '--require-license-materials',
        '--root', $repoRoot,
        '--artifact-scope', $artifactScope,
        '--license-material-index', $licenseIndex,
        '--out', $sbomPath
    ) -FailureMessage 'Strict artifact-scoped CycloneDX generation failed'

foreach ($required in @($artifactScope, $licenseIndex, $sbomPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Artifact legal pipeline omitted required output: $required"
    }
    $raw = Get-Content -LiteralPath $required -Raw
    if ($raw.IndexOf($repoRoot, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "Artifact legal output records an absolute repository path: $required"
    }
}
$scope = Get-Content -LiteralPath $artifactScope -Raw | ConvertFrom-Json
$index = Get-Content -LiteralPath $licenseIndex -Raw | ConvertFrom-Json
$sbom = Get-Content -LiteralPath $sbomPath -Raw | ConvertFrom-Json
if ($scope.schema_version -ne 1 -or $scope.artifact_id -ne 'windows-review-installer' -or
    $index.schema_version -ne 1 -or $index.artifact_id -ne $scope.artifact_id -or
    $sbom.bomFormat -ne 'CycloneDX' -or $sbom.specVersion -ne '1.6') {
    throw 'Artifact legal pipeline emitted an invalid or mismatched schema.'
}
$licenseFiles = @(Get-ChildItem -LiteralPath $licenseRoot -File -Recurse)
if ($licenseFiles.Count -le 1) {
    throw 'Artifact license corpus contains no exact package license bodies.'
}

[ordered]@{
    schema = 'interactive-npcs-artifact-legal-stage/v1'
    status = 'prepared'
    artifact_scope_path = $artifactScope
    artifact_scope_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $artifactScope).Hash.ToLowerInvariant()
    license_root = $licenseRoot
    license_index_path = $licenseIndex
    license_index_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $licenseIndex).Hash.ToLowerInvariant()
    license_file_count = $licenseFiles.Count
    sbom_path = $sbomPath
    sbom_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $sbomPath).Hash.ToLowerInvariant()
} | ConvertTo-Json -Compress | Write-Output
