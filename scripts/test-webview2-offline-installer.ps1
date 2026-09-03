[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: WebView2 Authenticode qualification requires Windows.'
    exit 0
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'windows/webview2-offline-installer.ps1')

function Assert-Rejected {
    param([Parameter(Mandatory = $true)][scriptblock]$Action, [Parameter(Mandatory = $true)][string]$Message)
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw $Message }
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-webview2-contract-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $tempRoot | Out-Null
try {
    $contract = Get-NpcPinnedWebView2InstallerContract -RepositoryRoot $repoRoot
    $identity = Assert-NpcPinnedWebView2Installer -Contract $contract
    if ($identity.Sha256 -ne '987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61' -or
        $identity.SizeBytes -ne 258438352 -or
        $identity.AuthenticodeStatus -ne 'Valid' -or
        $identity.SignerThumbprint -ne '4028CAD637509D4744B17EC5B42AED8D7A31E6AF') {
        throw 'Reviewed WebView2 installer identity changed.'
    }

    Assert-Rejected -Action {
        Assert-NpcPinnedWebView2Installer -Contract $contract -Path (Join-Path $tempRoot 'missing.exe')
    } -Message 'Missing WebView2 installer was accepted.'

    $tampered = Join-Path $tempRoot 'MicrosoftEdgeWebView2RuntimeInstallerX64.exe'
    [System.IO.File]::WriteAllBytes($tampered, [byte[]](0x4d, 0x5a, 0x00, 0x00))
    Assert-Rejected -Action {
        Assert-NpcPinnedWebView2Installer -Contract $contract -Path $tampered
    } -Message 'Tampered WebView2 installer was accepted.'

    $generated = New-NpcPinnedWebView2InstallerHook -RepositoryRoot $repoRoot -InstallerIdentity $identity
    $generatedSource = Get-Content -LiteralPath $generated.HookPath -Raw
    if (-not $generatedSource.Contains('NPC_WEBVIEW2_OFFLINE_INSTALLER_PATH') -or
        -not $generatedSource.Contains($identity.Sha256) -or
        -not $generatedSource.Contains('installer-hooks.nsh')) {
        throw 'Generated WebView2 NSIS hook does not bind the reviewed installer and committed hook source.'
    }
    Remove-NpcPinnedWebView2InstallerHookStage -RepositoryRoot $repoRoot
    [System.IO.File]::WriteAllText($generated.StageRoot, 'unsafe non-directory stage')
    Assert-Rejected -Action {
        Remove-NpcPinnedWebView2InstallerHookStage -RepositoryRoot $repoRoot
    } -Message 'WebView2 generated-stage cleanup accepted a non-directory destructive target.'
    Remove-Item -LiteralPath $generated.StageRoot -Force

    $hookSource = Get-Content -LiteralPath (Join-Path $repoRoot 'packaging/windows/nsis/installer-hooks.nsh') -Raw
    foreach ($required in @('File "/oname=$PLUGINSDIR\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"',
            'ExecWait', '/silent /install', 'Abort')) {
        if (-not $hookSource.Contains($required)) { throw "Pinned WebView2 NSIS hook omits: $required" }
    }
    if ($hookSource -match 'https?://|NSISdl::download') {
        throw 'Pinned WebView2 NSIS hook contains a package-time network acquisition route.'
    }

    foreach ($configPath in @(
            'apps/control/src-tauri/tauri.conf.json',
            'packaging/windows/tauri.review.conf.json',
            'packaging/windows/tauri.release.conf.json'
        )) {
        $config = Get-Content -LiteralPath (Join-Path $repoRoot $configPath) -Raw | ConvertFrom-Json
        if ([string]$config.bundle.windows.webviewInstallMode.type -ne 'skip') {
            throw "$configPath does not disable Tauri's mutable WebView acquisition route."
        }
    }
    foreach ($overlayPath in @('packaging/windows/tauri.review.conf.json', 'packaging/windows/tauri.release.conf.json')) {
        $overlay = Get-Content -LiteralPath (Join-Path $repoRoot $overlayPath) -Raw | ConvertFrom-Json
        if ([string]$overlay.bundle.windows.nsis.installerHooks -ne
            'generated-installer-inputs/installer-hooks.generated.nsh') {
            throw "$overlayPath does not require the generated pinned-offline hook."
        }
    }
    Write-Host 'Pinned offline WebView2 missing/tamper/signature/config regression passed.' -ForegroundColor Green
}
finally {
    Remove-NpcPinnedWebView2InstallerHookStage -RepositoryRoot $repoRoot
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

exit 0
