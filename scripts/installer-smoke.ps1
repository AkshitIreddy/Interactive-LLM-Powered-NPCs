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

    [string]$InstallerSmokeRoot,

    [string]$ResultPath,

    [switch]$CaptureInstalledPrivacyProof,

    [string]$PrivacyReleaseCandidateId,

    [string]$PrivacyApplicationVersion,

    [string]$PrivacyTelemetryInventoryPath,

    [string]$PrivacyProofPath,

    [string]$PrivacyCaptureEvidencePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:machineResultWritten = $false
. (Join-Path $PSScriptRoot 'windows/model-catalog-promotion-policy.ps1')

trap {
    if (-not $script:machineResultWritten) {
        $manifestVariable = Get-Variable -Name manifestFullPath -ErrorAction SilentlyContinue
        $resultVariable = Get-Variable -Name resultFullPath -ErrorAction SilentlyContinue
        $manifestValue = if ($null -ne $manifestVariable) { [string]$manifestVariable.Value } else { $null }
        $resultValue = if ($null -ne $resultVariable) { [string]$resultVariable.Value } else { $null }
        [ordered]@{
            schema_version = 1
            status = 'failed'
            result_path = $resultValue
            package_manifest = $manifestValue
            error = $_.Exception.Message
        } | ConvertTo-Json -Compress | Write-Output
        $script:machineResultWritten = $true
    }
    [Console]::Error.WriteLine("Installer smoke failed: $($_.Exception.Message)")
    exit 1
}

$productName = 'Interactive NPCs Response Console'
$uninstallRegistryPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$productName"
$manufacturerRegistryPath = "HKCU:\Software\github\$productName"
$runRegistryPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$mainExecutableName = 'interactive-npcs-control.exe'
$requiredInstalledFiles = @(
    $mainExecutableName,
    'npc-runtime.exe',
    'npc-media-broker.exe',
    'npc-mouth-worker.exe',
    'npc-subtitle-presenter.exe',
    'uninstall.exe',
    'catalog\v1\catalog.json',
    'packaging\model-packs\model-catalog-root-v1.json',
    'packaging\model-packs\model-catalog-v1.json',
    'packaging\model-packs\model-pack-manifest.example.json',
    'packaging\model-packs\model-pack-manifest.schema.json'
)

if ($CaptureInstalledPrivacyProof) {
    foreach ($requiredValue in @(
        $PrivacyReleaseCandidateId,
        $PrivacyApplicationVersion,
        $PrivacyTelemetryInventoryPath,
        $PrivacyProofPath,
        $PrivacyCaptureEvidencePath
    )) {
        if ([string]::IsNullOrWhiteSpace($requiredValue)) {
            throw 'Installed privacy capture requires candidate ID, application version, telemetry inventory, proof, and capture-evidence paths.'
        }
    }
}

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

function Get-DirectoryIdentity {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return [pscustomobject]@{ exists = $false; file_count = 0; composite_sha256 = $null }
    }
    $root = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    $lines = @(Get-ChildItem -LiteralPath $root -File -Recurse -Force -ErrorAction Stop |
        Sort-Object FullName | ForEach-Object {
            $relative = $_.FullName.Substring($root.Length).TrimStart('\').Replace('\', '/')
            "$relative`:$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())"
        })
    return [pscustomobject]@{
        exists = $true
        file_count = $lines.Count
        composite_sha256 = Get-StringSha256 -Value ($lines -join "`n")
    }
}

function Get-DirectoryMetadataIdentity {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return [pscustomobject]@{ exists = $false; entry_count = 0; composite_sha256 = $null }
    }
    $root = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    $lines = @(Get-ChildItem -LiteralPath $root -Recurse -Force -ErrorAction Stop |
        Sort-Object FullName | ForEach-Object {
            $relative = $_.FullName.Substring($root.Length).TrimStart('\').Replace('\', '/')
            $length = if ($_.PSIsContainer) { '-' } else { [string]$_.Length }
            "$relative`:$($_.PSIsContainer):$length`:$($_.LastWriteTimeUtc.Ticks)"
        })
    return [pscustomobject]@{
        exists = $true
        entry_count = $lines.Count
        composite_sha256 = Get-StringSha256 -Value ($lines -join "`n")
    }
}

function Get-FreeLoopbackPort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    try {
        $listener.Start()
        return ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    }
    finally { $listener.Stop() }
}

function Invoke-CdpExpression {
    param(
        [int]$Port,
        [string]$Expression,
        [int]$TimeoutSeconds = 5
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $target = $null
    do {
        try {
            $target = @(Invoke-RestMethod -Uri "http://127.0.0.1:$Port/json/list" -TimeoutSec 1 |
                Where-Object { $_.type -eq 'page' } | Select-Object -First 1)
        }
        catch { $target = @() }
        if ($target.Count -eq 0) { Start-Sleep -Milliseconds 100 }
    } while ($target.Count -eq 0 -and [DateTime]::UtcNow -lt $deadline)
    if ($target.Count -eq 0) { throw 'The installed WebView did not expose its bounded local inspection target.' }

    $socket = [System.Net.WebSockets.ClientWebSocket]::new()
    $cancelSource = [Threading.CancellationTokenSource]::new()
    $cancelSource.CancelAfter([TimeSpan]::FromSeconds($TimeoutSeconds))
    $cancel = $cancelSource.Token
    try {
        $null = $socket.ConnectAsync([Uri]$target[0].webSocketDebuggerUrl, $cancel).GetAwaiter().GetResult()
        $command = @{
            id = 1
            method = 'Runtime.evaluate'
            params = @{
                expression = $Expression
                returnByValue = $true
            }
        } | ConvertTo-Json -Compress -Depth 6
        $bytes = [Text.Encoding]::UTF8.GetBytes($command)
        $null = $socket.SendAsync(
            [ArraySegment[byte]]::new($bytes),
            [Net.WebSockets.WebSocketMessageType]::Text,
            $true,
            $cancel
        ).GetAwaiter().GetResult()

        $response = $null
        do {
            $stream = New-Object System.IO.MemoryStream
            try {
                do {
                    $buffer = New-Object byte[] 8192
                    $received = $socket.ReceiveAsync([ArraySegment[byte]]::new($buffer), $cancel).GetAwaiter().GetResult()
                    if ($received.MessageType -eq [Net.WebSockets.WebSocketMessageType]::Close) {
                        throw 'The WebView inspection socket closed before returning onboarding evidence.'
                    }
                    $stream.Write($buffer, 0, $received.Count)
                } while (-not $received.EndOfMessage)
                $candidateResponse = [Text.Encoding]::UTF8.GetString($stream.ToArray()) | ConvertFrom-Json
                if ($candidateResponse.id -eq 1) { $response = $candidateResponse }
            }
            finally { $stream.Dispose() }
        } while ($null -eq $response)
        if ($response.id -ne 1 -or $null -eq $response.result.result.value) {
            throw 'The WebView inspection response omitted its evaluated value.'
        }
        return [string]$response.result.result.value
    }
    finally {
        $socket.Dispose()
        $cancelSource.Dispose()
    }
}

function ConvertTo-NativeCommandLine {
    param([Parameter(Mandatory)][string[]]$Arguments)
    return (($Arguments | ForEach-Object {
        $argument = [string]$_
        if ($argument.Length -eq 0) { return '""' }
        if ($argument -notmatch '[\s"]') { return $argument }
        $escaped = [regex]::Replace($argument, '(\\*)"', '$1$1\"')
        $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
        return '"' + $escaped + '"'
    }) -join ' ')
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
        review_tauri_config_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'packaging/windows/tauri.review.conf.json')
        release_tauri_config_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'packaging/windows/tauri.release.conf.json')
        review_test_game_script_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'scripts/windows/prepare-review-test-game.ps1')
        review_test_game_verifier_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'scripts/windows/verify-review-test-game.ps1')
        review_test_game_source_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'scripts/synthetic-game-replay/SyntheticGameReplay.cs')
        review_test_game_source_manifest_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'scripts/synthetic-game-replay/SOURCE-MANIFEST.json')
        prepared_runtime_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/binaries/npc-runtime-x86_64-pc-windows-msvc.exe')
        prepared_broker_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/binaries/npc-media-broker-x86_64-pc-windows-msvc.exe')
        prepared_mouth_worker_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/binaries/npc-mouth-worker-x86_64-pc-windows-msvc.exe')
        prepared_subtitle_presenter_sha256 = Get-OptionalFileSha256 -Path (Join-Path $RepoRoot 'apps/control/src-tauri/binaries/npc-subtitle-presenter-x86_64-pc-windows-msvc.exe')
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
    $allowedNames = @(
        $mainExecutableName,
        'npc-runtime.exe',
        'npc-media-broker.exe',
        'npc-mouth-worker.exe',
        'npc-subtitle-presenter.exe'
    )
    return @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        $allowedNames -contains $_.Name -and (
            (-not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and
                ($_.ExecutablePath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) -or $_.ExecutablePath.IndexOf($operationToken, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)) -or
            (-not [string]::IsNullOrWhiteSpace($_.CommandLine) -and $_.CommandLine.IndexOf($operationToken, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
        )
    })
}

function Get-ReviewWebViewProcesses {
    param([string]$ReviewLocalAppDataRoot)
    $marker = [System.IO.Path]::GetFullPath($ReviewLocalAppDataRoot)
    return @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -eq 'msedgewebview2.exe' -and
        -not [string]::IsNullOrWhiteSpace($_.CommandLine) -and
        $_.CommandLine.IndexOf($marker, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
    })
}

function Get-RegistryValueSnapshot {
    param([string]$Path, [string]$Name)
    try {
        $key = Get-Item -LiteralPath $Path -ErrorAction Stop
        $names = @($key.GetValueNames())
        if ($names -notcontains $Name) { return [pscustomobject]@{ exists = $false; value = $null } }
        return [pscustomobject]@{ exists = $true; value = [string]$key.GetValue($Name) }
    }
    catch [System.Management.Automation.ItemNotFoundException] {
        return [pscustomobject]@{ exists = $false; value = $null }
    }
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

function Wait-InstallTargetsClean {
    param(
        [Parameter(Mandatory = $true)][string]$InstallRoot,
        [Parameter(Mandatory = $true)][string[]]$ShortcutPaths,
        [ValidateRange(1, 60)][int]$TimeoutSeconds = 15
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $snapshot = Get-InstallTargetSnapshot -InstallRoot $InstallRoot -ShortcutPaths $ShortcutPaths
        $clean = -not $snapshot.install_root_exists -and
            @($snapshot.processes).Count -eq 0 -and
            @($snapshot.files | Where-Object { $_.exists }).Count -eq 0 -and
            @($snapshot.shortcuts | Where-Object { $_.exists }).Count -eq 0 -and
            -not $snapshot.registry.uninstall_key_exists -and
            -not $snapshot.registry.manufacturer_key_exists -and
            -not $snapshot.registry.run_value_exists
        if ($clean) { return $snapshot }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    return (Get-InstallTargetSnapshot -InstallRoot $InstallRoot -ShortcutPaths $ShortcutPaths)
}

function Convert-ToComparableWindowsPath {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return '' }

    $candidate = $Path.Trim()
    if ($candidate.StartsWith('\\?\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) {
        return '\\' + $candidate.Substring(8)
    }
    if ($candidate.StartsWith('\\?\', [System.StringComparison]::OrdinalIgnoreCase)) {
        return $candidate.Substring(4)
    }
    if ($candidate.StartsWith('\??\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) {
        return '\\' + $candidate.Substring(7)
    }
    if ($candidate.StartsWith('\??\', [System.StringComparison]::OrdinalIgnoreCase)) {
        return $candidate.Substring(4)
    }
    return $candidate
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
    $comparableActualPath = Convert-ToComparableWindowsPath -Path $actualPath
    $comparableExpectedPath = Convert-ToComparableWindowsPath -Path $ExpectedExecutablePath
    $expectedName = [System.IO.Path]::GetFileName($ExpectedExecutablePath)
    $virtualizedSuffix = "\InteractiveNPCsInstallerSmoke\$OperationId\app\$expectedName"
    $pathMatches = -not [string]::IsNullOrWhiteSpace($comparableActualPath) -and (
        $comparableActualPath.Equals($comparableExpectedPath, [System.StringComparison]::OrdinalIgnoreCase) -or
        $comparableActualPath.EndsWith($virtualizedSuffix, [System.StringComparison]::OrdinalIgnoreCase)
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
$productionIdentifier = [string]$manifest.production_application_identifier
$reviewIdentifier = [string]$manifest.application_identifier
if ([string]::IsNullOrWhiteSpace($productionIdentifier) -or
    $reviewIdentifier -ne "$productionIdentifier.review" -or
    [string]$manifest.app_config_folder -ne $reviewIdentifier) {
    throw 'Installer smoke testing requires the isolated Debug .review application namespace.'
}
$expectedReviewConfigPath = Join-Path $repoRoot 'packaging/windows/tauri.review.conf.json'
if ([string]$manifest.packaging_config.path -ne 'packaging/windows/tauri.review.conf.json' -or
    [string]$manifest.packaging_config.sha256 -ne (Get-OptionalFileSha256 -Path $expectedReviewConfigPath)) {
    throw 'Package manifest is not bound to the current isolated review Tauri configuration.'
}
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
$webViewInstaller = $manifest.webview2_offline_installer
if ($null -eq $webViewInstaller -or
    [string]$webViewInstaller.file_name -ne 'MicrosoftEdgeWebView2RuntimeInstallerX64.exe' -or
    [string]$webViewInstaller.file_version -ne '1.3.263.3' -or
    [long]$webViewInstaller.size_bytes -ne 258438352 -or
    [string]$webViewInstaller.sha256 -ne '987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61' -or
    [string]$webViewInstaller.authenticode_status -ne 'Valid' -or
    [string]$webViewInstaller.signer_thumbprint -ne '4028CAD637509D4744B17EC5B42AED8D7A31E6AF' -or
    [string]$webViewInstaller.tauri_config_mode -ne 'skip' -or
    [string]$webViewInstaller.package_contract -ne 'custom-pinned-offline-installer' -or
    $webViewInstaller.package_time_network_acquisition -ne $false -or
    $webViewInstaller.cache_path_recorded -ne $false) {
    throw 'Package manifest does not bind the exact reviewed network-free WebView2 offline installer.'
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
$roamingRoot = [Environment]::GetFolderPath('ApplicationData')
$localAppDataRoot = [Environment]::GetFolderPath('LocalApplicationData')
$appConfigRoot = Join-Path $roamingRoot ([string]$manifest.app_config_folder)
$productionAppConfigRoot = Join-Path $roamingRoot $productionIdentifier
$reviewLocalAppDataRoot = Join-Path $localAppDataRoot $reviewIdentifier
$productionLocalAppDataRoot = Join-Path $localAppDataRoot $productionIdentifier
$healthProbePath = Join-Path $appConfigRoot 'installer-smoke-health-v1.json'
if ((Test-Path -LiteralPath $appConfigRoot) -or (Test-Path -LiteralPath $reviewLocalAppDataRoot)) {
    throw "The isolated review namespace is not clean; refusing to overwrite $appConfigRoot or $reviewLocalAppDataRoot"
}
$productionAppConfigBefore = Get-DirectoryIdentity -Path $productionAppConfigRoot
$productionLocalAppDataBefore = Get-DirectoryMetadataIdentity -Path $productionLocalAppDataRoot

$reviewGame = $manifest.review_test_game
if ($null -eq $reviewGame -or $reviewGame.status -ne 'prepared' -or
    $reviewGame.fixture_kind -ne 'synthetic-original-video-replay' -or
    $reviewGame.fixture_source -ne 'project-source-generated-native-v1' -or
    $reviewGame.renderer -ne 'project-owned-gdi-generated-v1' -or
    $reviewGame.generated_frame_rendering -ne $true -or
    $reviewGame.audio_source -ne 'project-owned-generated-pcm-v1' -or
    $reviewGame.audio_generated_pcm -ne $true -or
    $reviewGame.hardware_acceleration -ne $false -or $reviewGame.nvidia_compute_requested -ne $false -or
    $reviewGame.media_files_bundled -ne 0 -or @($reviewGame.third_party_binaries_bundled).Count -ne 0 -or
    $reviewGame.license_expression -ne 'MIT') {
    throw 'Package manifest does not bind a validated project-owned generated-frame/PCM synthetic review game.'
}
foreach ($gameFile in @(
    @{ path = [string]$reviewGame.executable_path; hash = [string]$reviewGame.executable_sha256 },
    @{ path = [string]$reviewGame.distribution_manifest_path; hash = [string]$reviewGame.distribution_manifest_sha256 },
    @{ path = [string]$reviewGame.sbom_path; hash = [string]$reviewGame.sbom_sha256 },
    @{ path = [string]$reviewGame.notices_path; hash = [string]$reviewGame.notices_sha256 },
    @{ path = [string]$reviewGame.source_path; hash = [string]$reviewGame.source_sha256 },
    @{ path = [string]$reviewGame.source_manifest_path; hash = [string]$reviewGame.source_manifest_sha256 },
    @{ path = [string]$reviewGame.license_path; hash = [string]$reviewGame.license_sha256 },
    @{ path = [string]$reviewGame.build_receipt_path; hash = [string]$reviewGame.build_receipt_sha256 }
)) {
    if (-not (Test-Path -LiteralPath $gameFile.path -PathType Leaf)) {
        throw "Review test-game file is missing: $($gameFile.path)"
    }
    $actualGameHash = (Get-FileHash -LiteralPath $gameFile.path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualGameHash -ne $gameFile.hash.ToLowerInvariant()) {
        throw "Review test-game hash mismatch: $($gameFile.path)"
    }
}
$reviewGameVerification = (& (Join-Path $PSScriptRoot 'windows/verify-review-test-game.ps1') -Directory ([string]$reviewGame.directory)) | ConvertFrom-Json
if ($reviewGameVerification.status -ne 'passed' -or $reviewGameVerification.file_count -ne 5 -or
    $reviewGameVerification.third_party_binary_count -ne 0 -or
    $reviewGameVerification.manifest_sha256 -cne [string]$reviewGame.distribution_manifest_sha256) {
    throw 'Review test-game fail-closed allowlist/provenance verification failed.'
}

$baseTestRoot = if ([string]::IsNullOrWhiteSpace($InstallerSmokeRoot)) {
    Join-Path $env:LOCALAPPDATA 'InteractiveNPCsInstallerSmoke'
} else {
    [System.IO.Path]::GetFullPath($InstallerSmokeRoot)
}
if ((Test-Path -LiteralPath $baseTestRoot) -and
    ((Get-Item -LiteralPath $baseTestRoot -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Installer smoke root must not be a reparse point: $baseTestRoot"
}
$operationId = [guid]::NewGuid().ToString('N')
$operationRoot = Join-Path $baseTestRoot $operationId
$installRoot = Join-Path $operationRoot 'app'
$appDataRoot = Join-Path $operationRoot 'data'
$logRoot = Join-Path $operationRoot 'logs'
$installedDistributionManifestEvidencePath = Join-Path $packageDirectory `
    "installed-distribution-manifest-$operationId.json"
$reinstalledDistributionManifestEvidencePath = Join-Path $packageDirectory `
    "reinstalled-distribution-manifest-$operationId.json"
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
    application_identifier = $reviewIdentifier
    production_application_identifier = $productionIdentifier
    source_identity = Get-SourceIdentity -RepoRoot $repoRoot -ManifestPath $manifestFullPath
    target_definition = [ordered]@{
        install_root = $installRoot
        required_files = @($requiredInstalledFiles)
        process_names = @(
            $mainExecutableName,
            'npc-runtime.exe',
            'npc-media-broker.exe',
            'npc-mouth-worker.exe',
            'npc-subtitle-presenter.exe'
        )
        expected_process_topology = "$mainExecutableName -> [npc-runtime.exe, npc-media-broker.exe] as direct persistent children"
        shortcut_paths = @($shortcutPaths)
        registry_paths = @($uninstallRegistryPath, $manufacturerRegistryPath, "${runRegistryPath}::$productName")
        health_probe_path = $healthProbePath
        review_app_config_root = $appConfigRoot
        review_local_app_data_root = $reviewLocalAppDataRoot
        production_app_config_root = $productionAppConfigRoot
        production_local_app_data_root = $productionLocalAppDataRoot
        review_test_game_executable = [string]$reviewGame.executable_path
    }
    pre_install_snapshot = $preInstallSnapshot
    pre_install_health_probe_exists = Test-Path -LiteralPath $healthProbePath -PathType Leaf
    clean_first_run_namespace = -not (Test-Path -LiteralPath $appConfigRoot) -and
        -not (Test-Path -LiteralPath $reviewLocalAppDataRoot)
    production_app_config_before = $productionAppConfigBefore
    production_app_config_after = $null
    production_local_app_data_before = $productionLocalAppDataBefore
    production_local_app_data_after = $null
    production_namespace_unchanged = $null
    post_uninstall_snapshot = $null
    elevated_host = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    installed_files_verified = $false
    installed_control_sha256 = $null
    control_hash_verified = $false
    installed_runtime_sha256 = $null
    installed_broker_sha256 = $null
    installed_mouth_worker_sha256 = $null
    installed_subtitle_presenter_sha256 = $null
    runtime_hash_verified = $false
    broker_hash_verified = $false
    mouth_worker_hash_verified = $false
    subtitle_presenter_hash_verified = $false
    installed_distribution_reconciled = $false
    installed_legal_resources_reconciled = $false
    unclassified_installed_files_rejected = $false
    installed_distribution_manifest_path = $null
    installed_distribution_manifest_sha256 = $null
    reinstalled_distribution_reconciled = $false
    installed_model_catalog_promotion_policy = $null
    installed_model_catalog_promotion_policy_verified = $false
    reinstalled_model_catalog_promotion_policy = $null
    reinstalled_model_catalog_promotion_policy_verified = $false
    installed_privacy_capture_requested = [bool]$CaptureInstalledPrivacyProof
    installed_privacy_capture_completed = $false
    installed_privacy_proof_path = $null
    installed_privacy_capture_evidence_path = $null
    promotion_eligible = $false
    webview2_offline_installer_contract_verified = $true
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
    app_gui_subsystem_verified = $false
    onboarding_auto_open_observed = $false
    onboarding_dom_evidence = $null
    webview_debug_port = $null
    review_test_game_observed = $false
    review_test_game_stopped = $null
    review_test_game_evidence = $null
    runtime_child_observed = $false
    broker_child_observed = $false
    runtime_supervision_observed = $false
    broker_supervision_observed = $false
    no_orphans_after_parent_termination = $null
    review_webview_processes_observed = 0
    no_review_webview_orphans = $null
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
    reinstall_cycle_verified = $false
    review_namespace_removed = $false
    review_local_namespace_removed = $false
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
    $script:machineResultWritten = $true
    [ordered]@{
        schema_version = 1
        status = 'preflight_passed'
        result_path = $resultFullPath
        installer_path = $installerPath
        review_app_path = Join-Path $installRoot $mainExecutableName
        review_identifier = $reviewIdentifier
        test_game_path = [string]$reviewGame.executable_path
    } | ConvertTo-Json -Compress | Write-Output
    exit 0
}

$mainProcess = $null
$testGameProcess = $null
$failure = $null
New-Item -ItemType Directory -Path $logRoot -Force | Out-Null

try {
    if (-not $PSCmdlet.ShouldProcess($installRoot, "silently install, smoke-test, and uninstall $productName")) {
        throw 'Installer smoke test was declined.'
    }

    $installProcess = Start-Process -FilePath $installerPath -ArgumentList @('/S', "/D=$installRoot") -WindowStyle Hidden -PassThru
    Wait-BoundedProcess -Process $installProcess -TimeoutSeconds $ProcessTimeoutSeconds -Label 'NSIS installer'

    foreach ($relativePath in $requiredInstalledFiles) {
        $requiredPath = Join-Path $installRoot $relativePath
        if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) { throw "Installed payload is missing: $relativePath" }
    }
    $result.installed_files_verified = $true
    $result.installed_control_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot $mainExecutableName)
    $result.control_hash_verified = $manifest.control_executable.pe_subsystem -eq 'windows_gui' -and
        $result.installed_control_sha256 -eq [string]$manifest.control_executable.sha256
    $result.installed_runtime_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-runtime.exe')
    $result.installed_broker_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-media-broker.exe')
    $result.installed_mouth_worker_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-mouth-worker.exe')
    $result.installed_subtitle_presenter_sha256 = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-subtitle-presenter.exe')
    $sidecarHashes = @{}
    foreach ($sidecar in @($manifest.sidecars.binaries)) {
        $installedName = ([string]$sidecar.file_name) -replace '-x86_64-pc-windows-msvc(?=\.exe$)', ''
        $sidecarHashes[$installedName] = [string]$sidecar.sha256
    }
    $result.runtime_hash_verified = -not [string]::IsNullOrWhiteSpace($sidecarHashes['npc-runtime.exe']) -and
        $result.installed_runtime_sha256 -eq $sidecarHashes['npc-runtime.exe']
    $result.broker_hash_verified = -not [string]::IsNullOrWhiteSpace($sidecarHashes['npc-media-broker.exe']) -and
        $result.installed_broker_sha256 -eq $sidecarHashes['npc-media-broker.exe']
    $result.mouth_worker_hash_verified = -not [string]::IsNullOrWhiteSpace($result.installed_mouth_worker_sha256) -and
        $result.installed_mouth_worker_sha256 -eq $sidecarHashes['npc-mouth-worker.exe']
    $result.subtitle_presenter_hash_verified = -not [string]::IsNullOrWhiteSpace($result.installed_subtitle_presenter_sha256) -and
        $result.installed_subtitle_presenter_sha256 -eq $sidecarHashes['npc-subtitle-presenter.exe']
    if (-not $result.control_hash_verified -or -not $result.runtime_hash_verified -or
        -not $result.broker_hash_verified -or -not $result.mouth_worker_hash_verified -or
        -not $result.subtitle_presenter_hash_verified) {
        throw 'Installed control/sidecar hash differs from the prepared package binary.'
    }

    $reconciliation = (& (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot `
            -PackageManifestPath $manifestFullPath `
            -OutputManifestPath $installedDistributionManifestEvidencePath `
            -RepositoryRoot $repoRoot) | ConvertFrom-Json
    if ($reconciliation.status -ne 'passed' -or
        $reconciliation.exact_four_sidecars_reconciled -ne $true -or
        $reconciliation.installed_legal_resources_reconciled -ne $true -or
        $reconciliation.unclassified_installed_files_rejected -ne $true) {
        throw 'Installed distribution reconciliation did not prove the exact package allowlist.'
    }
    $result.installed_distribution_reconciled = $true
    $result.installed_legal_resources_reconciled = $true
    $result.unclassified_installed_files_rejected = $true
    $result.installed_distribution_manifest_path = [string]$reconciliation.evidence_manifest_path
    $result.installed_distribution_manifest_sha256 = [string]$reconciliation.evidence_manifest_sha256
    $result.installed_model_catalog_promotion_policy = Test-NpcModelCatalogPromotionPolicy `
        -RootPath (Join-Path $installRoot 'packaging/model-packs/model-catalog-root-v1.json') `
        -CatalogPath (Join-Path $installRoot 'packaging/model-packs/model-catalog-v1.json')
    $result.installed_model_catalog_promotion_policy_verified =
        [bool]$result.installed_model_catalog_promotion_policy.eligible

    $profiles = @(Get-ChildItem -LiteralPath (Join-Path $installRoot 'profiles/games') -Filter 'profile.json' -File -Recurse)
    if ($profiles.Count -ne 21) { throw "Expected 21 installed game profiles; found $($profiles.Count)." }
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
    if ($modelManifest.schema -ne 'npc.model-pack/v2' -or [string]::IsNullOrWhiteSpace($modelManifest.pack_id)) {
        throw 'Installed model-pack manifest example is incompatible.'
    }
    $result.model_manifest_verified = $true

    $doctor = Invoke-CapturedProcess -FilePath (Join-Path $installRoot 'npc-runtime.exe') -Arguments @('--repo-root', $installRoot, '--app-data', $appDataRoot, 'doctor') -WorkingDirectory $installRoot -LogDirectory $logRoot -Label 'runtime-doctor' -TimeoutSeconds $ProcessTimeoutSeconds
    $doctorReport = $doctor.stdout | ConvertFrom-Json
    if ($doctorReport.profileCount -ne 21 -or $doctorReport.providerCount -le 0 -or $doctorReport.modelCount -le 0 -or $doctorReport.modelManifestExample -ne 'valid' -or $doctorReport.status -eq 'failed') {
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

    & (Join-Path $PSScriptRoot 'assert-pe-subsystem.ps1') -Path (Join-Path $installRoot $mainExecutableName) -Expected Gui | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Installed control executable is not a Windows GUI-subsystem binary.' }
    $result.app_gui_subsystem_verified = $true

    $placementHelper = Join-Path $env:USERPROFILE '.codex\skills\prefer-second-monitor\scripts\place_process_windows.ps1'
    $testGameMetadataPath = Join-Path $operationRoot 'test-game-capture.json'
    $testGameArguments = @(
        '--metadata', $testGameMetadataPath,
        '--title', 'Interactive NPCs Installer Smoke - Synthetic Game',
        '--width', '960', '--height', '600', '--fps', '15',
        '--exit-after-seconds', [string]([Math]::Min($ProcessTimeoutSeconds, 60))
    )
    if (Test-Path -LiteralPath $placementHelper -PathType Leaf) {
        $testGameArguments += @('--place-on-second-monitor', '--placement-helper', $placementHelper)
    }
    $testGameProcess = Start-Process `
        -FilePath ([string]$reviewGame.executable_path) `
        -ArgumentList (ConvertTo-NativeCommandLine -Arguments $testGameArguments) `
        -WorkingDirectory ([string]$reviewGame.directory) `
        -WindowStyle Hidden `
        -PassThru
    $gameDeadline = [DateTime]::UtcNow.AddSeconds(15)
    $gameMetadata = $null
    do {
        if ($testGameProcess.HasExited) {
            throw "Review test game exited before publishing evidence (code $($testGameProcess.ExitCode))."
        }
        if (Test-Path -LiteralPath $testGameMetadataPath -PathType Leaf) {
            try {
                $candidate = Get-Content -LiteralPath $testGameMetadataPath -Raw | ConvertFrom-Json
                if ([int]$candidate.process_id -eq $testGameProcess.Id -and
                    [int64]$candidate.window_handle -ne 0 -and [int]$candidate.generated_frames -ge 1 -and
                    [string]$candidate.fixture_source -eq 'project-source-generated-native-v1' -and
                    [string]$candidate.renderer -eq 'project-owned-gdi-generated-v1' -and
                    [string]$candidate.audio_source -eq 'project-owned-generated-pcm-v1' -and
                    [bool]$candidate.audio_output_started -eq $true -and
                    [bool]$candidate.third_party_binaries_loaded -eq $false) {
                    $gameMetadata = $candidate
                }
            }
            catch { }
        }
        if ($null -eq $gameMetadata) { Start-Sleep -Milliseconds 100 }
    } while ($null -eq $gameMetadata -and [DateTime]::UtcNow -lt $gameDeadline)
    if ($null -eq $gameMetadata) { throw 'Review test game did not generate a frame/audio stream and publish PID/HWND evidence.' }
    $result.review_test_game_observed = $true
    $result.review_test_game_evidence = [ordered]@{
        process_id = $testGameProcess.Id
        executable_path = [string]$reviewGame.executable_path
        executable_sha256 = [string]$reviewGame.executable_sha256
        window_handle = [int64]$gameMetadata.window_handle
        generated_frames = [int]$gameMetadata.generated_frames
        fixture_source = [string]$gameMetadata.fixture_source
        renderer = [string]$gameMetadata.renderer
        audio_source = [string]$gameMetadata.audio_source
        audio_output_started = [bool]$gameMetadata.audio_output_started
        distribution_manifest_sha256 = [string]$reviewGame.distribution_manifest_sha256
        sbom_sha256 = [string]$reviewGame.sbom_sha256
        notices_sha256 = [string]$reviewGame.notices_sha256
        media_files_bundled = 0
        third_party_binary_count = 0
        hardware_acceleration = $false
        nvidia_compute_requested = $false
    }

    $webviewDebugPort = Get-FreeLoopbackPort
    $result.webview_debug_port = $webviewDebugPort
    $previousSmokeTrigger = $env:NPC2_INSTALLER_SMOKE
    $previousWebViewArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
    try {
        $env:NPC2_INSTALLER_SMOKE = '1'
        $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$webviewDebugPort"
        $mainProcess = Start-Process -FilePath (Join-Path $installRoot $mainExecutableName) -WorkingDirectory $installRoot -WindowStyle Hidden -PassThru
    }
    finally {
        if ($null -eq $previousSmokeTrigger) { Remove-Item Env:\NPC2_INSTALLER_SMOKE -ErrorAction SilentlyContinue }
        else { $env:NPC2_INSTALLER_SMOKE = $previousSmokeTrigger }
        if ($null -eq $previousWebViewArguments) { Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
        else { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousWebViewArguments }
    }

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

    $onboardingDeadline = [DateTime]::UtcNow.AddSeconds([Math]::Min($ProcessTimeoutSeconds, 15))
    $onboardingEvidence = $null
    $onboardingExpression = @'
JSON.stringify({
  readyState: document.readyState,
  onboardingVisible: Boolean(document.querySelector('.setup-dialog[role="dialog"][aria-modal="true"]')),
  heading: document.querySelector('.setup-dialog #setup-title')?.textContent?.trim() ?? null,
  closeButtonPresent: Boolean(document.querySelector('.setup-dialog [aria-label^="Close"], .setup-dialog button[title^="Close"]'))
})
'@
    do {
        try {
            $evaluated = Invoke-CdpExpression -Port $webviewDebugPort -Expression $onboardingExpression -TimeoutSeconds 2
            $candidate = $evaluated | ConvertFrom-Json
            if ($candidate.readyState -eq 'complete' -and $candidate.onboardingVisible -eq $true) {
                $onboardingEvidence = $candidate
            }
        }
        catch { }
        if ($null -eq $onboardingEvidence) { Start-Sleep -Milliseconds 100 }
    } while ($null -eq $onboardingEvidence -and [DateTime]::UtcNow -lt $onboardingDeadline)
    if ($null -eq $onboardingEvidence) {
        throw 'A clean review namespace did not render the mandatory first-run onboarding overlay.'
    }
    if ([string]::IsNullOrWhiteSpace([string]$onboardingEvidence.heading)) {
        throw 'First-run onboarding rendered without its primary heading.'
    }
    if ($onboardingEvidence.closeButtonPresent -eq $true) {
        throw 'Mandatory first-run onboarding unexpectedly exposed a close control.'
    }
    $result.onboarding_auto_open_observed = $true
    $result.onboarding_dom_evidence = $onboardingEvidence
    $result.review_webview_processes_observed = @(Get-ReviewWebViewProcesses -ReviewLocalAppDataRoot $reviewLocalAppDataRoot).Count
    if ($result.review_webview_processes_observed -lt 1) {
        throw 'The installed app rendered onboarding without an attributable isolated WebView process.'
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
    $webviewDeathDeadline = [DateTime]::UtcNow.AddSeconds(10)
    $reviewWebViews = @(Get-ReviewWebViewProcesses -ReviewLocalAppDataRoot $reviewLocalAppDataRoot)
    while ($reviewWebViews.Count -gt 0 -and [DateTime]::UtcNow -lt $webviewDeathDeadline) {
        Start-Sleep -Milliseconds 100
        $reviewWebViews = @(Get-ReviewWebViewProcesses -ReviewLocalAppDataRoot $reviewLocalAppDataRoot)
    }
    $result.no_review_webview_orphans = $reviewWebViews.Count -eq 0
    if (-not $result.no_review_webview_orphans) {
        throw "Parent termination left $($reviewWebViews.Count) isolated WebView process(es) running."
    }

    # Exercise a complete uninstall/reinstall before the final evidence cycle.
    # This catches stale NSIS registry state and confirms that the exact same
    # current-user target can be restored without deleting app-owned data.
    $firstUninstaller = Join-Path $installRoot 'uninstall.exe'
    $firstUninstall = Start-Process -FilePath $firstUninstaller -ArgumentList @('/S') -WindowStyle Hidden -PassThru
    Wait-BoundedProcess -Process $firstUninstall -TimeoutSeconds $ProcessTimeoutSeconds -Label 'first-cycle NSIS uninstaller'
    $firstCycleSnapshot = Wait-InstallTargetsClean -InstallRoot $installRoot -ShortcutPaths $shortcutPaths
    if ($firstCycleSnapshot.install_root_exists -or @($firstCycleSnapshot.processes).Count -gt 0 -or
        @($firstCycleSnapshot.shortcuts | Where-Object { $_.exists }).Count -gt 0 -or
        $firstCycleSnapshot.registry.uninstall_key_exists -or
        $firstCycleSnapshot.registry.manufacturer_key_exists -or
        $firstCycleSnapshot.registry.run_value_exists) {
        throw 'First uninstall cycle left residue and is not reinstall-safe.'
    }

    $reinstall = Start-Process -FilePath $installerPath -ArgumentList @('/S', "/D=$installRoot") -WindowStyle Hidden -PassThru
    Wait-BoundedProcess -Process $reinstall -TimeoutSeconds $ProcessTimeoutSeconds -Label 'second-cycle NSIS installer'
    foreach ($relativePath in $requiredInstalledFiles) {
        if (-not (Test-Path -LiteralPath (Join-Path $installRoot $relativePath) -PathType Leaf)) {
            throw "Reinstalled payload is missing: $relativePath"
        }
    }
    $reinstalledControlHash = Get-OptionalFileSha256 -Path (Join-Path $installRoot $mainExecutableName)
    $reinstalledRuntimeHash = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-runtime.exe')
    $reinstalledBrokerHash = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-media-broker.exe')
    $reinstalledMouthHash = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-mouth-worker.exe')
    $reinstalledSubtitleHash = Get-OptionalFileSha256 -Path (Join-Path $installRoot 'npc-subtitle-presenter.exe')
    if ($reinstalledControlHash -ne $result.installed_control_sha256 -or
        $reinstalledRuntimeHash -ne $result.installed_runtime_sha256 -or
        $reinstalledBrokerHash -ne $result.installed_broker_sha256 -or
        $reinstalledMouthHash -ne $result.installed_mouth_worker_sha256 -or
        $reinstalledSubtitleHash -ne $result.installed_subtitle_presenter_sha256) {
        throw 'Reinstall changed one or more project-sidecar hashes.'
    }
    $reinstalledReconciliation = (& (Join-Path $PSScriptRoot 'windows/reconcile-installed-product.ps1') `
            -InstallRoot $installRoot `
            -PackageManifestPath $manifestFullPath `
            -OutputManifestPath $reinstalledDistributionManifestEvidencePath `
            -RepositoryRoot $repoRoot) | ConvertFrom-Json
    if ($reinstalledReconciliation.status -ne 'passed' -or
        [string]$reinstalledReconciliation.evidence_manifest_sha256 -ne
            [string]$result.installed_distribution_manifest_sha256) {
        throw 'Reinstalled payload differs from the first closed-world reconciliation.'
    }
    $result.reinstalled_distribution_reconciled = $true
    $result.reinstalled_model_catalog_promotion_policy = Test-NpcModelCatalogPromotionPolicy `
        -RootPath (Join-Path $installRoot 'packaging/model-packs/model-catalog-root-v1.json') `
        -CatalogPath (Join-Path $installRoot 'packaging/model-packs/model-catalog-v1.json')
    $result.reinstalled_model_catalog_promotion_policy_verified =
        [bool]$result.reinstalled_model_catalog_promotion_policy.eligible -and
        [string]$result.reinstalled_model_catalog_promotion_policy.root_sha256 -eq
            [string]$result.installed_model_catalog_promotion_policy.root_sha256 -and
        [string]$result.reinstalled_model_catalog_promotion_policy.catalog_sha256 -eq
            [string]$result.installed_model_catalog_promotion_policy.catalog_sha256
    $result.reinstall_cycle_verified = $true

    # The privacy collector must observe the exact second-cycle installed
    # candidate while it still exists. It applies temporary OS firewall rules,
    # validates fresh packaged-executable scenario receipts, writes retained
    # evidence outside the install root, and returns before normal cleanup.
    if ($CaptureInstalledPrivacyProof) {
        & (Join-Path $PSScriptRoot 'capture-installed-privacy-proof.ps1') `
            -PackageManifestPath $manifestFullPath `
            -PackagePath $installerPath `
            -InstalledManifestPath $reinstalledDistributionManifestEvidencePath `
            -InstalledExecutablePath (Join-Path $installRoot $mainExecutableName) `
            -TelemetryInventoryPath $PrivacyTelemetryInventoryPath `
            -ReleaseCandidateId $PrivacyReleaseCandidateId `
            -ApplicationVersion $PrivacyApplicationVersion `
            -ProofPath $PrivacyProofPath `
            -CaptureEvidencePath $PrivacyCaptureEvidencePath `
            -AcknowledgeElevatedOsDenyAll
        if ($LASTEXITCODE -ne 0) { throw 'Installed privacy proof capture failed.' }
        $result.installed_privacy_capture_completed = $true
        $result.installed_privacy_proof_path = [System.IO.Path]::GetFullPath($PrivacyProofPath)
        $result.installed_privacy_capture_evidence_path = [System.IO.Path]::GetFullPath($PrivacyCaptureEvidencePath)
    }
}
catch {
    $failure = $_.Exception.Message
}
finally {
    Stop-OwnedProcess -Process $testGameProcess
    if ($null -ne $testGameProcess) {
        $testGameProcess.Refresh()
        $result.review_test_game_stopped = $testGameProcess.HasExited
        if (-not $result.review_test_game_stopped -and $null -eq $failure) {
            $failure = 'The task-owned synthetic review game did not stop after bounded observation.'
        }
    } else {
        $result.review_test_game_stopped = $true
    }
    Stop-OwnedProcess -Process $mainProcess
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    $processesBeforeUninstall = @(Get-TestInstallProcesses -InstallRoot $installRoot)
    $mainStillRunning = @($processesBeforeUninstall | Where-Object { $_.Name -eq $mainExecutableName }).Count -gt 0
    if (-not $mainStillRunning -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
        $result.emergency_cleanup.normal_uninstall_attempted = $true
        try {
            $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList @('/S') -WindowStyle Hidden -PassThru
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

    # This snapshot is the acceptance evidence. Nothing below this point may
    # change these booleans from failure to success.
    $postUninstallSnapshot = Wait-InstallTargetsClean -InstallRoot $installRoot -ShortcutPaths $shortcutPaths
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
    $lingeringReviewWebViews = @(Get-ReviewWebViewProcesses -ReviewLocalAppDataRoot $reviewLocalAppDataRoot)
    foreach ($webview in $lingeringReviewWebViews) {
        $result.emergency_cleanup.used = $true
        try {
            Stop-Process -Id $webview.ProcessId -Force -ErrorAction Stop
            $result.emergency_cleanup.processes_terminated = @(
                $result.emergency_cleanup.processes_terminated + [int]$webview.ProcessId
            )
        }
        catch {
            $result.emergency_cleanup.errors = @(
                $result.emergency_cleanup.errors + "Could not terminate review WebView process $($webview.ProcessId)."
            )
        }
    }
    if ($lingeringReviewWebViews.Count -gt 0) { Start-Sleep -Milliseconds 500 }
    if (Test-Path -LiteralPath $appConfigRoot) {
        # The .review namespace was proven absent before the cycle, so the
        # harness owns every path beneath it and can remove it without
        # touching the production identifier or normal user state.
        $reviewCleanupDeadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            try { Remove-Item -LiteralPath $appConfigRoot -Recurse -Force -ErrorAction Stop }
            catch { Start-Sleep -Milliseconds 250 }
        } while ((Test-Path -LiteralPath $appConfigRoot) -and [DateTime]::UtcNow -lt $reviewCleanupDeadline)
        $result.review_namespace_removed = -not (Test-Path -LiteralPath $appConfigRoot)
        if (-not $result.review_namespace_removed -and $null -eq $failure) {
            $failure = 'The isolated review app-data namespace could not be removed.'
        }
    } else {
        $result.review_namespace_removed = $true
    }
    if (Test-Path -LiteralPath $reviewLocalAppDataRoot) {
        $reviewLocalCleanupDeadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
            try { Remove-Item -LiteralPath $reviewLocalAppDataRoot -Recurse -Force -ErrorAction Stop }
            catch { Start-Sleep -Milliseconds 250 }
        } while ((Test-Path -LiteralPath $reviewLocalAppDataRoot) -and [DateTime]::UtcNow -lt $reviewLocalCleanupDeadline)
        $result.review_local_namespace_removed = -not (Test-Path -LiteralPath $reviewLocalAppDataRoot)
        if (-not $result.review_local_namespace_removed -and $null -eq $failure) {
            $failure = 'The isolated review WebView/app-local namespace could not be removed.'
        }
    } else {
        $result.review_local_namespace_removed = $true
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
            $retryUninstaller = Start-Process -FilePath $uninstaller -ArgumentList @('/S') -WindowStyle Hidden -PassThru
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
    $productionAppConfigAfter = Get-DirectoryIdentity -Path $productionAppConfigRoot
    $productionLocalAppDataAfter = Get-DirectoryMetadataIdentity -Path $productionLocalAppDataRoot
    $result.production_app_config_after = $productionAppConfigAfter
    $result.production_local_app_data_after = $productionLocalAppDataAfter
    $result.production_namespace_unchanged =
        $productionAppConfigAfter.exists -eq $productionAppConfigBefore.exists -and
        $productionAppConfigAfter.file_count -eq $productionAppConfigBefore.file_count -and
        $productionAppConfigAfter.composite_sha256 -eq $productionAppConfigBefore.composite_sha256 -and
        $productionLocalAppDataAfter.exists -eq $productionLocalAppDataBefore.exists -and
        $productionLocalAppDataAfter.entry_count -eq $productionLocalAppDataBefore.entry_count -and
        $productionLocalAppDataAfter.composite_sha256 -eq $productionLocalAppDataBefore.composite_sha256
    if (-not $result.production_namespace_unchanged -and $null -eq $failure) {
        $failure = 'The production app-data namespace changed during isolated review smoke testing.'
    }
    if (-not $result.review_namespace_removed -and $null -eq $failure) {
        $failure = 'The isolated review app-data namespace remained after smoke testing.'
    }
    if (-not $result.review_local_namespace_removed -and $null -eq $failure) {
        $failure = 'The isolated review local-app-data namespace remained after smoke testing.'
    }
    if (@($result.emergency_cleanup.errors).Count -gt 0 -and $null -eq $failure) { $failure = 'Emergency cleanup reported one or more errors.' }

    $result.promotion_eligible = $null -eq $failure -and
        $manifest.evidence_class -eq 'pre-reconciliation-local-review' -and
        $manifest.source.dirty -eq $false -and
        $manifest.security_gates.skipped -eq $false -and
        $manifest.security_gates.tests -eq $true -and
        $manifest.security_gates.include_untracked_secret_scan -eq $true -and
        $manifest.security_gates.strict_license_provenance -eq $true -and
        $manifest.security_gates.deterministic_complete_sbom -eq $true -and
        $result.webview2_offline_installer_contract_verified -and
        $result.installed_distribution_reconciled -and
        $result.reinstalled_distribution_reconciled -and
        $result.installed_model_catalog_promotion_policy_verified -and
        $result.reinstalled_model_catalog_promotion_policy_verified -and
        $result.installed_legal_resources_reconciled -and
        $result.unclassified_installed_files_rejected -and
        $result.installed_privacy_capture_completed -and
        $result.runtime_supervision_observed -and $result.broker_supervision_observed -and
        $result.no_remaining_processes -and $result.no_install_files -and
        $result.no_shortcuts -and $result.no_registry_entries -and
        $result.production_namespace_unchanged
    $result.status = $(if ($null -eq $failure) { 'passed' } else { 'failed' })
    $result.error = $failure
    $result.completed_at_utc = [DateTime]::UtcNow.ToString('o')
    Write-SmokeResult -Result $result -Path $resultFullPath
}

if ($null -ne $failure) {
    $script:machineResultWritten = $true
    [ordered]@{
        schema_version = 1
        status = 'failed'
        result_path = $resultFullPath
        installer_path = $installerPath
        review_app_path = Join-Path $installRoot $mainExecutableName
        review_identifier = $reviewIdentifier
        test_game_path = [string]$reviewGame.executable_path
        error = $failure
    } | ConvertTo-Json -Compress | Write-Output
    Write-Error "Installer smoke test failed: $failure. Result: $resultFullPath"
    exit 1
}
Write-Host "Installer smoke test passed: $resultFullPath" -ForegroundColor Green
$script:machineResultWritten = $true
[ordered]@{
    schema_version = 1
    status = 'passed'
    result_path = $resultFullPath
    installer_path = $installerPath
    review_app_path = Join-Path $installRoot $mainExecutableName
    tested_install_path = Join-Path $installRoot $mainExecutableName
    review_identifier = $reviewIdentifier
    test_game_path = [string]$reviewGame.executable_path
    onboarding_auto_open_observed = [bool]$result.onboarding_auto_open_observed
    reinstall_cycle_verified = [bool]$result.reinstall_cycle_verified
    production_namespace_unchanged = [bool]$result.production_namespace_unchanged
    installed_distribution_reconciled = [bool]$result.installed_distribution_reconciled
    installed_legal_resources_reconciled = [bool]$result.installed_legal_resources_reconciled
    unclassified_installed_files_rejected = [bool]$result.unclassified_installed_files_rejected
    installed_model_catalog_promotion_policy_verified = [bool]$result.installed_model_catalog_promotion_policy_verified
    reinstalled_model_catalog_promotion_policy_verified = [bool]$result.reinstalled_model_catalog_promotion_policy_verified
    installed_privacy_capture_completed = [bool]$result.installed_privacy_capture_completed
    installed_privacy_proof_path = $result.installed_privacy_proof_path
    installed_privacy_capture_evidence_path = $result.installed_privacy_capture_evidence_path
    model_catalog_promotion_blockers = @($result.installed_model_catalog_promotion_policy.blockers)
    promotion_eligible = [bool]$result.promotion_eligible
} | ConvertTo-Json -Compress | Write-Output
exit 0
