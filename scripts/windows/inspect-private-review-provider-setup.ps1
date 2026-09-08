[CmdletBinding()]
param(
    [string]$ConfigDirectory = (Join-Path $env:APPDATA 'io.github.akshitireddy.interactive-npcs.review'),
    [string]$GameId = 'cyberpunk-2077',
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

if ($env:OS -ne 'Windows_NT') {
    throw 'Private review provider setup can only be inspected on Windows.'
}

$applicationNamespace = 'io.github.akshitireddy.interactive-npcs.review'
$credentialNamespace = 'interactive-npcs/v2/review'
$configRoot = [System.IO.Path]::GetFullPath($ConfigDirectory)
if ([System.IO.Path]::GetFileName($configRoot) -cne $applicationNamespace) {
    throw "Private review setup can only be read from the isolated $applicationNamespace directory."
}

$loadoutPath = Join-Path $configRoot 'provider-loadouts-v1.json'
if (-not (Test-Path -LiteralPath $loadoutPath -PathType Leaf)) {
    throw "The private review provider loadout document is missing: $loadoutPath"
}

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class NpcReviewCredentialPresence
{
    [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredRead(string target, UInt32 type, UInt32 flags, out IntPtr credential);

    [DllImport("advapi32.dll", SetLastError = false)]
    private static extern void CredFree(IntPtr credential);

    public static bool Exists(string target)
    {
        if (String.IsNullOrWhiteSpace(target))
            throw new ArgumentException("Credential target is required.", "target");

        IntPtr credential = IntPtr.Zero;
        try
        {
            if (CredRead(target, 1, 0, out credential))
                return true;

            int error = Marshal.GetLastWin32Error();
            if (error == 1168)
                return false;
            throw new Win32Exception(error);
        }
        finally
        {
            if (credential != IntPtr.Zero)
                CredFree(credential);
        }
    }
}
'@

$documentText = [System.IO.File]::ReadAllText($loadoutPath, [System.Text.Encoding]::UTF8)
$document = $documentText | ConvertFrom-Json
if ([string]$document.format -cne 'npc-provider-loadouts' -or [uint32]$document.schema_version -ne 1) {
    throw 'The private review provider loadout document is not a supported v1 document.'
}

$activeId = $null
if ($null -ne $document.activation.games -and
    $null -ne $document.activation.games.PSObject.Properties[$GameId]) {
    $activeId = [string]$document.activation.games.PSObject.Properties[$GameId].Value
}
if ([string]::IsNullOrWhiteSpace($activeId)) {
    $activeId = [string]$document.activation.global
}
if ([string]::IsNullOrWhiteSpace($activeId) -or
    $null -eq $document.loadouts.PSObject.Properties[$activeId]) {
    throw "The private review config has no resolvable loadout for $GameId."
}

function Resolve-Role {
    param(
        [Parameter(Mandatory)][string]$LoadoutId,
        [Parameter(Mandatory)][string]$Role,
        [System.Collections.Generic.HashSet[string]]$Visited
    )

    if (-not $Visited.Add($LoadoutId)) {
        throw "Provider loadout inheritance contains a cycle at $LoadoutId."
    }
    $property = $document.loadouts.PSObject.Properties[$LoadoutId]
    if ($null -eq $property) {
        throw "Provider loadout inheritance references missing loadout $LoadoutId."
    }
    $loadout = $property.Value
    if ($null -ne $loadout.roles -and $null -ne $loadout.roles.PSObject.Properties[$Role]) {
        return $loadout.roles.PSObject.Properties[$Role].Value
    }
    $parent = [string]$loadout.parent
    if ([string]::IsNullOrWhiteSpace($parent)) {
        return $null
    }
    return Resolve-Role -LoadoutId $parent -Role $Role -Visited $Visited
}

function Get-CredentialState {
    param([Parameter(Mandatory)][string]$Provider)

    if ($Provider -eq 'fts-only') {
        return 'not_required'
    }
    $target = "$credentialNamespace/providers/$Provider"
    if ([NpcReviewCredentialPresence]::Exists($target)) { 'present' } else { 'missing' }
}

function Get-EffectiveRouteSummary {
    param([Parameter(Mandatory)][string]$Role)

    $selection = Resolve-Role -LoadoutId $activeId -Role $Role `
        -Visited ([System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal))
    if ($null -eq $selection -or [string]$selection.mode -eq 'disabled') {
        return [ordered]@{ role = $Role; mode = 'disabled' }
    }
    if ([string]$selection.mode -cne 'route' -or $null -eq $selection.route.primary) {
        throw "The effective $Role selection has an unsupported shape."
    }

    $primary = $selection.route.primary
    $provider = [string]$primary.provider_id
    $result = [ordered]@{
        role = $Role
        mode = 'route'
        provider_id = $provider
        model_id = [string]$primary.model_id
        credential_status = Get-CredentialState -Provider $provider
        fallback_count = @($selection.route.fallbacks).Count
    }
    if ($null -ne $primary.PSObject.Properties['voice_id'] -and
        -not [string]::IsNullOrWhiteSpace([string]$primary.voice_id)) {
        $result['voice_id'] = [string]$primary.voice_id
    }
    return $result
}

function Get-Sha256Hex {
    param([Parameter(Mandatory)][string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha256.ComputeHash($stream)
        return ([System.BitConverter]::ToString($hash)).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
        $stream.Dispose()
    }
}

$supportedProviders = @(
    'openai',
    'anthropic',
    'gemini',
    'groq',
    'mistral',
    'openrouter',
    'cohere',
    'nvidia-nim',
    'deepgram',
    'assemblyai',
    'elevenlabs',
    'cartesia',
    'inworld'
)
$providerPresence = foreach ($provider in $supportedProviders) {
    [ordered]@{
        provider_id = $provider
        credential_status = Get-CredentialState -Provider $provider
    }
}
$providerPresence += [ordered]@{
    provider_id = 'cloudflare-workers-ai'
    credential_status = 'adapter_unavailable'
}

$activeLoadout = $document.loadouts.PSObject.Properties[$activeId].Value
$snapshot = [ordered]@{
    schema_version = 1
    status = 'passed'
    captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    classification = 'recorded_native_config_and_vault_presence'
    application_namespace = $applicationNamespace
    credential_namespace = $credentialNamespace
    production_namespace_consulted = $false
    credential_values_exposed = $false
    provider_network_requests_made = $false
    config = [ordered]@{
        path = $loadoutPath
        sha256 = Get-Sha256Hex -Path $loadoutPath
        format = [string]$document.format
        schema_version = [uint32]$document.schema_version
    }
    game = [ordered]@{
        id = $GameId
        active_loadout_id = $activeId
        active_loadout_name = [string]$activeLoadout.name
    }
    effective_routes = @(
        Get-EffectiveRouteSummary -Role 'llm'
        Get-EffectiveRouteSummary -Role 'stt'
        Get-EffectiveRouteSummary -Role 'tts'
        Get-EffectiveRouteSummary -Role 'embeddings'
        Get-EffectiveRouteSummary -Role 'vision'
        Get-EffectiveRouteSummary -Role 'lipsync'
    )
    providers = @($providerPresence)
}

$json = $snapshot | ConvertTo-Json -Depth 8
if ($json -match '(?i)"(?:api_key|credential_value|access_token|private_key|secret_value)"\s*:') {
    throw 'The redacted private review snapshot contains a forbidden secret-like field.'
}

if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
    $resolvedOutput = [System.IO.Path]::GetFullPath($OutputPath)
    $outputDirectory = [System.IO.Path]::GetDirectoryName($resolvedOutput)
    if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
        [System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
    }
    [System.IO.File]::WriteAllText($resolvedOutput, $json, [System.Text.UTF8Encoding]::new($false))
}

$json
