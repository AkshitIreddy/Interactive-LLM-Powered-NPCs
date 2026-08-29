# IPC and wire contracts

`crates/protocol` is the versioned Protobuf/serde contract crate. It defines wire types and validation; the production SID-restricted named-pipe server, shared-memory PCM transport, and shared D3D transport are separate integrations.

## EnvelopeV1

Each length-delimited frame includes protocol version, per-launch nonce, session/optional turn/trace IDs, sequence, QPC timestamp/frequency, deadline, cancellation generation, declared payload size, and one typed body. Bodies cover negotiation, heartbeat, acknowledgement, cancellation, STT/LLM/TTS events, effects, typed errors, actor selection, and runtime-control requests/responses.

Validation enforces version negotiation, nonce/session identity, deadline, size limits, sequence/order/replay policy, turn requirements, and cancellation generation. Unknown/stale/oversized/malformed frames are rejected before dispatch.

## Runtime control projection

The Tauri shell and trusted runtime continue using `EnvelopeV1` after negotiation; there is no weaker post-handshake frame type. `ControlRequestV1` carries an opaque request ID, a typed operation discriminator, and one bounded strict-JSON DTO. `ControlResponseV1` correlates the request ID and request sequence, repeats the operation, and contains either bounded success JSON or `IpcErrorV1`. Simulation operations require an envelope turn ID; health, profile, cancellation, and lifecycle operations forbid one.

Both directions use independent strict-contiguous sequence trackers. The runtime validates the launch nonce and session on every message, rejects generation changes that were not introduced by a valid cancellation request, and checks declared payload size before JSON parsing. Responses preserve the request trace, turn, deadline, and cancellation generation. The runtime does not enqueue work whose deadline has expired and drops response intents that become late or superseded. The shell consumes and discards a structurally valid response that races a local timeout so it cannot complete an expired request or break the next response sequence.

Control framing remains little-endian `u32` length-delimited and is bounded to 1 MiB including the prefix. Control JSON is capped at half that budget. Diagnostic errors expose stable codes and retryability only; credentials and conversation content are not written to control-plane logs.

## Provider events

- STT: started, partial/stability, final/language, error.
- LLM speech: delta, sentence ready, complete, error.
- TTS: audio frame, word/phoneme/viseme timing, complete, error.
- Effects: bounded emotion/valence/arousal/intensity, voice style, generic animation cues, interruption policy, and memory/relationship proposals; no game actions.

Legacy action-shaped fields, if retained temporarily for wire compatibility, are rejected at validation. The product contract has no game-action execution path.

## Other DTOs

The crate also contains provider capability/privacy/pricing/fallback DTOs plus the protocol-level `GameProfileWireV2`, `GameProfileWireCatalogV1`, generic-mode, and `ModelPackManifestV1` boundaries. These game-profile wire DTO names deliberately distinguish them from the single authoritative authored model, `npc_game_profile::GameProfileV2`. The dedicated game-profile, provider-catalog, and model-manager crates add stronger authored-data/lifecycle validation; passing a wire DTO alone is not sufficient for installation or execution. The Rust type rename does not alter the existing serialized field names or shape.

## Verification

```powershell
cargo test -p npc-protocol
```

Protocol tests cover negotiation, frames, ordering/replay, cancellation, effects fallback, provider metadata, pack/path/license validation, profile/generic-mode policy, and the 20-profile release-catalog gate.
