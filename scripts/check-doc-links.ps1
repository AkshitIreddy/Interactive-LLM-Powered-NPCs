[CmdletBinding()]
param(
    [string]$RepositoryRoot
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
}
$repositoryPath = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$excludedDirectories = @('.git', '.secrets', '.pnpm-store', '.render-work', 'artifacts', 'node_modules', 'target', 'out', 'dist', 'coverage')
$markdownFiles = New-Object 'System.Collections.Generic.List[System.IO.FileInfo]'
$pendingDirectories = New-Object 'System.Collections.Generic.Stack[string]'
$pendingDirectories.Push($repositoryPath)

while ($pendingDirectories.Count -gt 0) {
    $directory = $pendingDirectories.Pop()
    foreach ($childDirectory in @(Get-ChildItem -LiteralPath $directory -Directory -Force | Sort-Object Name -Descending)) {
        if ($excludedDirectories -notcontains $childDirectory.Name -and
            -not ($childDirectory.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            $pendingDirectories.Push($childDirectory.FullName)
        }
    }
    foreach ($markdownFile in @(Get-ChildItem -LiteralPath $directory -Filter '*.md' -File -Force | Sort-Object FullName)) {
        $markdownFiles.Add($markdownFile)
    }
}

$anchorCache = @{}

function Get-MarkdownAnchors {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ($anchorCache.ContainsKey($Path)) { return $anchorCache[$Path] }

    $anchors = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
    $slugCounts = @{}
    $inFence = $false
    foreach ($line in @(Get-Content -LiteralPath $Path)) {
        if ($line -match '^\s*(```|~~~)') {
            $inFence = -not $inFence
            continue
        }
        if ($inFence) { continue }

        foreach ($idMatch in @([regex]::Matches($line, '(?i)\bid\s*=\s*["''](?<id>[^"'']+)["'']'))) {
            [void]$anchors.Add($idMatch.Groups['id'].Value)
        }

        if ($line -notmatch '^\s{0,3}#{1,6}\s+(?<heading>.+?)\s*#*\s*$') { continue }
        $heading = $Matches.heading
        $heading = [regex]::Replace($heading, '!?(?:\[)(?<text>[^\]]+)(?:\])\([^)]*\)', '${text}')
        $heading = [regex]::Replace($heading, '<[^>]+>', '')
        $heading = [regex]::Replace($heading, '[`*_~]', '')
        $heading = $heading.ToLowerInvariant()
        $slug = [regex]::Replace($heading, '[^\p{L}\p{Nd}\s_-]', '')
        $slug = [regex]::Replace($slug.Trim(), '\s+', '-')
        if ([string]::IsNullOrWhiteSpace($slug)) { continue }

        $baseSlug = $slug
        if ($slugCounts.ContainsKey($baseSlug)) {
            $slugCounts[$baseSlug] = [int]$slugCounts[$baseSlug] + 1
            $slug = "$baseSlug-$($slugCounts[$baseSlug])"
        } else {
            $slugCounts[$baseSlug] = 0
        }
        [void]$anchors.Add($slug)
    }

    $anchorCache[$Path] = $anchors
    return $anchors
}

function Test-LocalLink {
    param(
        [Parameter(Mandatory = $true)][System.IO.FileInfo]$Source,
        [Parameter(Mandatory = $true)][int]$LineNumber,
        [Parameter(Mandatory = $true)][string]$RawTarget,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][System.Collections.Generic.List[string]]$Failures
    )

    $target = $RawTarget.Trim()
    if ($target.StartsWith('<') -and $target.EndsWith('>')) {
        $target = $target.Substring(1, $target.Length - 2)
    }
    if ([string]::IsNullOrWhiteSpace($target) -or $target.StartsWith('//') -or
        $target -match '^[A-Za-z][A-Za-z0-9+.-]*:') {
        return
    }

    $fragment = $null
    $fragmentIndex = $target.IndexOf('#')
    if ($fragmentIndex -ge 0) {
        $fragment = $target.Substring($fragmentIndex + 1)
        $target = $target.Substring(0, $fragmentIndex)
    }
    $queryIndex = $target.IndexOf('?')
    if ($queryIndex -ge 0) { $target = $target.Substring(0, $queryIndex) }

    try {
        $target = [System.Uri]::UnescapeDataString($target)
        if ($null -ne $fragment) { $fragment = [System.Uri]::UnescapeDataString($fragment) }
    }
    catch {
        $Failures.Add("$($Source.FullName):${LineNumber}: invalid URI escaping in '$RawTarget'.")
        return
    }

    if ([string]::IsNullOrWhiteSpace($target)) {
        $resolvedTarget = $Source.FullName
    } elseif ($target.StartsWith('/')) {
        $resolvedTarget = [System.IO.Path]::GetFullPath((Join-Path $repositoryPath $target.TrimStart('/')))
    } else {
        $platformTarget = $target.Replace('/', [System.IO.Path]::DirectorySeparatorChar)
        $resolvedTarget = [System.IO.Path]::GetFullPath((Join-Path $Source.DirectoryName $platformTarget))
    }

    $repositoryPrefix = $repositoryPath.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedTarget.StartsWith($repositoryPrefix, [System.StringComparison]::OrdinalIgnoreCase) -and
        -not $resolvedTarget.Equals($repositoryPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        $Failures.Add("$($Source.FullName):${LineNumber}: local link escapes the repository: '$RawTarget'.")
        return
    }
    if (-not (Test-Path -LiteralPath $resolvedTarget)) {
        $Failures.Add("$($Source.FullName):${LineNumber}: missing local link target '$RawTarget'.")
        return
    }

    if (-not [string]::IsNullOrWhiteSpace($fragment) -and
        [System.IO.Path]::GetExtension($resolvedTarget) -ieq '.md') {
        $anchors = Get-MarkdownAnchors -Path $resolvedTarget
        if (-not $anchors.Contains($fragment)) {
            $Failures.Add("$($Source.FullName):${LineNumber}: missing Markdown anchor '#$fragment' in '$RawTarget'.")
        }
    }
}

$failures = New-Object 'System.Collections.Generic.List[string]'
$checkedLinks = 0
$inlineLinkPattern = [regex]'!?\[[^\]]*\]\((?<target><[^>]+>|[^)\s]+)(?:\s+(?:"[^"]*"|''[^'']*''|\([^)]*\)))?\)'
$referencePattern = [regex]'^\s{0,3}\[[^\]]+\]:\s*(?<target><[^>]+>|\S+)'
$htmlPattern = [regex]'(?i)\b(?:href|src)\s*=\s*["''](?<target>[^"'']+)["'']'

foreach ($markdownFile in @($markdownFiles | Sort-Object FullName)) {
    $inFence = $false
    $lineNumber = 0
    foreach ($line in @(Get-Content -LiteralPath $markdownFile.FullName)) {
        $lineNumber += 1
        if ($line -match '^\s*(```|~~~)') {
            $inFence = -not $inFence
            continue
        }
        if ($inFence) { continue }

        $withoutInlineCode = [regex]::Replace($line, '`[^`]*`', '')
        $matches = New-Object 'System.Collections.Generic.List[System.Text.RegularExpressions.Match]'
        foreach ($match in @($inlineLinkPattern.Matches($withoutInlineCode))) { $matches.Add($match) }
        $referenceMatch = $referencePattern.Match($withoutInlineCode)
        if ($referenceMatch.Success) { $matches.Add($referenceMatch) }
        foreach ($match in @($htmlPattern.Matches($withoutInlineCode))) { $matches.Add($match) }

        foreach ($match in $matches) {
            $checkedLinks += 1
            Test-LocalLink -Source $markdownFile -LineNumber $lineNumber -RawTarget $match.Groups['target'].Value -Failures $failures
        }
    }
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { Write-Host "    FAIL  $failure" -ForegroundColor Red }
    throw "Local Markdown link validation found $($failures.Count) problem(s)."
}

Write-Host "Validated $checkedLinks Markdown links across $($markdownFiles.Count) files; remote URLs were not requested." -ForegroundColor Green
