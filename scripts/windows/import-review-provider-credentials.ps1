[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$CredentialFile
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') { throw 'Provider credentials can only be imported into Windows Credential Manager on Windows.' }
$credentialPath = [System.IO.Path]::GetFullPath($CredentialFile)
if (-not (Test-Path -LiteralPath $credentialPath -PathType Leaf)) { throw "Credential file is missing: $credentialPath" }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class NpcReviewCredentialWriter
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct Credential
    {
        public UInt32 Flags;
        public UInt32 Type;
        public IntPtr TargetName;
        public IntPtr Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public UInt32 CredentialBlobSize;
        public IntPtr CredentialBlob;
        public UInt32 Persist;
        public UInt32 AttributeCount;
        public IntPtr Attributes;
        public IntPtr TargetAlias;
        public IntPtr UserName;
    }

    [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredWrite(ref Credential credential, UInt32 flags);

    public static void Write(string target, byte[] secret)
    {
        if (String.IsNullOrWhiteSpace(target) || secret == null || secret.Length < 8 || secret.Length > 8192)
            throw new ArgumentException("Credential target or value is invalid.");
        IntPtr targetPointer = IntPtr.Zero;
        IntPtr userPointer = IntPtr.Zero;
        IntPtr blobPointer = IntPtr.Zero;
        try
        {
            targetPointer = Marshal.StringToCoTaskMemUni(target);
            userPointer = Marshal.StringToCoTaskMemUni("Interactive NPCs 2.0");
            blobPointer = Marshal.AllocHGlobal(secret.Length);
            Marshal.Copy(secret, 0, blobPointer, secret.Length);
            var credential = new Credential {
                Flags = 0,
                Type = 1,
                TargetName = targetPointer,
                CredentialBlobSize = (UInt32)secret.Length,
                CredentialBlob = blobPointer,
                Persist = 2,
                UserName = userPointer
            };
            if (!CredWrite(ref credential, 0))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        }
        finally
        {
            if (blobPointer != IntPtr.Zero) {
                for (var index = 0; index < secret.Length; index++) Marshal.WriteByte(blobPointer, index, 0);
                Marshal.FreeHGlobal(blobPointer);
            }
            if (targetPointer != IntPtr.Zero) Marshal.ZeroFreeCoTaskMemUnicode(targetPointer);
            if (userPointer != IntPtr.Zero) Marshal.ZeroFreeCoTaskMemUnicode(userPointer);
            Array.Clear(secret, 0, secret.Length);
        }
    }
}
'@

$aliases = [ordered]@{
    'nvidia-nim' = @('nvidia', 'nim')
    'cohere' = @('cohere')
    'elevenlabs' = @('elevenlabs', 'eleven labs', 'elevenlab')
    'assemblyai' = @('assemblyai', 'assembly ai')
}
$found = @{}
$current = $null
foreach ($raw in [System.IO.File]::ReadAllLines($credentialPath, [System.Text.Encoding]::UTF8)) {
    $line = $raw.Trim()
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $lowered = $line.ToLowerInvariant()
    $provider = $null
    foreach ($candidate in $aliases.Keys) {
        foreach ($alias in $aliases[$candidate]) {
            if ($lowered.Contains($alias)) { $provider = $candidate; break }
        }
        if ($null -ne $provider) { break }
    }
    if ($null -ne $provider) {
        $current = $provider
        if ($line -match '[:=]\s*([^\s].*)$') {
            $value = $Matches[1].Trim().Trim('"').Trim("'")
            if ($value.Length -ge 8 -and -not $found.ContainsKey($provider)) { $found[$provider] = $value }
            $current = $null
        }
        continue
    }
    if ($null -ne $current -and $line.Length -ge 8 -and -not $found.ContainsKey($current)) {
        $found[$current] = $line.Trim('"').Trim("'")
        $current = $null
    }
}

$results = @()
foreach ($provider in $aliases.Keys) {
    if (-not $found.ContainsKey($provider)) {
        $results += [ordered]@{ provider = $provider; status = 'not_found' }
        continue
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes([string]$found[$provider])
    try {
        [NpcReviewCredentialWriter]::Write("interactive-npcs/v2/providers/$provider", $bytes)
        $results += [ordered]@{ provider = $provider; status = 'saved_to_windows_credential_manager' }
    }
    finally {
        [Array]::Clear($bytes, 0, $bytes.Length)
        $found[$provider] = $null
    }
}

[ordered]@{
    schema_version = 1
    status = if (@($results | Where-Object { $_.status -ne 'saved_to_windows_credential_manager' }).Count -eq 0) { 'passed' } else { 'incomplete' }
    namespace = 'interactive-npcs/v2'
    contains_credential_values = $false
    providers = @($results)
} | ConvertTo-Json -Depth 4

if (@($results | Where-Object { $_.status -ne 'saved_to_windows_credential_manager' }).Count -ne 0) { exit 1 }
