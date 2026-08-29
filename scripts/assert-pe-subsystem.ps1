[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Path,
    [ValidateSet('Gui', 'Console')]
    [string]$Expected = 'Gui'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$resolved = (Resolve-Path -LiteralPath $Path).Path
$bytes = [System.IO.File]::ReadAllBytes($resolved)
if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    throw "Not a valid PE executable: $resolved"
}

$peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 94 -ge $bytes.Length -or
    $bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45) {
    throw "PE header is invalid or truncated: $resolved"
}

# IMAGE_OPTIONAL_HEADER32 and IMAGE_OPTIONAL_HEADER64 place Subsystem at
# offset 0x44. The optional header begins after the 4-byte signature and
# 20-byte COFF header.
$optionalHeader = $peOffset + 24
$subsystem = [BitConverter]::ToUInt16($bytes, $optionalHeader + 0x44)
$expectedValue = if ($Expected -eq 'Gui') { 2 } else { 3 }
if ($subsystem -ne $expectedValue) {
    $actual = switch ($subsystem) {
        2 { 'Windows GUI' }
        3 { 'Windows CUI' }
        default { "PE subsystem $subsystem" }
    }
    throw "$resolved is $actual; expected Windows $Expected."
}

Write-Output "${resolved}: Windows $Expected subsystem verified."
