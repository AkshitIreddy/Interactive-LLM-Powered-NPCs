Set-StrictMode -Version 2.0

function Get-NpcRepositoryBuildKey {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $canonicalRoot = [System.IO.Path]::GetFullPath($RepositoryRoot).
        Replace('/', '\').
        TrimEnd('\').
        ToUpperInvariant()
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($canonicalRoot)
        $digest = $sha256.ComputeHash($bytes)
    }
    finally {
        $sha256.Dispose()
    }

    return (($digest | ForEach-Object { $_.ToString('x2') }) -join '').Substring(0, 16)
}

function Get-NpcShortCMakeBuildPath {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)]
        [ValidatePattern('^[a-z0-9][a-z0-9-]{0,23}$')]
        [string]$Component
    )

    $localAppData = $env:LOCALAPPDATA
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        $localAppData = [System.Environment]::GetFolderPath(
            [System.Environment+SpecialFolder]::LocalApplicationData
        )
    }
    if ([string]::IsNullOrWhiteSpace($localAppData)) {
        throw 'LOCALAPPDATA could not be resolved for the disposable CMake build cache.'
    }

    $canonicalRepository = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd('\', '/')
    $buildBase = Join-Path $localAppData "InteractiveNPCs/build/$Component"
    $buildKey = Get-NpcRepositoryBuildKey -RepositoryRoot $canonicalRepository
    $buildPath = Join-Path $buildBase $buildKey

    $canonicalBase = [System.IO.Path]::GetFullPath($buildBase).TrimEnd('\', '/')
    $canonicalBuild = [System.IO.Path]::GetFullPath($buildPath).TrimEnd('\', '/')
    if ($buildKey -notmatch '^[0-9a-f]{16}$' -or
        -not $canonicalBuild.StartsWith(
            "$canonicalBase\",
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing unsafe disposable CMake build path: $buildPath"
    }
    if ($canonicalBuild.Equals($canonicalRepository, [System.StringComparison]::OrdinalIgnoreCase) -or
        $canonicalBuild.StartsWith(
            "$canonicalRepository\",
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "The disposable CMake build path must remain outside the repository: $buildPath"
    }

    return $canonicalBuild
}
