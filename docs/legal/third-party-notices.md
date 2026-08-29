# Third-party notices and provenance status

This file is a pre-release ledger placeholder, not a complete release notice. An RC is blocked until automated dependency inventories and human-reviewed notices cover every redistributed library, binary, model/runtime pack, font, demo asset, profile source, and build tool attribution required by its terms.

## Project license

Project-owned source is licensed under [MIT](../../LICENSE). That does not relicense third-party components, hosted services, models, games, or contributed assets.

## Architecture dependencies under review

The 2.0 source uses or targets ecosystems including Rust/Tokio, Tauri, React/React Aria, TypeScript/Vite, Protobuf, SQLite, Windows APIs, hosted speech/language/retrieval providers, optional generic lip-sync runtimes, and NSIS. Exact package versions and license texts must come from lockfiles/SBOM at RC build time; this prose is not a substitute.

## Models and providers

Conversation models are hosted and remain subject to provider terms; they are not redistributed. The only optional downloadable AI-pack category is generic local lip-sync. Every eligible pack needs its own source revision, license, attribution, use/redistribution limits, hashes, sizes, RAM/VRAM evidence, quality evidence, and runtime dependencies. Candidate names in documentation are evaluations, not proof of inclusion or availability.

## Games and profiles

Game names are used for nominative compatibility reference. No game assets or publisher licenses are bundled by default. Profile prose/assets require per-item provenance under the [game-content policy](game-content-policy.md).

## Release procedure

Generate and review:

```powershell
./scripts/security/generate-sbom.ps1 -Strict
./scripts/security/check-licenses.ps1 -Strict
```

Then reconcile the SBOM with pack manifests, profile provenance, demo asset hashes, required notice/license texts, and packaging layout. Missing/unknown/restricted entries block the RC; they are never silently omitted.
