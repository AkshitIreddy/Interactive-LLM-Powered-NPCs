Set-StrictMode -Version 2.0

function Initialize-NpcWindowsExecutableExtensions {
    if ($env:OS -ne 'Windows_NT') { return }

    $extensions = New-Object System.Collections.Generic.List[string]
    foreach ($extension in @($env:PATHEXT -split ';')) {
        if (-not [string]::IsNullOrWhiteSpace($extension) -and
            -not $extensions.Contains($extension.ToUpperInvariant())) {
            $extensions.Add($extension.ToUpperInvariant())
        }
    }
    foreach ($required in @('.COM', '.EXE', '.BAT', '.CMD')) {
        if (-not $extensions.Contains($required)) { $extensions.Add($required) }
    }
    $env:PATHEXT = $extensions -join ';'
}

function ConvertTo-NpcWindowsCommandLineArgument {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Value)

    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    $builder = New-Object System.Text.StringBuilder
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') {
            $backslashes++
            continue
        }
        if ($character -eq '"') {
            [void]$builder.Append(('\' * (($backslashes * 2) + 1)))
            [void]$builder.Append('"')
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            [void]$builder.Append(('\' * $backslashes))
            $backslashes = 0
        }
        [void]$builder.Append($character)
    }
    if ($backslashes -gt 0) {
        [void]$builder.Append(('\' * ($backslashes * 2)))
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function ConvertTo-NpcCmdCommandArgument {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Value)
    if ($Value -match '[\r\n"]') {
        throw 'cmd.exe arguments may not contain quotes or newlines.'
    }
    # cmd.exe treats metacharacters as syntax even when ProcessStartInfo keeps
    # argv boundaries. Quote every token and keep delayed expansion disabled.
    # Percent text passed through %~N is not recursively expanded by the batch
    # processor, so preserve it byte-for-byte instead of doubling it.
    return '"' + $Value + '"'
}

function Get-NpcCorepackPnpmInvocation {
    [CmdletBinding()]
    param(
        [string]$NodeExecutable,
        [string]$CorepackCommand
    )

    if ([string]::IsNullOrWhiteSpace($NodeExecutable)) {
        $node = Get-Command node.exe -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($null -ne $node) { $NodeExecutable = $node.Source }
    }

    if (-not [string]::IsNullOrWhiteSpace($NodeExecutable)) {
        if (-not (Test-Path -LiteralPath $NodeExecutable -PathType Leaf)) {
            throw "Resolved node.exe does not exist: $NodeExecutable"
        }
        if ([System.IO.Path]::GetFileName($NodeExecutable) -ine 'node.exe') {
            throw "Packaging requires an exact node.exe path, not a shell shim: $NodeExecutable"
        }
        $corepackScript = Join-Path (Split-Path -Parent $NodeExecutable) `
            'node_modules/corepack/dist/corepack.js'
        if (Test-Path -LiteralPath $corepackScript -PathType Leaf) {
            return [pscustomobject]@{
                FilePath = (Resolve-Path -LiteralPath $NodeExecutable).Path
                Prefix = @((Resolve-Path -LiteralPath $corepackScript).Path, 'pnpm')
                Mode = 'node-corepack-js'
            }
        }
    }

    if ([string]::IsNullOrWhiteSpace($CorepackCommand)) {
        $corepack = Get-Command corepack.cmd -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($null -ne $corepack) { $CorepackCommand = $corepack.Source }
    }

    if (-not [string]::IsNullOrWhiteSpace($CorepackCommand)) {
        if (-not (Test-Path -LiteralPath $CorepackCommand -PathType Leaf)) {
            throw "Resolved corepack.cmd does not exist: $CorepackCommand"
        }
        if ([System.IO.Path]::GetExtension($CorepackCommand) -ine '.cmd') {
            throw "Packaging refuses the extensionless/POSIX Corepack shim: $CorepackCommand"
        }
        return [pscustomobject]@{
            FilePath = (Resolve-Path -LiteralPath $CorepackCommand).Path
            Prefix = @('pnpm')
            Mode = 'corepack-cmd'
        }
    }

    throw 'Corepack was not found as node.exe + corepack.js or corepack.cmd. Install the Node.js version pinned by package.json, then run scripts/dev.ps1 setup.'
}

function Invoke-NpcHiddenProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [string]$WorkingDirectory,
        [ValidateRange(0, 86400)][int]$TimeoutSeconds = 0,
        [switch]$NoReplayOutput
    )

    # WSL-launched Windows shells can inherit PATHEXT=.CPL. Normalize the
    # executable extensions before resolving the parent tool and before its
    # pnpm/npm children attempt to launch tsc.cmd, vite.cmd, or tauri.cmd.
    Initialize-NpcWindowsExecutableExtensions
    $resolvedFilePath = $FilePath
    $resolved = Get-Command $FilePath -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $resolved -and -not [string]::IsNullOrWhiteSpace([string]$resolved.Source)) {
        $resolvedFilePath = [string]$resolved.Source
    }
    if ([System.IO.Path]::GetFileName($resolvedFilePath) -ieq 'node.exe') {
        $nodeDirectory = Split-Path -Parent $resolvedFilePath
        $pathEntries = @($env:PATH -split ';')
        if ($pathEntries -notcontains $nodeDirectory) {
            $env:PATH = "$nodeDirectory;$env:PATH"
        }
    }
    $extension = [System.IO.Path]::GetExtension($resolvedFilePath)
    $processFilePath = $resolvedFilePath
    $processArguments = @($ArgumentList)
    $processArgumentString = $null
    if ($extension -ieq '.cmd' -or $extension -ieq '.bat') {
        $commandProcessor = if (-not [string]::IsNullOrWhiteSpace($env:ComSpec)) {
            $env:ComSpec
        } else { 'cmd.exe' }
        $innerCommand = (@($resolvedFilePath) + @($ArgumentList) | ForEach-Object {
                ConvertTo-NpcCmdCommandArgument -Value ([string]$_)
            }) -join ' '
        $processFilePath = $commandProcessor
        $processArguments = @('/d', '/s', '/v:off', '/c', $innerCommand)
        # cmd.exe /s strips exactly one outer quote pair. Preserve the inner
        # quoted script path/arguments as a single /c command string.
        $processArgumentString = '/d /s /v:off /c "' + $innerCommand + '"'
    }

    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $processFilePath
    $startInfo.Arguments = if ($null -ne $processArgumentString) {
        $processArgumentString
    } else {
        (@($processArguments | ForEach-Object {
                    ConvertTo-NpcWindowsCommandLineArgument -Value ([string]$_)
                }) -join ' ')
    }
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    if (-not [string]::IsNullOrWhiteSpace($WorkingDirectory)) {
        $startInfo.WorkingDirectory = $WorkingDirectory
    }
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) { throw "Could not start hidden process: $FilePath" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $timedOut = $false
        if ($TimeoutSeconds -gt 0) {
            if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
                $timedOut = $true
                # Terminate only this exact process tree. Package commands can
                # spawn pnpm/node children, so killing only the parent would
                # leave a recursive or hung build running after the gate fails.
                $taskKillInfo = New-Object System.Diagnostics.ProcessStartInfo
                $taskKillInfo.FileName = 'taskkill.exe'
                $taskKillInfo.Arguments = "/pid $($process.Id) /t /f"
                $taskKillInfo.UseShellExecute = $false
                $taskKillInfo.CreateNoWindow = $true
                $taskKillInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
                $taskKillInfo.RedirectStandardOutput = $true
                $taskKillInfo.RedirectStandardError = $true
                $taskKill = New-Object System.Diagnostics.Process
                $taskKill.StartInfo = $taskKillInfo
                try {
                    if ($taskKill.Start()) {
                        $taskKill.StandardOutput.ReadToEnd() | Out-Null
                        $taskKill.StandardError.ReadToEnd() | Out-Null
                        $taskKill.WaitForExit()
                    }
                }
                finally { $taskKill.Dispose() }
                if (-not $process.WaitForExit(10000)) {
                    $process.Kill()
                    $process.WaitForExit()
                }
            }
        }
        else { $process.WaitForExit() }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        if (-not $NoReplayOutput) {
            if (-not [string]::IsNullOrEmpty($stdout)) { [Console]::Out.Write($stdout) }
            if (-not [string]::IsNullOrEmpty($stderr)) { [Console]::Error.Write($stderr) }
        }
        return [pscustomobject]@{
            ExitCode = [int]$process.ExitCode
            StandardOutput = $stdout
            StandardError = $stderr
            FilePath = $processFilePath
            CreateNoWindow = [bool]$startInfo.CreateNoWindow
            WindowStyle = [string]$startInfo.WindowStyle
            TimedOut = $timedOut
        }
    }
    finally {
        $process.Dispose()
    }
}

function Invoke-NpcCheckedCommand {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ArgumentList,
        [Parameter(Mandatory = $true)][string]$FailureMessage,
        [ValidateRange(0, 86400)][int]$TimeoutSeconds = 0
    )

    $result = Invoke-NpcHiddenProcess -FilePath $FilePath -ArgumentList $ArgumentList `
        -TimeoutSeconds $TimeoutSeconds
    if ($result.TimedOut) {
        throw "$FailureMessage (timed out after $TimeoutSeconds seconds)."
    }
    if ($result.ExitCode -ne 0) {
        throw "$FailureMessage (exit code $($result.ExitCode))."
    }
}
