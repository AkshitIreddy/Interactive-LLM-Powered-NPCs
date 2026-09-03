[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: exact native toolchain evidence requires Windows.'
    exit 0
}

$output = Join-Path ([System.IO.Path]::GetTempPath()) "npc-toolchain-evidence-$([Guid]::NewGuid().ToString('N')).json"
try {
    & (Join-Path $PSScriptRoot 'toolchain-evidence.ps1') -OutputPath $output -RequireNative
    if ($LASTEXITCODE -ne 0) { throw "Toolchain evidence exited with code $LASTEXITCODE." }
    $evidence = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
    if ($evidence.schema_version -ne 'interactive-npcs-toolchain-evidence/v1') {
        throw 'Toolchain evidence schema version is invalid.'
    }
    if ($evidence.observed.node.version -ne '20.20.2' -or
        $evidence.observed.rustc.version -notmatch '^1\.96\.1 ' -or
        [string]::IsNullOrWhiteSpace([string]$evidence.observed.cmake.sha256) -or
        [string]::IsNullOrWhiteSpace([string]$evidence.observed.msvc_compiler.sha256) -or
        [string]::IsNullOrWhiteSpace([string]$evidence.observed.windows_sdk.selected_default_version) -or
        [string]::IsNullOrWhiteSpace([string]$evidence.observed.windows_sdk.resource_compiler.sha256)) {
        throw 'Required pinned/native tool identities are missing or do not match the repository pins.'
    }
    if ($evidence.reproducibility.classification -ne 'pinned-input-non-byte-reproducible-local-review' -or
        $evidence.reproducibility.webview_bootstrapper_content_pinned -ne $true -or
        $evidence.reproducibility.webview_offline_installer_content_pinned -ne $true -or
        $evidence.reproducibility.webview_package_time_network_acquisition -ne $false -or
        $evidence.reproducibility.byte_reproducible_installer -ne $false -or
        $evidence.paths_recorded -ne $false) {
        throw 'Toolchain evidence overclaims byte reproducibility or records paths.'
    }
    if ($evidence.observed.webview2_offline_installer.sha256 -ne '987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61' -or
        $evidence.observed.webview2_offline_installer.size_bytes -ne 258438352 -or
        $evidence.observed.webview2_offline_installer.authenticode_status -ne 'Valid' -or
        $evidence.observed.webview2_offline_installer.cache_path_recorded -ne $false) {
        throw 'Toolchain evidence does not bind the reviewed Microsoft-signed WebView2 offline installer.'
    }
    $expectedInputs = @(
        'Cargo.lock',
        'apps/control/src-tauri/Cargo.lock',
        'apps/control/src-tauri/tauri.conf.json',
        'demo/readme/package-lock.json',
        'package.json',
        'packaging/security/installer-toolchain-provenance.json',
        'packaging/windows/nsis/installer-hooks.nsh',
        'packaging/windows/tauri.release.conf.json',
        'packaging/windows/tauri.review.conf.json',
        'pnpm-lock.yaml',
        'rust-toolchain.toml'
    )
    $actualInputs = @($evidence.source_inputs | ForEach-Object { [string]$_.path } | Sort-Object)
    if (($actualInputs -join '|') -ne (($expectedInputs | Sort-Object) -join '|')) {
        throw 'Toolchain evidence does not bind the exact reproducibility input set.'
    }
    Write-Host 'Exact native toolchain evidence regression passed.' -ForegroundColor Green
}
finally {
    Remove-Item -LiteralPath $output -Force -ErrorAction SilentlyContinue
}

exit 0
