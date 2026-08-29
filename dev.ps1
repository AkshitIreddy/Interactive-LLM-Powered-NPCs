[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet("environment", "setup", "dev", "lint", "test", "benchmark", "package")]
    [string]$Command = "environment",

    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Debug",

    [switch]$Offline,
    [switch]$Quick,
    [switch]$SkipChecks,
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$entrypoint = Join-Path $PSScriptRoot "scripts/dev.ps1"

if (-not (Test-Path -LiteralPath $entrypoint -PathType Leaf)) {
    throw "Developer command implementation was not found at $entrypoint"
}

$forward = @{
    Command = $Command
    Configuration = $Configuration
    Offline = $Offline
    Quick = $Quick
    SkipChecks = $SkipChecks
}
if (-not [string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $forward.OutputDirectory = $OutputDirectory
}

& $entrypoint @forward
exit $LASTEXITCODE
