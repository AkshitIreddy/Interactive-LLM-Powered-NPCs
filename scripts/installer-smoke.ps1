[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'Medium')]
param(
    [Parameter(Mandatory = $true)]
    [string]$PackageManifestPath,

    [switch]$AcknowledgeLocalInstall,

    [switch]$PreflightOnly,

    [ValidateRange(10, 300)]
    [int]$ProcessTimeoutSeconds = 90,

    [ValidateRange(2, 30)]
    [int]$LaunchObservationSeconds = 5,

    [string]$ResultPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$productName = 'Interactive NPCs Response Console'
$uninstallRegistryPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$productName"
$manufacturerRegistryPath = "HKCU:\Software\github\$productName"
$runRegistryPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$mainExecutableName = 'interactive-npcs-control.exe'
$requiredInstalledFiles = @(
    $mainExecutableName,
    'npc-runtime.exe',
    'npc-media-broker.exe',
    'uninstall.exe',
    'catalog\v1\catalog.json',
    'packaging\model-packs\model-pack-manifest.example.json',
    'packaging\model-packs\model-pack-manifest.schema.json'
)

function Convert-ToFullPath {
    param([string]$Path, [string]$BasePath)
    if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
    return [System.IO.Path]::GetFullPath((Join-Path $BasePath $Path))
}

function Get-OptionalFileSha256 {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-StringSha256 {
    param([string]$Value)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Value)
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally { $sha.Dispose() }
}

function Get-SourceIdentity {
    param([string]$RepoRoot, [string]$ManifestPath)
    $gitHead = $null
    $gitDirty = $null
    $gitCommand = Get-Command git -ErrorAction SilentlyContinue
    $gitExecutable = $(if ($null -ne $gitCommand) { $gitCommand.Source } else { $null })
    if ([string]::IsNullOrWhiteSpace($gitExecutable)) {
        foreach ($candidate in @(
            (Join-Path $env:ProgramFiles 'Git\cmd\git.exe'),
            (Join-Path ${env:ProgramFiles(x86)} 'Git\cmd\git.exe')
        )) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { $gitExecutable = $candidate; break }
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($gitExecutable) -and (Test-Path (Join-Path $RepoRoot '.git'))) {
        $originalPathExt = $env:PATHEXT
        $originalErrorAction = $ErrorActionPreference
        if ($env:PATHEXT -notmatch '(?i)(^|;)\.EXE($|;)') { $env:PATHEXT = '.COM;.EXE;.BAT;.CMD;.CPL' }
        $ErrorActionPreference = 'Continue'
        try {
            $global:LASTEXITCODE = 0
            $gitHeadOutput = @(& $gitExecutable -C $RepoRoot rev-parse HEAD 2>$null)
            if ($LASTEXITCODE -eq 0 -and $gitHeadOutput.Count -gt 0) { $gitHead = [string]$gitHeadOutput[0] }
            $global:LASTEXITCODE = 0
            & $gitExecutable -C $RepoRoot diff --quiet --ignore-submodules -- 2>$null
            $workingTreeDirty = $LASTEXITCODE -ne 0
            $global:LASTEXITCODE = 0
            & $gitExecutable -C $RepoRoot diff --cached --quiet --ignore-submodules -- 2>$null
            $indexDirty = $LASTEXITCODE -ne 0
            $global:LASTEXITCODE = 0
            $untracked = @(& $gitExecutable -C $RepoRoot ls-files --others --exclude-standard 2>$null)
            $gitDirty = $workingTreeDirty -or $indexDirty -or $untracked.Count -gt 0
        }
        finally {
            $env:PATHEXT = $originalPathExt
            $ErrorActionPreference = $originalErrorAction
        }
    }

    $profileRoot = Join-Path $RepoRoot 'profiles/games'
    $profileFiles = @(Get-ChildItem -LiteralPath $profileRoot -Filter 'profile.json' -File -Recurse -ErrorAction SilentlyContinue | Sort-Object FullName)
    $profileLines = foreach ($profile in $profileFiles) {
        $relative = $profile.FullName.Substring($RepoRoot.Length).TrimStart('\').Replace('\', '/')
        "$relative`:$((Get-FileHash -LiteralPath $profile.FullName -Algorithm SHA256).Hash.ToLowerInvariant())"
    }
    $profileDigest = $(if (@($profileLines).Count -gt 0) { Get-StringSha256 -Value ($profileLines -join "`n") } else { $null })
    $identity = [ordered]@{
        scope = 'local checkout at smoke execution; package_manifest_sha256 binds the tested artifact'
        git_head = $gitHead
        git_dirty = $gitDirty
        package_manifest_sha256 = Get-OptionalFileSha256 -Path $ManifestPath
        cargo_lock_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'Cargo.lock')
        pnpm_lock_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'pnpm-lock.yaml')
        profile_count = $profileFiles.Count
        profile_corpus_sha256 = $profileDigest
        catalog_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'catalog/v1/catalog.json')
        model_manifest_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'packaging/model-packs/model-pack-manifest.example.json')
        tauri_config_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/tauri.conf.json')
        prepared_runtime_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/binaries/npc-runtime-x86_64-pc-windows-msvc.exe')
        prepared_broker_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/binaries/npc-media-broker-x86_64-pc-windows-msvc.exe')
    }
    $identity['composite_sha256'] = Get-StringSha256 -Value (($identity.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join "`n")
    return [pscustomobject]$identity
}

function Write-SmokeResult {
    param([System.Collections.IDictionary]$Result, [string]$Path)
    $directory = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($directory)) { New-Item -ItemType Directory -Path $directory -Force | Out-Null }
    $json = ($Result | ConvertTo-Json -Depth 12) + [Environment]::NewLine
    [System.IO.File]::WriteAllText($Path, $json, (New-Object System.Text.UTF8Encoding($false)))
}

function Assert-ExitCode {
    param([System.Diagnostics.Process]$Process, [string]$Label)
    $Process.Refresh()
    if ($Process.ExitCode -ne 0) { throw "$Label exited with code $($Process.ExitCode)." }
}

function Wait-BoundedProcess {
    param(
        [System.Diagnostics.Process]$Process,
        [int]$TimeoutSeconds,
        [string]$Label
    )
    try {
        Wait-Process -Id $Process.Id -Timeout $TimeoutSeconds -ErrorAction Stop
    }
    catch {
        if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue }
        throw "$Label exceeded the $TimeoutSeconds second timeout."
    }
    $Process.WaitForExit()
    $Process.Refresh()
    Assert-ExitCode -Process $Process -Label $Label
}

function Invoke-CapturedProcess {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory,
        [string]$LogDirectory,
        [string]$Label,
        [int]$TimeoutSeconds
    )
    $safeLabel = $Label -replace '[^A-Za-z0-9.-]', '-'
    $stdoutPath = Join-Path $LogDirectory "$safeLabel.stdout.txt"
    $stderrPath = Join-Path $LogDirectory "$safeLabel.stderr.txt"
    $quotedArguments = @($Arguments | ForEach-Object { '"' + ([string]$_).Replace('"', '\"') + '"' })
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $FilePath
    $startInfo.Arguments = $quotedArguments -join ' '
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    if (-not $process.Start()) { throw "$Label could not be started." }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        if (-not $process.HasExited) { $process.Kill() }
        throw "$Label exceeded the $TimeoutSeconds second timeout."
    }
    $process.WaitForExit()
    $stdout = $stdoutTask.Result
    $stderr = $stderrTask.Result
    Set-Content -LiteralPath $stdoutPath -Value $stdout -Encoding UTF8
    Set-Content -LiteralPath $stderrPath -Value $stderr -Encoding UTF8
    $exitCode = $process.ExitCode
    $process.Dispose()
    if ($exitCode -ne 0) {
        $sanitizedError = ($stderr -replace '[\r\n]+', ' ').Trim()
        throw "$Label exited with code ${exitCode}: $sanitizedError"
    }
    return [pscustomobject]@{
        stdout = $stdout
        stderr = $stderr
    }
}

function Get-TestInstallProcesses {
    param([string]$InstallRoot)
    $prefix = $InstallRoot.TrimEnd('\') + '\'
    $operationToken = Split-Path -Leaf (Split-Path -Parent $InstallRoot)
    $allowedNames = @($mainExecutableName, 'npc-runtime.exe', 'npc-media-broker.exe')
    return @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        $allowedNames -contains $_.Name -and (
            (-not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and
                ($_.ExecutablePath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) -or $_.ExecutablePath.IndexOf($operationToken, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)) -or
            (-not [string]::IsNullOrWhiteSpace($_.CommandLine) -and $_.CommandLine.IndexOf($operationToken, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
        )
    })
}

function Get-RegistryValueSnapshot {
    param([string]$Path, [string]$Name)
    if (-not (Test-Path -LiteralPath $Path)) { return [pscustomobject]@{ exists = $false; value = $null } }
    $key = Get-Item -LiteralPath $Path
    $names = @($key.GetValueNames())
    if ($names -notcontains $Name) { return [pscustomobject]@{ exists = $false; value = $null } }
    return [pscustomobject]@{ exists = $true; value = [string]$key.GetValue($Name) }
}

function Get-InstallTargetSnapshot {
    param([string]$InstallRoot, [string[]]$ShortcutPaths)
    $processes = @(Get-TestInstallProcesses -InstallRoot $InstallRoot | ForEach-Object {
        [pscustomobject]@{
            process_id = [int]$_.ProcessId
            parent_process_id = [int]$_.ParentProcessId
            name = [string]$_.Name
            executable_path = [string]$_.ExecutablePath
        }
    })
    $files = foreach ($relativePath in $requiredInstalledFiles) {
        [pscustomobject]@{
            relative_path = $relativePath
            exists = Test-Path -LiteralPath (Join-Path $InstallRoot $relativePath) -PathType Leaf
        }
    }
    $shortcuts = foreach ($path in $ShortcutPaths) {
        [pscustomobject]@{ path = $path; exists = Test-Path -LiteralPath $path -PathType Leaf }
    }
    $uninstallLocation = Get-RegistryValueSnapshot -Path $uninstallRegistryPath -Name 'InstallLocation'
    $manufacturerLocation = Get-RegistryValueSnapshot -Path $manufacturerRegistryPath -Name ''
    $runValue = Get-RegistryValueSnapshot -Path $runRegistryPath -Name $productName
    return [pscustomobject]@{
        captured_at_utc = [DateTime]::UtcNow.ToString('o')
        install_root = $InstallRoot
        install_root_exists = Test-Path -LiteralPath $InstallRoot
        files = @($files)
        processes = $processes
        shortcuts = @($shortcuts)
        registry = [pscustomobject]@{
            uninstall_key_path = $uninstallRegistryPath
            uninstall_key_exists = Test-Path -LiteralPath $uninstallRegistryPath
            uninstall_install_location = $uninstallLocation.value
            manufacturer_key_path = $manufacturerRegistryPath
            manufacturer_key_exists = Test-Path -LiteralPath $manufacturerRegistryPath
            manufacturer_install_location = $manufacturerLocation.value
            run_key_path = $runRegistryPath
            run_value_name = $productName
            run_value_exists = $runValue.exists
            run_value = $runValue.value
        }
    }
}

function Get-SupervisedChildEvidence {
    param(
        [object]$Process,
        [int]$ExpectedParentId,
        [string]$ExpectedExecutablePath,
        [string]$OperationId
    )
    if ($null -eq $Process) { return $null }
    $actualPath = [string]$Process.ExecutablePath
    $expectedName = [System.IO.Path]::GetFileName($ExpectedExecutablePath)
    $virtualizedSuffix = "\InteractiveNPCsInstallerSmoke\$OperationId\app\$expectedName"
    $pathMatches = -not [string]::IsNullOrWhiteSpace($actualPath) -and (
        $actualPath.Equals($ExpectedExecutablePath, [System.StringComparison]::OrdinalIgnoreCase) -or
        $actualPath.EndsWith($virtualizedSuffix, [System.StringComparison]::OrdinalIgnoreCase)
    )
    $expectedHash = Get-OptionalFileSha256 -Path $ExpectedExecutablePath
    $actualHash = Get-OptionalFileSha256 -Path $actualPath
    return [pscustomobject]@{
        process_id = [int]$Process.ProcessId
        parent_process_id = [int]$Process.ParentProcessId
        name = [string]$Process.Name
        executable_path = $actualPath
        expected_executable_path = $ExpectedExecutablePath
        path_matches_install_target = $pathMatches
        parent_matches_shell = [int]$Process.ParentProcessId -eq $ExpectedParentId
        sha256 = $actualHash
        expected_sha256 = $expectedHash
        hash_matches_installed_file = -not [string]::IsNullOrWhiteSpace($actualHash) -and $actualHash -eq $expectedHash
    }
}

function Get-PrivateDirectoryAclEvidence {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return [pscustomobject]@{
            path = $Path
            exists = $false
            protected_dacl = $false
            inherited_ace_count = $null
            principal_names = @()
            unexpected_principal_names = @()
            private = $false
        }
    }
    $acl = Get-Acl -LiteralPath $Path
    $currentPrincipal = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $allowed = @($currentPrincipal, 'NT AUTHORITY\SYSTEM', 'BUILTIN\Administrators')
    $principalNames = New-Object System.Collections.Generic.List[string]
    $unexpected = New-Object System.Collections.Generic.List[string]
    $inheritedCount = 0
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.NTAccount]))
    foreach ($rule in $rules) {
        $name = [string]$rule.IdentityReference.Value
        if ($name -match '^S-\d(?:-\d+)+$') { $name = 'unresolved-principal' }
        if (-not $principalNames.Contains($name)) { $principalNames.Add($name) }
        if ($rule.IsInherited) { $inheritedCount++ }
        if (-not ($allowed | Where-Object { $_.Equals($name, [System.StringComparison]::OrdinalIgnoreCase) })) {
            if (-not $unexpected.Contains($name)) { $unexpected.Add($name) }
        }
    }
    $private = $acl.AreAccessRulesProtected -and $inheritedCount -eq 0 -and $unexpected.Count -eq 0
    return [pscustomobject]@{
        path = $Path
        exists = $true
        protected_dacl = [bool]$acl.AreAccessRulesProtected
        inherited_ace_count = $inheritedCount
        principal_names = @($principalNames | Sort-Object)
        unexpected_principal_names = @($unexpected | Sort-Object)
        private = $private
    }
}

function Stop-OwnedProcess {
    param([System.Diagnostics.Process]$Process)
    if ($null -eq $Process) { return }
    $Process.Refresh()
    if (-not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        Wait-Process -Id $Process.Id -Timeout 10 -ErrorAction SilentlyContinue
    }
}

if ($env:OS -ne 'Windows_NT') { throw 'Installer smoke testing is supported only on Windows.' }
if (-not [System.Environment]::Is64BitOperatingSystem) { throw 'Installer smoke testing requires 64-bit Windows.' }
if (-not $PreflightOnly -and -not $AcknowledgeLocalInstall) { throw 'Pass -AcknowledgeLocalInstall to opt in to the isolated install/uninstall cycle.' }

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$manifestFullPath = Convert-ToFullPath -Path $PackageManifestPath -BasePath (Get-Location).Path
if (-not (Test-Path -LiteralPath $manifestFullPath -PathType Leaf)) { throw "Package manifest not found: $manifestFullPath" }
$packageDirectory = Split-Path -Parent $manifestFullPath
$manifest = Get-Content -LiteralPath $manifestFullPath -Raw | ConvertFrom-Json

if ($manifest.schema_version -ne 1) { throw 'Only package manifest schema version 1 is accepted.' }
if (-not [string]::Equals([string]$manifest.configuration, 'Debug', [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Installer smoke testing refuses non-Debug packages.' }
if ($manifest.distribution -ne 'local-review-only') { throw 'Installer smoke testing refuses packages outside the local-review-only boundary.' }
if ($manifest.updater_enabled -ne $false) { throw 'Installer smoke testing refuses updater-enabled packages.' }
$manifestPropertyNames = @($manifest.PSObject.Properties.Name)
$declaresUnsigned = ($manifestPropertyNames -contains 'unsigned' -and $manifest.unsigned -eq $true) -or
    ($manifestPropertyNames -contains 'signed' -and $manifest.signed -eq $false)
if (-not $declaresUnsigned) { throw 'Installer smoke testing accepts only explicitly unsigned local packages.' }

$manifestFiles = @($manifest.files)
if ($manifestFiles.Count -eq 0) { throw 'Package manifest contains no files.' }
foreach ($entry in $manifestFiles) {
    $fileName = [string]$entry.file
    if ([string]::IsNullOrWhiteSpace($fileName) -or [System.IO.Path]::GetFileName($fileName) -ne $fileName) {
        throw "Unsafe package manifest filename: $fileName"
    }
    $filePath = Join-Path $packageDirectory $fileName
    if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) { throw "Manifest file is missing: $fileName" }
    $file = Get-Item -LiteralPath $filePath
    if ($file.Length -ne [long]$entry.size_bytes) { throw "Manifest size mismatch: $fileName" }
    $actualHash = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne ([string]$entry.sha256).ToLowerInvariant()) { throw "Manifest SHA-256 mismatch: $fileName" }
}

$installerEntries = @($manifestFiles | Where-Object { $_.file -like '*-setup.exe' })
if ($installerEntries.Count -ne 1) { throw 'Package manifest must contain exactly one NSIS setup executable.' }
$installerPath = Join-Path $packageDirectory ([string]$installerEntries[0].file)
$signature = Get-AuthenticodeSignature -LiteralPath $installerPath
if ($signature.Status -ne 'NotSigned') {
    throw 'Installer signature state no longer matches the unsigned local-review manifest.'
}
$installerEntryProperties = @($installerEntries[0].PSObject.Properties.Name)
if ($installerEntryProperties -contains 'authenticode_status' -and $installerEntries[0].authenticode_status -ne 'NotSigned') {
    throw 'Installer Authenticode state differs from its package manifest.'
}

if (Test-Path $uninstallRegistryPath) { throw "A $productName installation is already registered; refusing to disturb it." }
if (Test-Path $manufacturerRegistryPath) { throw "Existing $productName installer state was found; refusing to disturb it." }
$existingRunValue = Get-RegistryValueSnapshot -Path $runRegistryPath -Name $productName
if ($existingRunValue.exists) { throw "An existing $productName auto-start value was found; refusing to disturb it." }
$shortcutPaths = @(
    (Join-Path ([Environment]::GetFolderPath('Desktop')) "$productName.lnk"),
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$productName.lnk")
)
foreach ($shortcutPath in $shortcutPaths) {
    if (Test-Path -LiteralPath $shortcutPath) { throw "Existing product shortcut found; refusing to disturb it: $shortcutPath" }
}
$appConfigRoot = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'io.github.akshitireddy.interactive-npcs'
$healthProbePath = Join-Path $appConfigRoot 'installer-smoke-health-v1.json'
if (Test-Path -LiteralPath $healthProbePath -PathType Leaf) { throw "An existing installer health probe was found; refusing to overwrite it: $healthProbePath" }

$baseTestRoot = Join-Path $env:LOCALAPPDATA 'InteractiveNPCsInstallerSmoke'
$operationId = [guid]::NewGuid().ToString('N')
$operationRoot = Join-Path $baseTestRoot $operationId
$installRoot = Join-Path $operationRoot 'app'
$appDataRoot = Join-Path $operationRoot 'data'
$logRoot = Join-Path $operationRoot 'logs'
$expectedOperationPrefix = [System.IO.Path]::GetFullPath($baseTestRoot).TrimEnd('\') + '\'
if (-not [System.IO.Path]::GetFullPath($operationRoot).StartsWith($expectedOperationPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Generated smoke-test root escaped its dedicated boundary.'
}
if ([string]::IsNullOrWhiteSpace($ResultPath)) {
    $ResultPath = Join-Path $packageDirectory "installer-smoke-result-$operationId.json"
}
$resultFullPath = Convert-ToFullPath -Path $ResultPath -BasePath (Get-Location).Path
$preInstallSnapshot = Get-InstallTargetSnapshot -InstallRoot $installRoot -ShortcutPaths $shortcutPaths
if ($preInstallSnapshot.install_root_exists -or @($preInstallSnapshot.processes).Count -gt 0 -or
    @($preInstallSnapshot.files | Where-Object { $_.exists }).Count -gt 0 -or
    @($preInstallSnapshot.shortcuts | Where-Object { $_.exists }).Count -gt 0 -or
    $preInstallSnapshot.registry.uninstall_key_exists -or $preInstallSnapshot.registry.manufacturer_key_exists -or $preInstallSnapshot.registry.run_value_exists) {
    throw 'Generated smoke targets were not clean before installation.'
}

$result = [ordered]@{
    schema_version = 1
    operation_id = $operationId
    started_at_utc = [DateTime]::UtcNow.ToString('o')
    package_manifest = $manifestFullPath
    configuration = $manifest.configuration
    distribution = $manifest.distribution
    source_identity = Get-SourceIdentity -RepoRoot $repoRoot -ManifestPath $manifestFullPath
    target_definition = [ordered]@{
        install_root = $installRoot
        required_files = @($requiredInstalledFiles)
        process_names = @($mainExecutableName, 'npc-runtime.exe', 'npc-media-broker.exe')
        expected_process_topology = "$mainExecutableName -> [npc-runtime.exe, npc-media-broker.exe] as direct persistent children"
        shortcut_paths = @($shortcutPaths)
        registry_paths = @($uninstallRegistryPath, $manufacturerRegistryPath, "${runRegistryPath}::$productName")
        health_probe_path = $healthProbePath
    }
    pre_install_snapshot = $preInstallSnapshot
    pre_install_health_probe_exists = Test-Path -LiteralPath $healthProbePath -PathType Leaf
    post_uninstall_snapshot = $null
    elevated_host = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    installed_files_verified = $false
    installed_runtime_sha256 = $null
    installed_broker_sha256 = $null
    runtime_hash_verified = $false
    broker_hash_verified = $false
    profile_count = 0
    catalog_verified = $false
    model_manifest_verified = $false
    doctor_status = $null
    app_data_acl_private = $null
    app_data_acl_evidence = $null
    app_config_acl_private = $null
    app_config_acl_evidence = $null
    app_health_probe_observed = $false
    app_health_evidence = $null
    app_launch_observed = $false
    runtime_child_observed = $false
    broker_child_observed = $false
    runtime_supervision_observed = $false
    broker_supervision_observed = $false
    no_orphans_after_parent_termination = $null
    observed_process_topology = $null
    second_monitor_placement = [ordered]@{
        attempted = $false
        helper_available = $false
        dry_run_status = $null
        apply_status = $null
        applied = $false
        target_device = $null
        window_count = 0
        error = $null
    }
    uninstall_exit_code = $null
    no_remaining_processes = $null
    no_install_files = $null
    no_shortcuts = $null
    no_registry_entries = $null
    harness_workspace_removed = $false
    health_probe_removed = $false
    emergency_cleanup = [ordered]@{
        used = $false
        normal_uninstall_attempted = $false
        uninstaller_retried = $false
        processes_terminated = @()
        registry_entries_removed = @()
        shortcuts_removed = @()
        install_root_removed = $false
        errors = @()
        final_remaining_processes = $null
        final_install_root_exists = $null
        final_shortcuts_exist = $null
        final_registry_entries_exist = $null
    }
    status = 'running'
    error = $null
}

if ($PreflightOnly) {
    $result.status = 'preflight_passed'
    $result.completed_at_utc = [DateTime]::UtcNow.ToString('o')
    Write-SmokeResult -Result $result -Path $resultFullPath
    Write-Host "Installer smoke preflight passed without installing: $resultFullPath" -ForegroundColor Green
    exit 0
}

$mainProcess = $null
$failure = $null
New-Item -ItemType Directory -Path $logRoot -Force | Out-Null

try {
    if (-not $PSCmdlet.ShouldProcess($installRoot, "silently install, smoke-test, and uninstall $productName")) {
        throw 'Installer smoke test was declined.'
    }

    $installProcess = Start-Process -FilePath $installerPath -ArgumentList @('/S', "/D=$installRoot") -PassThru
    Wait-BoundedProcess -Process $installProcess -TimeoutSeconds $ProcessTimeoutSeconds -Label 'NSIS installer'

    foreach ($relativePath in $requiredInstalledFiles) {
        $requiredPath = Join-Path $installRoot $relativePath
        if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) { throw "Installed payload is missing: $relativePath" }
    }
    $result.installed_files_verified = $true
    $result.installed_runtime_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-runtime.exe')
    $result.installed_broker_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-media-broker.exe')
    $result.runtime_hash_verified = -not [string]::IsNullOrWhiteSpace($result.source_identity.prepared_runtime_sha256) -and
        $result.installed_runtime_sha256 -eq $result.source_identity.prepared_runtime_sha256
    $result.broker_hash_verified = -not [string]::IsNullOrWhiteSpace($result.source_identity.prepared_broker_sha256) -and
        $result.installed_broker_sha256 -eq $result.source_identity.prepared_broker_sha256
    if (-not $result.runtime_hash_verified -or -not $result.broker_hash_verified) {
        throw 'Installed project sidecar hash differs from the prepared source binary.'
    }

    $profiles = @(Get-ChildItem -LiteralPath (Join-Path $installRoot 'profiles/games') -Filter 'profile.json' -File -Recurse)
    if ($profiles.Count -ne 20) { throw "Expected 20 installed game profiles; found $($profiles.Count)." }
    $profileIds = New-Object System.Collections.Generic.HashSet[string]
    foreach ($profilePath in $profiles) {
        $profile = Get-Content -LiteralPath $profilePath.FullName -Raw | ConvertFrom-Json
        if ([string]::IsNullOrWhiteSpace($profile.id) -or -not $profileIds.Add([string]$profile.id)) {
            throw "Installed game profile has a missing or duplicate id: $($profilePath.FullName)"
        }
    }
    $result.profile_count = $profiles.Count

    $catalog = Get-Content -LiteralPath (Join-Path $installRoot 'catalog/v1/catalog.json') -Raw | ConvertFrom-Json
    if ($null -eq $catalog -or $null -eq $catalog.content -or @($catalog.content.providers).Count -eq 0) {
        throw 'Installed provider catalog is empty or incompatible.'
    }
    $result.catalog_verified = $true

    $modelManifest = Get-Content -LiteralPath (Join-Path $installRoot 'packaging/model-packs/model-pack-manifest.example.json') -Raw | ConvertFrom-Json
    if ($modelManifest.schema -ne 'npc.model-pack/v1' -or [string]::IsNullOrWhiteSpace($modelManifest.pack_id)) {
        throw 'Installed model-pack manifest example is incompatible.'
    }
    $result.model_manifest_verified = $true

    $doctor = Invoke-CapturedProcess -FilePath (Join-Path $installRoot 'npc-runtime.exe') -Arguments @('--repo-root', $installRoot, '--app-data', $appDataRoot, 'doctor') -WorkingDirectory $installRoot -LogDirectory $logRoot -Label 'runtime-doctor' -TimeoutSeconds $ProcessTimeoutSeconds
    $doctorReport = $doctor.stdout | ConvertFrom-Json
    if ($doctorReport.profileCount -ne 20 -or $doctorReport.providerCount -le 0 -or $doctorReport.modelCount -le 0 -or $doctorReport.modelManifestExample -ne 'valid' -or $doctorReport.status -eq 'failed') {
        throw 'Installed runtime doctor reported an invalid payload or failed state.'
    }
    if ($doctorReport.performanceMeasurementsCaptured -ne $false -or $doctorReport.powerProfileChanged -ne $false) {
        throw 'Installed runtime doctor changed performance state or claimed measurements.'
    }
    $result.doctor_status = [string]$doctorReport.status
    $result.app_data_acl_evidence = Get-PrivateDirectoryAclEvidence -Path (Join-Path $appDataRoot 'runtime')
    $result.app_data_acl_private = $result.app_data_acl_evidence.private
    if (-not $result.app_data_acl_private) {
        throw 'app_data_acl_private=false: runtime data DACL is inherited or grants an unexpected principal.'
    }

    $previousSmokeTrigger = $env:NPC2_INSTALLER_SMOKE
    try {
        $env:NPC2_INSTALLER_SMOKE = '1'
        $mainProcess = Start-Process -FilePath (Join-Path $installRoot $mainExecutableName) -WorkingDirectory $installRoot -PassThru
    }
    finally {
        if ($null -eq $previousSmokeTrigger) { Remove-Item Env:\NPC2_INSTALLER_SMOKE -ErrorAction SilentlyContinue }
        else { $env:NPC2_INSTALLER_SMOKE = $previousSmokeTrigger }
    }

    $placementHelper = Join-Path $env:USERPROFILE '.codex\skills\prefer-second-monitor\scripts\place_process_windows.ps1'
    $result.second_monitor_placement.attempted = $true
    $result.second_monitor_placement.helper_available = Test-Path -LiteralPath $placementHelper -PathType Leaf
    if ($result.second_monitor_placement.helper_available) {
        try {
            $placementDeadline = [DateTime]::UtcNow.AddSeconds(5)
            $dryPlacement = $null
            do {
                $dryOutput = Invoke-CapturedProcess -FilePath (Join-Path $PSHOME 'powershell.exe') -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $placementHelper, '-TargetProcessId', [string]$mainProcess.Id) -WorkingDirectory $installRoot -LogDirectory $logRoot -Label 'second-monitor-dry-run' -TimeoutSeconds 10
                $dryPlacement = $dryOutput.stdout | ConvertFrom-Json
                if ($dryPlacement.status -eq 'single_monitor_noop' -or $dryPlacement.window_count -gt 0) { break }
                Start-Sleep -Milliseconds 250
            } while ([DateTime]::UtcNow -lt $placementDeadline)
            $result.second_monitor_placement.dry_run_status = [string]$dryPlacement.status
            $result.second_monitor_placement.window_count = [int]$dryPlacement.window_count
            if (@($dryPlacement.PSObject.Properties.Name) -contains 'target' -and $null -ne $dryPlacement.target) {
                $result.second_monitor_placement.target_device = [string]$dryPlacement.target.device_name
            }
            if ($dryPlacement.window_count -gt 0 -and $dryPlacement.status -eq 'dry_run') {
                $applyOutput = Invoke-CapturedProcess -FilePath (Join-Path $PSHOME 'powershell.exe') -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $placementHelper, '-TargetProcessId', [string]$mainProcess.Id, '-Apply') -WorkingDirectory $installRoot -LogDirectory $logRoot -Label 'second-monitor-apply' -TimeoutSeconds 10
                $applyPlacement = $applyOutput.stdout | ConvertFrom-Json
                $result.second_monitor_placement.apply_status = [string]$applyPlacement.status
                $result.second_monitor_placement.applied = [bool]$applyPlacement.applied
                $result.second_monitor_placement.window_count = [int]$applyPlacement.window_count
            }
        }
        catch {
            $result.second_monitor_placement.error = $_.Exception.Message
        }
    } else {
        $result.second_monitor_placement.error = 'Placement helper unavailable; primary-display fallback used.'
    }

    $runtimeEvidence = $null
    $brokerEvidence = $null
    $childStartupDeadline = [DateTime]::UtcNow.AddSeconds([Math]::Min($ProcessTimeoutSeconds, 15))
    while ([DateTime]::UtcNow -lt $childStartupDeadline -and ($null -eq $runtimeEvidence -or $null -eq $brokerEvidence)) {
        $mainProcess.Refresh()
        if ($mainProcess.HasExited) { throw "Installed application exited during supervised-child startup with code $($mainProcess.ExitCode)." }
        $children = @(Get-TestInstallProcesses -InstallRoot $installRoot | Where-Object { [int]$_.ParentProcessId -eq $mainProcess.Id })
        $runtime = @($children | Where-Object { $_.Name -eq 'npc-runtime.exe' })
        $broker = @($children | Where-Object { $_.Name -eq 'npc-media-broker.exe' })
        if ($runtime.Count -eq 1) {
            $runtimeEvidence = Get-SupervisedChildEvidence -Process $runtime[0] -ExpectedParentId $mainProcess.Id -ExpectedExecutablePath (Join-Path $installRoot 'npc-runtime.exe') -OperationId $operationId
        }
        if ($broker.Count -eq 1) {
            $brokerEvidence = Get-SupervisedChildEvidence -Process $broker[0] -ExpectedParentId $mainProcess.Id -ExpectedExecutablePath (Join-Path $installRoot 'npc-media-broker.exe') -OperationId $operationId
        }
        if ($null -eq $runtimeEvidence -or $null -eq $brokerEvidence) { Start-Sleep -Milliseconds 100 }
    }
    $result.runtime_child_observed = $null -ne $runtimeEvidence
    $result.broker_child_observed = $null -ne $brokerEvidence
    $result.observed_process_topology = [ordered]@{
        shell = [ordered]@{
            process_id = $mainProcess.Id
            executable_path = Join-Path $installRoot $mainExecutableName
        }
        runtime = $runtimeEvidence
        broker = $brokerEvidence
    }
    if ($null -eq $runtimeEvidence -or -not $runtimeEvidence.parent_matches_shell -or -not $runtimeEvidence.path_matches_install_target -or -not $runtimeEvidence.hash_matches_installed_file) {
        throw 'runtime_supervision_observed=false: Tauri did not maintain one authenticated runtime child from the installed target.'
    }
    if ($null -eq $brokerEvidence -or -not $brokerEvidence.parent_matches_shell -or -not $brokerEvidence.path_matches_install_target -or -not $brokerEvidence.hash_matches_installed_file) {
        throw 'broker_supervision_observed=false: Tauri did not maintain one authenticated broker child from the installed target.'
    }

    $healthDeadline = [DateTime]::UtcNow.AddSeconds([Math]::Min($ProcessTimeoutSeconds, 15))
    while (-not (Test-Path -LiteralPath $healthProbePath -PathType Leaf) -and [DateTime]::UtcNow -lt $healthDeadline) {
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-Path -LiteralPath $healthProbePath -PathType Leaf)) {
        throw 'Installed app health probe was not produced through the Tauri backend path.'
    }
    $healthProbe = Get-Content -LiteralPath $healthProbePath -Raw | ConvertFrom-Json
    $runtimeHealthOk = $healthProbe.schemaVersion -eq 1 -and $healthProbe.runtimeAuthenticated -eq $true -and
        $healthProbe.runtime.connected -eq $true -and $healthProbe.runtime.fixtureOnly -eq $false -and
        $healthProbe.runtime.state -eq 'ready' -and [int]$healthProbe.runtime.processId -eq $runtimeEvidence.process_id -and
        $healthProbe.runtime.protocolVersion -eq '1.0.0'
    $brokerHealthOk = $healthProbe.schemaVersion -eq 1 -and $healthProbe.mediaBrokerAuthenticated -eq $true -and
        $healthProbe.mediaBroker.connected -eq $true -and $healthProbe.mediaBroker.fixtureOnly -eq $false -and
        $healthProbe.mediaBroker.state -eq 'ready' -and [int]$healthProbe.mediaBroker.processId -eq $brokerEvidence.process_id -and
        [int]$healthProbe.mediaBroker.protocolVersion -eq 1
    $result.app_health_evidence = [ordered]@{
        schema_version = $healthProbe.schemaVersion
        runtime_authenticated = [bool]$healthProbe.runtimeAuthenticated
        runtime_connected = [bool]$healthProbe.runtime.connected
        runtime_process_id = [int]$healthProbe.runtime.processId
        runtime_protocol_version = [string]$healthProbe.runtime.protocolVersion
        broker_authenticated = [bool]$healthProbe.mediaBrokerAuthenticated
        broker_connected = [bool]$healthProbe.mediaBroker.connected
        broker_process_id = [int]$healthProbe.mediaBroker.processId
        broker_protocol_version = [int]$healthProbe.mediaBroker.protocolVersion
    }
    $result.app_health_probe_observed = $runtimeHealthOk -and $brokerHealthOk
    if (-not $result.app_health_probe_observed) {
        throw 'Installed app health path did not confirm both authenticated supervised children.'
    }
    $result.app_config_acl_evidence = Get-PrivateDirectoryAclEvidence -Path $appConfigRoot
    $result.app_config_acl_private = $result.app_config_acl_evidence.private
    if (-not $result.app_config_acl_private) {
        throw 'app_config_acl_private=false: Tauri smoke-health directory DACL is inherited or grants an unexpected principal.'
    }

    $observationDeadline = [DateTime]::UtcNow.AddSeconds($LaunchObservationSeconds)
    while ([DateTime]::UtcNow -lt $observationDeadline) {
        $mainProcess.Refresh()
        if ($mainProcess.HasExited) { throw "Installed application exited during the bounded launch observation with code $($mainProcess.ExitCode)." }
        $children = @(Get-TestInstallProcesses -InstallRoot $installRoot | Where-Object { [int]$_.ParentProcessId -eq $mainProcess.Id })
        $runtime = @($children | Where-Object { $_.Name -eq 'npc-runtime.exe' -and [int]$_.ProcessId -eq $runtimeEvidence.process_id })
        $broker = @($children | Where-Object { $_.Name -eq 'npc-media-broker.exe' -and [int]$_.ProcessId -eq $brokerEvidence.process_id })
        if ($runtime.Count -ne 1 -or $broker.Count -ne 1) {
            throw 'A supervised project child exited or was replaced during the bounded observation.'
        }
        Start-Sleep -Milliseconds 250
    }
    $result.app_launch_observed = $true
    $result.runtime_supervision_observed = $true
    $result.broker_supervision_observed = $true
    Stop-OwnedProcess -Process $mainProcess

    $parentDeathDeadline = [DateTime]::UtcNow.AddSeconds(5)
    $unexpectedProcesses = @(Get-TestInstallProcesses -InstallRoot $installRoot)
    while ($unexpectedProcesses.Count -gt 0 -and [DateTime]::UtcNow -lt $parentDeathDeadline) {
        Start-Sleep -Milliseconds 100
        $unexpectedProcesses = @(Get-TestInstallProcesses -InstallRoot $installRoot)
    }
    $result.no_orphans_after_parent_termination = $unexpectedProcesses.Count -eq 0
    if ($unexpectedProcesses.Count -gt 0) {
        throw "Parent termination left $($unexpectedProcesses.Count) supervised project child process(es) running."
    }
}
catch {
    $failure = $_.Exception.Message
}
finally {
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    $processesBeforeUninstall = @(Get-TestInstallProcesses -InstallRoot $installRoot)
    $mainStillRunning = @($processesBeforeUninstall | Where-Object { $_.Name -eq $mainExecutableName }).Count -gt 0
    if (-not $mainStillRunning -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        $result.emergency_cleanup.normal_uninstall_attempted = $true
        try {
            $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList @('/S') -PassThru
            Wait-BoundedProcess -Process $uninstallProcess -TimeoutSeconds $ProcessTimeoutSeconds -Label 'NSIS uninstaller'
            $result.uninstall_exit_code = 0
        }
        catch {
            $result.uninstall_exit_code = -1
            if ($null -eq $failure) { $failure = $_.Exception.Message }
        }
    } elseif ($mainStillRunning -and $null -eq $failure) {
        $failure = 'The main application remained running, so a normal silent uninstall could not be attempted safely.'
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Test-Path -LiteralPath $installRoot) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 250 }

    # This snapshot is the acceptance evidence. Nothing below this point may
    # change these booleans from failure to success.
    $postUninstallSnapshot = Get-InstallTargetSnapshot -InstallRoot $installRoot -ShortcutPaths $shortcutPaths
    $result.post_uninstall_snapshot = $postUninstallSnapshot
    $result.no_remaining_processes = @($postUninstallSnapshot.processes).Count -eq 0
    $result.no_install_files = -not $postUninstallSnapshot.install_root_exists -and @($postUninstallSnapshot.files | Where-Object { $_.exists }).Count -eq 0
    $result.no_shortcuts = @($postUninstallSnapshot.shortcuts | Where-Object { $_.exists }).Count -eq 0
    $result.no_registry_entries = -not $postUninstallSnapshot.registry.uninstall_key_exists -and
        -not $postUninstallSnapshot.registry.manufacturer_key_exists -and
        -not $postUninstallSnapshot.registry.run_value_exists
    $dirtyAssertions = @()
    if (-not $result.no_remaining_processes) { $dirtyAssertions += 'processes' }
    if (-not $result.no_install_files) { $dirtyAssertions += 'install files' }
    if (-not $result.no_shortcuts) { $dirtyAssertions += 'shortcuts' }
    if (-not $result.no_registry_entries) { $dirtyAssertions += 'registry entries' }
    if ($dirtyAssertions.Count -gt 0 -and $null -eq $failure) {
        $failure = "Normal uninstall left residue: $($dirtyAssertions -join ', ')."
    }

    # Emergency cleanup is deliberately after the evidence snapshot and is
    # reported separately. It can make the host safe, never make the smoke pass.
    if (Test-Path -LiteralPath $healthProbePath -PathType Leaf) {
        try {
            Remove-Item -LiteralPath $healthProbePath -Force -ErrorAction Stop
            $result.health_probe_removed = $true
        }
        catch {
            if ($null -eq $failure) { $failure = 'The fixed installer health probe could not be removed.' }
        }
    } else {
        $result.health_probe_removed = $true
    }
    $ownedProcesses = @(Get-TestInstallProcesses -InstallRoot $installRoot)
    foreach ($owned in $ownedProcesses) {
        $result.emergency_cleanup.used = $true
        try {
            Stop-Process -Id $owned.ProcessId -Force -ErrorAction Stop
            $result.emergency_cleanup.processes_terminated = @($result.emergency_cleanup.processes_terminated + [int]$owned.ProcessId)
        }
        catch {
            $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + "Could not terminate owned process $($owned.ProcessId).")
        }
    }
    if ($ownedProcesses.Count -gt 0) { Start-Sleep -Milliseconds 500 }

    if ((-not $result.emergency_cleanup.normal_uninstall_attempted -or $result.uninstall_exit_code -ne 0) -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        $result.emergency_cleanup.used = $true
        $result.emergency_cleanup.uninstaller_retried = $true
        try {
            $retryUninstaller = Start-Process -FilePath $uninstaller -ArgumentList @('/S') -PassThru
            Wait-BoundedProcess -Process $retryUninstaller -TimeoutSeconds $ProcessTimeoutSeconds -Label 'emergency NSIS uninstaller'
        }
        catch {
            $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + $_.Exception.Message)
        }
    }

    if (Test-Path -LiteralPath $uninstallRegistryPath) {
        $registeredLocation = (Get-RegistryValueSnapshot -Path $uninstallRegistryPath -Name 'InstallLocation').value
        if (-not [string]::IsNullOrWhiteSpace($registeredLocation) -and $registeredLocation.Trim('"') -eq $installRoot) {
            $result.emergency_cleanup.used = $true
            try {
                Remove-Item -LiteralPath $uninstallRegistryPath -Recurse -Force -ErrorAction Stop
                $result.emergency_cleanup.registry_entries_removed = @($result.emergency_cleanup.registry_entries_removed + $uninstallRegistryPath)
            }
            catch { $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + $_.Exception.Message) }
        }
    }
    if (Test-Path -LiteralPath $manufacturerRegistryPath) {
        $registeredLocation = (Get-RegistryValueSnapshot -Path $manufacturerRegistryPath -Name '').value
        if ([string]::IsNullOrWhiteSpace($registeredLocation) -or $registeredLocation -eq $installRoot) {
            $result.emergency_cleanup.used = $true
            try {
                Remove-Item -LiteralPath $manufacturerRegistryPath -Recurse -Force -ErrorAction Stop
                $result.emergency_cleanup.registry_entries_removed = @($result.emergency_cleanup.registry_entries_removed + $manufacturerRegistryPath)
            }
            catch { $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + $_.Exception.Message) }
        }
    }
    $remainingRunValue = Get-RegistryValueSnapshot -Path $runRegistryPath -Name $productName
    if ($remainingRunValue.exists -and ($remainingRunValue.value -like "*$operationId*" -or $remainingRunValue.value -like "*$installRoot*")) {
        $result.emergency_cleanup.used = $true
        try {
            Remove-ItemProperty -LiteralPath $runRegistryPath -Name $productName -Force -ErrorAction Stop
            $result.emergency_cleanup.registry_entries_removed = @($result.emergency_cleanup.registry_entries_removed + "${runRegistryPath}::$productName")
        }
        catch { $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + $_.Exception.Message) }
    }
    foreach ($shortcutPath in $shortcutPaths) {
        if (Test-Path -LiteralPath $shortcutPath -PathType Leaf) {
            $result.emergency_cleanup.used = $true
            try {
                Remove-Item -LiteralPath $shortcutPath -Force -ErrorAction Stop
                $result.emergency_cleanup.shortcuts_removed = @($result.emergency_cleanup.shortcuts_removed + $shortcutPath)
            }
            catch { $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + $_.Exception.Message) }
        }
    }

    $installRootWasResidue = Test-Path -LiteralPath $installRoot
    if ($installRootWasResidue) { $result.emergency_cleanup.used = $true }
    if ($installRootWasResidue -and $null -eq $failure) {
        $failure = 'Install root remained after normal uninstall.'
    }
    $cleanupDeadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Test-Path -LiteralPath $operationRoot) -and [DateTime]::UtcNow -lt $cleanupDeadline) {
        try {
            Remove-Item -LiteralPath $operationRoot -Recurse -Force -ErrorAction Stop
            $result.harness_workspace_removed = $true
        }
        catch {
            Start-Sleep -Milliseconds 500
        }
    }
    if ($installRootWasResidue) { $result.emergency_cleanup.install_root_removed = -not (Test-Path -LiteralPath $installRoot) }
    if ((Test-Path -LiteralPath $baseTestRoot -PathType Container) -and @(Get-ChildItem -LiteralPath $baseTestRoot -Force).Count -eq 0) {
        try { Remove-Item -LiteralPath $baseTestRoot -Force -ErrorAction Stop }
        catch { $result.emergency_cleanup.errors = @($result.emergency_cleanup.errors + 'The empty smoke-test parent directory could not be removed.') }
    }

    $finalSnapshot = Get-InstallTargetSnapshot -InstallRoot $installRoot -ShortcutPaths $shortcutPaths
    $result.emergency_cleanup.final_remaining_processes = @($finalSnapshot.processes).Count
    $result.emergency_cleanup.final_install_root_exists = $finalSnapshot.install_root_exists
    $result.emergency_cleanup.final_shortcuts_exist = @($finalSnapshot.shortcuts | Where-Object { $_.exists }).Count -gt 0
    $result.emergency_cleanup.final_registry_entries_exist = $finalSnapshot.registry.uninstall_key_exists -or $finalSnapshot.registry.manufacturer_key_exists -or $finalSnapshot.registry.run_value_exists
    if (@($result.emergency_cleanup.errors).Count -gt 0 -and $null -eq $failure) { $failure = 'Emergency cleanup reported one or more errors.' }

    $result.status = $(if ($null -eq $failure) { 'passed' } else { 'failed' })
    $result.error = $failure
    $result.completed_at_utc = [DateTime]::UtcNow.ToString('o')
    Write-SmokeResult -Result $result -Path $resultFullPath
}

if ($null -ne $failure) {
    Write-Error "Installer smoke test failed: $failure. Result: $resultFullPath"
    exit 1
}
Write-Host "Installer smoke test passed: $resultFullPath" -ForegroundColor Green
exit 0
