# Local TTS integration handoff

## Immutable identity

| Field | Exact value |
|---|---|
| Manifest | packaging/model-packs/kokoro-sherpa-onnx-v1.0-int8-windows-x64.json |
| Pack ID | local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64 |
| Revision | 2026.08.30-r1 |
| Capability | tts |
| Model | kokoro-82m-v1.0-int8-multilang |
| Runtime | sherpa-onnx 1.13.6 C API |
| Runtime Git commit | 1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911 |
| ONNX Runtime | 1.27.1 |
| Native provider | `npc-local-tts-native::KokoroLocalProvider` |
| Native worker | `npc-local-tts-native-worker.exe` |
| Model ABI | sherpa-onnx 1.13.6 C API, isolated inside the worker |
| Placement | cpu_resident only |
| Audio | 24000 Hz, mono, pcm_s16le |
| Voices | 28 fixed en-US and en-GB stock voices |
| Cloning | prohibited |

Resolve the canonical manifest SHA-256 with manifest.load_manifest or the
pack-manager plan operation after all lanes stop editing the manifest. Bind
catalog trust to that canonical digest, not the whitespace-sensitive file hash.
The document itself is strict npc.model-pack/v2 and validates against
packaging/model-packs/model-pack-manifest.schema.json. The obsolete role-local
v1 schema is not a distributable manifest format.

## Catalog and lifecycle

1. Import this exact identity as an optional TTS candidate. It is never a
   default, automatic download, or silent fallback.
2. Replace the CLI-provided digest trust anchor with the signed catalog trust
   root. Preserve explicit user confirmation of pack ID and revision.
3. Present and record acknowledgement of all six disclosed entries: Kokoro
   model and fixed voices, Kokoro training attribution, sherpa-onnx, eSpeak-NG,
   piper-phonemize integration, and ONNX Runtime.
4. Reuse Model Manager download journaling if it preserves the same invariants:
   HTTPS and bounded redirect allowlist, pinned size and SHA-256, exclusive
   partial file, fsync, safe extraction, critical-file verification, complete
   inventory, and atomic revision activation.
5. Verify the full installed inventory before every worker launch. Repair is a
   staged replacement with rollback. Remove selects only the exact revision and
   must never broaden to the model-pack root.
6. Keep direct upstream download until legal review approves a project mirror,
   required notices, and GPL corresponding-source handling.

Install root layout:

    model-packs/
      local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64/
        2026.08.30-r1/
          runtime/lib/
          model/
          installation-record.v1.json

## Supervisor launch

The checked-in Python worker remains a qualification harness and protocol
reference. It is not a normal-startup dependency. The optional native route
uses the Rust `npc-local-tts-native` provider and launches
`npc-local-tts-native-worker.exe` with `CREATE_NO_WINDOW` and kill-on-drop
containment. A release supervisor should additionally apply a restricted token,
deny-egress policy, and kill-on-close job object.

The native worker accepts only the exact installed revision root and a CPU
thread count between 1 and 8:

    --pack-root <exact installed revision directory>
    --cpu-threads 2

The trusted host constructs the provider only after Model Manager verifies the
signed catalog, installation, selected stock voice, current-device envelope,
and whole-loadout lease. The worker independently hashes the six critical model
files, checks the exact root identity, loads only the C API DLL at the absolute
pack path, restricts dependent-DLL search to that directory and safe system
directories, and verifies sherpa version, Git SHA, ONNX Runtime version, 24 kHz
sample rate, and at least 53 embedded speakers. The public provider still
accepts only speaker IDs 0 through 27.

## Audio transport and scheduling

The private native stdio protocol uses a bounded four-byte big-endian JSON
command length. Each output frame uses a four-byte big-endian JSON-header length,
a four-byte big-endian PCM length, the closed JSON event, and optional PCM s16le.
Request IDs, provider-local cancellation generations, PCM sequence, sample rate,
channels, and the terminal flag are checked by the safe Rust provider. Treat
the terminal event, not the cancel acknowledgement, as the stream drain barrier.

The provider sets separate bounded load and response deadlines. A bad frame,
hung response, or internally rejected request retires the process; a later
synthesis starts and handshakes a new child. Cancellation applies while queued
for the single-synthesis gate; dropping the stream also cancels and drains the
active generation. PCM received after cancellation is discarded.
The provider holds back one real PCM frame so the core-facing final chunk is
both non-empty and marked end-of-stream.

The worker permits one in-flight synthesis. Kokoro 1.13.6 emits its callback
only after a complete input fragment. The native worker splits one runtime
sentence at natural boundaries into fragments of at most 36 characters, emits
each finished fragment as PCM, and checks cancellation between fragments. A
higher cancellation generation invalidates older synthesis and ends the PCM
stream as cancelled. This is clause-incremental synthesis, not continuous
neural streaming. Process termination remains a hung-child containment fallback.

The process-boundary regression suite uses a fake executable rather than a
descriptor mock:

    cargo test -p npc-local-tts-native --features process-boundary-tests --test worker_boundaries

## Timing and lip-sync

Trust only ordered PCM frames and their 24 kHz sample count as exact audio
timing. first_pcm_ms, total_synthesis_ms, callback receipt time, and realtime
factor are process measurements. The sherpa callback fraction is progress, not
a phoneme clock.

word_alignment, phoneme_alignment, and viseme_metadata are deliberately false.
Route the PCM stream to the separately qualified lip-sync worker. Do not invent
visemes from text or callback progress inside this TTS worker.

## Resource governor

Do not admit the pack from manifest planning values or one worker run. A current
signed QualifiedResourceEnvelopeV1 must bind:

- exact pack ID and revision;
- canonical manifest SHA-256;
- tts capability;
- current device fingerprint;
- sherpa runtime and CPU backend;
- validity window and monotonic sequence;
- at least 20 samples;
- cpu_resident placement;
- resident RAM and p99 total RAM;
- zero measured resident and workspace VRAM;
- p99 first-load, reload, and operation milliseconds.

measurement.py projects this shape but cannot sign or admit it. Missing,
expired, stale, differently bound, unsigned, or null evidence fails closed.
The product measurement harness must additionally account for installed bytes,
game/desktop memory pressure, game VRAM reserve, and frame-time impact.

qualify_windows.py prepares the reviewed content-free worker observations. Plan
mode is safe before artifact installation. The real mode cannot download or
repair and requires all of the following arguments in addition to the manifest:
exact pack root, shared lock path, output report, audio-review directory,
explicit real-model acknowledgement, and coordination grant
local-tts-kokoro-real-windows-cpu. It never changes the lock. The parent task
must grant this lane only after every prior model lane has restored the lock.

## Remaining gated qualification

Real acceptance remains pending all of the following. A 2026-09-05 native run
proved executable PCM and bounded cancellation, but the earlier 20-sample suite
failed its long pre-callback cancellation deadline and minted no accepted
report or envelope:

1. The parent task explicitly queues this lane after Qwen and grants use under
   the repository lock protocol.
2. The lifecycle code downloads and verifies the two pinned upstream archives.
3. A Windows x64 process loads the exact C ABI and passes self-test, voice
   discovery, stream, cancellation, drain, unload, and reload.
4. At least 20 load, reload, and synthesis observations cover three voices and
   short, medium, and long text under game contention.
5. Audio is listened to and independently checked for corruption, clipping,
   pacing, and intelligibility.
6. Legal review resolves eSpeak GPL obligations and approves the notice/source
   delivery mechanism.
7. Model Manager creates and verifies the signed measured envelope.
8. The final packaged worker is hidden, offline, and supervised in the release
   artifact.
