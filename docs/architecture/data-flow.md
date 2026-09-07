# Conversation and data flow

Status: integrated contracts plus an unqualified target vertical slice

## Integrated source flow

```text
bounded control request
  → Tauri control resolves target/profile/loadout/safety/subtitle authority
  → protobuf EnvelopeV1 carries a versioned JSON business request
  → runtime host validates the request and freezes its provider-route snapshot
  → TurnSupervisor assembles context and runs cancellation/delivery policy
       ├─ selected hosted provider bridges where executable
       ├─ local SQLite/FTS character and delivered-memory context
       ├─ optional subtitle/audio delivery paths
       └─ optional effects proposal lane (currently fixture-backed)
  → versioned JSON result returns in a protobuf response envelope
  → native-backed state/receipts are exposed to the UI
```

This source path is broader than the proven product path. The normal STT selection is
currently AssemblyAI `u3-rt-pro`; fixed-origin selected TTS routes and component tests do
not yet establish a complete normal physical-output route. The rights-cleared Eclipse Harbor target has the
safe synthetic policy. Authored non-synthetic profiles are console-isolated, with capture,
overlay, native identity, and visuals disabled. The production actor-lock bus is
unqualified.

## Target live turn

```text
PTT down
  ├─ media broker writes bounded microphone PCM
  └─ runtime emits measured Listening/Transcribing events

PTT up / final endpoint
  ├─ final transcript
  ├─ trusted target + optional actor evidence ─┐
  └─ local memory/context retrieval ──────────┼─ concurrent, deadline bounded
                                               ▼
                                      typed prompt context
                                               │
                                   selected LLM sentence stream
                                    │                    │
                           spoken-text policy      optional effects proposal
                                    │                    └─ validate or neutralize
                              SentenceReady
                                    │
                              selected TTS
                              ├─ PCM → broker → selected speaker
                              ├─ text → native subtitle presenter
                              └─ playback timing/visemes → optional visual worker
                                                │
                                     committed delivery receipts
                                                ▼
                                  commit only delivered text ranges
                                                │
                                async summary/index proposals
```

This target becomes product behavior only after one source-identified ordinary turn
proves the complete microphone, provider, playback, subtitle, cancellation, and memory
joins. A new player utterance increments `cancellation_generation` before starting new
output. Late work from an older generation has no side effect. Played sample ranges or a
committed subtitle receipt determine durable heard history; generation completion does
not.

## Provider route truth

Provider traits, catalog entries, credentials, and executable runtime bridges are
different layers:

- Hosted LLM bridges currently cover explicit OpenAI Responses, Anthropic Messages,
  Gemini, Groq, Mistral, OpenRouter, Cohere, and NVIDIA NIM routes. A cataloged generic
  `openai-compatible` entry is not an ordinary executable bridge.
- The normal selected STT bridge currently accepts AssemblyAI `u3-rt-pro`. Other stable
  STT catalog entries are not normal executable routes until the runtime bridge admits
  them.
- Credentialed fixed-origin TTS runtime construction exists for Cartesia, Deepgram,
  Inworld, ElevenLabs, and NVIDIA NIM Magpie. The normal selected-TTS-to-physical-output
  route remains unqualified.
- Hosted semantic retrieval currently has a specific NVIDIA
  `nvidia/nemotron-3-embed-1b` bridge. Local FTS/recent context remains available when
  semantic retrieval is absent.
- A supervised local LLM worker is integrated, but its current sample pack is not an
  admissible production model. Other local roles remain pack candidates.

The UI may call a route available only when its credential, exact model, executable
bridge, selected-loadout path, and delivery evidence all exist. Unsupported selection
fails clearly; it never changes provider or device silently.

## Spoken output and effects

Spoken text passes sanitizer and sentence segmentation before TTS/subtitles. Effects are
separate bounded proposals such as emotion/style, interruption, relationship, memory, or
generic animation cues. They contain no executable command, file path, provider call,
model parameter, game-memory symbol, or injection instruction.

The current runtime simulation uses `FixtureEffects`. Therefore effects are not a live
LLM capability and are not game actuation. Missing, malformed, late, or unsupported
proposals become neutral no-ops and cannot delay speech.

No clinical, biometric, demographic, age, race, gender, mental-state, or webcam-emotion
inference participates in the conversation flow. Optional actor identity is a narrow
character-selection aid from trusted game pixels after explicit pack admission.

## Identity and visual data

Captured pixels, crops, embeddings, landmarks, actor coordinates, D3D handles, and visual
residuals remain native-only. The WebView receives redacted readiness and receipt state.

The accepted target sequence is:

1. broker binds a safe target HWND and stamps frame, geometry, monitor, color-space, and
   QPC evidence;
2. an admitted vision pack produces bounded candidates from a leased current frame;
3. identity validates the lease/digest, tracks candidates, and publishes an actor lock;
4. optional lip-sync binds work to that actor, track epoch, capture sequence, audio clock,
   cancellation generation, mask, deadline, and resource lease;
5. the broker presents only an accepted residual over the newest compatible presentation
   frame; otherwise the untouched game remains visible.

Steps 2–5 are not a normal product path today: the private YuNet pack is measured and
signed but not installed or active, normal commercial capture is unqualified, and the
current mouth result has not passed visual review.

## Authoritative and derived data

| Data | Authority | Persistence |
| --- | --- | --- |
| Profile/lore/canon | Versioned profile content with provenance | Immutable/versioned content rows |
| Provider route | Frozen selected-loadout snapshot plus explicit authorization | Persisted configuration and per-turn receipt |
| Raw transcript | Recognizer result plus user correction state | Turn segment subject to retention policy |
| Delivered NPC dialogue | Audio/subtitle delivery receipts mapped to text | Immutable delivered turn segment |
| Working prompt context | Runtime assembly with separated authority lanes | Ephemeral; redacted trace IDs/timing only |
| Episodic fact/relationship | Validated proposal referencing delivered source turns | Transactional scoped record |
| Summary | Derived from a source-turn range and model/version | Rebuildable/versioned |
| FTS/vector index | Derived from accepted records | Rebuildable by embedding namespace |
| Frames/PCM | Native media transport | Ephemeral unless explicit diagnostics consent |
| Pack | Signed catalog metadata plus verified immutable files | Activated immutable pack revision |

## Memory retrieval

Retrieval is deadline bounded and keeps authority classes separate:

1. selected game/save/quest scope and explicit actor;
2. recent delivered conversation;
3. scoped canon and biography/style;
4. FTS lexical candidates;
5. compatible semantic candidates when a route/index is ready;
6. permitted episodic facts and relationship state;
7. deterministic rank, deduplication, and token budgeting.

If semantic retrieval is missing or stale, FTS/recent delivered context continues. A
mismatched embedding namespace is rebuilt rather than queried as compatible.

## Privacy and egress

Before a provider call, the runtime evaluates the frozen route, execution mode, requested
data classes, credential reference, and explicit fallback authorization. A call requiring
an unapproved class fails before connection. Credential values never enter the WebView,
broker, pack request, log, or exported diagnostics.

Offline must be proven behind deny-all networking. A hosted outage becomes an explicit
error or an exact pre-authorized fallback, never an implicit provider change.

## Resource and timing evidence

ResourceBroker supports opt-in measured turn admission in runtime-core, and Model Manager
supports measured loadout preflight. Production admission still needs a runtime-host join
fed by native-stamped current game/desktop budgets and qualified whole-turn envelopes.
Until then, reserve decisions from fixtures or host estimates are not production evidence.

All processes correlate events with session, turn, trace, cancellation generation, and
monotonic QPC timestamps. Product evidence must include STT first/final text, identity and
retrieval completion, LLM first token/sentence, TTS request/first PCM, playback and
subtitle commitment, optional visual onset, cancellation-to-silence, first-audio latency,
CPU/RAM/GPU/VRAM, and game frame impact. Wall clock is only for human-readable logs.

One bounded production-adapter chain measured Groq Qwen request to Cartesia
`RuntimeTtsBridge` first PCM at 492.964 ms. It excludes microphone/STT, normal
`HostState`, native broker drain, an OS speaker, and physical audibility, so it is not an
end-to-end turn latency claim.
