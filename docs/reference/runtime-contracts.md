# Runtime contracts

The implemented contract crate is `crates/runtime-core` (`npc-runtime-core`). It is a deterministic orchestration core, not a complete provider/device runtime.

## Turn lifecycle

`TurnSupervisor` creates a `TurnHandle` with session/turn identity and a monotonic cancellation generation. Barge-in increments the generation, cancels queued LLM/TTS/playback/animation work, and makes all older events ineligible. Only content reported delivered by the audio sink can be committed.

## Provider boundaries

- `StreamingRecognizer`: partial/final transcription with cancellation.
- `LanguageModelProvider`: streamed speech text.
- `EffectsProvider`: independent validated `NpcEffectsV1` output.
- `TtsProvider` / `TtsSession`: streamed audio/timing session.
- `IdentityResolver`: confidence-aware identity evidence.
- `MemoryStore`: retrieval and delivered-only commit contract.
- `AudioSink`: playback/delivery acknowledgement.

Complete sanitized sentences, not arbitrary token fragments, enter TTS. Invalid effects become neutral/no-op and do not retract or delay speech.

## Privacy and failure

Routes encode explicit hosted-provider authorization and Offline policy. A provider failure cannot silently authorize a different provider. Circuit breakers quarantine repeatedly failing features. Identity/memory/effects/animation failures have explicit degradation while LLM or required speech failure returns a retryable turn error.

## Current limitation

The crate has deterministic fixture tests for its contracts. Hosted service implementations, Windows audio/capture/playback, persistent SQLite wiring, and optional generic lip-sync worker IPC are separate integrations and must not be inferred from the contract tests. There is no per-game executable adapter path.
