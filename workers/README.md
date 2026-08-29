# Local inference worker contracts

This directory owns the process boundary between the NPC runtime and local inference engines. It contains a versioned wire contract and deterministic development workers for:

- llama.cpp-compatible language generation;
- Moonshine and whisper.cpp speech recognition;
- Kokoro ONNX speech synthesis;
- ONNX embeddings and vision;
- experimental generic current-frame lip-sync.

The development workers are deliberately small, deterministic, offline programs. They exercise process supervision, framing, cancellation, stale-event rejection, resource scheduling, and result handling without downloading a model or pretending to measure model quality. They are **not** implementations of the named third-party engines.

Real runtimes, model weights, tokenizers, voice data, and optional Python environments are downloaded, verified, installed, repaired, and removed by the Model Manager as signed/versioned packs. None are bundled here. A production adapter must implement the same protocol and pass the conformance suite before it can be activated.

## Run a stub

From the repository root:

```powershell
python workers/stubs/worker.py --descriptor workers/packs/llamacpp.stub-pack.json
```

The default transport is length-delimited JSON over binary stdin/stdout. Diagnostics go to stderr only. `--input-pipe` and `--output-pipe` accept already-created Windows named-pipe paths for supervisor integration; the worker never creates a permissive pipe or chooses its ACL.

The frame format is a four-byte unsigned big-endian payload length followed by UTF-8 JSON. A complete example client is in `examples/client.py`.

## Contracts and safety invariants

- Protocol version `1.0`; see `protocol/worker-control-v1.proto` and `protocol/PROTOCOL.md`.
- Maximum frame size is 1 MiB, maximum text is 256 KiB, and maximum decoded inline binary is 512 KiB. Production audio/video uses runtime-owned shared memory or D3D handles, never unbounded JSON.
- The worker does not initiate network access. Production workers are launched in a deny-egress policy and must advertise `network_access: false`.
- Every request has a unique request ID, monotonic sequence, cancellation generation, and optional deadline.
- Only `cancel` may advance the cancellation generation. Work and events from older generations are discarded.
- `load`, `unload`, `warm`, and `cancel` are idempotent for retry-safe supervision.
- Model paths must be supplied by a verified Model Manager lease. Worker descriptors never contain credentials or remote URLs.
- stdout is protocol-only. Human-readable diagnostics use stderr and must not contain prompts, audio, images, credentials, or model content.

## Generic lip-sync boundary

Lip-sync is an optional, separately installed local worker pack. It is external to the game: no profile-specific mod, rig, animation hook, memory inspection, or executable game adapter is part of this worker contract. A selected local encounter/track supplies identity while the media broker supplies the current tracked face, current mouth region, and synchronized audio.

The versioned `npc.mouth-residual/v1` JSON control request contains only bounded opaque frame/audio lease identities, the selected encounter and track ID/epoch, exact source-frame sequence and capture time, normalized face/landmark/mouth-mask bounds, tracking confidence, lease expiries, a presentation deadline, and the request cancellation generation. Lease IDs cannot be paths, URLs, handles, or encoded media. Actual frames, audio, and any production residual remain on the supervisor-owned out-of-band media plane.

`mouth_patch_proposal` binds the proposal to the exact frame, track epoch, and cancellation generation. It includes tracking/residual confidence, landmark/mask bounds, enumerated fail-open reasons, and mandatory freshness/discard behavior. Advancing from source sequence N to N+1 invalidates the proposal; it can affect at most one displayed frame. `no_pixels_inline` is always true. The deterministic stub emits metadata only, sets `patch_lease_id` to `null`, is never presentable, and never modifies an image.

The only permitted production output semantics are `additive_rgba_mouth_residual`: zero alpha outside the declared mouth mask, no base-frame mutation, and composition over the current tracked source. Full-frame replacement, synthesized/cached heads, static-avatar sources, and claims that talking-head benchmarks demonstrate live-game suitability are explicitly outside this contract. Any visual rejection restores the untouched game frame while audio and subtitles continue.

`packs/lipsync-pack-catalog-v1.development.json` is a validated candidate descriptor for development and UI fixtures, not a download catalog. It contains no URLs, models, binaries, resource guesses, automatic selection, automatic fallback, or automatic download. A candidate becomes eligible only after a user explicitly selects its ID and an independently installed pack is verified and qualified.

## Test

```powershell
python -m unittest discover -s workers/tests -v
```

The tests launch real child processes, exercise every stub, reject oversized/malformed frames, verify lifecycle rules, test cancellation generation behavior, and validate all development pack descriptors.
