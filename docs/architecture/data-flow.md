# Conversation and data flow

## Live turn

```text
PTT down / VAD start
  ├─ media broker streams PCM → recognizer
  └─ runtime emits Listening/Transcribing timing

PTT up / stable endpoint
  ├─ final transcript
  ├─ game-state/identity evidence ─┐
  └─ memory retrieval ────────────┼─ parallel, deadline bounded
                                  ▼
                         typed context assembly
                                  │
                    LLM spoken stream + effects request
                     │                         │
          sanitizer/clause segmenter   validate NpcEffectsV1
                     │                         └─ neutral no-op on failure
              SentenceReady
                     │
               streaming TTS
                     ├─ PCM → broker → speaker
                     └─ alignment/visemes → optional generic visual worker
                                  │
                   delivered-audio acknowledgements
                                  ▼
                     commit delivered turn range
                                  │
                 async summaries/memory proposals/indexing
```

PTT key-up is authoritative even if VAD has not fired. A new player utterance increments `cancellation_generation`, cancels LLM/TTS/playback/animation, and invalidates late messages. A delivered byte/sample range, not “generation finished,” determines what is eligible for durable heard history.

## Spoken and structured outputs

The spoken stream contains only player-safe NPC dialogue after sanitizer and policy checks. `NpcEffectsV1` is produced/parsed separately and includes bounded values:

- emotion label plus valence, arousal and intensity;
- provider-neutral voice style;
- allowlisted animation cues and interruption behavior;
- memory and relationship proposals with evidence/source turn IDs;
- non-executable relationship, memory, interruption, and generic animation proposals.

Effects never inject raw executable commands, file paths, provider calls, model parameters, game actions, or integration symbols. Each proposal is validated against profile capability and current safety policy; absent, malformed, late or unsupported effects become neutral no-ops and cannot delay first audio.

## Authoritative versus derived data

| Data | Authority | Persistence |
| --- | --- | --- |
| Profile/lore/canon | Signed profile content with provenance | Versioned immutable content rows |
| Raw transcript | Recognizer result plus user correction state | Immutable turn segment; sensitive retention setting applies |
| Delivered NPC dialogue | Audio delivery acknowledgements mapped to text ranges | Immutable turn segment |
| Working context | Runtime assembly | Ephemeral; trace may retain redacted IDs/timings only |
| Episodic memory/fact/relationship | Validated proposal referencing source turns | Transactional tables with confidence and scope |
| Summary | Derived from source turn range and model/version | Rebuildable/versioned; source turns remain authoritative |
| FTS/vector index | Derived from accepted records | Rebuildable namespace by embedding model/version |
| Frames/PCM | Media broker stream | Ephemeral by default; recording requires explicit diagnostic consent |
| Model pack | Signed catalog/TUF metadata + verified files | Immutable activated pack version |

## Memory retrieval

Retrieval runs within a hard budget and keeps authority classes separate:

1. exact current game/save/quest state and selected actor;
2. recent delivered conversation window;
3. scoped canon and character biography/style;
4. FTS5 lexical candidates;
5. sqlite-vec semantic candidates when enabled and ready;
6. long-term episodic facts/relationship state permitted for this player/save;
7. deterministic rank/deduplicate/token-budget pass.

If vector retrieval is unavailable or stale, FTS/recent context continues. A mismatched embedding namespace is never queried as if compatible; it is rebuilt asynchronously.

## Privacy/egress decision point

Before each provider call, the runtime evaluates execution mode, selected provider, data categories, feature policy and explicit fallback authorization. The UI can explain whether microphone audio, transcript, screenshot, webcam frame or game context would leave the machine. A provider request that requires an unapproved category fails before connection.

Offline mode is tested behind deny-all networking and must make no provider attempt. API-powered conversation requires the configured hosted routes; an outage becomes an explicit error or a pre-authorized cross-provider fallback, never an implicit route change.

## Resource and timing events

All stages use QPC timestamps from process messages and are correlated by session/turn/trace. Required metrics include VAD endpoint, STT first/final text, identity and retrieval completion, LLM first token, first sentence, TTS request and first PCM, playback start, visual onset, cancellation-to-silence, end-to-end first audio, CPU/GPU/VRAM/RAM, load time and game frame impact. Wall-clock time is attached only for human-readable logs; durations use monotonic QPC time.
