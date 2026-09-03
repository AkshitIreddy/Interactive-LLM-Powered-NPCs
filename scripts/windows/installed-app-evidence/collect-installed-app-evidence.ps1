[CmdletBinding()]
param(
    [ValidateSet('ValidatePlan', 'Preflight', 'Capture')]
    [string]$Mode = 'ValidatePlan',
    [string]$PlanPath,
    [string]$PackageManifestPath,
    [string]$InstallerSmokeReceiptPath,
    [string]$InstalledDistributionManifestPath,
    [string]$InstalledRoot,
    [string]$SourceEvidencePath,
    [string]$TestGameRoot,
    [string]$FrozenSourceReceiptPath,
    [string]$OutputRoot,
    [string]$NodePath,
    [string]$PlaywrightModulePath,
    [string]$FfmpegPath,
    [ValidateRange(10, 600)][int]$VideoSeconds = 180,
    [string]$GuiAuthorizationToken
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
if ([string]::IsNullOrWhiteSpace($PlanPath)) { $PlanPath = Join-Path $PSScriptRoot 'evidence-plan.v1.json' }

function Get-LowerSha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-File {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Label)
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { throw "$Label must not be a reparse point: $Path" }
    return $item.FullName
}

function Assert-Directory {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Label)
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Label is missing: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { throw "$Label must not be a reparse point: $Path" }
    return $item.FullName.TrimEnd('\', '/')
}

function ConvertTo-SafeRelativePath {
    param([Parameter(Mandatory)][string]$Value)
    $normalized = $Value.Replace('\', '/')
    if ([string]::IsNullOrWhiteSpace($normalized) -or [System.IO.Path]::IsPathRooted($normalized) -or
        @($normalized.Split('/') | Where-Object { $_ -in @('', '.', '..') }).Count -gt 0) {
        throw "Unsafe evidence-relative path: $Value"
    }
    return $normalized
}

function ConvertTo-NativeCommandLine {
    param([Parameter(Mandatory)][string[]]$Arguments)
    return (($Arguments | ForEach-Object {
        $argument = [string]$_
        if ($argument.Length -eq 0) { return '""' }
        if ($argument -notmatch '[\s"]') { return $argument }
        $escaped = [System.Text.RegularExpressions.Regex]::Replace($argument, '(\\*)"', '$1$1\"')
        $escaped = [System.Text.RegularExpressions.Regex]::Replace($escaped, '(\\+)$', '$1$1')
        return '"' + $escaped + '"'
    }) -join ' ')
}

function Start-HiddenProcess {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [string]$WorkingDirectory,
        [hashtable]$Environment = @{},
        [switch]$Redirect
    )
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $FilePath
    $info.Arguments = ConvertTo-NativeCommandLine -Arguments $ArgumentList
    if (-not [string]::IsNullOrWhiteSpace($WorkingDirectory)) { $info.WorkingDirectory = $WorkingDirectory }
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    foreach ($entry in $Environment.GetEnumerator()) { $info.EnvironmentVariables[[string]$entry.Key] = [string]$entry.Value }
    if ($Redirect) {
        $info.RedirectStandardOutput = $true
        $info.RedirectStandardError = $true
        $info.RedirectStandardInput = $true
    }
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    if (-not $process.Start()) { throw "Could not start hidden task process: $FilePath" }
    return $process
}

function Stop-OwnedProcess {
    param([System.Diagnostics.Process]$Process)
    if ($null -eq $Process) { return }
    try { if ($Process.HasExited) { return } } catch { return }
    try { [void]$Process.CloseMainWindow() } catch { }
    try { if ($Process.WaitForExit(5000)) { return } } catch { }
    try { $Process.Kill(); [void]$Process.WaitForExit(5000) } catch { }
}

function Get-FreeTcpPort {
    $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try { return ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port }
    finally { $listener.Stop() }
}

function Test-Plan {
    param([Parameter(Mandatory)][string]$ResolvedPlanPath)
    $plan = Get-Content -LiteralPath $ResolvedPlanPath -Raw | ConvertFrom-Json
    if ($plan.schema -ne 'interactive-npcs-installed-evidence-plan/v1') { throw 'Evidence plan schema is invalid.' }
    $pages = @($plan.retained_pages)
    $profiles = @($plan.view_profiles)
    if ($pages.Count -ne 5 -or (@($pages.id | Sort-Object -Unique)).Count -ne 5) { throw 'Evidence plan must retain exactly five unique product pages.' }
    if ($profiles.Count -lt 6 -or (@($profiles.id | Sort-Object -Unique)).Count -ne $profiles.Count) { throw 'Evidence plan must include unique normal/narrow 100/150/200 profiles.' }
    foreach ($required in @('normal-100', 'normal-150', 'normal-200', 'narrow-100', 'narrow-150', 'narrow-200')) {
        if (@($profiles.id) -notcontains $required) { throw "Evidence plan omits view profile: $required" }
    }
    if ([int]$plan.capture.closeups_min -lt 4 -or [int]$plan.capture.closeups_max -gt 6 -or
        [int]$plan.capture.closeups_min -gt [int]$plan.capture.closeups_max) {
        throw 'Evidence plan must capture four to six closeups per retained screen.'
    }
    if (@($plan.required_native_claims).Count -lt 9) { throw 'Evidence plan omits required installed-native claims.' }
    if (@($plan.control_rules).Count -lt 6) { throw 'Evidence plan does not define exhaustive control handling.' }
    return $plan
}

function Get-BoundInput {
    param([Parameter(Mandatory)][string]$Kind, [Parameter(Mandatory)][string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    return [ordered]@{ kind = $Kind; path = $item.FullName; size_bytes = [int64]$item.Length; sha256 = Get-LowerSha256 -Path $item.FullName }
}

function Test-HashAndSize {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Entry, [Parameter(Mandatory)][string]$Label)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Label is missing: $Path" }
    if ([string]$Entry.sha256 -cnotmatch '^[0-9a-f]{64}$' -or (Get-LowerSha256 -Path $Path) -cne [string]$Entry.sha256) {
        throw "$Label SHA-256 differs from its manifest: $Path"
    }
    if ($null -ne $Entry.size_bytes -and [int64]$Entry.size_bytes -ne (Get-Item -LiteralPath $Path).Length) {
        throw "$Label size differs from its manifest: $Path"
    }
}

function Resolve-CollectorBindings {
    param([switch]$RequireFrozenReceipt)
    $packagePath = Assert-File -Path $PackageManifestPath -Label 'Package manifest'
    $smokePath = Assert-File -Path $InstallerSmokeReceiptPath -Label 'Installer smoke receipt'
    $installedManifestPath = Assert-File -Path $InstalledDistributionManifestPath -Label 'Installed-distribution manifest'
    $sourcePath = Assert-File -Path $SourceEvidencePath -Label 'Source evidence'
    $install = Assert-Directory -Path $InstalledRoot -Label 'Installed product root'
    $fixtureRoot = Assert-Directory -Path $TestGameRoot -Label 'Synthetic review game root'
    $package = Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json
    if ($package.schema_version -ne 1 -or $package.distribution -ne 'local-review-only' -or
        $package.immutable_release_candidate -ne $true -or $package.source.dirty -ne $false -or
        $package.source.source_tree_clean -ne $true) {
        throw 'Package must be an immutable, clean, local-review release candidate.'
    }
    $packageDirectory = Split-Path -Parent $packagePath
    $installerEntries = @($package.files | Where-Object { [string]$_.file -like '*-setup.exe' })
    if ($installerEntries.Count -ne 1) { throw 'Package manifest must bind exactly one installer.' }
    $installerPath = Join-Path $packageDirectory ([string]$installerEntries[0].file)
    Test-HashAndSize -Path $installerPath -Entry $installerEntries[0] -Label 'Installer'
    $sourceEntries = @($package.files | Where-Object { [string]$_.file -eq (Split-Path -Leaf $sourcePath) })
    if ($sourceEntries.Count -ne 1) { throw 'Source evidence must be a package-manifest input.' }
    Test-HashAndSize -Path $sourcePath -Entry $sourceEntries[0] -Label 'Source evidence'
    $source = Get-Content -LiteralPath $sourcePath -Raw | ConvertFrom-Json
    if ($source.schema_version -ne 1 -or $source.dirty -ne $false -or $source.source_tree_clean -ne $true -or
        [string]$source.source_candidate_digest.sha256 -cnotmatch '^[0-9a-f]{64}$') {
        throw 'Source evidence is not a clean schema-v1 source candidate.'
    }
    if ([string]$source.source_candidate_digest.sha256 -cne [string]$package.source.source_candidate_digest.sha256) {
        throw 'Package and source evidence bind different source-candidate digests.'
    }
    $smoke = Get-Content -LiteralPath $smokePath -Raw | ConvertFrom-Json
    $packageHash = Get-LowerSha256 -Path $packagePath
    $installedManifestHash = Get-LowerSha256 -Path $installedManifestPath
    if ([string]$smoke.source_identity.package_manifest_sha256 -cne $packageHash -or
        [string]$smoke.installed_distribution_manifest_sha256 -cne $installedManifestHash -or
        $smoke.installed_distribution_reconciled -ne $true) {
        throw 'Installer smoke receipt does not bind this package and installed-distribution manifest.'
    }
    $installedManifest = Get-Content -LiteralPath $installedManifestPath -Raw | ConvertFrom-Json
    if ($installedManifest.schema_version -ne 1 -or @($installedManifest.files).Count -lt 6) { throw 'Installed-distribution manifest is invalid.' }
    $seenInstalled = @{}
    foreach ($entry in @($installedManifest.files)) {
        $relative = ConvertTo-SafeRelativePath -Value ([string]$entry.path)
        if ($seenInstalled.ContainsKey($relative)) { throw "Duplicate installed manifest path: $relative" }
        $seenInstalled[$relative] = $true
        Test-HashAndSize -Path (Join-Path $install $relative) -Entry $entry -Label "Installed file $relative"
    }
    $controlEntries = @($installedManifest.files | Where-Object { [string]$_.path -eq 'interactive-npcs-control.exe' })
    if ($controlEntries.Count -ne 1) { throw 'Installed distribution must bind exactly one control executable.' }
    $controlPath = Assert-File -Path (Join-Path $install 'interactive-npcs-control.exe') -Label 'Installed control executable'
    $fixtureManifestPath = Assert-File -Path (Join-Path $fixtureRoot 'REVIEW-FIXTURE-MANIFEST.json') -Label 'Synthetic fixture manifest'
    $fixtureManifest = Get-Content -LiteralPath $fixtureManifestPath -Raw | ConvertFrom-Json
    if ($fixtureManifest.schema_version -ne 1 -or $fixtureManifest.component_id -ne 'project:synthetic-review-target' -or
        $fixtureManifest.fixture_source -ne 'project-source-generated-native-v1' -or @($fixtureManifest.third_party_binaries).Count -ne 0) {
        throw 'Synthetic review game manifest violates the project-owned fixture contract.'
    }
    foreach ($entry in @($fixtureManifest.files)) {
        $relative = ConvertTo-SafeRelativePath -Value ([string]$entry.path)
        Test-HashAndSize -Path (Join-Path $fixtureRoot $relative) -Entry $entry -Label "Synthetic fixture file $relative"
    }
    $fixtureExe = Assert-File -Path (Join-Path $fixtureRoot 'interactive-npcs-synthetic-target.exe') -Label 'Synthetic review game executable'
    $bindings = @(
        Get-BoundInput -Kind 'package_manifest' -Path $packagePath
        Get-BoundInput -Kind 'installer' -Path $installerPath
        Get-BoundInput -Kind 'installer_smoke_receipt' -Path $smokePath
        Get-BoundInput -Kind 'installed_distribution_manifest' -Path $installedManifestPath
        Get-BoundInput -Kind 'installed_control_executable' -Path $controlPath
        Get-BoundInput -Kind 'source_evidence' -Path $sourcePath
        Get-BoundInput -Kind 'synthetic_fixture_manifest' -Path $fixtureManifestPath
        Get-BoundInput -Kind 'synthetic_game_executable' -Path $fixtureExe
        Get-BoundInput -Kind 'evidence_plan' -Path (Resolve-Path -LiteralPath $PlanPath).Path
    )
    if ($RequireFrozenReceipt) {
        $frozenPath = Assert-File -Path $FrozenSourceReceiptPath -Label 'Frozen source receipt'
        $frozen = Get-Content -LiteralPath $frozenPath -Raw | ConvertFrom-Json
        if ($frozen.schema -ne 'interactive-npcs-source-freeze/v1' -or $frozen.status -ne 'frozen' -or
            [string]$frozen.package_manifest_sha256 -cne $packageHash -or
            [string]$frozen.source_candidate_digest_sha256 -cne [string]$source.source_candidate_digest.sha256) {
            throw 'Frozen source receipt does not bind this exact package and source candidate.'
        }
        $bindings += Get-BoundInput -Kind 'frozen_source_receipt' -Path $frozenPath
    }
    return [pscustomobject][ordered]@{
        package = $package
        package_path = $packagePath
        installer_path = $installerPath
        installed_root = $install
        control_path = $controlPath
        fixture_root = $fixtureRoot
        fixture_executable = $fixtureExe
        inputs = $bindings
        source_candidate_digest_sha256 = [string]$source.source_candidate_digest.sha256
    }
}

function Get-VisibleWindowInventory {
    if (-not ('InstalledEvidence.NativeWindows' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
namespace InstalledEvidence {
  public sealed class WindowInfo { public long hwnd; public uint pid; public string title; public string class_name; }
  public static class NativeWindows {
    private delegate bool EnumProc(IntPtr hwnd, IntPtr state);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr state);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
    public static WindowInfo[] Visible() {
      var result = new List<WindowInfo>();
      EnumWindows((hwnd, _) => { if (!IsWindowVisible(hwnd)) return true; uint pid; GetWindowThreadProcessId(hwnd, out pid); var title=new StringBuilder(1024); var cls=new StringBuilder(256); GetWindowText(hwnd,title,title.Capacity); GetClassName(hwnd,cls,cls.Capacity); result.Add(new WindowInfo{hwnd=hwnd.ToInt64(),pid=pid,title=title.ToString(),class_name=cls.ToString()}); return true; }, IntPtr.Zero);
      return result.ToArray();
    }
  }
}
'@
    }
    return @([InstalledEvidence.NativeWindows]::Visible() | ForEach-Object {
        $process = Get-Process -Id ([int]$_.pid) -ErrorAction SilentlyContinue
        [pscustomobject][ordered]@{
            hwnd = [int64]$_.hwnd
            hwnd_hex = ('0x{0:X}' -f [int64]$_.hwnd)
            pid = [int]$_.pid
            process_name = if ($null -eq $process) { $null } else { $process.ProcessName }
            title = [string]$_.title
            class_name = [string]$_.class_name
            protected = [bool](($null -ne $process -and $process.ProcessName -match '(?i)^TransparencyApp$') -or [string]$_.title -match '(?i)Transparency App')
        }
    })
}

function Finalize-EvidenceDirectory {
    param([Parameter(Mandatory)][string]$RunRoot, [Parameter(Mandatory)][string]$RunId)
    $files = @(Get-ChildItem -LiteralPath $RunRoot -File -Recurse -Force | Where-Object { $_.Name -ne 'immutable-manifest.json' } | Sort-Object FullName)
    $entries = foreach ($file in $files) {
        [ordered]@{
            path = $file.FullName.Substring($RunRoot.Length + 1).Replace('\', '/')
            size_bytes = [int64]$file.Length
            sha256 = Get-LowerSha256 -Path $file.FullName
        }
    }
    $identityText = (@($entries | ForEach-Object { "$($_.path)`t$($_.size_bytes)`t$($_.sha256)" }) -join "`n")
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($identityText)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try { $rootHash = -join ($algorithm.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) } finally { $algorithm.Dispose() }
    $manifest = [ordered]@{
        schema = 'interactive-npcs-immutable-evidence-manifest/v1'
        run_id = $RunId
        finalized_at_utc = [DateTime]::UtcNow.ToString('o')
        hash_algorithm = 'SHA-256'
        file_count = $entries.Count
        evidence_root_sha256 = $rootHash
        manifest_self = [ordered]@{ path = 'immutable-manifest.json'; hash = 'excluded-to-avoid-circularity' }
        files = @($entries)
    }
    $manifestPath = Join-Path $RunRoot 'immutable-manifest.json'
    [System.IO.File]::WriteAllText($manifestPath, (($manifest | ConvertTo-Json -Depth 8) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
    Get-ChildItem -LiteralPath $RunRoot -File -Recurse -Force | ForEach-Object { $_.IsReadOnly = $true }
    return $manifest
}

$resolvedPlanPath = Assert-File -Path $PlanPath -Label 'Evidence plan'
$plan = Test-Plan -ResolvedPlanPath $resolvedPlanPath
if ($Mode -eq 'ValidatePlan') {
    [ordered]@{
        schema = 'interactive-npcs-installed-evidence-readiness/v1'
        status = 'passed'
        mode = 'validate-plan'
        gui_launched = $false
        plan_path = $resolvedPlanPath
        plan_sha256 = Get-LowerSha256 -Path $resolvedPlanPath
        retained_page_count = @($plan.retained_pages).Count
        view_profile_count = @($plan.view_profiles).Count
        retained_screen_count_before_special_states = @($plan.retained_pages).Count * @($plan.view_profiles).Count
        closeups_per_screen = "$($plan.capture.closeups_min)-$($plan.capture.closeups_max)"
        native_claim_count = @($plan.required_native_claims).Count
    } | ConvertTo-Json -Depth 5
    exit 0
}

$binding = Resolve-CollectorBindings -RequireFrozenReceipt:($Mode -eq 'Capture')
if ($Mode -eq 'Preflight') {
    [ordered]@{
        schema = 'interactive-npcs-installed-evidence-readiness/v1'
        status = 'passed'
        mode = 'preflight'
        gui_launched = $false
        source_candidate_digest_sha256 = $binding.source_candidate_digest_sha256
        inputs = $binding.inputs
    } | ConvertTo-Json -Depth 7
    exit 0
}

if ($GuiAuthorizationToken -cne 'ROOT_CONFIRMED_SOURCE_FROZEN') {
    throw 'Capture is disabled until GuiAuthorizationToken is exactly ROOT_CONFIRMED_SOURCE_FROZEN.'
}
if ([string]::IsNullOrWhiteSpace($OutputRoot)) { throw 'Capture requires an explicit OutputRoot.' }
$OutputRoot = [System.IO.Path]::GetFullPath($OutputRoot)
if (-not (Test-Path -LiteralPath $OutputRoot -PathType Container)) { [void](New-Item -ItemType Directory -Path $OutputRoot) }
$runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmss.fffffffZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 12)
$runRoot = Join-Path $OutputRoot $runId
if (Test-Path -LiteralPath $runRoot) { throw "Evidence run directory already exists: $runRoot" }
[void](New-Item -ItemType Directory -Path $runRoot)

$node = Assert-File -Path $NodePath -Label 'Windows Node.js executable'
$playwrightModule = Assert-Directory -Path $PlaywrightModulePath -Label 'Playwright module root'
$ffmpeg = Assert-File -Path $FfmpegPath -Label 'FFmpeg executable'
$driverPath = Assert-File -Path (Join-Path $PSScriptRoot 'drive-installed-app-evidence.cjs') -Label 'Installed evidence driver'
$placementHelper = Assert-File -Path (Join-Path $env:USERPROFILE '.codex\skills\prefer-second-monitor\scripts\place_process_windows.ps1') -Label 'Secondary-display placement helper'
$preWindows = Get-VisibleWindowInventory
$protectedBefore = @($preWindows | Where-Object { $_.protected })
[System.IO.File]::WriteAllText((Join-Path $runRoot 'windows-before.json'), (([ordered]@{ schema = 'interactive-npcs-visible-windows/v1'; captured_at_utc = [DateTime]::UtcNow.ToString('o'); windows = $preWindows } | ConvertTo-Json -Depth 6) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))

$taskNames = @('interactive-npcs-control', 'npc-runtime', 'npc-media-broker', 'npc-mouth-worker', 'npc-subtitle-presenter', 'interactive-npcs-synthetic-target')
$existingTask = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $taskNames -contains $_.ProcessName })
if ($existingTask.Count -gt 0) { throw "Refusing to capture while task-product processes already exist: $($existingTask.Id -join ', ')" }

$inputDocument = [ordered]@{
    schema = 'interactive-npcs-installed-evidence-inputs/v1'
    run_id = $runId
    source_candidate_digest_sha256 = $binding.source_candidate_digest_sha256
    inputs = $binding.inputs
}
[System.IO.File]::WriteAllText((Join-Path $runRoot 'input-bindings.json'), (($inputDocument | ConvertTo-Json -Depth 8) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))

$game = $null
$app = $null
$video = $null
try {
    Add-Type -AssemblyName System.Windows.Forms
    $screens = @([System.Windows.Forms.Screen]::AllScreens | Sort-Object DeviceName)
    $targetDisplay = @($screens | Where-Object { -not $_.Primary } | Select-Object -First 1)
    if ($targetDisplay.Count -eq 0) { $targetDisplay = @($screens | Where-Object { $_.Primary } | Select-Object -First 1) }
    if ($targetDisplay.Count -ne 1) { throw 'No Windows display is available.' }
    $display = $targetDisplay[0]
    $displayDocument = [ordered]@{
        schema = 'interactive-npcs-display-topology/v1'
        target = [ordered]@{ device_name = $display.DeviceName; primary = [bool]$display.Primary; bounds = [ordered]@{ left = $display.Bounds.Left; top = $display.Bounds.Top; width = $display.Bounds.Width; height = $display.Bounds.Height }; working_area = [ordered]@{ left = $display.WorkingArea.Left; top = $display.WorkingArea.Top; width = $display.WorkingArea.Width; height = $display.WorkingArea.Height } }
        displays = @($screens | ForEach-Object { [ordered]@{ device_name = $_.DeviceName; primary = [bool]$_.Primary; bounds = [ordered]@{ left = $_.Bounds.Left; top = $_.Bounds.Top; width = $_.Bounds.Width; height = $_.Bounds.Height } } })
        fallback_to_primary = [bool]$display.Primary
        physical_dpi = [ordered]@{ status = 'not_measured'; simulated = $false; detail = 'WebView view profiles are effective layout/scaling probes, not a physical monitor DPI measurement.' }
        hdr = [ordered]@{ status = 'not_measured'; simulated = $false; detail = 'The collector does not inspect or change Windows HDR/display settings.' }
    }
    [System.IO.File]::WriteAllText((Join-Path $runRoot 'display-topology.json'), (($displayDocument | ConvertTo-Json -Depth 7) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
    $gameMetadata = Join-Path $runRoot 'synthetic-game-metadata.json'
    $gameArgs = @('--metadata', $gameMetadata, '--width', '720', '--height', '720', '--fps', '15', '--exit-after-seconds', [string]($VideoSeconds + 60))
    $game = Start-HiddenProcess -FilePath $binding.fixture_executable -ArgumentList $gameArgs -WorkingDirectory $binding.fixture_root
    $port = Get-FreeTcpPort
    $app = Start-HiddenProcess -FilePath $binding.control_path -WorkingDirectory $binding.installed_root -Environment @{
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"
    }
    $windowDeadline = [DateTime]::UtcNow.AddSeconds(20)
    $taskWindows = @()
    while ([DateTime]::UtcNow -lt $windowDeadline) {
        $taskWindows = @(Get-VisibleWindowInventory | Where-Object { $_.pid -in @($game.Id, $app.Id) })
        if (@($taskWindows | Where-Object pid -eq $game.Id).Count -ge 1 -and @($taskWindows | Where-Object pid -eq $app.Id).Count -ge 1) { break }
        if ($game.HasExited -or $app.HasExited) { throw 'A task-owned process exited before its visible window was observed.' }
        Start-Sleep -Milliseconds 100
    }
    if (@($taskWindows | Where-Object pid -eq $game.Id).Count -lt 1 -or @($taskWindows | Where-Object pid -eq $app.Id).Count -lt 1) { throw 'Task-owned visible windows were not observed within 20 seconds.' }
    $placementDryRuns = @()
    foreach ($owned in @($game, $app)) {
        $dry = (& powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File $placementHelper -TargetProcessId $owned.Id -DeviceName $display.DeviceName) | ConvertFrom-Json
        $applied = (& powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File $placementHelper -TargetProcessId $owned.Id -DeviceName $display.DeviceName -Apply) | ConvertFrom-Json
        $placementDryRuns += [ordered]@{ pid = $owned.Id; dry_run = $dry; applied = $applied }
    }
    [System.IO.File]::WriteAllText((Join-Path $runRoot 'window-placement.json'), (([ordered]@{ schema = 'interactive-npcs-task-window-placement/v1'; task_owned_only = $true; records = $placementDryRuns } | ConvertTo-Json -Depth 9) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
    $visibleConsoleAtLaunch = @(Get-VisibleWindowInventory | Where-Object {
        $_.class_name -eq 'ConsoleWindowClass' -and ($_.pid -in @($game.Id, $app.Id) -or $_.process_name -in $taskNames)
    })
    if ($visibleConsoleAtLaunch.Count -ne 0) { throw 'A task-owned visible console window was detected at launch.' }
    $videoPath = Join-Path $runRoot 'installed-app-evidence.mp4'
    $videoLog = Join-Path $runRoot 'video.stderr.log'
    $width = $display.Bounds.Width - ($display.Bounds.Width % 2)
    $height = $display.Bounds.Height - ($display.Bounds.Height % 2)
    $videoArgs = @('-hide_banner', '-loglevel', 'warning', '-f', 'gdigrab', '-framerate', '30', '-draw_mouse', '0', '-video_size', "${width}x${height}", '-offset_x', [string]$display.Bounds.Left, '-offset_y', [string]$display.Bounds.Top, '-i', 'desktop', '-t', [string]$VideoSeconds, '-an', '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '18', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', $videoPath)
    $video = Start-HiddenProcess -FilePath $ffmpeg -ArgumentList $videoArgs -WorkingDirectory $runRoot -Redirect
    $videoErrorTask = $video.StandardError.ReadToEndAsync()
    $videoOutputTask = $video.StandardOutput.ReadToEndAsync()
    $driverArgs = @($driverPath, '--endpoint', "http://127.0.0.1:$port", '--output', $runRoot, '--plan', $resolvedPlanPath, '--playwright-module', $playwrightModule, '--run-id', $runId)
    $driver = Start-HiddenProcess -FilePath $node -ArgumentList $driverArgs -WorkingDirectory $runRoot -Redirect
    $driverOut = $driver.StandardOutput.ReadToEnd()
    $driverErr = $driver.StandardError.ReadToEnd()
    $driver.WaitForExit()
    [System.IO.File]::WriteAllText((Join-Path $runRoot 'driver.stdout.log'), $driverOut, (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::WriteAllText((Join-Path $runRoot 'driver.stderr.log'), $driverErr, (New-Object System.Text.UTF8Encoding($false)))
    if (-not $video.HasExited) { try { $video.StandardInput.WriteLine('q') } catch { }; [void]$video.WaitForExit(15000) }
    [System.IO.File]::WriteAllText($videoLog, $videoErrorTask.GetAwaiter().GetResult(), (New-Object System.Text.UTF8Encoding($false)))
    $null = $videoOutputTask.GetAwaiter().GetResult()
    if ($driver.ExitCode -ne 0) { throw "Installed evidence driver failed with exit $($driver.ExitCode): $driverErr" }
    if (-not (Test-Path -LiteralPath $videoPath -PathType Leaf) -or (Get-Item -LiteralPath $videoPath).Length -lt 100000) { throw 'Display-composited evidence video was not produced.' }
    $postWindows = Get-VisibleWindowInventory
    $protectedAfter = @($postWindows | Where-Object { $_.protected })
    $protectedUnchanged = (@($protectedBefore | ForEach-Object hwnd | Sort-Object) -join ',') -ceq (@($protectedAfter | ForEach-Object hwnd | Sort-Object) -join ',')
    if (-not $protectedUnchanged) { throw 'Protected Transparency App window inventory changed during capture.' }
    $visibleConsoleWindows = @($postWindows | Where-Object {
        $_.class_name -eq 'ConsoleWindowClass' -and ($_.pid -in @($game.Id, $app.Id, $video.Id, $driver.Id) -or $_.process_name -in $taskNames)
    })
    $videoIndex = [ordered]@{
        schema = 'interactive-npcs-installed-videos/v1'
        videos = @([ordered]@{ file = 'installed-app-evidence.mp4'; sha256 = Get-LowerSha256 -Path $videoPath; size_bytes = (Get-Item -LiteralPath $videoPath).Length; pixel_source = 'display-composited-gdigrab'; audio = 'none'; external_dimming_caveat = [string]$plan.capture.composited_video_note })
    }
    [System.IO.File]::WriteAllText((Join-Path $runRoot 'videos.json'), (($videoIndex | ConvertTo-Json -Depth 6) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
    $run = [ordered]@{
        schema = 'interactive-npcs-installed-evidence-run/v1'
        run_id = $runId
        status = if ($visibleConsoleWindows.Count -eq 0) { 'passed' } else { 'failed' }
        exact_installed_executable = $binding.control_path
        exact_synthetic_game_executable = $binding.fixture_executable
        task_pids = [ordered]@{ control = $app.Id; synthetic_game = $game.Id; recorder = $video.Id; driver = $driver.Id }
        protected_windows_unchanged = $protectedUnchanged
        visible_console_windows = $visibleConsoleWindows
        canonical_screenshot_pixel_source = 'webview-cdp'
        composited_video_is_color_canonical = $false
    }
    [System.IO.File]::WriteAllText((Join-Path $runRoot 'run.json'), (($run | ConvertTo-Json -Depth 8) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
    if ($visibleConsoleWindows.Count -ne 0) { throw 'A task-owned visible console window was detected.' }
}
finally {
    Stop-OwnedProcess -Process $video
    Stop-OwnedProcess -Process $app
    Stop-OwnedProcess -Process $game
}

$immutable = Finalize-EvidenceDirectory -RunRoot $runRoot -RunId $runId
[ordered]@{
    schema = 'interactive-npcs-installed-evidence-completion/v1'
    status = 'passed'
    run_id = $runId
    run_root = $runRoot
    evidence_root_sha256 = $immutable.evidence_root_sha256
    immutable_file_count = $immutable.file_count
} | ConvertTo-Json -Depth 4
