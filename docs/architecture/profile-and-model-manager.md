# Game profile and model manager design

## `GameProfileV2`

Profiles are data, not code. The current built-in profiles are development data;
production catalogs must authenticate them before release. Required logical sections:

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

## `ModelPackManifestV2`

The base application remains API-first and model-free. ADR-0005 permits explicitly
selected, signed, measured local packs for language-model, speech-recognition,
speech-synthesis, embedding, vision, and lip-sync roles. Manifest v2 carries
role-specific extensions; legacy v1 manifests are migration inputs rather than the
architecture's current product contract.

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
   → Verifying → ExtractingSafely → AwaitingSelfTest
   → Activating → Active → Updating/Repairing/Removing
                       └──────────────→ RolledBack

Any pre-activation failure → Failed (staging retained only when safe/resumable)
```

Downloads use TUF-selected metadata, bounded resume with ETag/content-range validation, declared size and hash. Extraction rejects path traversal, links/reparse points, ADS/device paths, duplicates and bombs. Activation switches an atomic version pointer only after verification, self-test, attestation, and an explicit user activation choice; the old version remains until health confirmation. Defaults, dependencies, migrations, profiles/games, and fallback policy cannot activate a pack. Removal enumerates exact manifest-owned files and respects shared reference counts.

Disk-full, changed ETag, bad size/hash/signature, interrupted extraction, crashed self-test, locked file and rollback are first-class tests.

The private YuNet/LM1 review catalog is a concrete pre-activation example: its
fresh CPU envelope is 2-of-2 signed with ephemeral keys and explicitly carries
`productionTrust=false`, required key rotation, and disabled promotion/publication.
An isolated review-state qualification now proves inactive import, exact-nonce retry
cleanup, fresh authenticated hidden-worker provider load/unload, exact active inventory,
duplicate rejection, and stale runtime-admission revocation. That test-created active
pointer does not install the pack in the user's normal state or authorize live rendering.
Catalog validity and native inference measurements alone still cannot create an active
pointer.

## Runtime admission and residency

Every requested local/hybrid combination is classified before download and activation as
safe resident, serialized/cold-load only, CPU-only, conflicting, or unverified. Admission
uses measured resident and p99 RAM/VRAM workspace, desktop usage, the larger of current
game usage and configured game reserve, backend/driver compatibility, target frame time,
and load/reload cost. A manifest minimum or advertised total VRAM is insufficient.

Model Manager implements this whole-loadout preflight. Runtime-core also has opt-in
ResourceBroker turn admission, but the normal runtime host does not yet receive the
trusted native-stamped selected-game budget and qualified whole-turn envelope required
for production enforcement. Missing or stale evidence fails local admission.

The scheduler uses exclusive leases for incompatible stages, exposes switching latency,
and never silently co-resides unsafe models, changes device/provider, evicts the game, or
switches to cloud. Under pressure it drops optional stale visual work first;
audio/subtitles and the user's selected routes retain their declared fallback behavior.
