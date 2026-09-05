# Optional local TTS candidate

This isolated lane implements a fail-closed, opt-in local text-to-speech pack
using the shared strict npc.model-pack/v2 schema.
The candidate is Kokoro-82M v1.0 INT8 through the sherpa-onnx 1.13.6 C API on
Windows x64 CPU. It streams 24 kHz mono signed 16-bit PCM and exposes 28 audited
American and British English stock voices.

The pinned model/runtime payload was downloaded to `E:\temp` and executed on
Windows on 2026-09-05. Archive and critical-file verification passed, and the
new Rust `npc-local-tts-native` provider emitted real PCM through its supervised
native worker. The checked-in manifest remains an integration candidate, and
activation stays blocked because the 20-sample qualification failed its
pre-callback cancellation deadline and no signed resource envelope exists.

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

`worker.py` remains the qualification harness and protocol reference. The
optional native candidate uses `npc-local-tts-native::KokoroLocalProvider`, which launches
`npc-local-tts-native-worker.exe` hidden and keeps the verified CPU model warm
across turn sessions. The native worker has no downloader, HTTP client,
arbitrary model path, reference-audio input, or voice-embedding input.

The native route uses bounded length-prefixed JSON commands and bounded framed
JSON-plus-PCM responses over private stdio. It validates request IDs, sequences,
format, and terminal state. It holds back one real PCM frame so the final
core-facing audio chunk is non-empty. Pipe writes and response inactivity are
deadline-bounded; malformed, hung, or internally rejected children are retired
and replaced on the next synthesis. Cancellation applies while waiting for the
single-synthesis gate; dropping the stream also cancels and drains the active
generation. PCM arriving after cancellation is discarded.

Only one native synthesis is active at once. The sherpa Kokoro callback arrives
after a full input fragment, so the native worker generates bounded natural
clauses and emits PCM after each clause. Cancellation advances a provider-local
generation and drains after the current clause. This is honest clause-level
incremental delivery, not continuous neural streaming.

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

The ordinary Python tests synthesize only a deterministic in-memory sine
fixture and use tiny local tar archives:

    python -m unittest discover -s workers/local-tts/tests -v

The ignored Rust integration test requires the separately installed verified
pack and exercises the real native provider, PCM framing, persistent warm
worker, voice pin, signal checks, and cancellation:

    cargo test --release -p npc-local-tts-native --test real_kokoro -- --ignored --nocapture

The fake-worker process suite tests cancellation, non-empty terminal PCM,
malformed frames, hung reads, child retirement, and transparent restart without
requiring model assets:

    cargo test -p npc-local-tts-native --features process-boundary-tests --test worker_boundaries

See docs/INTEGRATION_HANDOFF.md for the product wiring contract and
docs/RESEARCH_LEDGER.md for the source/license decision record.
