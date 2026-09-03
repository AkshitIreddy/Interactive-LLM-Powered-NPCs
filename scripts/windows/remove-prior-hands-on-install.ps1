[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [switch]$AcknowledgeExactCleanup,

    [Parameter(Mandatory = $true)]
    [string]$ResultPath,

    [ValidateRange(10, 180)]
    [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') { throw 'Prior hands-on install cleanup is Windows-only.' }
if (-not $AcknowledgeExactCleanup) { throw 'Pass -AcknowledgeExactCleanup for this exact task-owned installation.' }

$productName = 'Interactive NPCs Response Console'
$expectedInstallRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $env:LOCALAPPDATA 'InteractiveNPCsHandsOnTest/app')
).TrimEnd('\')
$expectedUninstaller = Join-Path $expectedInstallRoot 'uninstall.exe'
$uninstallRegistryPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$productName"
$manufacturerRegistryPath = "HKCU:\Software\github\$productName"
$productionIdentifier = 'io.github.akshitireddy.interactive-npcs'
$productionRoamingRoot = Join-Path ([Environment]::GetFolderPath('ApplicationData')) $productionIdentifier
$productionLocalRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) $productionIdentifier
$resultFullPath = [System.IO.Path]::GetFullPath($ResultPath)

function Get-RootMetadata {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        return [pscustomobject]@{ path = $Path; exists = $false; entry_count = 0; composite_sha256 = $null }
    }
    $root = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    $lines = @(Get-ChildItem -LiteralPath $root -Recurse -Force -ErrorAction Stop |
        Sort-Object FullName | ForEach-Object {
            $relative = $_.FullName.Substring($root.Length).TrimStart('\').Replace('\', '/')
            "$relative`:$($_.PSIsContainer):$($_.Length):$($_.LastWriteTimeUtc.Ticks)"
        })
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digest = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($lines -join "`n"))
        $composite = ([BitConverter]::ToString($digest)).Replace('-', '').ToLowerInvariant()
    }
    finally { $sha.Dispose() }
    return [pscustomobject]@{
        path = $Path
        exists = $true
        entry_count = $lines.Count
        composite_sha256 = $composite
    }
}

function Get-ExactProcesses {
    $prefix = $expectedInstallRoot + '\'
    return @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and
        $_.ExecutablePath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)
    } | ForEach-Object {
        [pscustomobject]@{
            process_id = [int]$_.ProcessId
            name = [string]$_.Name
            executable_path = [string]$_.ExecutablePath
        }
    })
}

function Get-Snapshot {
    $uninstallProperties = Get-ItemProperty -LiteralPath $uninstallRegistryPath -ErrorAction SilentlyContinue
    $manufacturerValue = if (Test-Path -LiteralPath $manufacturerRegistryPath) {
        [string](Get-Item -LiteralPath $manufacturerRegistryPath).GetValue('')
    } else { $null }
    return [pscustomobject]@{
        captured_at_utc = [DateTime]::UtcNow.ToString('o')
        install_root = $expectedInstallRoot
        install_root_exists = Test-Path -LiteralPath $expectedInstallRoot
        uninstaller_exists = Test-Path -LiteralPath $expectedUninstaller -PathType Leaf
        processes = @(Get-ExactProcesses)
        uninstall_registry_exists = Test-Path -LiteralPath $uninstallRegistryPath
        uninstall_install_location = if ($null -ne $uninstallProperties) { [string]$uninstallProperties.InstallLocation } else { $null }
        uninstall_string = if ($null -ne $uninstallProperties) { [string]$uninstallProperties.UninstallString } else { $null }
        manufacturer_registry_exists = Test-Path -LiteralPath $manufacturerRegistryPath
        manufacturer_install_location = $manufacturerValue
        production_roaming = Get-RootMetadata -Path $productionRoamingRoot
        production_local = Get-RootMetadata -Path $productionLocalRoot
    }
}

$before = Get-Snapshot
if (-not $before.uninstall_registry_exists -or -not $before.manufacturer_registry_exists -or
    -not $before.install_root_exists -or -not $before.uninstaller_exists) {
    throw 'The exact prior HandsOnTest installation is not fully registered; refusing partial cleanup.'
}
$registeredLocation = ([string]$before.uninstall_install_location).Trim('"').TrimEnd('\')
$manufacturerLocation = ([string]$before.manufacturer_install_location).Trim('"').TrimEnd('\')
$registeredUninstaller = ([string]$before.uninstall_string).Trim('"')
if (-not $registeredLocation.Equals($expectedInstallRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
    -not $manufacturerLocation.Equals($expectedInstallRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
    -not $registeredUninstaller.Equals($expectedUninstaller, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Registered paths do not match the exact task-owned HandsOnTest target; refusing cleanup.'
}
if (@($before.processes).Count -gt 0) {
    throw 'The prior HandsOnTest installation still has running processes; refusing to terminate them implicitly.'
}

$uninstallerHash = (Get-FileHash -LiteralPath $expectedUninstaller -Algorithm SHA256).Hash.ToLowerInvariant()
$process = Start-Process -FilePath $expectedUninstaller -ArgumentList @('/S') -PassThru
if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    throw "The registered uninstaller exceeded the $TimeoutSeconds second timeout."
}
$process.Refresh()
$uninstallerExitCode = $process.ExitCode
if ($uninstallerExitCode -ne 0) { throw "The registered uninstaller exited with code $uninstallerExitCode." }

$deadline = [DateTime]::UtcNow.AddSeconds(15)
do {
    Start-Sleep -Milliseconds 250
    $after = Get-Snapshot
} while (($after.install_root_exists -or $after.uninstall_registry_exists -or
    $after.manufacturer_registry_exists -or @($after.processes).Count -gt 0) -and
    [DateTime]::UtcNow -lt $deadline)

$productionPreserved =
    $after.production_roaming.exists -eq $before.production_roaming.exists -and
    $after.production_roaming.entry_count -eq $before.production_roaming.entry_count -and
    $after.production_roaming.composite_sha256 -eq $before.production_roaming.composite_sha256 -and
    $after.production_local.exists -eq $before.production_local.exists -and
    $after.production_local.entry_count -eq $before.production_local.entry_count -and
    $after.production_local.composite_sha256 -eq $before.production_local.composite_sha256
$clean = -not $after.install_root_exists -and -not $after.uninstall_registry_exists -and
    -not $after.manufacturer_registry_exists -and @($after.processes).Count -eq 0
$result = [ordered]@{
    schema_version = 1
    status = if ($clean -and $productionPreserved) { 'passed' } else { 'failed' }
    target_classification = 'exact-task-owned-prior-hands-on-install'
    uninstaller_sha256 = $uninstallerHash
    uninstaller_exit_code = $uninstallerExitCode
    production_app_data_preserved = $productionPreserved
    before = $before
    after = $after
}
$resultDirectory = Split-Path -Parent $resultFullPath
if (-not [string]::IsNullOrWhiteSpace($resultDirectory)) {
    New-Item -ItemType Directory -Path $resultDirectory -Force | Out-Null
}
[System.IO.File]::WriteAllText(
    $resultFullPath,
    (($result | ConvertTo-Json -Depth 8) + [Environment]::NewLine),
    (New-Object System.Text.UTF8Encoding($false))
)
$result | ConvertTo-Json -Compress -Depth 4 | Write-Output
if (-not $clean) { throw 'The registered normal uninstaller left install files, registry entries, or processes.' }
if (-not $productionPreserved) { throw 'Production app-data metadata changed during exact prior-install cleanup.' }
exit 0
