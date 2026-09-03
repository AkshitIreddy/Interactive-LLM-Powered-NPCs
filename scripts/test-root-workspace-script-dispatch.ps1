[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: exact root workspace-script dispatch requires Windows Node/Corepack.'
    exit 0
}

. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$packagePath = Join-Path $repoRoot 'package.json'
$package = Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json
$expected = [ordered]@{
    dev = 'corepack pnpm --filter @npc2/control dev'
    tauri = 'corepack pnpm --filter @npc2/control tauri'
    typecheck = 'corepack pnpm --filter @npc2/control run typecheck'
    build = 'corepack pnpm --filter @npc2/control run build'
    test = 'corepack pnpm --filter @npc2/control test'
    'test:frontend' = 'corepack pnpm --filter @npc2/control test'
    'test:sim' = 'corepack pnpm --filter @npc2/simulation-harness test'
    'format:check' = 'corepack pnpm --filter @npc2/control format:check'
    sim = 'corepack pnpm --filter @npc2/simulation-harness sim'
    'sim:verify' = 'corepack pnpm --filter @npc2/simulation-harness verify'
}

foreach ($entry in $expected.GetEnumerator()) {
    $actual = [string]$package.scripts.($entry.Key)
    if ($actual -cne [string]$entry.Value) {
        throw "Root $($entry.Key) must filter only the control package without recursive root re-entry. Expected '$($entry.Value)', observed '$actual'."
    }
    if ($actual -match '(?:^|\s)(?:-r|--recursive)(?:\s|$)' -or
        $actual -notmatch '^corepack pnpm --filter @npc2/(?:control|simulation-harness) ') {
        throw "Root $($entry.Key) contains recursive or unfiltered Corepack dispatch: $actual"
    }
}

$pnpm = Get-NpcCorepackPnpmInvocation
foreach ($scriptName in @('typecheck', 'build')) {
    $result = Invoke-NpcHiddenProcess `
        -FilePath $pnpm.FilePath `
        -ArgumentList @($pnpm.Prefix + @('run', $scriptName)) `
        -WorkingDirectory $repoRoot `
        -TimeoutSeconds 180 `
        -NoReplayOutput
    if ($result.TimedOut) {
        throw "Exact pinned-Corepack root $scriptName timed out; recursive workspace re-entry or a hung frontend command is possible."
    }
    if ($result.ExitCode -ne 0) {
        $detail = ($result.StandardOutput + $result.StandardError).Trim()
        if ($detail.Length -gt 4000) { $detail = $detail.Substring($detail.Length - 4000) }
        throw "Exact pinned-Corepack root $scriptName failed with exit code $($result.ExitCode): $detail"
    }
    $rootHeader = '>\s*' + [regex]::Escape([string]$package.name) + '@' +
        [regex]::Escape([string]$package.version) + '\s+' + [regex]::Escape($scriptName)
    $rootInvocations = [regex]::Matches($result.StandardOutput, $rootHeader).Count
    if ($rootInvocations -ne 1) {
        throw "Root $scriptName ran $rootInvocations times; expected exactly one bounded root dispatch."
    }
}

[ordered]@{
    schema_version = 1
    status = 'passed'
    corepack_mode = [string]$pnpm.Mode
    scripts_source_checked = @($expected.Keys)
    scripts_executed = @('typecheck', 'build')
    root_invocations_each = 1
    recursive_root_dispatch = $false
    timeout_seconds_each = 180
} | ConvertTo-Json -Compress | Write-Output
