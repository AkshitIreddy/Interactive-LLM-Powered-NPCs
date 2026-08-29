[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$releaseConfigPath = Join-Path $repoRoot 'packaging/windows/tauri.release.conf.json'
$tauriConfigPath = Join-Path $repoRoot 'apps/control/src-tauri/tauri.conf.json'
$release = Get-Content -LiteralPath $releaseConfigPath -Raw | ConvertFrom-Json
$tauri = Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json

$hookSetting = [string]$release.bundle.windows.nsis.installerHooks
if ($hookSetting -ne '../../../packaging/windows/nsis/installer-hooks.nsh') {
    throw 'Release config must reference the reviewed hook relative to the Tauri src-tauri config directory.'
}
$resolvedHook = [System.IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $tauriConfigPath) $hookSetting))
$expectedHook = [System.IO.Path]::GetFullPath((Join-Path $repoRoot 'packaging/windows/nsis/installer-hooks.nsh'))
if (-not $resolvedHook.Equals($expectedHook, [System.StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $resolvedHook -PathType Leaf)) {
    throw 'Configured NSIS hook does not resolve to the reviewed packaging hook.'
}
if ($release.bundle.windows.nsis.installMode -ne 'currentUser') {
    throw 'Registry symmetry policy is valid only for the current-user installer mode.'
}
if ($tauri.productName -ne 'Interactive NPCs Response Console' -or $tauri.identifier -ne 'io.github.akshitireddy.interactive-npcs') {
    throw 'Product identity changed; review the exact NSIS registry paths before packaging.'
}

$hook = Get-Content -LiteralPath $resolvedHook -Raw
$productKey = 'Software\github\Interactive NPCs Response Console'
foreach ($required in @(
    "ReadRegStr `$R8 HKCU `"$productKey`" `"`"",
    'StrCmp $R8 "$INSTDIR" 0 npc2_postuninstall_registry_done',
    "DeleteRegValue HKCU `"$productKey`" `"`"",
    "DeleteRegKey /ifempty HKCU `"$productKey`"",
    'DeleteRegKey /ifempty HKCU "Software\github"'
)) {
    if ($hook.IndexOf($required, [System.StringComparison]::Ordinal) -lt 0) {
        throw "NSIS uninstall symmetry hook is missing: $required"
    }
}
if ($hook -match '(?im)^\s*(DeleteRegKey|DeleteRegValue)(?:\s+/ifempty)?\s+HKLM\b') {
    throw 'Current-user uninstall hook must not mutate HKLM.'
}
if ($hook -match '(?im)^\s*DeleteRegKey\s+HKCU\s+"Software\\github') {
    throw 'Product/vendor keys may only be deleted with /ifempty.'
}

Write-Host 'NSIS current-user install/uninstall registry symmetry policy passed.' -ForegroundColor Green
exit 0
