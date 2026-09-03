[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-True {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw $Message }
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) `
    "npc-resource-stage-$([Guid]::NewGuid().ToString('N'))"
$destination = Join-Path $fixtureRoot 'generated-resources'
$sbomPath = Join-Path $fixtureRoot 'lockfiles.cdx.json'
$sidecarManifestPath = Join-Path $fixtureRoot 'sidecar-manifest.v1.json'
$artifactScopePath = Join-Path $fixtureRoot 'windows-artifact-scope.json'
$licenseRoot = Join-Path $fixtureRoot 'legal/packages'
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

try {
    [ordered]@{
        bomFormat = 'CycloneDX'
        specVersion = '1.6'
        components = @([ordered]@{ type = 'application'; name = 'fixture' })
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $sbomPath -Encoding UTF8
    $sidecarNames = @(
        'npc-runtime-x86_64-pc-windows-msvc.exe',
        'npc-media-broker-x86_64-pc-windows-msvc.exe',
        'npc-mouth-worker-x86_64-pc-windows-msvc.exe',
        'npc-subtitle-presenter-x86_64-pc-windows-msvc.exe'
    )
    [ordered]@{
        schema = 'interactive-npcs-sidecars/v1'
        binaries = @($sidecarNames | ForEach-Object { [ordered]@{ file_name = $_ } })
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $sidecarManifestPath -Encoding UTF8
    [ordered]@{
        schema_version = 1
        artifact_id = 'windows-review-installer'
        components = [ordered]@{ 'pkg:cargo/fixture@1.0.0' = 'required' }
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $artifactScopePath -Encoding UTF8
    $licenseComponentRoot = Join-Path $licenseRoot 'components/0001-fixture-1.0.0'
    New-Item -ItemType Directory -Path $licenseComponentRoot -Force | Out-Null
    $licenseBody = Join-Path $licenseComponentRoot 'LICENSE'
    [System.IO.File]::WriteAllText($licenseBody, 'Fixture license body')
    [ordered]@{
        schema_version = 1
        artifact_id = 'windows-review-installer'
        components = [ordered]@{
            'pkg:cargo/fixture@1.0.0' = @([ordered]@{
                    path = 'components/0001-fixture-1.0.0/LICENSE'
                    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $licenseBody).Hash.ToLowerInvariant()
                    bytes = (Get-Item -LiteralPath $licenseBody).Length
                })
        }
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $licenseRoot 'THIRD-PARTY-LICENSE-FILES.json') -Encoding UTF8

    $result = (& (Join-Path $PSScriptRoot 'stage-product-resources.ps1') `
            -SbomPath $sbomPath `
            -SidecarManifestPath $sidecarManifestPath `
            -ArtifactScopePath $artifactScopePath `
            -LicenseMaterialRoot $licenseRoot `
            -DestinationRoot $destination `
            -AllowDisposableDestination) | ConvertFrom-Json
    Assert-True -Condition ($result.status -eq 'staged') -Message 'Resource staging did not report success.'
    Assert-True -Condition ($result.file_count -eq 28) -Message 'Resource staging did not include all static, scope, reviewed override/toolchain, index, and license-body artifacts.'
    Assert-True -Condition (Test-Path -LiteralPath $result.manifest_path -PathType Leaf) -Message 'Resource manifest is missing.'

    $manifest = Get-Content -LiteralPath $result.manifest_path -Raw | ConvertFrom-Json
    Assert-True -Condition ($manifest.schema -eq 'interactive-npcs-product-resources/v1') -Message 'Resource manifest schema is wrong.'
    Assert-True -Condition (@($manifest.files).Count -eq 28) -Message 'Resource manifest is incomplete.'
    foreach ($entry in $manifest.files) {
        $stagedPath = Join-Path $destination ([string]$entry.destination)
        Assert-True -Condition (Test-Path -LiteralPath $stagedPath -PathType Leaf) -Message "Missing staged resource: $($entry.destination)"
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $stagedPath).Hash.ToLowerInvariant()
        Assert-True -Condition ($hash -eq $entry.sha256) -Message "Hash mismatch: $($entry.destination)"
    }

    $invalidSbom = Join-Path $fixtureRoot 'invalid-sbom.json'
    '{}' | Set-Content -LiteralPath $invalidSbom -Encoding UTF8
    $invalidRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'stage-product-resources.ps1') `
            -SbomPath $invalidSbom `
            -SidecarManifestPath $sidecarManifestPath `
            -DestinationRoot $destination `
            -AllowDisposableDestination | Out-Null
    }
    catch { $invalidRejected = $_.Exception.Message -match 'CycloneDX 1.6' }
    Assert-True -Condition $invalidRejected -Message 'Invalid SBOM was not rejected.'

    $unsafeDestination = Join-Path $PSScriptRoot '../important/generated-resources'
    $unsafeRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'stage-product-resources.ps1') `
            -SbomPath $sbomPath `
            -SidecarManifestPath $sidecarManifestPath `
            -DestinationRoot $unsafeDestination `
            -AllowDisposableDestination | Out-Null
    }
    catch { $unsafeRejected = $_.Exception.Message -match 'unsafe generated resource destination' }
    Assert-True -Condition $unsafeRejected -Message 'A noncanonical recursive-delete target was accepted.'

    $unapprovedDisposableRejected = $false
    try {
        & (Join-Path $PSScriptRoot 'stage-product-resources.ps1') `
            -SbomPath $sbomPath `
            -SidecarManifestPath $sidecarManifestPath `
            -DestinationRoot $destination | Out-Null
    }
    catch { $unapprovedDisposableRejected = $_.Exception.Message -match 'unsafe generated resource destination' }
    Assert-True -Condition $unapprovedDisposableRejected -Message 'Disposable destination did not require explicit test authorization.'

    Write-Host 'Product resource staging regression checks passed.' -ForegroundColor Green
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
    }
}

exit 0
