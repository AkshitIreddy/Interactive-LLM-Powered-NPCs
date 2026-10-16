<#
Stage an already-built debug review app into a runnable folder.

Build first (from the repo root):
  scripts/prepare-sidecars.ps1 -Configuration Debug
  scripts/prepare-artifact-legal-resources.ps1 + scripts/stage-product-resources.ps1
  (cd apps/control) corepack pnpm run build
  (cd apps/control) corepack pnpm run tauri build --debug --no-bundle --ci --config ../../packaging/windows/tauri.review.conf.json

Then:
  powershell -NoProfile -ExecutionPolicy Bypass -File tools/live-review/stage-review.ps1 `
      -Destination 'E:\work bench\Interactive LLM\review' -TestGame 'E:\work bench\Interactive LLM\local-app-data\test-game' `
      [-MouthAtlas 'E:\work bench\Interactive LLM\.cache\mara-atlas\review-mouth-atlas']

Unlike scripts/windows/prepare-portable-review.ps1 this does no evidence or
provenance checks and needs no camera sequence; it is for local live testing
only. Without -MouthAtlas the worker uses its restrained source-only route.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Destination,
    [Parameter(Mandatory = $true)][string]$TestGame,
    # Optional reviewed mouth atlas folder. Debug builds load it from the
    # review-mouth-atlas folder next to the control executable.
    [string]$MouthAtlas = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path

$metadata = & cargo metadata --format-version 1 --no-deps --offline `
    --manifest-path (Join-Path $repoRoot 'apps/control/src-tauri/Cargo.toml') | ConvertFrom-Json
$controlBinary = Join-Path $metadata.target_directory 'debug/interactive-npcs-control.exe'
if (-not (Test-Path -LiteralPath $controlBinary -PathType Leaf)) { throw "Control app not built: $controlBinary" }

New-Item -ItemType Directory -Path $Destination -Force | Out-Null
Copy-Item -LiteralPath $controlBinary -Destination (Join-Path $Destination 'interactive-npcs-control.exe') -Force
Get-ChildItem -LiteralPath (Join-Path $repoRoot 'apps/control/src-tauri/binaries') -Filter '*-x86_64-pc-windows-msvc.exe' |
    ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Force `
            -Destination (Join-Path $Destination ($_.Name.Replace('-x86_64-pc-windows-msvc', '')))
    }

$mappings = @(
    @('profiles/games', 'profiles/games'),
    @('profiles/content-packs/local-review', 'profiles/content-packs/local-review'),
    @('catalog/v1', 'catalog/v1'),
    @('packaging/model-packs', 'packaging/model-packs'),
    @('apps/control/src-tauri/generated-resources', 'product-audit')
)
foreach ($mapping in $mappings) {
    $source = Join-Path $repoRoot $mapping[0]
    $target = Join-Path $Destination $mapping[1]
    if (-not (Test-Path -LiteralPath $source)) { Write-Warning "Skipping missing $source"; continue }
    if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Recurse -Force }
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $target -Recurse
}

$gameTarget = Join-Path $Destination 'local-app-data/test-game'
New-Item -ItemType Directory -Path $gameTarget -Force | Out-Null
Get-ChildItem -LiteralPath $TestGame -File | Copy-Item -Destination $gameTarget -Force

$atlasTarget = Join-Path $Destination 'review-mouth-atlas'
if (Test-Path -LiteralPath $atlasTarget) { Remove-Item -LiteralPath $atlasTarget -Recurse -Force }
$stagedAtlas = $null
if ($MouthAtlas) {
    foreach ($name in @('atlas.json', 'atlas-bgra8-premultiplied.bin', 'reviewed-artifact-receipt.v1.json')) {
        if (-not (Test-Path -LiteralPath (Join-Path $MouthAtlas $name) -PathType Leaf)) {
            throw "Mouth atlas is missing ${name}: ${MouthAtlas}"
        }
    }
    New-Item -ItemType Directory -Path $atlasTarget -Force | Out-Null
    Get-ChildItem -LiteralPath $MouthAtlas -File | Copy-Item -Destination $atlasTarget -Force
    $stagedAtlas = $atlasTarget
}

[pscustomobject]@{
    destination = (Resolve-Path $Destination).Path
    control = Join-Path $Destination 'interactive-npcs-control.exe'
    test_game = Join-Path $gameTarget 'interactive-npcs-synthetic-target.exe'
    mouth_atlas = $stagedAtlas
} | ConvertTo-Json
