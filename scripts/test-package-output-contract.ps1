[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

. (Join-Path $PSScriptRoot 'windows/package-output-contract.ps1')
function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "npc-package-output-$([Guid]::NewGuid().ToString('N'))"
$nsisRoot = Join-Path $fixtureRoot 'apps/control/src-tauri/target/debug/bundle/nsis'
$expectedName = 'Interactive NPCs Response Console_2.0.0-alpha.1_x64-setup.exe'
New-Item -ItemType Directory -Path $nsisRoot -Force | Out-Null
try {
    $stale = Join-Path $nsisRoot $expectedName
    [System.IO.File]::WriteAllText($stale, 'stale')
    Clear-NpcNsisOutputRoots -RepositoryRoot $fixtureRoot -Candidates @($nsisRoot)
    Assert-True -Condition (-not (Test-Path -LiteralPath $nsisRoot)) -Message 'Pre-build cleanup left stale NSIS output.'

    $zeroRejected = $false
    try {
        Resolve-NpcCurrentInstaller -RepositoryRoot $fixtureRoot -Candidates @($nsisRoot) `
            -ExpectedFileName $expectedName -BuildStartedUtc ([DateTime]::UtcNow) | Out-Null
    } catch { $zeroRejected = $_.Exception.Message -match 'exactly one' }
    Assert-True $zeroRejected 'Zero installer outputs were accepted.'

    New-Item -ItemType Directory -Path $nsisRoot -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $nsisRoot $expectedName), 'current')
    [System.IO.File]::WriteAllText((Join-Path $nsisRoot 'unexpected.exe'), 'extra')
    $extraRejected = $false
    try {
        Resolve-NpcCurrentInstaller -RepositoryRoot $fixtureRoot -Candidates @($nsisRoot) `
            -ExpectedFileName $expectedName -BuildStartedUtc ([DateTime]::UtcNow.AddMinutes(-1)) | Out-Null
    } catch { $extraRejected = $_.Exception.Message -match 'exactly one' }
    Assert-True $extraRejected 'Multiple/extra NSIS outputs were accepted.'

    Remove-Item -LiteralPath (Join-Path $nsisRoot 'unexpected.exe')
    $installerPath = Join-Path $nsisRoot $expectedName
    (Get-Item -LiteralPath $installerPath).LastWriteTimeUtc = [DateTime]::UtcNow.AddMinutes(-5)
    $staleRejected = $false
    try {
        Resolve-NpcCurrentInstaller -RepositoryRoot $fixtureRoot -Candidates @($nsisRoot) `
            -ExpectedFileName $expectedName -BuildStartedUtc ([DateTime]::UtcNow) | Out-Null
    } catch { $staleRejected = $_.Exception.Message -match 'predates' }
    Assert-True $staleRejected 'A stale single installer was accepted.'

    (Get-Item -LiteralPath $installerPath).LastWriteTimeUtc = [DateTime]::UtcNow
    $resolved = Resolve-NpcCurrentInstaller -RepositoryRoot $fixtureRoot -Candidates @($nsisRoot) `
        -ExpectedFileName $expectedName -BuildStartedUtc ([DateTime]::UtcNow.AddSeconds(-1))
    Assert-True ($resolved.FullName -eq (Resolve-Path $installerPath).Path) 'Current exact installer was not resolved.'

    $unsafeRejected = $false
    try {
        Clear-NpcNsisOutputRoots -RepositoryRoot $fixtureRoot `
            -Candidates @((Join-Path $fixtureRoot 'important'))
    } catch { $unsafeRejected = $_.Exception.Message -match 'unsafe NSIS output root' }
    Assert-True $unsafeRejected 'Broad recursive NSIS cleanup target was accepted.'

    $operationA = [Guid]::NewGuid().ToString('N')
    $operationB = [Guid]::NewGuid().ToString('N')
    $stageA = New-NpcPackageStageRoot -OutputDirectory $fixtureRoot -OperationId $operationA
    $stageB = New-NpcPackageStageRoot -OutputDirectory $fixtureRoot -OperationId $operationB
    Assert-True ($stageA -ne $stageB) 'Same-second package stages collided.'

    $controlFixture = Join-Path $fixtureRoot 'control-fixture.exe'
    $fixtureBytes = [System.Text.Encoding]::ASCII.GetBytes('prefix__TAURI_BUNDLE_TYPE_VAR_UNKsuffix')
    [System.IO.File]::WriteAllBytes($controlFixture, $fixtureBytes)
    $identity = Get-NpcExpectedNsisControlIdentity -Path $controlFixture
    $expectedInstalledBytes = [System.Text.Encoding]::ASCII.GetBytes('prefix__TAURI_BUNDLE_TYPE_VAR_NSSsuffix')
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $expectedInstalledHash = ([BitConverter]::ToString($sha.ComputeHash($expectedInstalledBytes))).Replace('-', '').ToLowerInvariant()
    }
    finally { $sha.Dispose() }
    Assert-True ($identity.installed_sha256 -eq $expectedInstalledHash) 'Expected installed NSIS control hash is incorrect.'
    Assert-True ($identity.changed_bytes -eq 3) 'NSIS bundle patch must change exactly three marker bytes.'

    [System.IO.File]::WriteAllText($controlFixture, 'missing marker')
    $missingMarkerRejected = $false
    try { Get-NpcExpectedNsisControlIdentity -Path $controlFixture | Out-Null }
    catch { $missingMarkerRejected = $_.Exception.Message -match 'exactly one' }
    Assert-True $missingMarkerRejected 'A missing Tauri bundle marker was accepted.'

    [System.IO.File]::WriteAllText($controlFixture, '__TAURI_BUNDLE_TYPE_VAR_UNK__TAURI_BUNDLE_TYPE_VAR_UNK')
    $duplicateMarkerRejected = $false
    try { Get-NpcExpectedNsisControlIdentity -Path $controlFixture | Out-Null }
    catch { $duplicateMarkerRejected = $_.Exception.Message -match 'exactly one' }
    Assert-True $duplicateMarkerRejected 'Duplicate Tauri bundle markers were accepted.'

    Write-Host 'Package output freshness regression checks passed.' -ForegroundColor Green
}
finally {
    if (Test-Path -LiteralPath $fixtureRoot) { Remove-Item -LiteralPath $fixtureRoot -Recurse -Force }
}
exit 0
