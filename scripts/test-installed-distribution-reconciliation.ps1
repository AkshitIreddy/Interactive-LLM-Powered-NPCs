[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-True {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw $Message }
}

function Write-JsonNoBom {
    param([Parameter(Mandatory = $true)]$Value, [Parameter(Mandatory = $true)][string]$Path)
    $parent = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    $json = ($Value | ConvertTo-Json -Depth 12) + [Environment]::NewLine
    [System.IO.File]::WriteAllText($Path, $json, (New-Object System.Text.UTF8Encoding($false)))
}

function Get-Hash {
    param([string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) `
    "npc-installed-reconcile-$([Guid]::NewGuid().ToString('N'))"
$installRoot = Join-Path $fixtureRoot 'installed app'
$packageRoot = Join-Path $fixtureRoot 'package evidence'
New-Item -ItemType Directory -Path $installRoot, $packageRoot -Force | Out-Null

try {
    $componentDefinitions = [ordered]@{}
    foreach ($definition in @(
            @('project:control-app', 'MIT'),
            @('project:npc-runtime', 'MIT'),
            @('project:npc-media-broker', 'MIT'),
            @('project:npc-mouth-worker', 'MIT'),
            @('project:npc-subtitle-renderer', 'MIT'),
            @('project:static-resources', 'MIT'),
            @('project:legal-resources', 'MIT'),
            @('thirdparty:tauri-nsis-toolchain', 'MIT')
        )) {
        $componentDefinitions[$definition[0]] = [ordered]@{
            spdx = $definition[1]
            redistributed = $true
            scopes = @('base-installer')
            source_reference = "fixture-source/$($definition[0])"
            notice_reference = 'product-audit/legal/NOTICE.md'
        }
    }

    $staticPath = Join-Path $installRoot 'static/fixture.txt'
    New-Item -ItemType Directory -Path (Split-Path -Parent $staticPath) -Force | Out-Null
    [System.IO.File]::WriteAllText($staticPath, 'static fixture')
    $transientRelative = 'product-audit/audit/installed-distribution-manifest.v1.json'
    $requiredLegal = @(
        'product-audit/legal/distribution-components.json',
        'product-audit/legal/windows-artifact-scope.json',
        'product-audit/legal/packages/THIRD-PARTY-LICENSE-FILES.json',
        'product-audit/legal/packages/components/0001-example/LICENSE',
        'product-audit/legal/lockfiles.cdx.json',
        $transientRelative
    )
    $ledger = [ordered]@{
        schema_version = 1
        components = $componentDefinitions
        static_files = @([ordered]@{
                path = 'fixture-source/static.txt'
                install_path = 'static/fixture.txt'
                sha256 = Get-Hash -Path $staticPath
                component_id = 'project:static-resources'
            })
        distribution_profiles = [ordered]@{
            installer = [ordered]@{
                scope = 'base-installer'
                required_install_paths = $requiredLegal
                forbidden_component_ids = @()
            }
        }
    }
    $ledgerPath = Join-Path $fixtureRoot 'distribution-components.json'
    Write-JsonNoBom -Value $ledger -Path $ledgerPath

    $legalRoot = Join-Path $installRoot 'product-audit/legal'
    $installedLedgerPath = Join-Path $legalRoot 'distribution-components.json'
    New-Item -ItemType Directory -Path $legalRoot -Force | Out-Null
    Copy-Item -LiteralPath $ledgerPath -Destination $installedLedgerPath
    $licenseBody = Join-Path $legalRoot 'packages/components/0001-example/LICENSE'
    New-Item -ItemType Directory -Path (Split-Path -Parent $licenseBody) -Force | Out-Null
    [System.IO.File]::WriteAllText($licenseBody, 'exact fixture license body')
    $scopePath = Join-Path $legalRoot 'windows-artifact-scope.json'
    Write-JsonNoBom -Path $scopePath -Value ([ordered]@{
            schema_version = 1
            artifact_id = 'windows-review-installer'
            components = [ordered]@{ 'pkg:cargo/example@1.0.0' = 'required' }
        })
    $indexPath = Join-Path $legalRoot 'packages/THIRD-PARTY-LICENSE-FILES.json'
    Write-JsonNoBom -Path $indexPath -Value ([ordered]@{
            schema_version = 1
            artifact_id = 'windows-review-installer'
            components = [ordered]@{
                'pkg:cargo/example@1.0.0' = @([ordered]@{
                        path = 'components/0001-example/LICENSE'
                        sha256 = Get-Hash -Path $licenseBody
                        bytes = (Get-Item -LiteralPath $licenseBody).Length
                    })
            }
        })
    $sbomPath = Join-Path $legalRoot 'lockfiles.cdx.json'
    Write-JsonNoBom -Path $sbomPath -Value ([ordered]@{
            bomFormat = 'CycloneDX'
            specVersion = '1.6'
            metadata = [ordered]@{ properties = @(
                    [ordered]@{ name = 'interactive-npcs:artifact-id'; value = 'windows-review-installer' },
                    [ordered]@{ name = 'interactive-npcs:artifact-scope-resolved'; value = 'true' },
                    [ordered]@{ name = 'interactive-npcs:license-material-index-sha256'; value = Get-Hash -Path $indexPath }
                ) }
            components = @([ordered]@{
                    type = 'library'
                    name = 'example'
                    version = '1.0.0'
                    purl = 'pkg:cargo/example@1.0.0'
                    scope = 'required'
                })
        })

    $binaryContents = [ordered]@{
        'interactive-npcs-control.exe' = 'control release fixture'
        'npc-runtime.exe' = 'runtime release fixture'
        'npc-media-broker.exe' = 'broker release fixture'
        'npc-mouth-worker.exe' = 'mouth release fixture'
        'npc-subtitle-presenter.exe' = 'subtitle release fixture'
        'uninstall.exe' = 'tauri nsis release fixture'
    }
    foreach ($entry in $binaryContents.GetEnumerator()) {
        [System.IO.File]::WriteAllText((Join-Path $installRoot $entry.Key), [string]$entry.Value)
    }

    $resourceMappings = @(
        @('legal/distribution-components.json', $installedLedgerPath),
        @('legal/windows-artifact-scope.json', $scopePath),
        @('legal/packages/THIRD-PARTY-LICENSE-FILES.json', $indexPath),
        @('legal/packages/components/0001-example/LICENSE', $licenseBody),
        @('legal/lockfiles.cdx.json', $sbomPath)
    )
    $resourceEntries = @($resourceMappings | ForEach-Object {
            [ordered]@{
                destination = $_[0]
                source = "generated/$($_[0])"
                sha256 = Get-Hash -Path $_[1]
                size_bytes = (Get-Item -LiteralPath $_[1]).Length
            }
        })
    $resourceManifest = [ordered]@{
        schema = 'interactive-npcs-product-resources/v1'
        install_root = 'product-audit'
        files = $resourceEntries
    }
    $installedResourceManifestPath = Join-Path $installRoot 'product-audit/audit/resource-manifest.v1.json'
    Write-JsonNoBom -Value $resourceManifest -Path $installedResourceManifestPath

    $sidecars = @(
        @('npc-runtime-x86_64-pc-windows-msvc.exe', 'npc-runtime.exe'),
        @('npc-media-broker-x86_64-pc-windows-msvc.exe', 'npc-media-broker.exe'),
        @('npc-mouth-worker-x86_64-pc-windows-msvc.exe', 'npc-mouth-worker.exe'),
        @('npc-subtitle-presenter-x86_64-pc-windows-msvc.exe', 'npc-subtitle-presenter.exe')
    )
    $packageManifest = [ordered]@{
        schema_version = 1
        distribution = 'local-review-only'
        immutable_release_candidate = $false
        control_executable = [ordered]@{
            file_name = 'interactive-npcs-control.exe'
            sha256 = Get-Hash -Path (Join-Path $installRoot 'interactive-npcs-control.exe')
        }
        sidecars = [ordered]@{
            schema = 'interactive-npcs-sidecars/v1'
            binaries = @($sidecars | ForEach-Object {
                    [ordered]@{
                        file_name = $_[0]
                        sha256 = Get-Hash -Path (Join-Path $installRoot $_[1])
                    }
                })
        }
        product_resources = $resourceManifest
        files = @([ordered]@{
                file = 'resource-manifest.v1.json'
                sha256 = Get-Hash -Path $installedResourceManifestPath
                size_bytes = (Get-Item -LiteralPath $installedResourceManifestPath).Length
            })
    }
    $packageManifestPath = Join-Path $packageRoot 'package-manifest.json'
    Write-JsonNoBom -Value $packageManifest -Path $packageManifestPath

    $firstEvidence = Join-Path $packageRoot 'installed-files-first.json'
    $first = (& (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot -PackageManifestPath $packageManifestPath `
            -OutputManifestPath $firstEvidence -RepositoryRoot $repoRoot -LedgerPath $ledgerPath) | ConvertFrom-Json
    Assert-True -Condition ($first.status -eq 'passed' -and $first.exact_four_sidecars_reconciled) `
        -Message 'Exact installed fixture did not reconcile.'
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $installRoot $transientRelative))) `
        -Message 'Transient reconciliation manifest was not removed from the installed fixture.'

    $secondEvidence = Join-Path $packageRoot 'installed-files-second.json'
    $second = (& (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot -PackageManifestPath $packageManifestPath `
            -OutputManifestPath $secondEvidence -RepositoryRoot $repoRoot -LedgerPath $ledgerPath) | ConvertFrom-Json
    Assert-True -Condition ($first.evidence_manifest_sha256 -eq $second.evidence_manifest_sha256) `
        -Message 'Installed manifest generation is not deterministic.'

    $originalLedgerBytes = [System.IO.File]::ReadAllBytes($ledgerPath)
    $blockedLedger = Get-Content -LiteralPath $ledgerPath -Raw | ConvertFrom-Json
    $blockedLedger.components.'thirdparty:tauri-nsis-toolchain' | Add-Member `
        -NotePropertyName release_status -NotePropertyValue 'blocked-fixture-inputs-unreviewed'
    Write-JsonNoBom -Value $blockedLedger -Path $ledgerPath
    Copy-Item -LiteralPath $ledgerPath -Destination $installedLedgerPath -Force
    $blockedRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot -PackageManifestPath $packageManifestPath `
            -OutputManifestPath (Join-Path $packageRoot 'unexpected-blocked.json') `
            -RepositoryRoot $repoRoot -LedgerPath $ledgerPath | Out-Null
    }
    catch { $blockedRejected = $_.Exception.Message -match 'active release blocker' }
    Assert-True -Condition $blockedRejected -Message 'Blocked installer component was not rejected.'
    [System.IO.File]::WriteAllBytes($ledgerPath, $originalLedgerBytes)
    [System.IO.File]::WriteAllBytes($installedLedgerPath, $originalLedgerBytes)

    [System.IO.File]::WriteAllText((Join-Path $installRoot 'surprise.dll'), 'unclassified')
    $extraRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot -PackageManifestPath $packageManifestPath `
            -OutputManifestPath (Join-Path $packageRoot 'unexpected-extra.json') `
            -RepositoryRoot $repoRoot -LedgerPath $ledgerPath | Out-Null
    }
    catch { $extraRejected = $_.Exception.Message -match 'unexplained' }
    Assert-True -Condition $extraRejected -Message 'Unclassified installed file was not rejected.'
    Remove-Item -LiteralPath (Join-Path $installRoot 'surprise.dll') -Force

    [System.IO.File]::WriteAllText((Join-Path $installRoot 'npc-mouth-worker.exe'), 'tampered')
    $tamperRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot -PackageManifestPath $packageManifestPath `
            -OutputManifestPath (Join-Path $packageRoot 'unexpected-tamper.json') `
            -RepositoryRoot $repoRoot -LedgerPath $ledgerPath | Out-Null
    }
    catch { $tamperRejected = $_.Exception.Message -match 'SHA-256 mismatch' }
    Assert-True -Condition $tamperRejected -Message 'Tampered sidecar was not rejected.'

    Write-Host 'Installed distribution reconciliation regression checks passed.' -ForegroundColor Green
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
    }
}

exit 0
