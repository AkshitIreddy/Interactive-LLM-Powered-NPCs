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
| ABI | npc-local-tts-sherpa-c-api-v1 |
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

Launch the reviewed/frozen worker without a visible console. On Windows use a
GUI-subsystem wrapper or CREATE_NO_WINDOW, a restricted token, deny-egress
policy, and a kill-on-close job object. Create three current-user-only,
reject-remote-client named pipes before launch.

Required arguments:

    --manifest <trusted manifest path>
    --pack-root <exact installed revision directory>
    --expected-manifest-sha256 <canonical catalog digest>
    --launch-nonce <random per-launch secret>
    --worker-instance-id <opaque per-launch ID>
    --input-pipe <supervisor control-input pipe>
    --output-pipe <supervisor control-output pipe>
    --pcm-output-pipe <supervisor PCM pipe>

The worker checks the manifest digest, exact root identity, installation
inventory, runtime version, runtime Git SHA, ONNX Runtime version, 24 kHz sample
rate, and at least 53 embedded speakers. Its public discovery remains limited
to speaker IDs 0 through 27.

Perform handshake first with generation zero and the launch nonce. Then issue
load with exactly:

    {
      "pack_id": "local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64",
      "revision": "2026.08.30-r1",
      "lease_id": "<opaque model-manager lease>",
      "num_threads": 2
    }

## Audio transport and scheduling

Control framing is four-byte unsigned big-endian length followed by bounded
UTF-8 JSON for npc.local-tts-worker/v1.

PCM framing is:

    NPCTTS01
    uint32be header_bytes
    uint32be pcm_bytes
    canonical UTF-8 JSON header
    pcm_s16le payload

Preserve stream ID, request ID, generation, chunk sequence, sample_start,
frame_count, sample rate, and payload SHA-256. Refuse a reused stream ID. Treat
the end record, not the cancel response, as the authoritative stream drain
barrier. Only then release the audio buffer or issue unload.

The worker permits one in-flight synthesis. A higher cancellation generation
invalidates older synthesis, returns zero from the native callback, and ends
the PCM stream as cancelled. Resource pressure should cancel and drain speech
before unload. Never terminate the process as a routine cancellation mechanism;
reserve job-object termination for a hung or compromised sidecar.

## Timing and lip-sync

Trust only sample_start and frame_count as exact timing. first_pcm_ms,
total_synthesis_ms, callback receipt time, and realtime factor are process
measurements. The sherpa callback fraction is progress, not a phoneme clock.

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

Real acceptance remains pending all of the following:

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
