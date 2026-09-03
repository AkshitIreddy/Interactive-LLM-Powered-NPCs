# Optional local TTS candidate

This isolated lane implements a fail-closed, opt-in local text-to-speech pack
using the shared strict npc.model-pack/v2 schema.
The candidate is Kokoro-82M v1.0 INT8 through the sherpa-onnx 1.13.6 C API on
Windows x64 CPU. It streams 24 kHz mono signed 16-bit PCM and exposes 28 audited
American and British English stock voices.

No model or runtime payload was downloaded or executed while this lane was
built. The checked-in manifest is an integration candidate, not a trusted
catalog entry, and activation remains blocked until a current-device benchmark
has been reviewed, bound to the exact manifest, and signed by Model Manager.

## Exact identity

Manifest:

    packaging/model-packs/kokoro-sherpa-onnx-v1.0-int8-windows-x64.json

Pack ID:

    local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64

Revision:

    2026.08.30-r1

Capability:

    tts

The manifest pins both upstream archives by URL, byte size, and SHA-256, plus
the critical model, voices, token, lexicon, eSpeak data, and model-license
files. Run the read-only plan command to display the canonical manifest digest
and acknowledgements required by the lifecycle command:

    python workers/local-tts/pack_manager.py plan \
      --manifest packaging/model-packs/kokoro-sherpa-onnx-v1.0-int8-windows-x64.json \
      --root C:/path/to/model-packs

Install and repair additionally require the exact trusted manifest digest,
confirmed pack ID, confirmed revision, and acknowledgement of every disclosed
model, voice-training, runtime, phonemizer, and ONNX Runtime license entry.
They stream to an exclusive temporary file, verify size and SHA-256, reject
unsafe archive members, verify critical extracted files, write a complete
inventory, and activate atomically. Verify rehashes the full installed
inventory. Remove requires the exact pack-at-revision token.

The project must not mirror these artifacts until legal review approves the
full notice/source bundle. Direct upstream download is the only candidate route
in this revision.

## Worker boundary

worker.py is a supervised offline sidecar. Production has no fixture switch,
downloader, HTTP server, arbitrary model path, reference-audio input, or
voice-embedding input. It re-verifies the installed pack before loading, then
installs a deny-network audit hook. The supervisor must still enforce a
restricted token, deny-egress policy, current-user-only named pipes, and a
kill-on-close job object.

The control plane uses bounded length-prefixed JSON with monotonically
increasing sequence numbers and cancellation generations. PCM travels only on
a separate supervisor-created endpoint using NPCTTS01 binary records. Each PCM
chunk carries an exact output sample clock, SHA-256, stream ID, generation, and
clipping count. The terminal PCM record is the authoritative drain barrier.

Supported operations are handshake, capabilities, discover_voices, health,
load, synthesize, self_test, cancel, unload, and shutdown. Only one synthesis
is active at once. Cancellation advances the generation and stops the native
callback; unload is refused until the PCM stream drains.

The pinned API does not expose trustworthy word, phoneme, or viseme timestamps.
The worker therefore reports those features as unavailable. Downstream lip-sync
must derive animation from the streamed audio in its own qualified model rather
than relabel callback progress as facial timing.

## Qualification hook

measurement.py accepts content-free observations from an external supervisor.
It requires at least 20 independent loads, 20 reloads, and 20 synthesis runs
covering at least three voices and short, medium, and long text, plus a
cancellation trial. It projects every frozen PlacementMeasurementV1 field,
including the separately measured p99 reload cost, but always marks its output
unsigned and non-admissible.

Model Manager alone supplies the device fingerprint, validity window, monotonic
sequence, game reserve measurements, signature, and final
QualifiedResourceEnvelopeV1.

qualify_windows.py is the gated real Windows CPU harness. Its plan mode is
model-free. A real run has no download path and requires an already verified
install, explicit acknowledgement, the exact parent grant token, and the shared
AI-model lock. It records at least 20 independent loads, reloads, and streamed
syntheses; target-process RAM and CPU; zero NVIDIA compute VRAM for the exact
PID; first playable PCM; full realtime factor; cancellation and drain; unload;
all 28 typed voices; and three WAV files for human listening. Its report remains
unsigned and non-admissible for Model Manager review.

## Fixture verification

These tests synthesize only a deterministic in-memory sine fixture and use tiny
local tar archives. They never fetch or execute the real candidate:

    python -m unittest discover -s workers/local-tts/tests -v

See docs/INTEGRATION_HANDOFF.md for the product wiring contract and
docs/RESEARCH_LEDGER.md for the source/license decision record.
