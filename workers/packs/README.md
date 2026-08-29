# Worker pack examples and production activation

The `*.stub-pack.json` files are complete, runnable development manifests for the deterministic workers in `../stubs`. They are examples of the worker-facing contract, but they are not entries in the signed downloadable catalog and contain no third-party code or model data.

A downloadable production worker is represented by two independently validated records:

1. A Model Manager `ModelPackManifestV1` owns immutable upstream revision, HTTPS download location, archive/artifact SHA-256 values and byte sizes, safe extraction, runtime ABI, platform/hardware constraints, model license, attribution, self-tests, dependencies, and atomic activation/rollback.
2. A worker descriptor owns the process contract: worker ID/kind/engine, Worker Control protocol compatibility, transports, modalities, compute backends, concurrency, cancellation, no-egress declaration, and resource estimates.

The Model Manager resolves the descriptor only from an activated, verified pack root. It passes an opaque lease ID and verified absolute model path to `load`; a worker is not allowed to download missing files, invoke a package manager, resolve a URL, inspect credentials, or search arbitrary directories. Deactivating or repairing the Model Manager pack invalidates its leases before files are replaced.

Every production pack derived from an example must provide the artifact roles and pass every activation gate listed in `production_pack_requirements`. The stub `resource_estimate` values are process-fixture scheduling ceilings, not model measurements. Production resource envelopes come from the signed `ModelPackManifestV1`, include hardware/backend and benchmark revision, and supersede the stub estimates.

Suggested catalog pairing:

| Development descriptor | Production runtime ABI |
|---|---|
| `llamacpp.stub-pack.json` | `npc-llamacpp-v1` |
| `moonshine.stub-pack.json` | `npc-onnx-stt-v1` |
| `whispercpp.stub-pack.json` | `npc-whispercpp-v1` |
| `kokoro.stub-pack.json` | `npc-onnx-tts-v1` |
| `onnx-embedding.stub-pack.json` | `npc-onnx-embedding-v1` |
| `onnx-vision.stub-pack.json` | `npc-onnx-vision-v1` |
| `experimental-lipsync.stub-pack.json` | qualification-specific and never stable by implication |

The mapping is an interface expectation, not permission to redistribute an engine or model. Exact code and model licenses must be reviewed independently.

## Optional generic lip-sync choices

`lipsync-pack-catalog-v1.development.json` is a development-only, Windows x64 candidate list. It is neither the signed Model Manager catalog nor a source of downloads. Its safety flags deliberately prohibit download locations, bundled model payloads, worker networking, game-specific adapters, automatic selection, automatic download, and fallback chains.

The list records only current status and activation requirements:

- Audio2Face-3D regression v2.3 is recorded only as an unqualified, unselectable coefficient-driver candidate. A separate mapper would have to produce a current-frame `npc.mouth-residual/v1` output; native-rig or static-avatar demonstrations are not evidence of generic moving-game suitability.
- NVIDIA AR SDK LipSync is conditional/private NGC access, not something an ordinary NIM API key unlocks. Its contract requires 16 kHz audio and 14 source frames of pre-roll. It remains selectable only after the user confirms access and a separately installed Windows pack is verified and qualified.
- MuseTalk 1.5 is a public experimental candidate, not a claim of live-game readiness.
- The tracked mouth-warp option is the lightweight baseline and has the same exact-frame, tracking, occlusion, freshness, and visual gates.
- Ditto is deferred because its available setup is Linux-oriented, pickle handling is rejected, and the Windows build remains unqualified.
- LatentSync is offline-only and is always rejected for live selection.

No candidate has guessed download size, RAM, VRAM, speed, or quality numbers. Users explicitly choose an installed candidate; there is no default and failure does not silently switch models.
