[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    Write-Host 'SKIP: package Corepack resolution regression requires Windows command semantics.'
    exit 0
}

. (Join-Path $PSScriptRoot 'windows/node-tooling.ps1')

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) { throw $Message }
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) `
    "npc-package-corepack-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
$savedPath = $env:PATH
$savedPathExt = $env:PATHEXT

try {
    $nodeRoot = Join-Path $fixtureRoot 'node'
    $corepackRoot = Join-Path $nodeRoot 'node_modules/corepack/dist'
    New-Item -ItemType Directory -Path $corepackRoot -Force | Out-Null
    $nodePath = Join-Path $nodeRoot 'node.exe'
    $corepackScript = Join-Path $corepackRoot 'corepack.js'
    [System.IO.File]::WriteAllBytes($nodePath, [byte[]](0x4d, 0x5a))
    [System.IO.File]::WriteAllText($corepackScript, '// fixture')

    $preferred = Get-NpcCorepackPnpmInvocation `
        -NodeExecutable $nodePath `
        -CorepackCommand (Join-Path $fixtureRoot 'missing-corepack.cmd')
    Assert-True -Condition ($preferred.Mode -eq 'node-corepack-js') `
        -Message 'Exact node.exe + corepack.js was not preferred.'
    Assert-True -Condition ($preferred.FilePath -eq (Resolve-Path $nodePath).Path) `
        -Message 'The resolved Node executable was not exact.'
    Assert-True -Condition ($preferred.Prefix.Count -eq 2 -and
        $preferred.Prefix[0] -eq (Resolve-Path $corepackScript).Path -and
        $preferred.Prefix[1] -eq 'pnpm') `
        -Message 'Corepack JavaScript invocation prefix was not exact.'

    Remove-Item -LiteralPath $corepackScript -Force
    $corepackCmd = Join-Path $fixtureRoot 'corepack.cmd'
    [System.IO.File]::WriteAllText($corepackCmd, "@exit /b 0`r`n")
    $fallback = Get-NpcCorepackPnpmInvocation `
        -NodeExecutable $nodePath `
        -CorepackCommand $corepackCmd
    Assert-True -Condition ($fallback.Mode -eq 'corepack-cmd') `
        -Message 'corepack.cmd was not selected as the only fallback.'
    Assert-True -Condition ($fallback.FilePath -eq (Resolve-Path $corepackCmd).Path) `
        -Message 'corepack.cmd fallback was not resolved to an exact path.'

    $posixShim = Join-Path $fixtureRoot 'corepack'
    [System.IO.File]::WriteAllText($posixShim, '#!/bin/sh')
    $rejected = $false
    try {
        Get-NpcCorepackPnpmInvocation `
            -NodeExecutable $nodePath `
            -CorepackCommand $posixShim | Out-Null
    }
    catch { $rejected = $_.Exception.Message -match 'POSIX Corepack shim' }
    Assert-True -Condition $rejected -Message 'The extensionless Corepack shim was not rejected.'

    $failureCmd = Join-Path $fixtureRoot 'failure.cmd'
    [System.IO.File]::WriteAllText($failureCmd, "@exit /b 23`r`n")
    $exitPropagated = $false
    try {
        Invoke-NpcCheckedCommand `
            -FilePath $failureCmd `
            -ArgumentList @('pnpm', '--version') `
            -FailureMessage 'fixture failed'
    }
    catch { $exitPropagated = $_.Exception.Message -match 'exit code 23' }
    Assert-True -Condition $exitPropagated -Message 'Corepack nonzero exit code was not propagated.'

    $spacedRoot = Join-Path $fixtureRoot 'Code Palace fixture'
    New-Item -ItemType Directory -Path $spacedRoot -Force | Out-Null
    $argumentReceipt = Join-Path $spacedRoot 'argument receipt.txt'
    $recorderCmd = Join-Path $spacedRoot 'record arguments.cmd'
    [System.IO.File]::WriteAllText(
        $recorderCmd,
        "@echo %~1^|%~2>`"$argumentReceipt`"`r`n@exit /b 0`r`n"
    )
    Invoke-NpcCheckedCommand `
        -FilePath $recorderCmd `
        -ArgumentList @('Code Palace', 'Visual Studio 17 2022') `
        -FailureMessage 'spaced argument fixture failed'
    $received = [System.IO.File]::ReadAllText($argumentReceipt).Trim()
    Assert-True -Condition ($received -eq 'Code Palace|Visual Studio 17 2022') `
        -Message "Windows argument quoting split a path/generator: $received"

    $metacharReceipt = Join-Path $spacedRoot 'metachar receipt.txt'
    $metacharCmd = Join-Path $spacedRoot 'record metacharacters.cmd'
    [System.IO.File]::WriteAllText(
        $metacharCmd,
        ("@set `"a1=%~1`"`r`n@set `"a2=%~2`"`r`n@set `"a3=%~3`"`r`n" +
            "@set `"a4=%~4`"`r`n@set `"a5=%~5`"`r`n" +
            "@> `"$metacharReceipt`" set a`r`n@exit /b 0`r`n")
    )
    Invoke-NpcCheckedCommand -FilePath $metacharCmd `
        -ArgumentList @('amp&ersand', 'pipe|value', 'caret^value', 'percent%NPC_UNDEFINED_FIXTURE%', 'paren(value)') `
        -FailureMessage 'cmd metacharacter fixture failed'
    $metacharReceived = @([System.IO.File]::ReadAllLines($metacharReceipt) |
        Where-Object { $_ -match '^a[1-5]=' })
    $expectedMetachar = @('a1=amp&ersand', 'a2=pipe|value', 'a3=caret^value',
        'a4=percent%NPC_UNDEFINED_FIXTURE%', 'a5=paren(value)')
    Assert-True -Condition (($metacharReceived -join "`n") -eq ($expectedMetachar -join "`n")) `
        -Message "Windows cmd argument quoting changed metacharacters: $($metacharReceived -join '; ')"

    $hiddenCmd = Join-Path $spacedRoot 'hidden process fixture.cmd'
    [System.IO.File]::WriteAllText(
        $hiddenCmd,
        "@echo hidden-stdout`r`n@echo hidden-stderr 1>&2`r`n@cd`r`n@exit /b 0`r`n"
    )
    $hidden = Invoke-NpcHiddenProcess -FilePath $hiddenCmd `
        -ArgumentList @() -WorkingDirectory $spacedRoot -NoReplayOutput
    Assert-True -Condition ($hidden.ExitCode -eq 0 -and $hidden.CreateNoWindow -eq $true -and
        $hidden.WindowStyle -eq 'Hidden') -Message 'Hidden process flags were not enforced.'
    Assert-True -Condition ($hidden.StandardOutput -match 'hidden-stdout' -and
        $hidden.StandardOutput -match [regex]::Escape($spacedRoot) -and
        $hidden.StandardError -match 'hidden-stderr') `
        -Message 'Hidden process output/working-directory capture is incomplete.'

    $pathExtCmd = Join-Path $spacedRoot 'pathext-child.cmd'
    [System.IO.File]::WriteAllText($pathExtCmd, "@echo pathext-child-ok`r`n@exit /b 0`r`n")
    $env:PATHEXT = '.CPL'
    $env:PATH = "$spacedRoot;$savedPath"
    $pathExtResult = Invoke-NpcHiddenProcess -FilePath 'pathext-child' -NoReplayOutput
    Assert-True -Condition ($pathExtResult.ExitCode -eq 0 -and
        $pathExtResult.StandardOutput -match 'pathext-child-ok') `
        -Message 'Hidden package children could not resolve .cmd tools from a WSL-inherited PATHEXT=.CPL environment.'
    foreach ($requiredExtension in @('.COM', '.EXE', '.BAT', '.CMD')) {
        Assert-True -Condition (@($env:PATHEXT -split ';') -contains $requiredExtension) `
            -Message "Shared hidden launcher did not restore required PATHEXT entry $requiredExtension."
    }

    $timeoutCmd = Join-Path $spacedRoot 'bounded timeout fixture.cmd'
    [System.IO.File]::WriteAllText(
        $timeoutCmd,
        "@%SystemRoot%\System32\ping.exe -n 8 127.0.0.1 >nul`r`n@exit /b 0`r`n"
    )
    $timeoutResult = Invoke-NpcHiddenProcess -FilePath $timeoutCmd `
        -TimeoutSeconds 1 -NoReplayOutput
    Assert-True -Condition $timeoutResult.TimedOut `
        -Message 'Hidden launcher did not terminate a bounded hung process tree.'

    Write-Host 'Package Corepack resolution regression checks passed.' -ForegroundColor Green
}
finally {
    [System.Environment]::SetEnvironmentVariable('PATH', $savedPath, 'Process')
    [System.Environment]::SetEnvironmentVariable('PATHEXT', $savedPathExt, 'Process')
    if (Test-Path -LiteralPath $fixtureRoot) {
        Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
    }
}

exit 0
