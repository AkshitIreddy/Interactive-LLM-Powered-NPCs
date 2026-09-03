[CmdletBinding()]
param(
    [switch]$Offline,
    [string]$RepositoryRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    throw 'The pinned WebView2 installer cache can only be prepared on Windows.'
}
if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
} else {
    $RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
}
. (Join-Path $PSScriptRoot 'webview2-offline-installer.ps1')

$contract = Get-NpcPinnedWebView2InstallerContract -RepositoryRoot $RepositoryRoot
if (Test-Path -LiteralPath $contract.CachePath -PathType Leaf) {
    $identity = Assert-NpcPinnedWebView2Installer -Contract $contract
} else {
    if ($Offline) {
        throw "Offline setup requires the exact reviewed WebView2 installer cache: $($contract.CachePath)"
    }
    $cacheParent = Split-Path -Parent $contract.CachePath
    $localRoot = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'InteractiveNPCs'))
    if (-not [System.IO.Path]::GetFullPath($cacheParent).StartsWith(
            $localRoot + [System.IO.Path]::DirectorySeparatorChar,
            [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Pinned WebView2 cache destination escaped the task-owned LOCALAPPDATA root.'
    }
    $cursor = $localRoot
    foreach ($segment in @('toolchain', 'webview2', $contract.FileVersion, $contract.Sha256)) {
        $cursor = Join-Path $cursor $segment
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                throw "Pinned WebView2 cache path may not traverse a reparse point: $cursor"
            }
        } else {
            New-Item -ItemType Directory -Path $cursor | Out-Null
        }
    }
    $partialPath = Join-Path $cacheParent "$($contract.FileName).$([Guid]::NewGuid().ToString('N')).part"
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $client = New-Object System.Net.WebClient
        try {
            $client.DownloadFile([Uri]$contract.ResolvedUrl, $partialPath)
        } finally {
            $client.Dispose()
        }
        $identity = Assert-NpcPinnedWebView2Installer -Contract $contract -Path $partialPath
        Move-Item -LiteralPath $partialPath -Destination $contract.CachePath
        $identity = Assert-NpcPinnedWebView2Installer -Contract $contract
    }
    finally {
        Remove-Item -LiteralPath $partialPath -Force -ErrorAction SilentlyContinue
    }
}

[ordered]@{
    schema_version = 1
    status = 'prepared'
    offline = [bool]$Offline
    file_name = [string]$identity.FileName
    file_version = [string]$identity.FileVersion
    size_bytes = [long]$identity.SizeBytes
    sha256 = [string]$identity.Sha256
    authenticode_status = [string]$identity.AuthenticodeStatus
    signer_thumbprint = [string]$identity.SignerThumbprint
    package_time_network_acquisition = $false
    cache_path_recorded = $false
} | ConvertTo-Json -Compress | Write-Output

exit 0
