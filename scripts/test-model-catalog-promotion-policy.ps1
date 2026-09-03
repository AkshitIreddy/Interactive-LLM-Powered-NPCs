[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'windows/model-catalog-promotion-policy.ps1')
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$catalogDirectory = Join-Path $repoRoot 'packaging/model-packs'
$catalogPath = Join-Path $catalogDirectory 'model-catalog-v1.json'
$bootstrapRootPath = Join-Path $catalogDirectory 'model-catalog-root-v1.json'
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
$now = [long]$catalog.signed.generated_unix_seconds + 1

$bootstrap = Test-NpcModelCatalogPromotionPolicy -RootPath $bootstrapRootPath -CatalogPath $catalogPath -NowUnixSeconds $now
if ($bootstrap.eligible -or $bootstrap.production_trust -or -not $bootstrap.rotation_required_before_release -or
    $bootstrap.promotion_supported -or $bootstrap.publication_supported) {
    throw 'The checked bootstrap trust root must remain ineligible for production promotion.'
}

$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("npc-model-catalog-policy-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
try {
    $productionRoot = Get-Content -LiteralPath $bootstrapRootPath -Raw | ConvertFrom-Json
    $productionRoot.trustScope = 'production_release'
    $productionRoot.productionTrust = $true
    $productionRoot.rotationRequiredBeforeRelease = $false
    $productionRoot.promotionSupported = $true
    $productionRoot.publicationSupported = $true
    $productionPath = Join-Path $testRoot 'production-root.json'
    [System.IO.File]::WriteAllText(
        $productionPath,
        ($productionRoot | ConvertTo-Json -Depth 20),
        (New-Object System.Text.UTF8Encoding($false)))

    $eligible = Test-NpcModelCatalogPromotionPolicy -RootPath $productionPath -CatalogPath $catalogPath -NowUnixSeconds $now
    if (-not $eligible.eligible -or -not $eligible.declared_signature_shape_thresholds_satisfied) {
        throw "A structurally valid production promotion policy was rejected: $($eligible.blockers -join '; ')"
    }

    $expired = Test-NpcModelCatalogPromotionPolicy -RootPath $productionPath -CatalogPath $catalogPath `
        -NowUnixSeconds ([long]$catalog.signed.expires_unix_seconds)
    if ($expired.eligible) { throw 'Promotion policy accepted an expired catalog.' }

    $missing = Test-NpcModelCatalogPromotionPolicy -RootPath (Join-Path $testRoot 'missing-root.json') `
        -CatalogPath $catalogPath -NowUnixSeconds $now
    if ($missing.eligible) { throw 'Promotion policy accepted a missing trust root.' }

    $tamperedCatalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
    $tamperedCatalog.signatures[0].signature = 'not-an-ed25519-signature'
    $tamperedCatalogPath = Join-Path $testRoot 'tampered-catalog.json'
    [System.IO.File]::WriteAllText(
        $tamperedCatalogPath,
        ($tamperedCatalog | ConvertTo-Json -Depth 100),
        (New-Object System.Text.UTF8Encoding($false)))
    $signatureRejected = Test-NpcModelCatalogPromotionPolicy -RootPath $productionPath `
        -CatalogPath $tamperedCatalogPath -NowUnixSeconds $now
    if ($signatureRejected.eligible) { throw 'Promotion policy accepted a malformed declared signature set.' }

    foreach ($mutation in @(
        @{ name = 'rotation'; property = 'rotationRequiredBeforeRelease'; value = $true },
        @{ name = 'promotion'; property = 'promotionSupported'; value = $false },
        @{ name = 'publication'; property = 'publicationSupported'; value = $false },
        @{ name = 'trust'; property = 'productionTrust'; value = $false },
        @{ name = 'type-confusion'; property = 'productionTrust'; value = 'true' }
    )) {
        $tampered = Get-Content -LiteralPath $productionPath -Raw | ConvertFrom-Json
        $tampered.($mutation.property) = $mutation.value
        $tamperedPath = Join-Path $testRoot ("tampered-$($mutation.name).json")
        [System.IO.File]::WriteAllText(
            $tamperedPath,
            ($tampered | ConvertTo-Json -Depth 20),
            (New-Object System.Text.UTF8Encoding($false)))
        $rejected = Test-NpcModelCatalogPromotionPolicy -RootPath $tamperedPath -CatalogPath $catalogPath -NowUnixSeconds $now
        if ($rejected.eligible) { throw "Promotion policy accepted the $($mutation.name) negative fixture." }
    }
}
finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}

Write-Host 'Model catalog promotion policy regressions passed.' -ForegroundColor Green
