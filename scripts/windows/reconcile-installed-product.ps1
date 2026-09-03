[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallRoot,
    [Parameter(Mandatory = $true)][string]$PackageManifestPath,
    [Parameter(Mandatory = $true)][string]$OutputManifestPath,
    [string]$RepositoryRoot,
    [string]$LedgerPath,
    [string]$PythonPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'node-tooling.ps1')

function Get-NpcSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function ConvertTo-NpcSafeInstalledPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    $normalized = $Path.Replace('\', '/')
    if ([string]::IsNullOrWhiteSpace($normalized) -or
        [System.IO.Path]::IsPathRooted($normalized) -or
        @($normalized.Split('/') | Where-Object { $_ -eq '' -or $_ -eq '.' -or $_ -eq '..' }).Count -gt 0) {
        throw "Unsafe installed manifest path: $Path"
    }
    return $normalized
}

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
}
$RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$InstallRoot = (Resolve-Path -LiteralPath $InstallRoot).Path.TrimEnd('\', '/')
$PackageManifestPath = (Resolve-Path -LiteralPath $PackageManifestPath).Path
if ([string]::IsNullOrWhiteSpace($LedgerPath)) {
    $LedgerPath = Join-Path $RepositoryRoot 'packaging/security/distribution-components.json'
}
$LedgerPath = (Resolve-Path -LiteralPath $LedgerPath).Path
if (-not [System.IO.Path]::IsPathRooted($OutputManifestPath)) {
    $OutputManifestPath = Join-Path (Get-Location).Path $OutputManifestPath
}
$OutputManifestPath = [System.IO.Path]::GetFullPath($OutputManifestPath)

$installItem = Get-Item -LiteralPath $InstallRoot -Force
if (($installItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw 'Installed distribution root must not be a reparse point.'
}
$reparseEntries = @(Get-ChildItem -LiteralPath $InstallRoot -Force -Recurse -ErrorAction Stop |
    Where-Object { ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 })
if ($reparseEntries.Count -gt 0) {
    throw "Installed distribution contains a reparse point: $($reparseEntries[0].FullName)"
}

$package = Get-Content -LiteralPath $PackageManifestPath -Raw | ConvertFrom-Json
$ledger = Get-Content -LiteralPath $LedgerPath -Raw | ConvertFrom-Json
if ($package.schema_version -ne 1 -or $package.distribution -ne 'local-review-only' -or
    $package.immutable_release_candidate -ne $false) {
    throw 'Installed reconciliation requires a schema-v1 pending local-review package manifest.'
}
if ($ledger.schema_version -ne 1 -or $null -eq $ledger.components -or $null -eq $ledger.static_files) {
    throw 'Distribution component ledger is invalid.'
}
foreach ($componentProperty in @($ledger.components.PSObject.Properties)) {
    $component = $componentProperty.Value
    $releaseStatus = if (@($component.PSObject.Properties.Name) -contains 'release_status') {
        [string]$component.release_status
    } else { '' }
    if ($component.redistributed -eq $true -and
        @($component.scopes) -contains 'base-installer' -and
        $releaseStatus.StartsWith('blocked', [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Installer component still has an active release blocker: $($componentProperty.Name) ($releaseStatus)"
    }
}

$expected = @{}
function Add-NpcExpectedInstalledFile {
    param(
        [Parameter(Mandatory = $true)][string]$RelativePath,
        [Parameter(Mandatory = $true)][string]$Sha256,
        [Parameter(Mandatory = $true)][string]$ComponentId
    )
    $relative = ConvertTo-NpcSafeInstalledPath -Path $RelativePath
    if ($Sha256 -notmatch '^[a-f0-9]{64}$') { throw "Invalid expected SHA-256 for $relative" }
    if ($expected.ContainsKey($relative)) { throw "Duplicate expected installed path: $relative" }
    $componentProperty = $ledger.components.PSObject.Properties[$ComponentId]
    if ($null -eq $componentProperty) { throw "Unknown installed component ID: $ComponentId" }
    $component = $componentProperty.Value
    $expected[$relative] = [ordered]@{
        path = $relative
        sha256 = $Sha256
        component_id = $ComponentId
        spdx = [string]$component.spdx
        distribution_scope = 'base-installer'
        source_reference = [string]$component.source_reference
        notice_reference = [string]$component.notice_reference
    }
}

$controlName = ConvertTo-NpcSafeInstalledPath -Path ([string]$package.control_executable.file_name)
Add-NpcExpectedInstalledFile -RelativePath $controlName `
    -Sha256 ([string]$package.control_executable.sha256) -ComponentId 'project:control-app'

$sidecarComponentIds = @{
    'npc-runtime.exe' = 'project:npc-runtime'
    'npc-media-broker.exe' = 'project:npc-media-broker'
    'npc-mouth-worker.exe' = 'project:npc-mouth-worker'
    'npc-subtitle-presenter.exe' = 'project:npc-subtitle-renderer'
}
$sidecarBinaries = @($package.sidecars.binaries)
if ($package.sidecars.schema -ne 'interactive-npcs-sidecars/v1' -or $sidecarBinaries.Count -ne 4) {
    throw 'Package manifest does not bind exactly four audited sidecars.'
}
foreach ($sidecar in $sidecarBinaries) {
    $installedName = ([string]$sidecar.file_name) -replace '-x86_64-pc-windows-msvc(?=\.exe$)', ''
    if (-not $sidecarComponentIds.ContainsKey($installedName)) {
        throw "Unexpected packaged sidecar name: $($sidecar.file_name)"
    }
    Add-NpcExpectedInstalledFile -RelativePath $installedName `
        -Sha256 ([string]$sidecar.sha256) -ComponentId $sidecarComponentIds[$installedName]
}
if (($sidecarComponentIds.Keys | Sort-Object) -join "`n" -ne
    (@($expected.Keys | Where-Object { $sidecarComponentIds.ContainsKey($_) } | Sort-Object) -join "`n")) {
    throw 'Package sidecar set differs from the four installed product children.'
}

foreach ($static in @($ledger.static_files)) {
    $installed = if (@($static.PSObject.Properties.Name) -contains 'install_path') {
        [string]$static.install_path
    } else {
        [string]$static.path
    }
    if ($installed -eq 'icon-resource-in-executable' -or $installed.StartsWith('product-audit/')) { continue }
    Add-NpcExpectedInstalledFile -RelativePath $installed `
        -Sha256 ([string]$static.sha256) -ComponentId ([string]$static.component_id)
}

$resourceEvidenceEntries = @($package.files | Where-Object { $_.file -eq 'resource-manifest.v1.json' })
if ($resourceEvidenceEntries.Count -ne 1) {
    throw 'Package evidence must contain exactly one product resource manifest.'
}
$installedResourceManifestPath = Join-Path $InstallRoot 'product-audit/audit/resource-manifest.v1.json'
if (-not (Test-Path -LiteralPath $installedResourceManifestPath -PathType Leaf) -or
    (Get-NpcSha256 -Path $installedResourceManifestPath) -ne [string]$resourceEvidenceEntries[0].sha256) {
    throw 'Installed resource manifest is missing or differs from package evidence.'
}
$installedResourceManifest = Get-Content -LiteralPath $installedResourceManifestPath -Raw | ConvertFrom-Json
if ($installedResourceManifest.schema -ne 'interactive-npcs-product-resources/v1' -or
    $installedResourceManifest.install_root -ne 'product-audit') {
    throw 'Installed resource manifest schema is invalid.'
}
$packagedResourceFiles = @($package.product_resources.files)
$installedResourceFiles = @($installedResourceManifest.files)
if ($package.product_resources.schema -ne 'interactive-npcs-product-resources/v1' -or
    $packagedResourceFiles.Count -ne $installedResourceFiles.Count) {
    throw 'Installed resource manifest differs from the package-bound resource set.'
}
$installedResourceByDestination = @{}
foreach ($entry in $installedResourceFiles) {
    $destination = ConvertTo-NpcSafeInstalledPath -Path ([string]$entry.destination)
    if ($installedResourceByDestination.ContainsKey($destination)) {
        throw "Duplicate installed resource manifest destination: $destination"
    }
    $installedResourceByDestination[$destination] = $entry
}
foreach ($entry in $packagedResourceFiles) {
    $destination = ConvertTo-NpcSafeInstalledPath -Path ([string]$entry.destination)
    if (-not $installedResourceByDestination.ContainsKey($destination) -or
        [string]$installedResourceByDestination[$destination].sha256 -ne [string]$entry.sha256 -or
        [long]$installedResourceByDestination[$destination].size_bytes -ne [long]$entry.size_bytes) {
        throw "Installed resource entry differs from package evidence: $destination"
    }
    $installedPath = "product-audit/$destination"
    $staticMatch = @($ledger.static_files | Where-Object {
            $candidate = if (@($_.PSObject.Properties.Name) -contains 'install_path') { [string]$_.install_path } else { [string]$_.path }
            $candidate -eq $installedPath
        })
    $componentId = if ($staticMatch.Count -eq 1) {
        [string]$staticMatch[0].component_id
    } elseif ($destination.StartsWith('assets/subtitles/') -or $destination.StartsWith('packaging/runtime-components/')) {
        'project:static-resources'
    } else {
        'project:legal-resources'
    }
    Add-NpcExpectedInstalledFile -RelativePath $installedPath `
        -Sha256 ([string]$entry.sha256) -ComponentId $componentId
}
Add-NpcExpectedInstalledFile -RelativePath 'product-audit/audit/resource-manifest.v1.json' `
    -Sha256 ([string]$resourceEvidenceEntries[0].sha256) -ComponentId 'project:legal-resources'

$uninstallerPath = Join-Path $InstallRoot 'uninstall.exe'
if (-not (Test-Path -LiteralPath $uninstallerPath -PathType Leaf)) {
    throw 'Installed NSIS uninstaller is missing.'
}
Add-NpcExpectedInstalledFile -RelativePath 'uninstall.exe' `
    -Sha256 (Get-NpcSha256 -Path $uninstallerPath) -ComponentId 'thirdparty:tauri-nsis-toolchain'

$transientRelative = 'product-audit/audit/installed-distribution-manifest.v1.json'
$transientPath = Join-Path $InstallRoot $transientRelative
if (Test-Path -LiteralPath $transientPath) {
    throw 'Installed tree unexpectedly already contains the transient reconciliation manifest.'
}
$document = [ordered]@{
    schema_version = 1
    files = @($expected.Values | Sort-Object path)
    manifest_self = [ordered]@{
        path = $transientRelative
        hash = 'excluded-to-avoid-circularity'
    }
}
$json = ($document | ConvertTo-Json -Depth 8) + [Environment]::NewLine
New-Item -ItemType Directory -Path (Split-Path -Parent $transientPath) -Force | Out-Null
[System.IO.File]::WriteAllText($transientPath, $json, (New-Object System.Text.UTF8Encoding($false)))

if ([string]::IsNullOrWhiteSpace($PythonPath)) {
    $python = Get-Command python -ErrorAction SilentlyContinue
    if ($null -eq $python) { $python = Get-Command python3 -ErrorAction SilentlyContinue }
    if ($null -eq $python -and -not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $python = Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Programs/Python') `
            -Directory -ErrorAction SilentlyContinue |
            ForEach-Object {
                Get-Item -LiteralPath (Join-Path $_.FullName 'python.exe') -ErrorAction SilentlyContinue
            } | Sort-Object FullName -Descending | Select-Object -First 1
    }
    if ($null -eq $python) { throw 'Python 3 is required for installed distribution reconciliation.' }
    $PythonPath = if ($python -is [System.IO.FileInfo]) { $python.FullName } else { $python.Source }
}
$reconcilerPath = Join-Path $RepositoryRoot 'scripts/security/reconcile_distribution.py'
$outputDirectory = Split-Path -Parent $OutputManifestPath
if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
}
try {
    $reconcilerArguments = @(
        $reconcilerPath,
        '--root', $InstallRoot,
        '--manifest', $transientPath,
        '--ledger', $LedgerPath,
        '--kind', 'installer'
    )
    $process = Invoke-NpcHiddenProcess -FilePath $PythonPath `
        -ArgumentList $reconcilerArguments -NoReplayOutput
    if ($process.ExitCode -ne 0) {
        throw "Installed distribution reconciliation failed: $($process.StandardError)$($process.StandardOutput)"
    }
    $reconciliation = $process.StandardOutput | ConvertFrom-Json
    if ($reconciliation.status -ne 'passed') {
        throw 'Installed distribution reconciler did not report passed.'
    }
    Copy-Item -LiteralPath $transientPath -Destination $OutputManifestPath -Force
    $outputHash = Get-NpcSha256 -Path $OutputManifestPath
    [ordered]@{
        schema_version = 1
        status = 'passed'
        installed_file_count = [int]$reconciliation.files
        manifested_file_count = [int]$reconciliation.manifested_files
        evidence_manifest_path = $OutputManifestPath
        evidence_manifest_sha256 = $outputHash
        installed_legal_resources_reconciled = $true
        unclassified_installed_files_rejected = $true
        exact_four_sidecars_reconciled = $true
    } | ConvertTo-Json -Compress | Write-Output
}
finally {
    if (Test-Path -LiteralPath $transientPath -PathType Leaf) {
        Remove-Item -LiteralPath $transientPath -Force
    }
}
