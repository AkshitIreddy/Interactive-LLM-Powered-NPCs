[CmdletBinding()]
param(
    [string]$OutputPath,
    [switch]$RequireNative
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$npcIsWindows = $env:OS -eq 'Windows_NT'
. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')
. (Join-Path $PSScriptRoot 'windows/webview2-offline-installer.ps1')

function Get-CommandPath {
    param([Parameter(Mandatory = $true)][string[]]$Names)
    foreach ($name in $Names) {
        $command = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($null -ne $command) { return $command.Source }
    }
    return $null
}

function Invoke-Captured {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @()
    )
    $result = Invoke-NpcHiddenProcess -FilePath $FilePath -ArgumentList $ArgumentList -NoReplayOutput
    if ($result.ExitCode -ne 0) {
        throw "Toolchain probe failed for $([System.IO.Path]::GetFileName($FilePath)) with exit code $($result.ExitCode): $($result.StandardError)"
    }
    return @(($result.StandardOutput + [Environment]::NewLine + $result.StandardError) -split '\r?\n' |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Get-FileIdentity {
    param(
        [string]$Path,
        [string]$Version
    )
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    $file = Get-Item -LiteralPath $Path
    return [ordered]@{
        version = $Version
        sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        size_bytes = $file.Length
        executable_name = $file.Name
        path_recorded = $false
    }
}

function Get-InputIdentity {
    param([Parameter(Mandatory = $true)][string]$RelativePath)
    $path = Join-Path $repoRoot $RelativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required reproducibility input is missing: $RelativePath"
    }
    return [ordered]@{
        path = $RelativePath.Replace('\', '/')
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        size_bytes = (Get-Item -LiteralPath $path).Length
    }
}

$nodePath = Get-CommandPath -Names @('node.exe', 'node')
$rustcPath = Get-CommandPath -Names @('rustc.exe', 'rustc')
$cargoPath = Get-CommandPath -Names @('cargo.exe', 'cargo')
$cmakePath = Get-CommandPath -Names @('cmake.exe', 'cmake')
foreach ($required in @(
        @{ Name = 'Node.js'; Path = $nodePath },
        @{ Name = 'rustc'; Path = $rustcPath },
        @{ Name = 'Cargo'; Path = $cargoPath },
        @{ Name = 'CMake'; Path = $cmakePath }
    )) {
    if ([string]::IsNullOrWhiteSpace($required.Path)) {
        throw "$($required.Name) was not found; exact toolchain evidence cannot be recorded."
    }
}

$nodeVersion = ((Invoke-Captured -FilePath $nodePath -ArgumentList @('--version')) | Select-Object -First 1).TrimStart('v')
$rustcOutput = Invoke-Captured -FilePath $rustcPath -ArgumentList @('-vV')
$rustcRelease = (($rustcOutput | Where-Object { $_ -match '^release:' } | Select-Object -First 1) -replace '^release:\s*', '').Trim()
$rustcCommit = (($rustcOutput | Where-Object { $_ -match '^commit-hash:' } | Select-Object -First 1) -replace '^commit-hash:\s*', '').Trim()
$rustcHost = (($rustcOutput | Where-Object { $_ -match '^host:' } | Select-Object -First 1) -replace '^host:\s*', '').Trim()
$cargoVersion = ((Invoke-Captured -FilePath $cargoPath -ArgumentList @('--version')) | Select-Object -First 1).Trim()
$cmakeVersionLine = (Invoke-Captured -FilePath $cmakePath -ArgumentList @('--version')) | Select-Object -First 1
$cmakeVersion = ($cmakeVersionLine -replace '^cmake version\s+', '').Trim()

$package = Get-Content -LiteralPath (Join-Path $repoRoot 'package.json') -Raw | ConvertFrom-Json
$pinnedPackageManager = [string]$package.packageManager
$corepackVersion = $null
$observedPnpmVersion = $null
$corepackScript = Join-Path (Split-Path -Parent $nodePath) 'node_modules/corepack/dist/corepack.js'
if (Test-Path -LiteralPath $corepackScript -PathType Leaf) {
    $corepackVersion = ((Invoke-Captured -FilePath $nodePath -ArgumentList @($corepackScript, '--version')) | Select-Object -First 1).Trim()
    $observedPnpmVersion = ((Invoke-Captured -FilePath $nodePath -ArgumentList @($corepackScript, 'pnpm', '--version')) | Select-Object -First 1).Trim()
}

$visualStudio = $null
$msvcCompiler = $null
$windowsSdk = $null
if ($npcIsWindows) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
        $vsJson = (Invoke-Captured -FilePath $vswhere -ArgumentList @(
                '-latest', '-products', '*',
                '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
                '-format', 'json', '-utf8'
            )) -join [Environment]::NewLine
        $instances = @($vsJson | ConvertFrom-Json)
        if ($instances.Count -gt 0) {
            $instance = $instances[0]
            $installationPath = [string]$instance.installationPath
            $toolsetVersionFile = Join-Path $installationPath 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt'
            $toolsetVersion = if (Test-Path -LiteralPath $toolsetVersionFile -PathType Leaf) {
                (Get-Content -LiteralPath $toolsetVersionFile -Raw).Trim()
            } else { $null }
            $clPath = if (-not [string]::IsNullOrWhiteSpace($toolsetVersion)) {
                Join-Path $installationPath "VC/Tools/MSVC/$toolsetVersion/bin/Hostx64/x64/cl.exe"
            } else { $null }
            $visualStudio = [ordered]@{
                product_id = [string]$instance.productId
                installation_version = [string]$instance.installationVersion
                catalog_product_line_version = [string]$instance.catalog.productLineVersion
                msvc_toolset_version = $toolsetVersion
                path_recorded = $false
            }
            $msvcCompiler = Get-FileIdentity -Path $clPath -Version $toolsetVersion
        }
    }

    $kitsRoot = $null
    $kitsKey = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue
    if ($null -ne $kitsKey) { $kitsRoot = [string]$kitsKey.KitsRoot10 }
    if (-not [string]::IsNullOrWhiteSpace($kitsRoot)) {
        $sdkVersions = @(Get-ChildItem -LiteralPath (Join-Path $kitsRoot 'Include') -Directory -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match '^10\.\d+\.\d+\.0$' } |
                Sort-Object { [version]$_.Name } -Descending)
        if ($sdkVersions.Count -gt 0) {
            $sdkVersion = $sdkVersions[0].Name
            $rcPath = Join-Path $kitsRoot "bin/$sdkVersion/x64/rc.exe"
            $windowsSdk = [ordered]@{
                selected_default_version = $sdkVersion
                installed_versions = @($sdkVersions | ForEach-Object { $_.Name })
                resource_compiler = Get-FileIdentity -Path $rcPath -Version $sdkVersion
                path_recorded = $false
            }
        }
    }
}

$nativeIdentityComplete = $null -ne $visualStudio -and
    $null -ne $msvcCompiler -and
    $null -ne $windowsSdk -and
    $null -ne $windowsSdk.resource_compiler
if ($RequireNative -and -not $nativeIdentityComplete) {
    throw 'Visual Studio C++ toolset and Windows SDK identities are required but could not be resolved.'
}

$webViewVersions = New-Object System.Collections.Generic.List[string]
if ($npcIsWindows) {
    foreach ($registryPath in @(
            'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
            'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
        )) {
        $entry = Get-ItemProperty -Path $registryPath -ErrorAction SilentlyContinue
        if ($null -ne $entry -and -not [string]::IsNullOrWhiteSpace([string]$entry.pv)) {
            $webViewVersions.Add([string]$entry.pv)
        }
    }
}

$tauriConfig = Get-Content -LiteralPath (Join-Path $repoRoot 'apps/control/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$bootstrapperMode = [string]$tauriConfig.bundle.windows.webviewInstallMode.type
$webViewInstaller = $null
if ($npcIsWindows) {
    $webViewContract = Get-NpcPinnedWebView2InstallerContract -RepositoryRoot $repoRoot
    if (Test-Path -LiteralPath $webViewContract.CachePath -PathType Leaf) {
        $webViewInstaller = Assert-NpcPinnedWebView2Installer -Contract $webViewContract
    } elseif ($RequireNative) {
        throw "Exact pinned WebView2 offline-installer input is required: $($webViewContract.CachePath)"
    }
}
$inputFiles = @(
    'rust-toolchain.toml',
    'package.json',
    'pnpm-lock.yaml',
    'Cargo.lock',
    'apps/control/src-tauri/Cargo.lock',
    'demo/readme/package-lock.json',
    'apps/control/src-tauri/tauri.conf.json',
    'packaging/windows/tauri.review.conf.json',
    'packaging/windows/tauri.release.conf.json',
    'packaging/windows/nsis/installer-hooks.nsh',
    'packaging/security/installer-toolchain-provenance.json'
)

$evidence = [ordered]@{
    schema_version = 'interactive-npcs-toolchain-evidence/v1'
    collected_at_utc = [DateTime]::UtcNow.ToString('o')
    platform = [ordered]@{
        os_version = [System.Environment]::OSVersion.VersionString
        process_architecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        is_64_bit_operating_system = [System.Environment]::Is64BitOperatingSystem
    }
    declared = [ordered]@{
        node = [string]$package.engines.node
        package_manager = $pinnedPackageManager
        rust = ((Get-Content -LiteralPath (Join-Path $repoRoot 'rust-toolchain.toml') | Where-Object { $_ -match '^channel\s*=' } | Select-Object -First 1) -replace '^.+"([^\"]+)".+$', '$1')
        cmake_minimum = '3.24'
        cmake_generator = 'Visual Studio 17 2022'
    }
    observed = [ordered]@{
        node = Get-FileIdentity -Path $nodePath -Version $nodeVersion
        corepack_version = $corepackVersion
        pnpm_version = $observedPnpmVersion
        rustc = Get-FileIdentity -Path $rustcPath -Version "$rustcRelease ($rustcCommit; $rustcHost)"
        cargo = Get-FileIdentity -Path $cargoPath -Version $cargoVersion
        cmake = Get-FileIdentity -Path $cmakePath -Version $cmakeVersion
        visual_studio = $visualStudio
        msvc_compiler = $msvcCompiler
        windows_sdk = $windowsSdk
        webview2_runtime_versions = @($webViewVersions | Sort-Object -Unique)
        webview2_offline_installer = $(if ($null -eq $webViewInstaller) { $null } else {
                [ordered]@{
                    file_name = [string]$webViewInstaller.FileName
                    file_version = [string]$webViewInstaller.FileVersion
                    product_version = [string]$webViewInstaller.ProductVersion
                    sha256 = [string]$webViewInstaller.Sha256
                    size_bytes = [long]$webViewInstaller.SizeBytes
                    authenticode_status = [string]$webViewInstaller.AuthenticodeStatus
                    signer_subject = [string]$webViewInstaller.SignerSubject
                    signer_thumbprint = [string]$webViewInstaller.SignerThumbprint
                    cache_path_recorded = $false
                }
            })
    }
    source_inputs = @($inputFiles | ForEach-Object { Get-InputIdentity -RelativePath $_ })
    reproducibility = [ordered]@{
        locked_source_graph = $true
        native_toolchain_identity_recorded = $nativeIdentityComplete
        webview_install_mode = $bootstrapperMode
        webview_bootstrapper_content_pinned = $true
        webview_offline_installer_content_pinned = $null -ne $webViewInstaller
        webview_package_time_network_acquisition = $false
        byte_reproducible_installer = $false
        classification = 'pinned-input-non-byte-reproducible-local-review'
        reason = 'Locks reproduce dependency selection and the exact Microsoft-signed WebView2 offline installer is pinned with no package-time acquisition. Native compiler/linker and NSIS outputs are not configured for bit-for-bit reproducibility, so independently built installer bytes may still differ.'
    }
    paths_recorded = $false
}

$json = ($evidence | ConvertTo-Json -Depth 9) + [Environment]::NewLine
if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
    if (-not [System.IO.Path]::IsPathRooted($OutputPath)) {
        $OutputPath = Join-Path $repoRoot $OutputPath
    }
    $parent = Split-Path -Parent $OutputPath
    if (-not [string]::IsNullOrWhiteSpace($parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($OutputPath, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "Toolchain evidence: $OutputPath" -ForegroundColor Green
}
else {
    Write-Output $json.TrimEnd()
}

exit 0
