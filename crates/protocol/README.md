# npc-protocol

Shared contracts for Interactive LLM Powered NPCs 2.0.

The crate has two deliberately separate surfaces:

- Protobuf-compatible wire messages for the Tauri shell, Rust runtime, media
  broker, and persistent inference workers. `EnvelopeV1` authenticates a launch,
  scopes work to a session/turn/trace, enforces QPC deadlines, carries monotonic
  cancellation generations, and rejects oversized, replayed, or reordered data.
- Serde DTOs for reviewed game profiles and immutable model-pack manifests.
  Validators reject placeholder content, unsafe executable paths, ambiguous
  licensing, path traversal, mutable downloads, and capability claims without
  their required safety controls.

Provider output is normalized into STT, LLM, and TTS event streams. Spoken text
and `NpcEffectsV1` remain independent: invalid effects are replaced by a neutral
no-op and never need to delay or discard valid speech.

Run the crate independently with:

```powershell
cargo test --manifest-path crates/protocol/Cargo.toml
cargo fmt --manifest-path crates/protocol/Cargo.toml -- --check
```

The protocol contains no transport implementation, secret storage, executable
game logic, provider SDK, or model runtime. Those layers consume these bounded
contracts and enforce their own ACL, credential, and user-authorization policy.
