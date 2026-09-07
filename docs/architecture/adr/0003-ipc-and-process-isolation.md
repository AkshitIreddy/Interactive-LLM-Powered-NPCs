# ADR-0003: Protobuf envelopes with versioned JSON payloads over SID-restricted named pipes

Status: Accepted; wire-format description reconciled with current source

Date: 2026-08-28

Updated: 2026-09-05

## Context

Realtime Windows media, Rust orchestration, and optional local role workers need
independent crash and resource boundaries. Unframed JSON over stdout lacks transport
identity, bounds, duplex discipline, and binary/handle ownership; an in-process worker
failure could take down conversation and device state.

## Decision

Use length-delimited Protobuf `EnvelopeV1` over Windows named pipes restricted to the
current user SID. The envelope carries protocol version, per-launch nonce, session/turn
IDs, monotonic sequence/QPC timestamp, deadline, cancellation generation, payload size,
trace ID, payload kind, and typed transport error.

Current control/runtime business requests and results are versioned, bounded Serde JSON
inside protobuf request/response wrappers. The protobuf envelope provides framing and
transport invariants; it does not make those application fields protobuf-typed. Validate
both layers, reject incompatible business schema versions, bound JSON before allocation,
and test JSON evolution explicitly. A future typed-protobuf migration requires a separate
compatibility plan; it is not implied by this record.

Use shared-memory rings for PCM and qualified shared D3D handles for textures;
control/ownership remains on the pipe.

Supervise all descendants in Windows Job Objects with heartbeat, bounded restart, circuit breakers and feature quarantine.

## Consequences

- Processes can restart or degrade independently.
- Envelope and JSON business schemas require separate compatibility and fuzz testing.
- Shared-resource lifetime and cancellation require explicit generation/handle ownership tests.
- Protobuf evolution follows “never reuse field number,” compatible additions and
  major-version negotiation; business JSON evolves under its own explicit version and
  unknown-field policy.

## Rejected alternatives

- **In-process FFI for all components:** lowest theoretical call overhead but unacceptable crash/ABI/runtime coupling.
- **HTTP localhost:** larger parsing/network surface and awkward media handle exchange; reserved for user-configured external OpenAI-compatible servers.
- **Unframed stdout JSON:** simple spikes but no robust duplex framing, ACL, binary/handle,
  or backpressure contract. This rejection does not prohibit validated JSON inside the
  authenticated protobuf envelope.

## Evidence

- Windows named-pipe security: <https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights>
- Job Objects: <https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects>
- Protocol Buffers compatibility: <https://protobuf.dev/programming-guides/proto3/#updating>
