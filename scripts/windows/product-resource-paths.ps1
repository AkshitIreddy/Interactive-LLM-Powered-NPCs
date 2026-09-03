Set-StrictMode -Version 2.0

function Test-NpcPathWithinBoundary {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Boundary
    )
    $separator = [System.IO.Path]::DirectorySeparatorChar
    return $Path.Equals($Boundary, [System.StringComparison]::OrdinalIgnoreCase) -or
        $Path.StartsWith("$Boundary$separator", [System.StringComparison]::OrdinalIgnoreCase)
}

function Assert-NpcGeneratedResourceRoot {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$AllowDisposableDestination
    )

    $repo = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd('\', '/')
    $candidate = [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $canonical = [System.IO.Path]::GetFullPath(
        (Join-Path $repo 'apps/control/src-tauri/generated-resources')).TrimEnd('\', '/')
    $temporary = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\', '/')
    $isCanonical = $candidate.Equals($canonical, [System.StringComparison]::OrdinalIgnoreCase)
    $isDisposable = $AllowDisposableDestination -and
        (Test-NpcPathWithinBoundary -Path $candidate -Boundary $temporary) -and
        -not $candidate.Equals($temporary, [System.StringComparison]::OrdinalIgnoreCase)
    if ([System.IO.Path]::GetFileName($candidate) -ne 'generated-resources' -or
        (-not $isCanonical -and -not $isDisposable)) {
        throw "Refusing unsafe generated resource destination: $candidate"
    }

    $boundary = if ($isCanonical) { $repo } else { $temporary }
    $cursor = $candidate
    while (Test-NpcPathWithinBoundary -Path $cursor -Boundary $boundary) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Refusing generated resource path through a reparse point: $cursor"
            }
        }
        if ($cursor.Equals($boundary, [System.StringComparison]::OrdinalIgnoreCase)) { break }
        $parent = Split-Path -Parent $cursor
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
    return $candidate
}

function Remove-NpcGeneratedResourceStage {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$AllowDisposableDestination
    )
    $validated = Assert-NpcGeneratedResourceRoot `
        -RepositoryRoot $RepositoryRoot `
        -Path $Path `
        -AllowDisposableDestination:$AllowDisposableDestination
    if (Test-Path -LiteralPath $validated) {
        Remove-Item -LiteralPath $validated -Recurse -Force
    }
}
