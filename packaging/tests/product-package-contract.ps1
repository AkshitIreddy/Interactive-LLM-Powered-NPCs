[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-True {
    param([Parameter(Mandatory = $true)][bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw $Message }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$base = Get-Content -LiteralPath (Join-Path $repoRoot 'apps/control/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$review = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/windows/tauri.review.conf.json') -Raw | ConvertFrom-Json
$release = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/windows/tauri.release.conf.json') -Raw | ConvertFrom-Json
$expectedBase = @('binaries/npc-media-broker', 'binaries/npc-runtime') | Sort-Object
$expectedProduct = @(
    'binaries/npc-media-broker',
    'binaries/npc-mouth-worker',
    'binaries/npc-runtime',
    'binaries/npc-subtitle-presenter'
) | Sort-Object
Assert-True -Condition ((@($base.bundle.externalBin | Sort-Object) -join "`n") -eq ($expectedBase -join "`n")) `
    -Message 'Base Tauri config must remain independent of absent mouth/subtitle product staging.'
foreach ($overlay in @($review, $release)) {
    Assert-True -Condition ($null -ne $overlay.build -and [string]$overlay.build.beforeBuildCommand -eq '') `
        -Message 'Packaging overlay does not disable the inherited bare-Corepack frontend hook.'
    Assert-True -Condition ((@($overlay.bundle.externalBin | Sort-Object) -join "`n") -eq ($expectedProduct -join "`n")) `
        -Message 'Packaging overlay does not require exactly four product sidecars.'
    $resource = $overlay.bundle.resources.PSObject.Properties['generated-resources/']
    Assert-True -Condition ($null -ne $resource -and [string]$resource.Value -eq 'product-audit/') `
        -Message 'Packaging overlay does not install the generated product audit resource root.'
    Assert-True -Condition ([string]$overlay.bundle.windows.webviewInstallMode.type -eq 'skip' -and
        [string]$overlay.bundle.windows.nsis.installerHooks -eq 'generated-installer-inputs/installer-hooks.generated.nsh') `
        -Message 'Packaging overlay does not require the custom pinned-offline WebView2 contract.'
}
Assert-True -Condition ([string]$base.bundle.windows.webviewInstallMode.type -eq 'skip') `
    -Message 'Base Tauri config still exposes a mutable WebView2 acquisition route.'

$packagePath = Join-Path $repoRoot 'scripts/package.ps1'
$preparePath = Join-Path $repoRoot 'scripts/prepare-sidecars.ps1'
$stagePath = Join-Path $repoRoot 'scripts/stage-product-resources.ps1'
$resourcePathHelper = Join-Path $repoRoot 'scripts/windows/product-resource-paths.ps1'
$webViewPathHelper = Join-Path $repoRoot 'scripts/windows/webview2-offline-installer.ps1'
$packageSource = Get-Content -LiteralPath $packagePath -Raw
$prepareSource = Get-Content -LiteralPath $preparePath -Raw
$stageSource = Get-Content -LiteralPath $stagePath -Raw
foreach ($path in @($packagePath, $preparePath, $stagePath, $resourcePathHelper, $webViewPathHelper)) {
    $tokens = $null; $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -gt 0) { throw "$path has $($errors.Count) PowerShell parse error(s)." }
}

Assert-True -Condition ($packageSource -notmatch '(?m)&\s+corepack(?:\s|$)') `
    -Message 'Package script still invokes the extensionless Corepack shim.'
$frontendBuildIndex = $packageSource.IndexOf("-FailureMessage 'Pinned frontend build failed'", [System.StringComparison]::Ordinal)
$sidecarStageIndex = $packageSource.IndexOf("'prepare-sidecars.ps1'", [System.StringComparison]::Ordinal)
$tauriBuildIndex = $packageSource.IndexOf("-FailureMessage 'Pinned Tauri package build failed'", [System.StringComparison]::Ordinal)
Assert-True -Condition ($frontendBuildIndex -ge 0 -and $sidecarStageIndex -gt $frontendBuildIndex -and
    $tauriBuildIndex -gt $sidecarStageIndex) `
    -Message 'Exact frontend build must precede sidecar staging and the Tauri bundle invocation.'
Assert-True -Condition ($packageSource.Contains('immutable_release_candidate = $false') -and
    $packageSource.Contains("status = 'pending_installed_artifact_reconciliation'") -and
    $packageSource.Contains('promotion_supported = $false')) `
    -Message 'Package output is not truthfully downgraded until installed-artifact reconciliation.'
foreach ($required in @(
        'Get-NpcCorepackPnpmInvocation',
        "@('run', 'build')",
        'Pinned frontend build failed',
        'beforeBuildCommand',
        'check-source-hygiene.ps1',
        'assert-acceptance-evidence-consistency.ps1',
        'gap_map_sha256',
        'authoritative_ledger_sha256',
        'evidence_report_sha256',
        'Invoke-NpcCheckedCommand',
        'prepare-sidecars.ps1',
        'stage-product-resources.ps1',
        'prepare-artifact-legal-resources.ps1',
        'toolchain-evidence.ps1',
        'sidecar-manifest.v1.json',
        'resource-manifest.v1.json',
        'generated-resources',
        'audit-pe-product-binary.ps1',
        'Clear-NpcNsisOutputRoots',
        'Resolve-NpcCurrentInstaller',
        'New-NpcPackageStageRoot',
        'artifact_scope_resolved',
        'exact_license_materials'
        'Get-NpcPinnedWebView2InstallerContract'
        'Assert-NpcPinnedWebView2Installer'
        'New-NpcPinnedWebView2InstallerHook'
        'package_time_network_acquisition'
    )) {
    Assert-True -Condition ($packageSource.Contains($required)) -Message "Package integration omits $required."
}
Assert-True -Condition ($packageSource.Contains('Remove-NpcGeneratedResourceStage')) `
    -Message 'Generated Tauri resource staging is not cleaned after bundle consumption.'

Assert-True -Condition ($prepareSource -notmatch 'dev-wasapi-audio') `
    -Message 'Product sidecar preparation still enables dev-wasapi-audio.'
foreach ($required in @(
        "`$nativeConfiguration = 'Release'",
        "'--no-default-features'",
        "Target = 'npc-media-broker'",
        "Target = 'npc-mouth-worker'",
        "Target = 'npc_subtitle_presenter'",
        'npc-runtime-x86_64-pc-windows-msvc.exe',
        '-ExpectedSubsystem Gui',
        "product_audio_route = 'media-broker'",
        "schema = 'interactive-npcs-sidecars/v1'"
    )) {
    Assert-True -Condition ($prepareSource.Contains($required)) -Message "Sidecar product contract omits $required."
}
foreach ($required in @(
        'legal/LICENSE.txt',
        'legal/THIRD-PARTY-NOTICES.md',
        'legal/distribution-components.json',
        'legal/manual-license-review-2026-08-30.md',
        'legal/license-material-overrides.json',
        'legal/lockfiles.cdx.json',
        'legal/windows-artifact-scope.json',
        'legal/packages/',
        'legal/licenses/NVIDIA-RIVA-PROTO-MIT.txt',
        'legal/licenses/Nugine-simd-d74c030-MIT.txt',
        'legal/installer-toolchain-provenance.json',
        'legal/licenses/NSIS-3.11-COPYING.txt',
        'legal/licenses/nsis-tauri-utils-v0.5.3-APACHE-2.0.txt',
        'legal/licenses/nsis-tauri-utils-v0.5.3-MIT.txt',
        'legal/licenses/Dropbox-rust-alloc-no-stdlib-ae42d22-BSD-3-Clause.txt',
        'legal/licenses/Mozilla-MPL-2.0.txt',
        'legal/licenses/rust-unic-0.9.0-MIT.txt',
        'legal/licenses/rust-unic-0.9.0-APACHE-2.0.txt',
        'legal/licenses/rust-unic-0.9.0-COPYRIGHT.md',
        'legal/licenses/rust-unic-0.9.0-AUTHORS',
        'legal/licenses/webview2-rs-0.38.2-MIT.txt',
        'assets/subtitles/fonts.v1.json',
        'assets/subtitles/licenses.v1.json',
        'assets/subtitles/styles.v1.json',
        'packaging/runtime-components/onnxruntime-1.22.1-cpu.json',
        "schema = 'interactive-npcs-product-resources/v1'"
    )) {
    Assert-True -Condition ($stageSource.Contains($required)) -Message "Product resource contract omits $required."
}

$runtimeMain = Get-Content -LiteralPath (Join-Path $repoRoot 'apps/runtime-host/src/main.rs') -Raw
$brokerCmake = Get-Content -LiteralPath (Join-Path $repoRoot 'native/media-broker/CMakeLists.txt') -Raw
$mouthCmake = Get-Content -LiteralPath (Join-Path $repoRoot 'native/mouth-worker/CMakeLists.txt') -Raw
$subtitleCmake = Get-Content -LiteralPath (Join-Path $repoRoot 'native/subtitle-renderer/CMakeLists.txt') -Raw
Assert-True -Condition ($runtimeMain.Contains('windows_subsystem = "windows"')) -Message 'Runtime child is not Windows GUI subsystem.'
Assert-True -Condition ($brokerCmake.Contains('WIN32_EXECUTABLE TRUE') -and $brokerCmake.Contains('/ENTRY:mainCRTStartup')) -Message 'Broker child is not hidden GUI subsystem.'
Assert-True -Condition ($mouthCmake.Contains('WIN32_EXECUTABLE TRUE') -and $mouthCmake.Contains('/ENTRY:mainCRTStartup')) -Message 'Mouth worker child is not hidden GUI subsystem.'
Assert-True -Condition ($subtitleCmake.Contains('add_executable(npc_subtitle_presenter WIN32')) -Message 'Subtitle presenter child is not hidden GUI subsystem.'

Write-Host 'Product package contract regression checks passed.' -ForegroundColor Green
exit 0
