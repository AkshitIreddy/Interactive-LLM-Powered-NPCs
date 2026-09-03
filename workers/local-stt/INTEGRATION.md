# Local STT integration handoff

This is the exact boundary for integrating the optional Moonshine worker into
the shared supervisor and model manager. The pack remains an unqualified
candidate until all qualification steps below produce signed evidence on the
target PC. Fixture success must never make it selectable.

## Immutable identity

| Field | Value |
| --- | --- |
| Candidate manifest | `packaging/model-packs/moonshine-v2-medium-streaming-en-win-x64-v0.1.5.json` |
| Pack ID | `npc.stt.moonshine-v2-medium-streaming-en.win-x64` |
| Revision | `0.1.5-moonshine.234f60f` |
| Capability | `speech_recognition` / generic STT |
| Upstream tag | `v0.1.5` |
| Upstream commit | `234f60faa0eb388b01cdf7e60aca232af37aefda` |
| Release artifact | `windows-cli-transcriber.tar.gz` |
| Artifact bytes | `402991031` |
| Artifact SHA-256 | `a56bcd27765fefa4ab9a9b219cbb26e6de7d48b6536c88b92b6d24ffa7eb4c25` |
| Model architecture | `MOONSHINE_MODEL_ARCH_MEDIUM_STREAMING` (`5`) |
| Runtime backend | Windows x64 CPU only |
| License | MIT; notice and explicit download acceptance required |

The artifact size and digest are published by the tagged GitHub release. The
tagged `LICENSE` says all streaming STT models and all English models are MIT;
the non-MIT exceptions are legacy non-streaming models in named non-English
languages. Do not broaden this pack to those exceptions. The release archive
does not contain that repository license; display and install the exact vendored
`workers/local-stt/THIRD_PARTY_LICENSES/MOONSHINE-v0.1.5-LICENSE.txt` whose
SHA-256 is `fa7d1174dd8af6a7cd280be20b80d10095ed4c19b5b20b61a7715c3ad790dc5f`.

The JSON is the shared model manager's strict canonical
`npc.model-pack/v2` envelope and must be parsed as `ModelPackManifestV2`.
Preserve the exact pack ID, revision, artifact digest, capability, generic
scope, license, and fail-closed unmeasured state. Bind signed measurement
evidence to the manager's canonical manifest digest. The real qualification
tool additionally records the raw manifest SHA-256 for forensic reproducibility;
that raw-file digest is not a substitute for the canonical admission identity.

## Installation lifecycle

The UI first displays artifact size, MIT notice, disk requirement, and the fact
that installation does not activate the route. Only an explicit confirmation
may call:

```text
pack_cli.py install MANIFEST --root DEDICATED_PACK_ROOT \
  --confirm-license-and-download YES
```

The lifecycle implementation downloads to `.part`, resumes only with HTTP 206,
checks exact byte length and SHA-256, rejects unsafe tar members and extraction
bombs, verifies the required file list, retains the immutable archive for
offline repair, inventories every installed file with SHA-256, writes a durable
receipt, then atomically renames the staging directory into place. `verify`
checks the retained archive and exact inventory, including rejection of added
files. `repair` quarantines the bad installation before reinstalling. `remove`
renames the exact pack revision into quarantine and deletes only that resolved
revision. Calls are serialized with a per-pack OS lock.

The model manager must hold no active lease while repair/remove runs. It must
cancel a session, unload the worker, release mapped DLLs, then perform the
lifecycle operation. Never install under an unvalidated path or a broad root.

## Native bridge build

After verified extraction and before activation, build only from
`workers/local-stt/native` using the pinned archive's SDK:

```powershell
cmake -S workers/local-stt/native -B BUILD_DIR -G "Visual Studio 17 2022" -A x64 `
  -DNPC_MOONSHINE_SDK_ROOT=PACK_ROOT\cli-transcriber\moonshine-voice-windows-x86_64
cmake --build BUILD_DIR --config Release --target npc-moonshine-bridge
```

Record the bridge DLL and copied `onnxruntime.dll` hashes in the installed
receipt. Check `npc_stt_bridge_abi_version() == 1` before creating a model. The
bridge pins the official CPU provider, disables input WAV saving, disables
speaker identification, enables incomplete-line decoding and speculative
decoding, and requests word timestamps. The model path is
`PACK_ROOT\cli-transcriber\models\medium-streaming-en` and architecture is 5.

## Hidden supervised process

Use `pythonw.exe` (or a packaged windowless executable), `CREATE_NO_WINDOW`, a
restricted token, and a Windows Job Object with kill-on-close, one-process,
memory, and CPU limits. Permit only the verified pack and worker files; deny
network access for the inference process. The supervisor creates all IPC before
launch and passes only inheritable child ends. Do not pass secrets or audio on
the command line.

Required environment:

```text
NPC_STT_WORKER_INSTANCE_ID=<fresh bounded identifier>
NPC_STT_LAUNCH_NONCE=<fresh high-entropy secret>
NPC_STT_PCM_HANDLE=<decimal inherited one-way read handle>
NPC_STT_BRIDGE_PATH=<verified absolute bridge DLL>
NPC_STT_MODEL_PATH=<verified absolute medium-streaming-en directory>
```

Wire a current-user-only named pipe to the child's stdin/stdout handles. Each
control message is `u32be byte_length || UTF-8 JSON`; the schema is
`protocol-v1.schema.json`. Create a separate anonymous one-way pipe for PCM.
Each PCM frame is `u32be metadata_length || u32be pcm_length || metadata JSON ||
pcm_s16le bytes`. The media broker owns microphone consent and capture; the
worker never opens or enumerates a microphone.

Start with `handshake`, validating the fresh nonce and worker instance. Then
`warm`, acquire a model-manager lease, and `load` using exact pack ID, revision,
and lease ID. A stale instance, duplicate request, non-increasing sequence,
expired deadline, or generation mismatch is rejected.

## Turn flow and event mapping

1. Media broker captures mono signed 16-bit PCM, preferably 16 kHz. It writes a
   bounded PCM frame with a contiguous chunk index and QPC capture timestamp.
2. Supervisor sends `pcm_commit` for that chunk ID only after the write
   succeeds. The worker never accepts audio inside control JSON.
3. Worker emits `utterance_started`, stable `partial_transcript` updates,
   `final_transcript`, optional word timestamps, and `utterance_ended`.
4. For push-to-talk, key-up sends `session_end` with `ptt_key_up`; this is the
   authoritative flush. For open microphone, the broker/runtime may send
   `vad_end_of_turn` after its configured end-of-turn rule. Moonshine's own VAD
   also marks completed lines, but it cannot override explicit PTT key-up.
5. Do not send a player utterance to the LLM until `final_transcript` for the
   current generation. UI may show partials but must replace them by stable
   utterance ID.

For barge-in, route change, deadline, shutdown, or user cancellation, increment
the monotonic generation and send `cancel`. The worker frees the stream and
clears buffered PCM. The supervisor drops every event with an older generation.
An equal-generation cancel is idempotent. Create a fresh session after a real
generation barrier.

The worker supports only one session. Backpressure is explicit at eight queued
chunks and 30 seconds declared maximum buffered audio. The supervisor should
pause capture forwarding or fall back to the configured API STT route rather
than growing an unbounded queue.

## Resource qualification and admission

No zero, estimate, fixture, vendor claim, or policy-only value is qualified
evidence. After the API routes pass, the Qwen lane releases, and the shared GPU
lock protocol authorizes this lane, run at least 20 repetitions of a rights-clear
speech fixture on the target PC. Even though this route is CPU-only, account for
game RAM/VRAM and concurrent selected local models.

Capture cold load, reload, resident and p99 total RAM, DXGI/NVML-confirmed
resident and p99 workspace VRAM (expected zero, but measured externally), p99
operation latency, first partial after first PCM, final after end-of-turn,
real-time factor, CPU use, long-session drift, cancellation latency, and a
known-transcript accuracy check. Preserve raw sample records and hardware,
runtime, bridge, model, and benchmark-suite identities. Do not log PCM or
transcript text in diagnostics.

Convert the results into the shared manager's measured-resource envelope:

```text
schema = "npc.measured-resource-envelope/v1"
identity = PackRevision(exact pack ID, exact revision)
capability = ModelPackKindV1::SpeechRecognition
manifest_sha256 = ModelPackManifestV2 canonical digest
runtime = supervised Python/windowless worker identity
runtime_revision = worker + bridge revision
backend = moonshine-v0.1.5-native-cpu
sample_count >= 20
placements[CpuResident] = {
  resident_ram_bytes,
  p99_total_ram_bytes,
  resident_vram_bytes,
  p99_workspace_vram_bytes,
  p99_load_millis,
  p99_reload_millis,
  p99_operation_millis
}
```

Also populate report ID, monotonic sequence, measurement/expiry timestamps,
device fingerprint, and benchmark-suite revision. Sign through the existing
catalog signature path. Only a verified `QualifiedResourceEnvelopeV1` plus a
successful whole-loadout admission may clear `candidate_unqualified` and make
the pack selectable. The current manifest deliberately leaves RAM/load/latency
fields null and the default route false.

## Focused verification before shared wiring

These checks are safe and model-free:

```powershell
py -3 -m unittest discover -s workers/local-stt/tests -v
py -3 workers/local-stt/worker.py --self-test-fixture
py -3 workers/local-stt/pack_cli.py inspect `
  packaging/model-packs/moonshine-v2-medium-streaming-en-win-x64-v0.1.5.json
```

After authorization, add evidence for: real archive install/verify/repair/remove,
Release bridge build, ABI check, known WAV transcription, partial/final/word
timing flow, VAD and PTT ends, cancellation under load, 20-sample resource run,
and a combined game plus local-model admission scenario. Until all of those pass,
keep API STT as the default and show this local pack as unavailable for routing.

The development-PC qualification completed 21 real cycles, including 20
reloads. Its unsigned evidence is at
`%LOCALAPPDATA%\InteractiveNPCs\out\moonshine-qualification-20260830\evidence`.
The run verified a stable known transcript, partial/final/VAD/PTT/cancel flow,
word timestamps, terminal unload, measured CPU/RSS, and zero Moonshine-process
VRAM according to NVIDIA process telemetry. It deliberately leaves
`admissionEligible=false`; catalog signing and combined-game/loadout admission
remain external gates.
