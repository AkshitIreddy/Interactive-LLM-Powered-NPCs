# Process and supervision model

## Trust and failure domains

| Process | Privilege/data | Must not receive | Restart consequence |
| --- | --- | --- | --- |
| Tauri shell | Public configuration, status, redacted diagnostics | API-key values, continuous media, model tensors | UI reconnects to runtime; active audio may continue by policy. |
| Rust runtime | Session/state, credential references, prompt/context, provider connections | Raw credentials in logs/UI messages | Active turn is cancelled; persisted delivered history survives. |
| Media broker | PCM, captured textures, HWND/monitor geometry, hotkey | Credentials, lore, prompts, database | Visual/audio device state is rebuilt; text session survives. |
| Native lip-sync worker | Explicitly selected/attested pack and typed frame/audio data | Global settings, unrelated sessions, credentials | Optional animation degrades; audio/subtitles continue. |
| Python lip-sync worker | Same bounded visual request inside an isolated user-selected pack | System Python/site-packages, profile code, credentials | Optional animation is quarantined after bounded restart failures. |

All child processes join a Windows Job Object configured to close descendants when the owning runtime exits. Each process receives a per-launch nonce and least-privilege handles. Process ACLs and named-pipe ACLs are restricted to the current user SID; production builds reject remote pipe clients.

## Envelope and stream rules

Every length-delimited Protobuf `EnvelopeV1` contains:

```text
protocol_version
launch_nonce
session_id
turn_id
sequence
qpc_timestamp
deadline_qpc
cancellation_generation
payload_kind
payload_size
trace_id
typed_error? {subsystem, code, retryable, safe_user_message}
```

Rules:

- reject an unknown major protocol version, nonce, session or payload kind;
- negotiate compatible minor capabilities during `Hello`/`HelloAck`;
- enforce byte, rate and in-flight limits before decoding/allocating;
- require monotonically increasing sequence per stream and discard duplicates;
- discard expired or older-cancellation-generation messages without side effects;
- never place a secret value in an envelope sent to the UI, media broker or model worker;
- use explicit end/cancel/error records—pipe closure is not a successful end;
- apply backpressure to lossless streams and drop stale frames/telemetry only on declared lossy channels.

PCM travels through a bounded shared-memory ring whose ownership and cursors are exchanged over the pipe. Textures use shared D3D handles only for allowlisted producer/consumer combinations; otherwise the broker performs an explicit copy. Handles are duplicated directly into the target process, tagged with generation, and closed on cancellation/restart.

## Worker lifecycle

```text
Absent → Starting → Handshake → Loading → Ready → Busy
   ▲         │          │          │       │      │
   └─────────┴──────────┴──────────┴───────┴──────┘ restart/failure
                                      │
                         Draining → Stopped
                                      │
                         Quarantined (circuit open)
```

1. The supervisor launches an immutable executable from an activated, hash-verified pack.
2. Handshake validates executable/pack/runtime ABI and capabilities.
3. Loading has a model-specific deadline and reports measured memory use.
4. Ready workers send heartbeats with queue depth and resource envelope.
5. A lease moves a worker to Busy; cancellation must acknowledge and release resources.
6. Shutdown first drains bounded work, then requests graceful stop, then terminates through the Job Object after a deadline.

Crashes use exponential backoff with jitter and a finite rolling-window budget. A repeated crash opens a circuit for the feature/model pack, records a diagnostic, and invokes the degradation ladder. It does not repeatedly relaunch under game load.

## Turn concurrency

- At most one optional lip-sync worker holds its declared GPU lease; conversation APIs do not consume a local model lease.
- Lightweight non-model endpointing and SQLite FTS may remain CPU-resident.
- TTS may overlap ongoing LLM streaming when its measured resource class fits the active budget.
- Continuous vision uses a latest-frame mailbox rather than an unbounded queue.
- Optional burst GPU work must obtain an exclusive or compatible lease from the resource broker.
- Barge-in increments the turn cancellation generation before starting new audio, so late output cannot leak into the new turn.

## Restart and recovery expectations

Kill tests cover every worker at every Response Spine stage. At most the in-flight, not-yet-delivered portion of a turn may be lost. The next turn must either succeed or reach the documented fallback without an orphan process, locked database, stuck audio device, leaked texture, or false committed reply.

The runtime records clean shutdown/recovery markers. On restart it:

1. rolls back incomplete database transactions;
2. marks started-but-undelivered outputs aborted;
3. reclaims stale staging/model-download leases;
4. revalidates active pack manifests;
5. re-enumerates audio/display/game state;
6. resumes only explicit resumable downloads—not live AI turns.
