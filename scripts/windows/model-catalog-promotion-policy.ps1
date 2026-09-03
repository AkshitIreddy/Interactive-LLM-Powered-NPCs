Set-StrictMode -Version 2.0

function Test-NpcModelCatalogPromotionPolicy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RootPath,
        [Parameter(Mandatory = $true)][string]$CatalogPath,
        [long]$NowUnixSeconds = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    )

    $blockers = New-Object System.Collections.Generic.List[string]
    $rootHash = $null
    $catalogHash = $null
    $root = $null
    $catalog = $null

    if (-not (Test-Path -LiteralPath $RootPath -PathType Leaf)) {
        $blockers.Add('model catalog trust root is missing')
    }
    else {
        try {
            $rootHash = (Get-FileHash -LiteralPath $RootPath -Algorithm SHA256).Hash.ToLowerInvariant()
            $root = Get-Content -LiteralPath $RootPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        }
        catch {
            $blockers.Add('model catalog trust root is unreadable or invalid JSON')
        }
    }
    if (-not (Test-Path -LiteralPath $CatalogPath -PathType Leaf)) {
        $blockers.Add('signed model catalog bundle is missing')
    }
    else {
        try {
            $catalogHash = (Get-FileHash -LiteralPath $CatalogPath -Algorithm SHA256).Hash.ToLowerInvariant()
            $catalog = Get-Content -LiteralPath $CatalogPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        }
        catch {
            $blockers.Add('signed model catalog bundle is unreadable or invalid JSON')
        }
    }

    $threshold = 0
    $rootKeyIds = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::Ordinal)
    if ($null -ne $root) {
        if ([string]$root.schema -cne 'npc.model-catalog-root/v1') {
            $blockers.Add('model catalog trust-root schema is not v1')
        }
        if ([string]$root.trustScope -cne 'production_release') {
            $blockers.Add('model catalog trust scope is not production_release')
        }
        foreach ($contract in @(
            @('productionTrust', $true),
            @('rotationRequiredBeforeRelease', $false),
            @('promotionSupported', $true),
            @('publicationSupported', $true)
        )) {
            $name = [string]$contract[0]
            $expected = [bool]$contract[1]
            $property = $root.PSObject.Properties[$name]
            if ($null -eq $property -or $property.Value -isnot [bool] -or $property.Value -ne $expected) {
                $blockers.Add("model catalog $name must be the JSON boolean $($expected.ToString().ToLowerInvariant())")
            }
        }

        $thresholdProperty = $root.PSObject.Properties['signatureThreshold']
        if ($null -eq $thresholdProperty -or $thresholdProperty.Value -is [string] -or
            -not [long]::TryParse([string]$thresholdProperty.Value, [ref]$threshold) -or $threshold -lt 2) {
            $blockers.Add('model catalog signature threshold must be an integer of at least two')
            $threshold = 0
        }
        $keys = @($root.keys)
        if ($threshold -gt $keys.Count) {
            $blockers.Add('model catalog signature threshold exceeds the trust-root key count')
        }
        foreach ($key in $keys) {
            $keyId = [string]$key.keyId
            if ([string]::IsNullOrWhiteSpace($keyId) -or -not $rootKeyIds.Add($keyId)) {
                $blockers.Add('model catalog trust-root key ids are empty or duplicated')
            }
            if ([string]$key.publicKeyHex -cnotmatch '^[0-9a-f]{64}$') {
                $blockers.Add("model catalog trust-root key '$keyId' is not an exact lowercase Ed25519 public key")
            }
        }
    }

    $catalogVersion = 0L
    $generated = 0L
    $expires = 0L
    $catalogEntryCount = 0
    $catalogSignatureShapeValid = $false
    if ($null -ne $catalog) {
        if ([string]$catalog.signed.schema -cne 'npc.model-catalog/v1') {
            $blockers.Add('signed model catalog payload schema is not v1')
        }
        if ([string]$catalog.source_inventory.schema -cne 'npc.model-pack-source-inventory/v1') {
            $blockers.Add('signed model catalog source-inventory schema is not v1')
        }
        if ($catalog.signed.version -is [string] -or
            -not [long]::TryParse([string]$catalog.signed.version, [ref]$catalogVersion) -or $catalogVersion -le 0) {
            $blockers.Add('signed model catalog version must be a positive integer')
        }
        if ($catalog.signed.generated_unix_seconds -is [string] -or
            -not [long]::TryParse([string]$catalog.signed.generated_unix_seconds, [ref]$generated) -or
            $catalog.signed.expires_unix_seconds -is [string] -or
            -not [long]::TryParse([string]$catalog.signed.expires_unix_seconds, [ref]$expires) -or
            $generated -le 0 -or $expires -le $generated -or $generated -gt $NowUnixSeconds -or $expires -le $NowUnixSeconds) {
            $blockers.Add('signed model catalog validity window is invalid, future-dated, or expired')
        }
        $catalogEntryCount = @($catalog.signed.entries).Count
        if ($catalogEntryCount -le 0) {
            $blockers.Add('signed model catalog has no entries')
        }
        if (@($catalog.source_inventory.manifests).Count -ne $catalogEntryCount) {
            $blockers.Add('signed model catalog and source inventory have different entry counts')
        }

        $signatureGroupsValid = $true
        foreach ($groupName in @('signatures', 'source_inventory_signatures')) {
            $validKeyIds = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::Ordinal)
            foreach ($signature in @($catalog.$groupName)) {
                $keyId = [string]$signature.key_id
                if ($rootKeyIds.Contains($keyId) -and
                    [string]$signature.algorithm -ceq 'ed25519' -and
                    [string]$signature.signature -cmatch '^[A-Za-z0-9_-]{86}$') {
                    [void]$validKeyIds.Add($keyId)
                }
            }
            if ($threshold -lt 2 -or $validKeyIds.Count -lt $threshold) {
                $signatureGroupsValid = $false
                $blockers.Add("signed model catalog $groupName does not declare the required unique Ed25519 signature set")
            }
        }
        $catalogSignatureShapeValid = $signatureGroupsValid
    }

    [pscustomobject][ordered]@{
        schema = 'interactive-npcs-model-catalog-promotion-policy/v1'
        eligible = $blockers.Count -eq 0
        blockers = @($blockers)
        root_sha256 = $rootHash
        catalog_sha256 = $catalogHash
        trust_scope = $(if ($null -ne $root) { [string]$root.trustScope } else { $null })
        production_trust = $null -ne $root -and $root.productionTrust -is [bool] -and $root.productionTrust
        rotation_required_before_release = $null -eq $root -or $root.rotationRequiredBeforeRelease -isnot [bool] -or $root.rotationRequiredBeforeRelease
        promotion_supported = $null -ne $root -and $root.promotionSupported -is [bool] -and $root.promotionSupported
        publication_supported = $null -ne $root -and $root.publicationSupported -is [bool] -and $root.publicationSupported
        signature_threshold = $threshold
        catalog_version = $catalogVersion
        catalog_entry_count = $catalogEntryCount
        declared_signature_shape_thresholds_satisfied = $catalogSignatureShapeValid
        native_cryptographic_verification_boundary = 'installed-control-model-manager'
    }
}

