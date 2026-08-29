[CmdletBinding()]
param(
  [switch]$Live
)

$ErrorActionPreference = 'Stop'
$ToolRoot = Split-Path -Parent $PSScriptRoot
$CMakeCommand = Get-Command cmake.exe -ErrorAction SilentlyContinue
if ($CMakeCommand) {
  $CMake = $CMakeCommand.Source
}
else {
  $CMake = Join-Path $env:ProgramFiles 'CMake/bin/cmake.exe'
}
if (-not (Test-Path $CMake)) {
  throw 'CMake 3.24+ was not found on PATH or under Program Files/CMake/bin.'
}
$CTest = Join-Path (Split-Path -Parent $CMake) 'ctest.exe'

function Invoke-CheckedProcess {
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [Parameter(Mandatory = $true)][string[]]$ArgumentList,
    [Parameter(Mandatory = $true)][string]$Description
  )
  $Process = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList `
    -Wait -NoNewWindow -PassThru
  if ($Process.ExitCode -ne 0) {
    throw "$Description exited with $($Process.ExitCode)"
  }
}

Push-Location $ToolRoot
try {
  Invoke-CheckedProcess $CMake @('--preset', 'windows-msvc') 'CMake configure'
  Invoke-CheckedProcess $CMake @('--build', '--preset', 'windows-msvc') 'CMake build'
  Invoke-CheckedProcess $CTest @('--preset', 'windows-msvc') 'CTest'

  $Executable = Join-Path $ToolRoot 'out/build/windows-msvc/RelWithDebInfo/game-load.exe'
  if (-not (Test-Path $Executable)) {
    throw "Built executable was not found at $Executable"
  }

  $ResultDirectory = Join-Path $ToolRoot 'out/smoke'
  New-Item -ItemType Directory -Force -Path $ResultDirectory | Out-Null
  $DryResult = Join-Path $ResultDirectory 'dry-run.json'
  Invoke-CheckedProcess $Executable @(
    '--smoke', '--dry-run', '--profile', 'idle',
    '--power-profile-metadata', 'smoke-test-user-metadata',
    '--output', "`"$DryResult`""
  ) 'Dry-run smoke'

  $Manifest = Get-Content -Raw $DryResult | ConvertFrom-Json
  if (-not $Manifest.dry_run) { throw 'Smoke manifest did not report dry_run=true' }
  if ($Manifest.power.settings_changed_by_harness) {
    throw 'Safety contract violation: manifest reports a power-setting change'
  }
  if ($Manifest.power.profile_metadata -ne 'smoke-test-user-metadata') {
    throw 'Power metadata did not round-trip through the manifest'
  }

  if ($Live) {
    $LiveResult = Join-Path $ResultDirectory 'live-idle.json'
    Invoke-CheckedProcess $Executable @(
      '--smoke', '--run', '--profile', 'idle',
      '--thermal-policy', 'best-effort',
      '--power-profile-metadata', 'user-approved-live-smoke',
      '--output', "`"$LiveResult`""
    ) 'Live idle smoke'
  }

  Write-Host 'Game-load harness smoke passed.' -ForegroundColor Green
  if (-not $Live) {
    Write-Host 'No live workload was run. Pass -Live only for an explicit visible idle smoke.'
  }
}
finally {
  Pop-Location
}
