[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-hygiene-$([Guid]::NewGuid().ToString('N'))"
try {
    New-Item -ItemType Directory -Path (Join-Path $fixtureRoot 'native/component') -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $fixtureRoot 'native/component/CMakeLists.txt'), 'add_library(fixture STATIC fixture.cpp)')
    [System.IO.File]::WriteAllText((Join-Path $fixtureRoot 'native/component/fixture.cpp'), 'int fixture() { return 1; }')
    $git = Get-Command git.exe -ErrorAction SilentlyContinue
    if ($null -eq $git) {
        $gitCandidate = Join-Path $env:ProgramFiles 'Git/cmd/git.exe'
        if (Test-Path -LiteralPath $gitCandidate -PathType Leaf) { $git = Get-Item -LiteralPath $gitCandidate }
    }
    if ($null -eq $git) { throw 'Git for Windows is required for source-hygiene regression fixtures.' }
    $gitPath = if ($git -is [System.IO.FileInfo]) { $git.FullName } else { $git.Source }
    Invoke-NpcCheckedCommand -FilePath $gitPath -ArgumentList @('init', '-q', $fixtureRoot) `
        -FailureMessage 'Could not create source-hygiene Git fixture'
    Invoke-NpcCheckedCommand -FilePath $gitPath `
        -ArgumentList @('-C', $fixtureRoot, 'add', 'native/component/CMakeLists.txt', 'native/component/fixture.cpp') `
        -FailureMessage 'Could not index source-hygiene fixture'

    $clean = (& (Join-Path $PSScriptRoot 'security/check-source-hygiene.ps1') -Root $fixtureRoot) | ConvertFrom-Json
    Assert-True ($clean.status -eq 'passed') 'Clean native source fixture did not pass.'

    [System.IO.File]::WriteAllText((Join-Path $fixtureRoot '=12.8'), 'shell redirection artifact')
    $redirectionRejected = $false
    try { & (Join-Path $PSScriptRoot 'security/check-source-hygiene.ps1') -Root $fixtureRoot | Out-Null }
    catch { $redirectionRejected = $_.Exception.Message -match 'root-shell-redirection-artifact' }
    Assert-True $redirectionRejected 'Root =12.8 artifact was not rejected.'
    Remove-Item -LiteralPath (Join-Path $fixtureRoot '=12.8') -Force

    $artifactPath = Join-Path $fixtureRoot 'native/component/build/CMakeCache.txt'
    New-Item -ItemType Directory -Path (Split-Path -Parent $artifactPath) -Force | Out-Null
    [System.IO.File]::WriteAllText($artifactPath, 'generated cache')
    $nativeRejected = $false
    try { & (Join-Path $PSScriptRoot 'security/check-source-hygiene.ps1') -Root $fixtureRoot | Out-Null }
    catch { $nativeRejected = $_.Exception.Message -match 'native-in-checkout-build-artifact' }
    Assert-True $nativeRejected 'In-source native CMake output was not rejected.'
    Remove-Item -LiteralPath (Join-Path $fixtureRoot 'native/component/build') -Recurse -Force

    $rootBuildArtifact = Join-Path $fixtureRoot 'build/native-product/npc-product.vcxproj'
    New-Item -ItemType Directory -Path (Split-Path -Parent $rootBuildArtifact) -Force | Out-Null
    [System.IO.File]::WriteAllText($rootBuildArtifact, '<Project />')
    $rootBuildRejected = $false
    try { & (Join-Path $PSScriptRoot 'security/check-source-hygiene.ps1') -Root $fixtureRoot | Out-Null }
    catch { $rootBuildRejected = $_.Exception.Message -match 'native-in-checkout-build-artifact' }
    Assert-True $rootBuildRejected 'Repo-root native build tree was not rejected.'

    Write-Host 'Source hygiene regression checks passed.' -ForegroundColor Green
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) { Remove-Item -LiteralPath $fixtureRoot -Recurse -Force }
}

exit 0
