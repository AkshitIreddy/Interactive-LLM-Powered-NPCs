[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$SbomPath,
    [Parameter(Mandatory = $true)][string]$SidecarManifestPath,
    [string]$ArtifactScopePath,
    [string]$LicenseMaterialRoot,
    [string]$DestinationRoot,
    [switch]$AllowDisposableDestination
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'windows/product-resource-paths.ps1')

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($DestinationRoot)) {
    $DestinationRoot = Join-Path $repoRoot 'apps/control/src-tauri/generated-resources'
}
if (-not [System.IO.Path]::IsPathRooted($DestinationRoot)) {
    $DestinationRoot = Join-Path $repoRoot $DestinationRoot
}
$DestinationRoot = Assert-NpcGeneratedResourceRoot `
    -RepositoryRoot $repoRoot `
    -Path $DestinationRoot `
    -AllowDisposableDestination:$AllowDisposableDestination

$sbom = Get-Content -LiteralPath (Resolve-Path -LiteralPath $SbomPath) -Raw | ConvertFrom-Json
$sbomFields = @($sbom.PSObject.Properties | ForEach-Object { $_.Name })
if ($sbomFields -notcontains 'bomFormat' -or $sbomFields -notcontains 'specVersion' -or
    $sbomFields -notcontains 'components' -or $sbom.bomFormat -ne 'CycloneDX' -or
    $sbom.specVersion -ne '1.6' -or @($sbom.components).Count -eq 0) {
    throw 'The staged SBOM must be a non-empty CycloneDX 1.6 document.'
}
$sidecarManifest = Get-Content -LiteralPath `
    (Resolve-Path -LiteralPath $SidecarManifestPath) -Raw | ConvertFrom-Json
$expectedSidecars = @(
    'npc-media-broker-x86_64-pc-windows-msvc.exe',
    'npc-mouth-worker-x86_64-pc-windows-msvc.exe',
    'npc-runtime-x86_64-pc-windows-msvc.exe',
    'npc-subtitle-presenter-x86_64-pc-windows-msvc.exe'
)
$sidecarFields = @($sidecarManifest.PSObject.Properties | ForEach-Object { $_.Name })
if ($sidecarFields -notcontains 'schema' -or $sidecarFields -notcontains 'binaries' -or
    $sidecarManifest.schema -ne 'interactive-npcs-sidecars/v1') {
    throw 'The sidecar manifest schema is not interactive-npcs-sidecars/v1.'
}
$observedSidecars = @($sidecarManifest.binaries | ForEach-Object { [string]$_.file_name } | Sort-Object)
if (($observedSidecars -join "`n") -ne (($expectedSidecars | Sort-Object) -join "`n")) {
    throw "The sidecar manifest must contain exactly: $($expectedSidecars -join ', ')."
}

$sources = @(
    [pscustomobject]@{ Source = 'LICENSE'; Destination = 'legal/LICENSE.txt' },
    [pscustomobject]@{ Source = 'docs/legal/third-party-notices.md'; Destination = 'legal/THIRD-PARTY-NOTICES.md' },
    [pscustomobject]@{ Source = 'packaging/security/distribution-components.json'; Destination = 'legal/distribution-components.json' },
    [pscustomobject]@{ Source = 'packaging/security/manual-license-review-2026-08-30.md'; Destination = 'legal/manual-license-review-2026-08-30.md' },
    [pscustomobject]@{ Source = 'packaging/security/license-material-overrides.json'; Destination = 'legal/license-material-overrides.json' },
    [pscustomobject]@{ AbsoluteSource = (Resolve-Path -LiteralPath $SbomPath).Path; Source = 'generated/lockfiles.cdx.json'; Destination = 'legal/lockfiles.cdx.json' },
    [pscustomobject]@{ Source = 'crates/providers-tts/proto/LICENSE'; Destination = 'legal/licenses/NVIDIA-RIVA-PROTO-MIT.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/Nugine-simd-d74c030-MIT.txt'; Destination = 'legal/licenses/Nugine-simd-d74c030-MIT.txt' },
    [pscustomobject]@{ Source = 'packaging/security/installer-toolchain-provenance.json'; Destination = 'legal/installer-toolchain-provenance.json' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/NSIS-3.11-COPYING.txt'; Destination = 'legal/licenses/NSIS-3.11-COPYING.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/nsis-tauri-utils-v0.5.3-APACHE-2.0.txt'; Destination = 'legal/licenses/nsis-tauri-utils-v0.5.3-APACHE-2.0.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/nsis-tauri-utils-v0.5.3-MIT.txt'; Destination = 'legal/licenses/nsis-tauri-utils-v0.5.3-MIT.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/Dropbox-rust-alloc-no-stdlib-ae42d22-BSD-3-Clause.txt'; Destination = 'legal/licenses/Dropbox-rust-alloc-no-stdlib-ae42d22-BSD-3-Clause.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/Mozilla-MPL-2.0.txt'; Destination = 'legal/licenses/Mozilla-MPL-2.0.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/rust-unic-0.9.0-MIT.txt'; Destination = 'legal/licenses/rust-unic-0.9.0-MIT.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/rust-unic-0.9.0-APACHE-2.0.txt'; Destination = 'legal/licenses/rust-unic-0.9.0-APACHE-2.0.txt' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/rust-unic-0.9.0-COPYRIGHT.md'; Destination = 'legal/licenses/rust-unic-0.9.0-COPYRIGHT.md' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/rust-unic-0.9.0-AUTHORS'; Destination = 'legal/licenses/rust-unic-0.9.0-AUTHORS' },
    [pscustomobject]@{ Source = 'docs/legal/licenses/webview2-rs-0.38.2-MIT.txt'; Destination = 'legal/licenses/webview2-rs-0.38.2-MIT.txt' },
    [pscustomobject]@{ Source = 'assets/subtitles/fonts.v1.json'; Destination = 'assets/subtitles/fonts.v1.json' },
    [pscustomobject]@{ Source = 'assets/subtitles/licenses.v1.json'; Destination = 'assets/subtitles/licenses.v1.json' },
    [pscustomobject]@{ Source = 'assets/subtitles/styles.v1.json'; Destination = 'assets/subtitles/styles.v1.json' },
    [pscustomobject]@{ Source = 'assets/subtitles/README.md'; Destination = 'assets/subtitles/README.md' },
    [pscustomobject]@{ Source = 'packaging/runtime-components/onnxruntime-1.22.1-cpu.json'; Destination = 'packaging/runtime-components/onnxruntime-1.22.1-cpu.json' },
    [pscustomobject]@{ AbsoluteSource = (Resolve-Path -LiteralPath $SidecarManifestPath).Path; Source = 'generated/sidecar-manifest.v1.json'; Destination = 'audit/sidecar-manifest.v1.json' }
)
if ([string]::IsNullOrWhiteSpace($ArtifactScopePath) -ne
    [string]::IsNullOrWhiteSpace($LicenseMaterialRoot)) {
    throw 'Artifact scope and license-material root must be staged together.'
}
if (-not [string]::IsNullOrWhiteSpace($ArtifactScopePath)) {
    $artifactScope = Get-Content -LiteralPath (Resolve-Path -LiteralPath $ArtifactScopePath) -Raw | ConvertFrom-Json
    if ($artifactScope.schema_version -ne 1 -or
        $artifactScope.artifact_id -ne 'windows-review-installer' -or
        @($artifactScope.components.PSObject.Properties).Count -eq 0) {
        throw 'Artifact scope is invalid or empty.'
    }
    $sources += [pscustomobject]@{
        AbsoluteSource = (Resolve-Path -LiteralPath $ArtifactScopePath).Path
        Source = 'generated/windows-artifact-scope.json'
        Destination = 'legal/windows-artifact-scope.json'
    }
}
if (-not [string]::IsNullOrWhiteSpace($LicenseMaterialRoot)) {
    $resolvedLicenseRoot = (Resolve-Path -LiteralPath $LicenseMaterialRoot).Path
    $licenseFiles = @(Get-ChildItem -LiteralPath $resolvedLicenseRoot -File -Recurse | Sort-Object FullName)
    $licenseIndex = Join-Path $resolvedLicenseRoot 'THIRD-PARTY-LICENSE-FILES.json'
    if ($licenseFiles.Count -le 1 -or -not (Test-Path -LiteralPath $licenseIndex -PathType Leaf)) {
        throw 'Exact package license corpus is missing or empty.'
    }
    $licenseIndexDocument = Get-Content -LiteralPath $licenseIndex -Raw | ConvertFrom-Json
    if ($licenseIndexDocument.schema_version -ne 1 -or
        $licenseIndexDocument.artifact_id -ne $artifactScope.artifact_id -or
        @($licenseIndexDocument.components.PSObject.Properties).Count -eq 0) {
        throw 'Exact package license index is invalid or differs from artifact scope.'
    }
    foreach ($licenseFile in $licenseFiles) {
        $relative = $licenseFile.FullName.Substring($resolvedLicenseRoot.Length).TrimStart('\', '/')
        if ($relative -match '(^|[\\/])\.\.([\\/]|$)' -or [System.IO.Path]::IsPathRooted($relative)) {
            throw "Unsafe license-material path: $relative"
        }
        $sources += [pscustomobject]@{
            AbsoluteSource = $licenseFile.FullName
            Source = "generated/legal/packages/$($relative.Replace('\', '/'))"
            Destination = "legal/packages/$($relative.Replace('\', '/'))"
        }
    }
}

# Resolve the complete allowlist before touching an earlier generated stage.
# A missing canonical source therefore fails closed without leaving a partial
# directory that a subsequent Tauri invocation could mistake for complete.
foreach ($mapping in $sources) {
    $hasAbsoluteSource = @($mapping.PSObject.Properties.Name) -contains 'AbsoluteSource'
    $sourcePath = if ($hasAbsoluteSource) {
        [string]$mapping.AbsoluteSource
    } else {
        Join-Path $repoRoot ([string]$mapping.Source)
    }
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Required packaged resource is missing: $sourcePath"
    }
}

Remove-NpcGeneratedResourceStage `
    -RepositoryRoot $repoRoot `
    -Path $DestinationRoot `
    -AllowDisposableDestination:$AllowDisposableDestination
New-Item -ItemType Directory -Path $DestinationRoot -Force | Out-Null

$entries = foreach ($mapping in $sources | Sort-Object Destination) {
    $hasAbsoluteSource = @($mapping.PSObject.Properties.Name) -contains 'AbsoluteSource'
    $sourcePath = if ($hasAbsoluteSource) {
        [string]$mapping.AbsoluteSource
    } else {
        Join-Path $repoRoot ([string]$mapping.Source)
    }
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Required packaged resource is missing: $sourcePath"
    }
    $destinationPath = Join-Path $DestinationRoot ([string]$mapping.Destination)
    New-Item -ItemType Directory -Path (Split-Path -Parent $destinationPath) -Force | Out-Null
    Copy-Item -LiteralPath $sourcePath -Destination $destinationPath -Force
    $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $sourcePath).Hash.ToLowerInvariant()
    $destinationHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destinationPath).Hash.ToLowerInvariant()
    if ($sourceHash -ne $destinationHash) {
        throw "Packaged resource hash changed while staging: $($mapping.Destination)"
    }
    [ordered]@{
        destination = ([string]$mapping.Destination).Replace('\', '/')
        source = ([string]$mapping.Source).Replace('\', '/')
        sha256 = $destinationHash
        size_bytes = (Get-Item -LiteralPath $destinationPath).Length
    }
}

$manifest = [ordered]@{
    schema = 'interactive-npcs-product-resources/v1'
    install_root = 'product-audit'
    files = @($entries)
}
$manifestPath = Join-Path $DestinationRoot 'audit/resource-manifest.v1.json'
$manifestJson = ($manifest | ConvertTo-Json -Depth 5) + [Environment]::NewLine
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, (New-Object System.Text.UTF8Encoding($false)))

[ordered]@{
    schema = 'interactive-npcs-product-resource-stage/v1'
    status = 'staged'
    destination_root = $DestinationRoot
    manifest_path = $manifestPath
    manifest_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $manifestPath).Hash.ToLowerInvariant()
    file_count = @($entries).Count
} | ConvertTo-Json -Compress | Write-Output
