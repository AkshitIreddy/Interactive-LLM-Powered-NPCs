# ADR-0003: Protobuf over SID-restricted named pipes

Status: Accepted  
Date: 2026-08-28

## Context

Realtime Windows media, Rust orchestration, and an optional generic lip-sync runtime need independent crash and resource boundaries. JSON/stdout lacks schema, framing and binary-stream discipline; an in-process visual-runtime failure could take down conversation and device state.

## Decision

Use length-delimited Protobuf `EnvelopeV1` over Windows named pipes restricted to the current user SID. Include protocol version, per-launch nonce, session/turn IDs, monotonic sequence/QPC timestamp, deadline, cancellation generation, payload size, trace ID and typed error. Use shared-memory rings for PCM and qualified shared D3D handles for textures; control/ownership remains on the pipe.

Supervise all descendants in Windows Job Objects with heartbeat, bounded restart, circuit breakers and feature quarantine.

## Consequences

- Processes can restart or degrade independently.
- Schemas support compatibility and fuzz testing.
- Shared-resource lifetime and cancellation require explicit generation/handle ownership tests.
- Protobuf evolution follows “never reuse field number,” compatible additions and major-version negotiation.

## Rejected alternatives

- **In-process FFI for all components:** lowest theoretical call overhead but unacceptable crash/ABI/runtime coupling.
- **HTTP localhost:** larger parsing/network surface and awkward media handle exchange; reserved for user-configured external OpenAI-compatible servers.
- **StdIO JSON:** simple spikes but no robust duplex framing, ACL, binary/handle or backpressure contract.

## Evidence

- Windows named-pipe security: <https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights>
- Job Objects: <https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects>
- Protocol Buffers compatibility: <https://protobuf.dev/programming-guides/proto3/#updating>
