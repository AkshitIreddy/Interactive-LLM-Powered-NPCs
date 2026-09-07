# Process and supervision model

Status: integrated process boundaries with role- and route-specific qualification

## Trust and failure domains

| Process | May receive | Must not receive | Current qualification |
| --- | --- | --- | --- |
| Tauri shell/WebView | Public configuration, redacted readiness, commands, status, receipts | Credential values, continuous media, actor coordinates, embeddings, D3D handles, tensors | Source integrated and headless browser-reviewed; installed/native presentation review open |
| Rust control/runtime host | Session/turn state, credential references, prompt context, provider connections, delivery receipts | Secret values in logs/UI envelopes | Source integrated; complete ordinary selected route remains open |
| C++ media broker | PCM, HWND/display geometry, captured textures, hotkey and presentation requests | Credentials, lore, prompts, database, provider/network authority | Native source integrated; physical display/commercial-game matrix open |
| Native/local role worker | Exact admitted pack plus bounded role-specific request and lease | Global settings, unrelated sessions, credential values, arbitrary files/code | Per role and pack; no catalog candidate is implicitly qualified |
| Optional CPython pack runtime | Same pack-scoped request inside a self-contained explicitly installed pack | System Python/site-packages, profile code, credentials, unrestricted filesystem | Target capability only where a qualified pack requires it |

Child processes are launched without visible consoles and assigned to a Windows Job Object
so owner shutdown closes descendants. Native broker pipes verify the expected client and
current user/session. Each process has a per-launch nonce and least-privilege handles.

## Wire format

The current control/runtime sidecar protocol is deliberately hybrid:

```text
length prefix
└─ protobuf EnvelopeV1
   ├─ protocol/launch/session/turn/sequence/deadline/cancellation/trace metadata
   ├─ payload kind and declared size
   ├─ typed transport error when present
   └─ ControlRequestV1 or response wrapper
      └─ payload_json: versioned validated business request/result
```

Do not describe this as a fully protobuf-typed application protocol. Protobuf establishes
the transport envelope; Serde JSON currently carries business fields. Both layers require
limits and compatibility tests.

Rules:

- reject unknown major protocol versions, launch nonce, session, payload kind, or
  business schema version;
- negotiate compatible minor capabilities during handshake;
- enforce byte, rate, depth, and in-flight limits before large allocation;
- require monotonic sequence within a declared stream and discard duplicates;
- discard expired or older-cancellation-generation work without side effects;
- reject unknown/duplicate security-sensitive JSON fields where the target schema does;
- never place credential values in UI, broker, worker, log, or diagnostic payloads;
- use explicit end/cancel/error records; pipe closure is never successful completion;
- use backpressure for lossless streams and drop only declared lossy stale frame/telemetry
  channels.

PCM may travel through a bounded shared-memory ring after the pipe exchanges ownership
and cursors. D3D textures use shared handles only for allowlisted producer/consumer pairs
with qualified interop and lifetime behavior. Other paths copy or remain unavailable.

## Supervised child lifecycle

```text
Absent → Starting → Handshake → Loading → Ready → Busy
   ▲         │          │          │       │      │
   └─────────┴──────────┴──────────┴───────┴──────┘ failure/restart
                                      │
                         Draining → Stopped
                                      │
                         Quarantined (circuit open)
```

1. The supervisor launches an immutable executable from the application bundle or an
   explicitly activated hash-verified pack.
2. Handshake validates executable identity, pack/runtime ABI, role, and capabilities.
3. Loading has a role-specific deadline. Readiness does not establish measured resource
   safety by itself.
4. A lease admits bounded work. Cancellation acknowledges and releases resources.
5. Shutdown drains bounded work, requests graceful stop, then terminates the Job Object
   after a deadline.
6. Repeated crashes open a feature/pack circuit and select the declared fallback rather
   than relaunching indefinitely under game load.

This lifecycle is a reusable target contract. Only a worker whose executable route,
manifest, measurement, host join, and end-to-end evidence exist may be called integrated.

## Local-resource admission

Two implemented layers must be distinguished:

1. Model Manager validates signed manifests and measured per-placement/whole-loadout
   preflight, including p99 RAM, resident and transient VRAM, load/reload/operation cost,
   desktop use, live game use, and configured game reserve.
2. Runtime-core ResourceBroker has an opt-in TurnSupervisor constructor driven by a
   `TurnResourcePlanner`. It can deny the turn before provider work, own the resource job,
   and release it on completion or cancellation.

The normal runtime host does not yet wire that broker path because it lacks a trusted
native-stamped selected-game budget and a qualified whole-turn resource plan delivered
across the process boundary. `ResourceGovernorV1` preflight and runtime-core unit evidence
must not be described as production resource enforcement. The production join must:

- bind budget evidence to target HWND, adapter, telemetry generation, and timestamp;
- bind every estimate to selected route, exact pack revision, placement, and measurement
  provenance;
- protect the larger of observed game use and the configured game reserve plus desktop
  use;
- account for simultaneous resident and p99 transient allocations for the whole turn;
- fail before local execution when evidence is missing/stale or the reserve is short;
- release/cancel without evicting the selected game or silently changing provider/device.

## Turn concurrency

- A provider-route snapshot is immutable for the turn.
- Optional local jobs run only after explicit pack selection and role-specific admission.
- Lightweight endpointing and SQLite FTS may remain CPU resident within measured bounds.
- TTS may overlap LLM sentence generation only when the active route/resource plan admits
  it.
- Continuous vision uses a latest-frame mailbox, not an unbounded queue.
- Visual work carries actor lock, frame, geometry, audio-clock, deadline, and cancellation
  generations; stale work is dropped.
- Barge-in increments cancellation before new output, preventing old audio, subtitles, or
  optional visuals from entering the new turn.

The current production actor-lock bus begins unqualified. A private YuNet/LM1 catalog is
measured and signed, and its provider-load lifecycle passed in an isolated state with a
fresh authenticated worker and exact active inventory. Setup activation cannot authorize
live rendering, normal user-state installation and whole-loadout admission remain open,
and identity-bound vision/lip-sync therefore cannot obtain product authority. Authored
commercial profiles are also
console-isolated; normal capture and overlay require separate qualification.

## Effects boundary

Effects are optional validated data proposals, not a process with game authority. Current
ordinary simulation constructs fixture effects. No process may turn them into keyboard,
mouse, memory, script, executable, or injected game actions. Unsupported or invalid
proposals neutralize without delaying spoken output. No demographic or clinical inference
process exists in this architecture.

## Restart and recovery expectations

Kill tests must eventually cover every integrated worker and each visible turn stage. At
most the undelivered part of an in-flight turn may be lost. The next turn reaches success
or its declared fallback without an orphan process, locked database, stuck audio device,
leaked texture, resource lease, or false committed reply.

On restart the runtime:

1. rolls back incomplete database transactions;
2. marks started but undelivered outputs aborted;
3. reclaims stale staging, download, media, and resource leases;
4. revalidates active pack manifests;
5. re-enumerates audio, display, and game targets;
6. resets identity/visual authority until fresh trusted evidence exists;
7. resumes only explicit resumable downloads, never live AI turns.

These are acceptance expectations where a current installed crash/recovery report is not
yet named; they must not be inferred from the state-machine design alone.
