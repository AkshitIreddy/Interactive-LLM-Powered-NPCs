[CmdletBinding()]
param()

& (Join-Path $PSScriptRoot 'dev.ps1') environment
exit $LASTEXITCODE
