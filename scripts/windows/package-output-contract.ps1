Set-StrictMode -Version 2.0

function Assert-NpcOwnedNsisRoot {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$Path,
        [string[]]$OwnedTargetRoots = @()
    )
    $repo = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd('\', '/')
    $candidate = [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $ownershipRoot = $null
    if ($candidate.StartsWith("$repo\", [System.StringComparison]::OrdinalIgnoreCase) -and
        $candidate -match '[\\/]target[\\/](debug|release)[\\/]bundle[\\/]nsis$') {
        $ownershipRoot = $repo
    }
    foreach ($targetRoot in $OwnedTargetRoots) {
        if ([string]::IsNullOrWhiteSpace($targetRoot)) { continue }
        $canonicalTarget = [System.IO.Path]::GetFullPath($targetRoot).TrimEnd('\', '/')
        foreach ($profile in @('debug', 'release')) {
            $expected = [System.IO.Path]::GetFullPath(
                (Join-Path $canonicalTarget "$profile/bundle/nsis")).TrimEnd('\', '/')
            if ($candidate.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase)) {
                $ownershipRoot = $canonicalTarget
            }
        }
    }
    if ([string]::IsNullOrWhiteSpace($ownershipRoot)) {
        throw "Refusing unsafe NSIS output root: $candidate"
    }
    $cursor = $candidate
    while ($cursor.StartsWith($ownershipRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Refusing NSIS output through a reparse point: $cursor"
            }
        }
        if ($cursor.Equals($ownershipRoot, [System.StringComparison]::OrdinalIgnoreCase)) { break }
        $parent = Split-Path -Parent $cursor
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
    return $candidate
}

function Clear-NpcNsisOutputRoots {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string[]]$Candidates,
        [string[]]$OwnedTargetRoots = @()
    )
    foreach ($candidate in $Candidates) {
        $validated = Assert-NpcOwnedNsisRoot -RepositoryRoot $RepositoryRoot -Path $candidate `
            -OwnedTargetRoots $OwnedTargetRoots
        if (Test-Path -LiteralPath $validated) {
            Remove-Item -LiteralPath $validated -Recurse -Force
        }
    }
}

function Resolve-NpcCurrentInstaller {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string[]]$Candidates,
        [Parameter(Mandatory = $true)][string]$ExpectedFileName,
        [Parameter(Mandatory = $true)][DateTime]$BuildStartedUtc,
        [string[]]$OwnedTargetRoots = @()
    )
    $allFiles = New-Object System.Collections.Generic.List[System.IO.FileInfo]
    foreach ($candidate in $Candidates) {
        $validated = Assert-NpcOwnedNsisRoot -RepositoryRoot $RepositoryRoot -Path $candidate `
            -OwnedTargetRoots $OwnedTargetRoots
        if (Test-Path -LiteralPath $validated -PathType Container) {
            foreach ($file in @(Get-ChildItem -LiteralPath $validated -File | Sort-Object FullName)) {
                $allFiles.Add($file)
            }
        }
    }
    if ($allFiles.Count -ne 1) {
        throw "Expected exactly one current NSIS output file, found $($allFiles.Count)."
    }
    $installer = $allFiles[0]
    if ($installer.Name -ne $ExpectedFileName) {
        throw "Unexpected NSIS output file: $($installer.Name); expected $ExpectedFileName."
    }
    if ($installer.LastWriteTimeUtc -lt $BuildStartedUtc.AddSeconds(-2)) {
        throw "NSIS output predates the current build operation: $($installer.FullName)"
    }
    if ($installer.Length -le 0) { throw 'NSIS installer is empty.' }
    return $installer
}

function New-NpcPackageStageRoot {
    param(
        [Parameter(Mandatory = $true)][string]$OutputDirectory,
        [Parameter(Mandatory = $true)][string]$OperationId
    )
    if ($OperationId -notmatch '^[0-9a-f]{32}$') { throw 'Package operation ID must be a 32-character lowercase hex nonce.' }
    $name = "{0}-{1}" -f [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'), $OperationId
    $path = Join-Path $OutputDirectory $name
    if (Test-Path -LiteralPath $path) { throw "Unique package stage already exists: $path" }
    New-Item -ItemType Directory -Path $path | Out-Null
    return (Resolve-Path -LiteralPath $path).Path
}

function Get-NpcExpectedNsisControlIdentity {
    param(
        [Parameter(Mandatory = $true)][string]$Path
    )
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $bytes = [System.IO.File]::ReadAllBytes($resolved)
    $sourceMarker = '__TAURI_BUNDLE_TYPE_VAR_UNK'
    $installedMarker = '__TAURI_BUNDLE_TYPE_VAR_NSS'
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    $markerOffset = $text.IndexOf($sourceMarker, [System.StringComparison]::Ordinal)
    if ($markerOffset -lt 0 -or
        $text.IndexOf($sourceMarker, $markerOffset + 1, [System.StringComparison]::Ordinal) -ge 0) {
        throw 'Control executable must contain exactly one unpatched Tauri bundle-type marker.'
    }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $preBundleHash = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
        $replacement = [System.Text.Encoding]::ASCII.GetBytes($installedMarker)
        [Array]::Copy($replacement, 0, $bytes, $markerOffset, $replacement.Length)
        $installedHash = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }

    return [pscustomobject]@{
        pre_bundle_sha256 = $preBundleHash
        installed_sha256 = $installedHash
        source_marker = $sourceMarker
        installed_marker = $installedMarker
        marker_offset = $markerOffset
        changed_bytes = 3
    }
}
