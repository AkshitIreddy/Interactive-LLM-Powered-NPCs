# Game profile and model manager design

## `GameProfileV2`

Profiles are signed data, not code. Required logical sections:

```text
schema/version/profile ID/title
content provenance and supported locale(s)
store/install/process/build detection
safety policy and online/anti-cheat refusal signals
capture mode, client/UI exclusions and display compatibility
capability levels and evidence: external capture, manual/read-only identity, overlay, generic visual fallback
identity strategy and confidence/fallback rules
world/canon content with spoiler/quest scopes
characters with stable IDs, biographies, personality/style and voice traits
background-NPC identity/persona rules
prompt/context policy and model/voice recommendations
external integration requirements and tested build/display scope
diagnostic checks, known limitations and troubleshooting
replay/live certification metadata
```

Executable discovery uses built-in read-only strategy IDs. A profile cannot provide a command, DLL, script, hook, injected component, registry write or arbitrary path traversal. Built-in and community profiles are data-only, and no profile requires a mod.

Capabilities distinguish `unsupported`, `generic`, `replay_verified`, `live_certified` and `blocked`. Authored content completeness is independent from live visual certification: an offscreen/unsupported visual path still supports explicit character selection and audio/subtitles.

## Discovery and launch safety

Discovery reads Steam library/app manifests, Epic manifests, GOG registry/database sources and common directories, then offers manual executable selection. Detected candidates are normalized by executable identity, store/app ID and install path. Discovery alone never launches or modifies the game.

Start rechecks process/build and online/protected/anti-cheat policy. Ambiguity disables capture and overlay. Manual selection cannot override the no-bypass policy; where policy permits, it can still select an audio/subtitle-only session.

## `ModelPackManifestV1`

The downloadable pack scope is limited to optional generic screen-space lip-sync. Local LLM, STT, TTS, and embedding packs are not product routes. The base app and API-powered conversation remain model-free.

Required logical fields:

```text
manifest/runtime ABI version and immutable pack ID/version
source repository and revision
files[] {relative path, byte size, SHA-256, role}
model architecture/task/quantization/languages
supported OS/CPU/GPU/backend/driver constraints
measured cold/warm load, RAM, VRAM, latency/quality envelope and benchmark fixture
license ID/text URL, redistribution/commercial terms and attribution
runtime entrypoint declared from a fixed allowlist
self-test definition and expected bounded result
dependencies and shared-file reference keys
upgrade/repair/removal/rollback metadata
```

The manifest never supplies a shell command. Entrypoints select application-owned worker kinds and validated arguments.

## Download and activation state machine

```text
Available → Resolving → Downloading ⇄ Paused
   → Verifying → ExtractingSafely → SelfTesting → ReadyToActivate
   → Active → Updating/Repairing/Removing
                       └──────────────→ RolledBack

Any pre-activation failure → Failed (staging retained only when safe/resumable)
```

Downloads use TUF-selected metadata, bounded resume with ETag/content-range validation, declared size and hash. Extraction rejects path traversal, links/reparse points, ADS/device paths, duplicates and bombs. Activation switches an atomic version pointer only after verification, self-test, attestation, and an explicit user activation choice; the old version remains until health confirmation. Defaults, dependencies, migrations, profiles/games, and fallback policy cannot activate a pack. Removal enumerates exact manifest-owned files and respects shared reference counts.

Disk-full, changed ETag, bad size/hash/signature, interrupted extraction, crashed self-test, locked file and rollback are first-class tests.
