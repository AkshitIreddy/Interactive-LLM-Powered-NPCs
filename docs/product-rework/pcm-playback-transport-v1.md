# Production PCM playback transport v2

The release audio path is Control → authenticated native media broker → shared-mode
WASAPI. The runtime host never opens a speaker device in release builds. Direct
runtime-host WASAPI remains a debug-only qualification feature and is not an
accepted fallback for installed builds.

## Allocation and ownership

Control first ensures the supervised runtime child is ready and obtains its
process ID. For an audio-enabled turn it requests a bounded pool of at most 16
`AllocatePlaybackStream` leases from the already authenticated broker control
channel. One lease is consumed by one `AudioSink::play` call (one delivered
sentence); it cannot be reused.

Every allocation binds all of the following before any PCM is accepted:

- broker session ID;
- turn ID and monotonic runtime generation;
- PCM S16LE sample rate and channel count;
- maximum source frames;
- exact supervised runtime producer PID;
- a broker-generated stream ID, private named-pipe endpoint, and 256-bit token;
- a monotonic allocation expiry and maximum 64 KiB PCM chunk.
- the explicitly persisted output selection (`systemDefault` or an opaque
  Windows endpoint ID), the exact resolved endpoint ID, and its generation.

Control must enumerate real broker/Windows endpoints and persist an explicit
selection before allocating audio. First-run playback fails with a typed
selection-required error until the user chooses either System Default or a
specific endpoint; System Default is never silently inferred. Every allocation
revalidates the persisted selection. A removed, disabled, unplugged, renamed, or
newly replaced endpoint, and a changed system default, fail closed before audio
submission.

Only the native Control-to-runtime request contains leases. They are never
accepted from the WebView, saved to preferences, logged, included in diagnostics,
or exposed through a Tauri command result. Token `Debug` output is always
redacted. Unused leases are cancelled when a turn ends, is superseded, crashes,
or is cancelled.

## Producer wire

Each request and response is a little-endian `u32` body length followed by a
bounded binary body. The native reference codec is
`playback_transport.cpp`; `maximum_wire_frame_bytes` is 69,632 bytes. V1 is
intentionally rejected because it cannot prove the selected output endpoint.

Request body:

| Field | Encoding |
| --- | --- |
| magic | four bytes `NPCP` |
| schema version | `u32`, exactly 2 |
| command | `u16`: Begin=1, Chunk=2, Finish=3, Cancel=4 |
| reserved | `u16`, exactly 0 |
| sequence | `u64`, strictly increasing |
| deadline QPC | `u64`, no more than five seconds ahead |
| generation | `u64`, exact lease generation |
| stream/session/turn IDs | each `u16` byte length + 1..128 UTF-8 bytes |
| token | exactly 32 bytes |
| payload length | `u32` |
| payload | PCM only for Chunk; empty for every other command |

Chunk PCM must be non-empty, frame-aligned PCM S16LE, at most 65,536 bytes,
within the lease frame budget, and fit the two-second SPSC ring. `backpressure`
accepts zero frames; the producer retries the same PCM with a new sequence and a
fresh deadline. Reusing the old sequence is a replay and is rejected.

Response body:

| Field | Encoding |
| --- | --- |
| magic | four bytes `NPCR` |
| schema version | `u32`, exactly 1 |
| status | `u16` typed status |
| has receipt | `u16`, 0 or 1 |
| response sequence | `u64` |
| accepted source frames | `u64` |
| optional receipt | fixed numeric fields plus bounded IDs |

The v2 receipt ends with output selection mode (`u8`: SystemDefault=1,
EndpointId=2), opaque endpoint ID (`u16` byte length + at most 1,024 UTF-8
bytes), and endpoint generation (`u64`).

The final `PlaybackReceipt` includes receipt/stream/session/turn identity,
generation, exact source frames, exact frames submitted to the WASAPI source
format, source duration in microseconds, `sourceSubmissionComplete`,
`endpointDrainComplete`, `cancelled`, and the exact output selection mode,
endpoint ID, and generation copied from the consumed lease. A turn may claim audio delivery only
when source submission and endpoint drain are both true and cancellation is
false. This is device-submission evidence, not a claim that a physical speaker
was audible.

## Security and bounded failure behavior

The per-stream named pipe has a current-user-only ACL, rejects remote clients,
permits one connection, and verifies the kernel-reported pipe client PID plus
same-user/session process tokens. The token comparison is constant-time. A valid
Begin consumes the token for that endpoint; identity or authentication failures
close it. Commands are ordered and deadline-bound. The total connected lifetime
is bounded to twice the declared maximum source duration plus 15 seconds.

The broker keeps at most 16 live endpoints. The ring holds no more than two
seconds of source PCM. Cancellation clears queued PCM and flushes the WASAPI
client. Producer exit, pipe disconnect, parent death, broker shutdown, turn
supersession, drain timeout, and device loss all clean up the endpoint and return
or preserve a non-success terminal receipt where a producer is still connected.

Portable tests cover framing, malformed sizes, authentication, exact producer
PID, identity binding, replay, deadlines, frame budgets, backpressure,
cancellation, and receipt round trips. The Windows smoke sends 2,400 frames of
24 kHz mono PCM through the real one-time pipe and requires exactly 2,400 source
and device frames, 100,000 microseconds, source completion, and endpoint drain.
