Set-StrictMode -Version 2.0

function Get-NpcPinnedWebView2InstallerContract {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
    $provenancePath = Join-Path $root 'packaging/security/installer-toolchain-provenance.json'
    if (-not (Test-Path -LiteralPath $provenancePath -PathType Leaf)) {
        throw "Installer toolchain provenance is missing: $provenancePath"
    }
    $provenance = Get-Content -LiteralPath $provenancePath -Raw | ConvertFrom-Json
    $entry = $provenance.webview2_evergreen_standalone_x64
    if ($null -eq $entry -or
        [string]$entry.configured_mode -ne 'skip' -or
        [string]$entry.packaging_contract -ne 'customPinnedOfflineInstaller' -or
        [string]$entry.canonical_filename -ne 'MicrosoftEdgeWebView2RuntimeInstallerX64.exe' -or
        [string]$entry.sha256 -notmatch '^[0-9a-f]{64}$' -or
        [long]$entry.bytes -le 0 -or
        [string]$entry.authenticode.status -ne 'Valid' -or
        [string]::IsNullOrWhiteSpace([string]$entry.authenticode.signer_thumbprint) -or
        [bool]$entry.redistributed -ne $true -or
        [string]$entry.release_status -like 'blocked*') {
        throw 'Pinned WebView2 standalone-installer provenance is incomplete or blocked.'
    }
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw 'LOCALAPPDATA is required to resolve the isolated WebView2 installer cache.'
    }
    $cachePath = Join-Path $env:LOCALAPPDATA (Join-Path `
            "InteractiveNPCs/toolchain/webview2/$([string]$entry.file_version)/$([string]$entry.sha256)" `
            ([string]$entry.canonical_filename))
    $expandedCanonicalCache = [Environment]::ExpandEnvironmentVariables([string]$entry.canonical_cache_path)
    if ([System.IO.Path]::GetFullPath($cachePath) -ine [System.IO.Path]::GetFullPath($expandedCanonicalCache) -or
        [string]$entry.resolved_url -notmatch '^https://msedge\.sf\.dl\.delivery\.mp\.microsoft\.com/filestreamingservice/files/[0-9a-f-]+/MicrosoftEdgeWebView2RuntimeInstallerX64\.exe$') {
        throw 'Pinned WebView2 cache path or immutable resolved URL does not match reviewed provenance.'
    }
    return [pscustomobject]@{
        FileName = [string]$entry.canonical_filename
        FileVersion = [string]$entry.file_version
        ProductVersion = [string]$entry.product_version
        SizeBytes = [long]$entry.bytes
        Sha256 = [string]$entry.sha256
        SignerSubject = [string]$entry.authenticode.signer_subject
        SignerThumbprint = [string]$entry.authenticode.signer_thumbprint
        CachePath = [System.IO.Path]::GetFullPath($cachePath)
        ResolvedUrl = [string]$entry.resolved_url
        PackagingContract = [string]$entry.packaging_contract
        ProvenancePath = $provenancePath
    }
}

function Assert-NpcPinnedWebView2Installer {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Contract,
        [string]$Path
    )

    if ([string]::IsNullOrWhiteSpace($Path)) { $Path = [string]$Contract.CachePath }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Pinned WebView2 offline installer is missing. Provision the reviewed bytes before packaging: $Path"
    }
    $file = Get-Item -LiteralPath $Path
    if ($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw "Pinned WebView2 installer may not be a reparse point: $Path"
    }
    if ($file.Length -ne [long]$Contract.SizeBytes) {
        throw "Pinned WebView2 installer size mismatch: expected $($Contract.SizeBytes), observed $($file.Length)."
    }
    $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne [string]$Contract.Sha256) {
        throw "Pinned WebView2 installer SHA-256 mismatch: expected $($Contract.Sha256), observed $hash."
    }
    if ([string]$file.VersionInfo.FileVersion -ne [string]$Contract.FileVersion -or
        [string]$file.VersionInfo.ProductVersion -ne [string]$Contract.ProductVersion) {
        throw 'Pinned WebView2 installer version metadata does not match reviewed provenance.'
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ([string]$signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate -or
        [string]$signature.SignerCertificate.Thumbprint -ine [string]$Contract.SignerThumbprint -or
        [string]$signature.SignerCertificate.Subject -ne [string]$Contract.SignerSubject) {
        throw 'Pinned WebView2 installer does not have the exact reviewed valid Microsoft Authenticode signer.'
    }
    return [pscustomobject]@{
        Path = $file.FullName
        FileName = $file.Name
        SizeBytes = $file.Length
        Sha256 = $hash
        FileVersion = [string]$file.VersionInfo.FileVersion
        ProductVersion = [string]$file.VersionInfo.ProductVersion
        AuthenticodeStatus = [string]$signature.Status
        SignerSubject = [string]$signature.SignerCertificate.Subject
        SignerThumbprint = [string]$signature.SignerCertificate.Thumbprint
    }
}

function ConvertTo-NpcNsisLiteral {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ($Value -match '[\r\n"$]') {
        throw "NSIS generated-hook paths may not contain a quote, dollar sign, or newline: $Value"
    }
    return $Value
}

function Assert-NpcPinnedWebViewStageBoundary {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)
    $root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
    $cursor = $root
    foreach ($segment in @('apps', 'control', 'src-tauri')) {
        $cursor = Join-Path $cursor $segment
        if (-not (Test-Path -LiteralPath $cursor -PathType Container)) {
            throw "Canonical Tauri source parent is missing: $cursor"
        }
        $item = Get-Item -LiteralPath $cursor -Force
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw "Generated WebView2 hook path may not traverse a reparse point: $cursor"
        }
    }
    $stageRoot = Join-Path $cursor 'generated-installer-inputs'
    $expected = Join-Path $root 'apps/control/src-tauri/generated-installer-inputs'
    if ([System.IO.Path]::GetFullPath($stageRoot) -ine [System.IO.Path]::GetFullPath($expected)) {
        throw 'Generated WebView2 installer-input stage escaped its exact canonical boundary.'
    }
    return $stageRoot
}

function New-NpcPinnedWebView2InstallerHook {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)]$InstallerIdentity
    )

    $root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
    $stageRoot = Assert-NpcPinnedWebViewStageBoundary -RepositoryRoot $root
    if (Test-Path -LiteralPath $stageRoot) {
        $stage = Get-Item -LiteralPath $stageRoot -Force
        if ($stage.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw "Generated WebView2 installer-input stage may not be a reparse point: $stageRoot"
        }
        if (-not $stage.PSIsContainer) {
            throw "Generated WebView2 installer-input stage must be a directory: $stageRoot"
        }
        Remove-Item -LiteralPath $stageRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stageRoot | Out-Null
    $hookSource = (Resolve-Path -LiteralPath (Join-Path $root 'packaging/windows/nsis/installer-hooks.nsh')).Path
    $installerPath = (Resolve-Path -LiteralPath ([string]$InstallerIdentity.Path)).Path
    $hookPath = Join-Path $stageRoot 'installer-hooks.generated.nsh'
    $content = @(
        '; Generated from exact reviewed local inputs. Do not commit.',
        "!define NPC_WEBVIEW2_OFFLINE_INSTALLER_PATH `"$(ConvertTo-NpcNsisLiteral -Value $installerPath)`"",
        "!define NPC_WEBVIEW2_OFFLINE_INSTALLER_SHA256 `"$([string]$InstallerIdentity.Sha256)`"",
        "!include `"$(ConvertTo-NpcNsisLiteral -Value $hookSource)`""
    ) -join "`r`n"
    [System.IO.File]::WriteAllText($hookPath, $content + "`r`n", (New-Object System.Text.UTF8Encoding($false)))
    return [pscustomobject]@{
        StageRoot = $stageRoot
        HookPath = $hookPath
        HookSha256 = (Get-FileHash -LiteralPath $hookPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Remove-NpcPinnedWebView2InstallerHookStage {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
    $stageRoot = Assert-NpcPinnedWebViewStageBoundary -RepositoryRoot $root
    if (-not (Test-Path -LiteralPath $stageRoot)) { return }
    $stage = Get-Item -LiteralPath $stageRoot -Force
    if ($stage.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to remove reparse-point WebView2 installer-input stage: $stageRoot"
    }
    if (-not $stage.PSIsContainer) {
        throw "Refusing to remove non-directory WebView2 installer-input stage: $stageRoot"
    }
    Remove-Item -LiteralPath $stageRoot -Recurse -Force
}
