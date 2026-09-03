[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [ValidateSet('Gui', 'Console', 'Any')][string]$ExpectedSubsystem = 'Gui',
    [switch]$AsJson
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$resolved = (Resolve-Path -LiteralPath $Path).Path
$bytes = [System.IO.File]::ReadAllBytes($resolved)

function Assert-ReadableRange {
    param([int64]$Offset, [int64]$Length, [string]$Description)
    if ($Offset -lt 0 -or $Length -lt 0 -or $Offset + $Length -gt $bytes.LongLength) {
        throw "$Description is outside the PE file: $resolved"
    }
}

function Read-UInt16 { param([int64]$Offset) Assert-ReadableRange $Offset 2 '16-bit field'; return [BitConverter]::ToUInt16($bytes, [int]$Offset) }
function Read-UInt32 { param([int64]$Offset) Assert-ReadableRange $Offset 4 '32-bit field'; return [BitConverter]::ToUInt32($bytes, [int]$Offset) }
function Read-UInt64 { param([int64]$Offset) Assert-ReadableRange $Offset 8 '64-bit field'; return [BitConverter]::ToUInt64($bytes, [int]$Offset) }

if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    throw "Not a valid PE executable: $resolved"
}
$peOffset = [int](Read-UInt32 0x3c)
Assert-ReadableRange $peOffset 24 'PE header'
if ($bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45 -or
    $bytes[$peOffset + 2] -ne 0 -or $bytes[$peOffset + 3] -ne 0) {
    throw "PE signature is invalid: $resolved"
}

$machine = Read-UInt16 ($peOffset + 4)
if ($machine -ne 0x8664) {
    throw ('{0} has PE machine 0x{1:x4}; packaged sidecars must be x86_64 (0x8664).' -f $resolved, $machine)
}
$sectionCount = Read-UInt16 ($peOffset + 6)
$optionalSize = Read-UInt16 ($peOffset + 20)
$optionalOffset = $peOffset + 24
Assert-ReadableRange $optionalOffset $optionalSize 'PE optional header'
$magic = Read-UInt16 $optionalOffset
if ($magic -ne 0x20b) {
    throw ('{0} has optional-header magic 0x{1:x}; packaged sidecars must be PE32+.' -f $resolved, $magic)
}
$subsystemValue = Read-UInt16 ($optionalOffset + 0x44)
$subsystemName = switch ($subsystemValue) {
    2 { 'windows_gui' }
    3 { 'windows_console' }
    default { "pe_subsystem_$subsystemValue" }
}
if (($ExpectedSubsystem -eq 'Gui' -and $subsystemValue -ne 2) -or
    ($ExpectedSubsystem -eq 'Console' -and $subsystemValue -ne 3)) {
    throw "$resolved is $subsystemName; expected Windows $ExpectedSubsystem."
}

$imageBase = Read-UInt64 ($optionalOffset + 0x18)
$numberOfDirectories = Read-UInt32 ($optionalOffset + 0x6c)
$dataDirectoryOffset = $optionalOffset + 0x70
$sectionOffset = $optionalOffset + $optionalSize
Assert-ReadableRange $sectionOffset ([int64]$sectionCount * 40) 'PE section table'

$sections = for ($index = 0; $index -lt $sectionCount; $index++) {
    $offset = $sectionOffset + ($index * 40)
    [pscustomobject]@{
        VirtualSize = [uint64](Read-UInt32 ($offset + 8))
        VirtualAddress = [uint64](Read-UInt32 ($offset + 12))
        RawSize = [uint64](Read-UInt32 ($offset + 16))
        RawOffset = [uint64](Read-UInt32 ($offset + 20))
    }
}

function Convert-RvaToOffset {
    param([Parameter(Mandatory = $true)][uint64]$Rva)
    foreach ($section in $sections) {
        $extent = [Math]::Max($section.VirtualSize, $section.RawSize)
        if ($Rva -ge $section.VirtualAddress -and $Rva -lt $section.VirtualAddress + $extent) {
            $result = $section.RawOffset + ($Rva - $section.VirtualAddress)
            Assert-ReadableRange $result 1 "RVA 0x$($Rva.ToString('x'))"
            return [int64]$result
        }
    }
    throw "PE RVA 0x$($Rva.ToString('x')) is not mapped by any section: $resolved"
}

function Read-AsciiZAtRva {
    param([Parameter(Mandatory = $true)][uint64]$Rva)
    $offset = Convert-RvaToOffset $Rva
    $buffer = New-Object System.Collections.Generic.List[byte]
    for ($index = $offset; $index -lt $bytes.LongLength -and $buffer.Count -lt 1024; $index++) {
        $value = $bytes[$index]
        if ($value -eq 0) { return [System.Text.Encoding]::ASCII.GetString($buffer.ToArray()) }
        $buffer.Add($value)
    }
    throw "PE import name is unterminated or too long at RVA 0x$($Rva.ToString('x')): $resolved"
}

function Read-ImportDirectory {
    param([uint32]$DirectoryIndex, [bool]$Delayed)
    if ($numberOfDirectories -le $DirectoryIndex) { return @() }
    $entryOffset = $dataDirectoryOffset + ($DirectoryIndex * 8)
    Assert-ReadableRange $entryOffset 8 'PE data directory'
    $directoryRva = Read-UInt32 $entryOffset
    $directorySize = Read-UInt32 ($entryOffset + 4)
    if ($directoryRva -eq 0 -or $directorySize -eq 0) { return @() }

    $directoryOffset = Convert-RvaToOffset $directoryRva
    $descriptorSize = if ($Delayed) { 32 } else { 20 }
    $limit = [Math]::Min([int64]4096, [Math]::Max([int64]1, [Math]::Ceiling($directorySize / $descriptorSize)))
    $names = New-Object System.Collections.Generic.List[string]
    for ($index = 0; $index -lt $limit; $index++) {
        $descriptor = $directoryOffset + ($index * $descriptorSize)
        Assert-ReadableRange $descriptor $descriptorSize 'PE import descriptor'
        $allZero = $true
        for ($byteIndex = 0; $byteIndex -lt $descriptorSize; $byteIndex++) {
            if ($bytes[$descriptor + $byteIndex] -ne 0) { $allZero = $false; break }
        }
        if ($allZero) { break }

        if ($Delayed) {
            $attributes = Read-UInt32 $descriptor
            $nameValue = [uint64](Read-UInt32 ($descriptor + 4))
            $nameRva = if (($attributes -band 1) -eq 1) {
                $nameValue
            } else {
                if ($nameValue -lt $imageBase) { throw "Invalid VA-based delayed import name in $resolved" }
                $nameValue - $imageBase
            }
        } else {
            $nameRva = [uint64](Read-UInt32 ($descriptor + 12))
        }
        if ($nameRva -eq 0) { throw "PE import descriptor has no DLL name: $resolved" }
        $names.Add((Read-AsciiZAtRva $nameRva).ToLowerInvariant())
    }
    return @($names | Sort-Object -Unique)
}

$imports = @(Read-ImportDirectory -DirectoryIndex 1 -Delayed $false)
$delayedImports = @(Read-ImportDirectory -DirectoryIndex 13 -Delayed $true)
$allImports = @($imports + $delayedImports | Sort-Object -Unique)
$debugCrtImports = @($allImports | Where-Object {
        $_ -match '^(ucrtbased|msvcrtd)\.dll$' -or
        $_ -match '^(vcruntime|msvcp|concrt)[0-9]+(?:_[0-9]+)?d\.dll$'
    })
if ($debugCrtImports.Count -gt 0) {
    throw "$resolved imports Debug CRT libraries: $($debugCrtImports -join ', '). C++ product sidecars must be staged from Release builds."
}

$file = Get-Item -LiteralPath $resolved
$report = [ordered]@{
    schema_version = 1
    file_name = $file.Name
    size_bytes = $file.Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolved).Hash.ToLowerInvariant()
    machine = 'x86_64'
    pe_subsystem = $subsystemName
    imports = @($imports)
    delayed_imports = @($delayedImports)
    debug_crt_imports = @()
}

if ($AsJson) {
    $report | ConvertTo-Json -Compress -Depth 4 | Write-Output
} else {
    [pscustomobject]$report
}
