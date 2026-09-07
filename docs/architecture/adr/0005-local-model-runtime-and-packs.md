# ADR-0005: API-first base with explicit measured local role packs

Status: Accepted; supersedes the lip-sync-only policy in the original 2026-08-28 record

Date: 2026-08-28

Updated: 2026-09-05

Reconciled: 2026-09-07

## Context

The default product should start on ordinary gaming PCs without bundled model weights,
Python, CUDA, FFmpeg, or a local-model-capable GPU. Hosted providers therefore remain the
first qualification route.

The product direction also includes optional local and hybrid loadouts. Current Model
Manager source already models six generic pack roles: language model, speech recognition,
speech synthesis, embedding, vision, and lip-sync. Manifest v2, local catalog,
whole-loadout selection, and resource-governor code cover those roles. Restricting the
pack lifecycle to lip-sync now contradicts both source and accepted product scope.

Model availability and resource safety are separate questions. A valid catalog entry or
working worker does not prove that a combination can coexist with the selected game.
Likewise, runtime-core contains opt-in ResourceBroker turn admission, but the normal
runtime host does not yet receive trusted native game-budget evidence and a qualified
whole-turn envelope. Architecture must not turn those partial joins into a production
claim.

## Decision

The base installer remains model-free and API-first. Users may explicitly install and
select qualified local packs for any of the six generic roles:

- language model;
- speech recognition;
- speech synthesis;
- embedding;
- vision/identity;
- screen-space lip-sync.

No role is privileged merely because it is visual. Every local pack follows the same
trust, measurement, lifecycle, admission, privacy, and truthful-availability rules.
Screen-space lip-sync additionally follows ADR-0007's current-frame visual gates.

A local/hybrid preset is shown as usable only when every selected role has an executable
runtime route and the complete combination is admitted. Missing roles never cause silent
cloud, device, model, or precision substitution. An API-only user never needs to install
a local pack.

## Pack trust and lifecycle

Before download the UI must disclose:

- exact model, immutable revision, role, runtime ABI, and intended placement;
- source, license, access, use, and redistribution terms;
- download and installed sizes plus shared-runtime accounting;
- supported Windows, CPU/GPU/backend/driver constraints;
- measured p99 RAM/VRAM workspace, load/reload/operation latency, and quality evidence;
- privacy behavior, expected game impact, experimental limits, and fallback.

No pack is bundled, auto-downloaded, auto-selected, activated by migration/dependency, or
represented as available until its metadata, license, signature, files, self-test, and
measured envelope pass. Installation is explicit. The manager stages and resumes safely,
verifies size/hash/signature, extracts without path escape, self-tests, atomically
activates, repairs, rolls back, removes, and reference-counts shared files.

A self-contained CPython runtime may accompany a selected pack only when its qualified
Windows path requires it. Pack/profile/model output remains data and is never executed as
generated code. Packs do not use system Python or inherit provider credentials.

## Whole-loadout admission

Pre-download and pre-activation policy classifies the requested combination, not each
model in isolation. The decision includes:

- measured resident and p99 total system RAM;
- measured resident and p99 transient/workspace VRAM;
- current desktop usage;
- the larger of current game usage and the configured game reserve;
- cold load, warm load, reload, and p99 operation latency;
- target frame-time/FPS policy, backend/driver compatibility, and user ceiling;
- compatible co-residency, serialized/exclusive stages, CPU-only placement, conflicts,
  and unverified combinations.

Model Manager's `ResourceGovernorV1` currently implements measured loadout preflight.
Runtime-core ResourceBroker also has an opt-in TurnSupervisor path with a host-provided
`TurnResourcePlanner`, reserve rejection, and lease release. Production execution may use
that path only after the runtime host receives:

1. native-stamped budget evidence bound to the selected game HWND, adapter, timestamp,
   and telemetry generation; and
2. a qualified whole-turn plan bound to exact selected routes, pack revisions,
   placements, and measurement provenance.

Until that join exists, these are implemented policy components rather than production
resource enforcement. Missing or stale evidence fails local admission before provider or
worker work. Pressure drops stale visual work and releases optional leases before it can
impair the game; it never evicts the game or changes the user's route silently.

## Current qualification state

- The supervised local LLM worker is a real integrated route, but its current sample pack
  has stale/non-admissible measurement metadata and is not production-available.
- Vision/identity worker and actor-lock contracts exist, but product identity activation
  is deliberately withheld until a measured trusted pack is admitted.
- Lip-sync worker, visual scheduling, and compositor contracts exist, but no candidate
  has passed moving-character visual and game-load qualification.
- A private YuNet/LM1 vision candidate has fresh 20-fresh/20-reload CPU measurements:
  458.756 ms p99 fresh load, 299.530 ms p99 reload, 102.274 ms worst inference p99,
  and 86,331,392 bytes absolute process-RAM p99. OS disk cache and game contention were
  uncontrolled. Its 2-of-2 ephemeral signed review catalog sets
  `productionTrust=false`. A later isolated activation qualification proved exact-nonce
  failure/retry, fresh authenticated hidden-worker provider load/unload, exact active
  inventory, duplicate rejection, and stale runtime-admission revocation. The result
  applies only to the isolated test state and cannot claim normal installation,
  production trust, live-rendering authority, natural tracking, game contention, or
  current-turn authority.
- Other role manifests, candidates, adapters, and tests do not establish downloadable or
  executable product routes.

The UI must distinguish **candidate**, **installed**, **verified**, **admitted**, and
**active**. Only the last two support a current-turn availability claim.

## Role-specific gates

All roles require correctness, cancellation, recovery, and resource evidence. Additional
minimum gates are:

| Role | Additional qualification |
| --- | --- |
| Language model | Prompt/schema conformance, streaming, context limits, quality, load/reload behavior |
| Speech recognition | Real microphone/PTT, endpointing, WER/noise/language matrix, cancellation |
| Speech synthesis | First PCM, named voice provenance, physical endpoint, alignment/viseme timing, cancellation |
| Embedding | Retrieval relevance, namespace compatibility, deterministic rebuild, CPU/GPU contention |
| Vision/identity | Rights-cleared calibration, false-lock/unknown rates, crossing/occlusion/reacquisition, native-only privacy |
| Lip-sync | ADR-0007 visual/latency/FPS/mask/identity/contention gates and audio/subtitle fail-open behavior |

## Consequences

- Hosted routes remain the fastest path to a useful base application, without making
  local/hybrid operation architecturally second-class.
- Pack selection and resource admission must reason across the whole chosen loadout.
- Local capability can roll out one qualified role at a time; “Fully Local” remains
  unavailable until every required role and their combination qualify.
- Catalog breadth and isolated tests may exceed exposed product choices.
- Restricted, research-only, non-commercial, or unverifiable model assets remain outside
  normal distribution. Direct user download cannot bypass prohibited use or missing
  qualification.
- Audio/subtitles remain the dependable fallback when optional identity or animation is
  absent.

## Evidence

Repository evidence:

- `crates/model-manager/src/manifest.rs`
- `crates/model-manager/src/manifest_v2.rs`
- `crates/model-manager/src/local_catalog.rs`
- `crates/model-manager/src/selection.rs`
- `crates/model-manager/src/resource_governor.rs`
- `crates/model-manager/examples/issue_yunet_review_envelope.rs`
- `crates/runtime-core/src/resource_broker.rs`
- `crates/runtime-core/src/supervisor.rs`
- `apps/control/src-tauri/src/local_resources.rs`
- `apps/control/src-tauri/src/optional_pack_activation.rs`
- `apps/control/src-tauri/src/identity_runtime.rs`
- `apps/runtime-host/src/simulation.rs`

External standards and platform references:

- TUF specification: <https://theupdateframework.github.io/specification/latest/>
- Windows graphics memory budgeting: <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/nf-dxgi1_4-idxgiadapter3-queryvideomemoryinfo>
- Windows Credential Manager: <https://learn.microsoft.com/en-us/windows/win32/secauthn/credential-manager>
