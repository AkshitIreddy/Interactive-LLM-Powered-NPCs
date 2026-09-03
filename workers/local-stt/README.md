# Optional local STT worker

This directory contains the isolated production-shaped local speech-to-text
worker for the optional Moonshine v2 route. It is deliberately not wired into
the shared runtime yet. The integration boundary is documented in
[`INTEGRATION.md`](INTEGRATION.md).

The selected candidate is Moonshine Voice v0.1.5 Medium Streaming English on
Windows x64, CPU only. It is packaged as an explicit user download, never as a
silent dependency. The upstream release archive is pinned by its GitHub
release SHA-256 and includes the runtime libraries, ONNX Runtime, an English
streaming model, and the upstream Windows smoke application. The model and
runtime are MIT-licensed.

The tagged upstream license is vendored byte-for-byte at
[`THIRD_PARTY_LICENSES/MOONSHINE-v0.1.5-LICENSE.txt`](THIRD_PARTY_LICENSES/MOONSHINE-v0.1.5-LICENSE.txt)
with SHA-256
`fa7d1174dd8af6a7cd280be20b80d10095ed4c19b5b20b61a7715c3ad790dc5f`.
The release archive itself does not include the repository license, so the UI
and installed-pack notice must use this pinned local copy.

## What is implemented

- bounded, replay-resistant `npc.local-stt/v1` control envelopes;
- a supervisor-owned hidden worker process and a separate inherited PCM handle;
- 16-bit mono PCM streaming with exact sample-clock timestamps;
- partial/final transcript events, stable utterance IDs, optional word timing,
  PTT key-up and explicit flush semantics;
- monotonic cancellation generations and stale-result suppression;
- Moonshine C-API bridge source plus a deterministic fixture backend;
- install, verify, repair, remove, quarantine, and receipt machinery;
- load, RSS, CPU, real-time-factor, first-partial, final-after-end, and upstream
  latency measurement hooks;
- fail-closed qualification: unmeasured packs cannot be activated.

Fixture tests exercise the complete protocol without requiring model weights.
After the hosted API qualification and exclusive local-model authorization, the
pinned archive was also installed and verified on the development PC, the
Release bridge was built, and 21 bounded real inference/lifecycle cycles were
recorded. That machine-local, unsigned evidence remains outside the source tree
under `%LOCALAPPDATA%\InteractiveNPCs\out`; it does not make the pack generally
qualified or selectable on a different machine.

## Development checks

```powershell
py -3 -m unittest discover -s workers/local-stt/tests -v
py -3 workers/local-stt/worker.py --self-test-fixture
py -3 workers/local-stt/pack_cli.py inspect `
  packaging/model-packs/moonshine-v2-medium-streaming-en-win-x64-v0.1.5.json
```

Real installation and qualification are intentionally separate:

```powershell
py -3 workers/local-stt/pack_cli.py install MANIFEST --root PACK_ROOT `
  --confirm-license-and-download YES
py -3 workers/local-stt/pack_cli.py verify MANIFEST --root PACK_ROOT
py -3 workers/local-stt/qualify_real.py --bridge BRIDGE_DLL --model MODEL_DIR `
  --wav RIGHTS_CLEAR_WAV --manifest MANIFEST --gpu-lock GPU_LOCK `
  --output LOCALAPPDATA_OUT --cycles 21 --pace-audio --authorized YES
```

Installation does not imply activation. Activation remains blocked until the
native bridge is built, its self-test passes, a real speech fixture is
transcribed, and signed resource/latency evidence is recorded for the current
hardware.

## Security and privacy invariants

- The worker does not enumerate or open microphones. The media broker owns user
  consent and capture, then writes PCM to an inherited one-way handle.
- Model downloads occur only in the lifecycle command after explicit user
  confirmation. The inference worker has no network requirement.
- Production control messages never contain audio bytes, filesystem paths, OS
  handles, or transcript fixture hints.
- Raw audio is not written to disk. Diagnostic events carry counts and timings,
  never PCM or transcript text.
- Archive extraction rejects absolute paths, traversal, links, devices, excess
  entries, and expanded-size bombs before making an install visible.

See [`RESEARCH.md`](RESEARCH.md) for the model decision and source ledger.
