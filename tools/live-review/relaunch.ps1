<#
Restart the staged review app with WebView2 remote debugging (for cdp.py) and
place its window on a chosen screen rectangle.

  powershell -NoProfile -ExecutionPolicy Bypass -File tools/live-review/relaunch.ps1 `
      -ReviewRoot 'E:\work bench\Interactive LLM\review' -Rect '3900,60,1700,1300' [-CopyFreshBuild]
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ReviewRoot,
    [string]$Rect = '',
    [int]$DebugPort = 9333,
    [switch]$CopyFreshBuild
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Get-Process interactive-npcs-control, interactive-npcs-synthetic-target -ErrorAction SilentlyContinue |
    Stop-Process -Force -Confirm:$false
Start-Sleep -Seconds 2

$exe = Join-Path $ReviewRoot 'interactive-npcs-control.exe'
if ($CopyFreshBuild) {
    $metadata = & cargo metadata --format-version 1 --no-deps --offline `
        --manifest-path (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml') | ConvertFrom-Json
    Copy-Item -LiteralPath (Join-Path $metadata.target_directory 'debug/interactive-npcs-control.exe') `
        -Destination $exe -Force
}

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
Start-Process -FilePath $exe -WorkingDirectory $ReviewRoot | Out-Null

if ($Rect) {
    $python = (Get-Command python -CommandType Application | Select-Object -First 1).Source
    $placer = Join-Path $PSScriptRoot 'place_window.py'
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        & $python $placer --process interactive-npcs-control.exe --rect $Rect 2>$null
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Milliseconds 100
    }
}
