[CmdletBinding()]
param(
    [string]$ConfigDirectory = (Join-Path $env:APPDATA 'io.github.akshitireddy.interactive-npcs.review'),
    [ValidateSet('groq', 'mistral', 'gemini')]
    [string]$ActivateLlm = 'groq'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$catalogPath = Join-Path $repoRoot 'catalog/v1/catalog.json'
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
$catalogRevision = [uint64]$catalog.catalog_revision
if ($catalogRevision -eq 0) { throw 'The bundled provider catalog revision is invalid.' }

$applicationNamespace = 'io.github.akshitireddy.interactive-npcs.review'
$configRoot = [System.IO.Path]::GetFullPath($ConfigDirectory)
if ([System.IO.Path]::GetFileName($configRoot) -cne $applicationNamespace) {
    throw "Private review loadouts can only be written to the isolated $applicationNamespace directory."
}
[System.IO.Directory]::CreateDirectory($configRoot) | Out-Null
$loadoutPath = Join-Path $configRoot 'provider-loadouts-v1.json'
$document = $null
$seededReviewConfig = $false
if (Test-Path -LiteralPath $loadoutPath -PathType Leaf) {
    $document = Get-Content -LiteralPath $loadoutPath -Raw | ConvertFrom-Json
    if ([string]$document.format -cne 'npc-provider-loadouts' -or [uint32]$document.schema_version -ne 1) {
        throw 'The existing review provider loadout document is not a supported v1 document.'
    }
}

function New-HostedRole {
    param(
        [Parameter(Mandatory)][string]$Provider,
        [Parameter(Mandatory)][string]$Model,
        [string]$Voice,
        [Parameter(Mandatory)][string]$Privacy,
        [Parameter(Mandatory)][string]$Cost,
        [Parameter(Mandatory)][string[]]$Data
    )
    $primary = [ordered]@{
        provider_id = $Provider
        model_id = $Model
        credential = [ordered]@{
            provider_id = $Provider
            reference_id = 'personal'
        }
        disclosure = [ordered]@{
            catalog_revision = $catalogRevision
            execution = 'hosted'
            egress = 'provider_cloud'
            privacy_summary = $Privacy
            cost_summary = $Cost
            transmitted_data = @($Data)
        }
        explicit_user_selection = $true
    }
    if (-not [string]::IsNullOrWhiteSpace($Voice)) {
        $primary['voice_id'] = $Voice
    }
    [ordered]@{
        mode = 'route'
        route = [ordered]@{
            primary = $primary
            fallbacks = @()
        }
    }
}

function New-FtsRole {
    [ordered]@{
        mode = 'route'
        route = [ordered]@{
            primary = [ordered]@{
                provider_id = 'fts-only'
                model_id = 'sqlite-fts5'
                credential = $null
                disclosure = [ordered]@{
                    catalog_revision = $catalogRevision
                    execution = 'local'
                    egress = 'none'
                    privacy_summary = 'Cyberpunk memory search stays on this PC.'
                    cost_summary = 'Uses the bundled SQLite keyword index.'
                    transmitted_data = @()
                }
                explicit_user_selection = $true
            }
            fallbacks = @()
        }
    }
}

if ($null -eq $document) {
    $starterRoles = [ordered]@{
        llm = New-HostedRole -Provider 'groq' -Model 'qwen/qwen3.6-27b' `
            -Privacy 'Transcript, selected game context, and memory context are sent to Groq when this route is used.' `
            -Cost "Uses the user's Groq account and its current free or paid limits." `
            -Data @('transcript', 'game_context', 'memory_context')
        stt = New-HostedRole -Provider 'assemblyai' -Model 'u3-rt-pro' `
            -Privacy 'Microphone audio is sent to AssemblyAI only while this speech route is used.' `
            -Cost "Uses the user's AssemblyAI account and its current trial or paid limits." `
            -Data @('microphone_audio')
        tts = New-HostedRole -Provider 'cartesia' -Model 'sonic-3.6' `
            -Voice 'a0e99841-438c-4a64-b679-ae501e7d6091' `
            -Privacy 'Response text is sent to Cartesia only while this voice route is used.' `
            -Cost "Uses the user's Cartesia account and its current free or paid limits." `
            -Data @('response_text')
        embeddings = New-FtsRole
        vision = [ordered]@{ mode = 'disabled' }
        lipsync = [ordered]@{ mode = 'disabled' }
    }
    $starter = [ordered]@{
        id = 'api-first-starter'
        name = 'API-first starter'
        scope = [ordered]@{ kind = 'global' }
        parent = $null
        roles = $starterRoles
    }
    $loadouts = [pscustomobject][ordered]@{ 'api-first-starter' = $starter }
    $document = [pscustomobject][ordered]@{
        format = 'npc-provider-loadouts'
        schema_version = 1
        loadouts = $loadouts
        activation = [pscustomobject][ordered]@{
            global = 'api-first-starter'
            games = [pscustomobject]@{}
            characters = [pscustomobject]@{}
        }
    }
    $seededReviewConfig = $true
}

$globalId = [string]$document.activation.global
if ([string]::IsNullOrWhiteSpace($globalId) -or
    $null -eq $document.loadouts.PSObject.Properties[$globalId]) {
    throw 'The existing review provider loadout document has no active global parent.'
}

$llmPresets = [ordered]@{
    groq = [ordered]@{
        id = 'cyberpunk-private-fast-groq'
        name = 'Cyberpunk - Groq fast'
        model = 'qwen/qwen3.6-27b'
        privacy = 'Dialogue and selected Cyberpunk context are sent to Groq for this turn.'
        cost = 'Uses the connected Groq account.'
    }
    mistral = [ordered]@{
        id = 'cyberpunk-private-fast-mistral'
        name = 'Cyberpunk - Mistral fast'
        model = 'ministral-8b-2512'
        privacy = 'Dialogue and selected Cyberpunk context are sent to Mistral for this turn.'
        cost = 'Uses the connected Mistral account.'
    }
    gemini = [ordered]@{
        id = 'cyberpunk-private-fast-gemini'
        name = 'Cyberpunk - Gemini fast'
        model = 'gemini-3.1-flash-lite'
        privacy = 'Dialogue and selected Cyberpunk context are sent to Gemini for this turn.'
        cost = 'Uses the connected Gemini account.'
    }
}

$changes = @()
foreach ($provider in $llmPresets.Keys) {
    $preset = $llmPresets[$provider]
    $id = [string]$preset.id
    if ($null -ne $document.loadouts.PSObject.Properties[$id]) {
        $existing = $document.loadouts.PSObject.Properties[$id].Value
        if ([string]$existing.scope.kind -cne 'game' -or
            [string]$existing.scope.game_id -cne 'cyberpunk-2077' -or
            [string]$existing.parent -cne $globalId -or
            [string]$existing.roles.llm.route.primary.provider_id -cne $provider -or
            [string]$existing.roles.llm.route.primary.model_id -cne [string]$preset.model) {
            throw "Existing loadout $id does not match the private review preset; it was preserved."
        }
        $changes += [ordered]@{ id = $id; status = 'already_present' }
        continue
    }

    $roles = [ordered]@{
        llm = New-HostedRole -Provider $provider -Model ([string]$preset.model) `
            -Privacy ([string]$preset.privacy) -Cost ([string]$preset.cost) `
            -Data @('transcript', 'game_context', 'memory_context')
        stt = New-HostedRole -Provider 'assemblyai' -Model 'u3-rt-pro' `
            -Privacy 'Microphone audio is sent to AssemblyAI while push to talk is active.' `
            -Cost 'Uses the connected AssemblyAI account.' -Data @('microphone_audio')
        tts = New-HostedRole -Provider 'cartesia' -Model 'sonic-3.6' `
            -Voice 'a0e99841-438c-4a64-b679-ae501e7d6091' `
            -Privacy 'Generated dialogue text is sent to Cartesia for speech.' `
            -Cost 'Uses the connected Cartesia account.' -Data @('response_text')
        embeddings = New-FtsRole
        vision = [ordered]@{ mode = 'disabled' }
        lipsync = [ordered]@{ mode = 'disabled' }
    }
    $loadout = [ordered]@{
        id = $id
        name = [string]$preset.name
        scope = [ordered]@{ kind = 'game'; game_id = 'cyberpunk-2077' }
        parent = $globalId
        roles = $roles
    }
    $document.loadouts | Add-Member -NotePropertyName $id -NotePropertyValue $loadout
    $changes += [ordered]@{ id = $id; status = 'created' }
}

$activeId = [string]$llmPresets[$ActivateLlm].id
if ($null -eq $document.activation.games) {
    $document.activation | Add-Member -NotePropertyName games -NotePropertyValue ([pscustomobject]@{})
}
if ($null -eq $document.activation.games.PSObject.Properties['cyberpunk-2077']) {
    $document.activation.games | Add-Member -NotePropertyName 'cyberpunk-2077' -NotePropertyValue $activeId
} else {
    $document.activation.games.PSObject.Properties['cyberpunk-2077'].Value = $activeId
}

$json = $document | ConvertTo-Json -Depth 30
foreach ($forbidden in @('api_key', 'credential_value', 'access_token', 'private_key')) {
    if ($json.IndexOf($forbidden, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw 'The generated loadout document contains a forbidden secret-like field.'
    }
}

$temporaryPath = Join-Path $configRoot ('.provider-loadouts-v1.' + [Guid]::NewGuid().ToString('N') + '.tmp')
$recoveryPath = Join-Path $configRoot 'provider-loadouts-v1.last-good.json'
try {
    [System.IO.File]::WriteAllText($temporaryPath, $json, [System.Text.UTF8Encoding]::new($false))
    if (Test-Path -LiteralPath $loadoutPath -PathType Leaf) {
        [System.IO.File]::Replace($temporaryPath, $loadoutPath, $recoveryPath, $true)
    } else {
        [System.IO.File]::Move($temporaryPath, $loadoutPath)
    }
} finally {
    if (Test-Path -LiteralPath $temporaryPath -PathType Leaf) {
        [System.IO.File]::Delete($temporaryPath)
    }
}

[ordered]@{
    schema_version = 1
    status = 'passed'
    application_namespace = $applicationNamespace
    config_directory = $configRoot
    credential_values_recorded = $false
    production_state_changed = $false
    seeded_review_config = $seededReviewConfig
    active_game = 'cyberpunk-2077'
    active_llm = $ActivateLlm
    active_loadout = $activeId
    speech_to_text = 'assemblyai/u3-rt-pro'
    text_to_speech = 'cartesia/sonic-3.6/a0e99841-438c-4a64-b679-ae501e7d6091'
    memory = 'local/sqlite-fts5'
    automatic_fallbacks = $false
    changes = @($changes)
} | ConvertTo-Json -Depth 5
