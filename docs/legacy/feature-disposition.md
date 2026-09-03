# Version 1 feature disposition

Legend: **preserve** keeps the product behavior; **redesign** keeps the intent with a new contract; **replace** changes the underlying technique; **optional** means failure cannot block conversation; **remove** means the behavior is deliberately absent from 2.0.

| V1 feature or behavior | Evidence | Decision | Version 2 disposition |
| --- | --- | --- | --- |
| Push-to-talk | notebook `interact_key` and delayed microphone capture | Preserve + redesign | Global hotkey, immediate streaming capture, authoritative key-up endpoint, configurable bindings and conflict diagnostics. |
| Audio-only interaction | animation switch/no-face branch | Preserve | First-class fallback, not a lesser error path; subtitles and explicit character selection remain available. |
| Known-character replies | per-character bios, knowledge, style and conversation | Preserve + redesign | Authored `GameProfileV2` content, stable IDs, provenance, spoiler tiers, provider-neutral voice traits and isolated memory. |
| Background NPC generation | mutable `default` folder | Redesign | Stable encounter IDs, deterministic persona/voice assignment, multiple concurrent identities and explicit expiration rules. |
| Name-based selection | exact/prefix scan of transcript | Replace | Explicit UI selection plus optional read-only dialogue UI/OCR, subtitle, tracking and temporal screen evidence; name text is one signal. |
| Single-frame face recognition | DeepFace `find` over image folders | Replace + optional | Capability-based temporal identity evidence; no demographic inference; confidence lock and offscreen continuity. |
| Highest-confidence face targeting | one RetinaFace crop | Replace | Multi-track detector with interaction evidence and sticky actor IDs; ambiguity asks the user or stays on the selected target. |
| Webcam emotion | automatic camera 0 capture every turn | Remove by default + optional redesign | Local-only, explicit opt-in presence feature with preview, device choice, permission, retention policy and no demographic inference. |
| Age/race/gender inference | DeepFace analysis for unknown NPC personality | Remove | Never infer protected/demographic traits from faces. Game metadata or user-authored neutral characteristics may inform a persona. |
| Player emotion in giant prompt | webcam label interpolated into text | Replace + optional | Validated bounded presence signal; neutral no-op on failure; visible privacy/capability state. |
| NPC emotional response | implicit prose and TTS punctuation | Redesign | Separate `NpcEffectsV1` with emotion, valence/arousal/intensity and voice/animation hints; schema failure becomes neutral. |
| World lore | complete `world.txt` in every prompt | Preserve + redesign | Canon table with provenance, scope, spoiler/quest filters and retrieval; never inject the whole corpus by default. |
| Character biography | `bio.txt` | Preserve + redesign | Authored/profile-scoped content with stable character ID, provenance, compatibility and overrides. |
| Talking style examples | shuffled `pre_conversation.json` | Preserve + redesign | Curated style traits/examples with deterministic selection and token budget; no random prompt drift in tests. |
| Public and character RAG | separate Chroma collections | Preserve concept, replace store | SQLite FTS5 + pinned sqlite-vec derived indexes; authority types remain separate and rebuildable. |
| Recent conversation | mutable JSON | Preserve + redesign | Immutable turn ledger with delivery state, session/character/save scope and transaction boundaries. |
| Long-conversation summarization | >500-token Cohere summary into Chroma | Redesign | Asynchronous, versioned summaries with source-turn ranges; raw immutable turns remain authoritative. |
| Random API-key rotation | `apikeys.json` list | Remove | One credential reference per configured provider/account in Windows Credential Manager; explicit retry and rate-limit policy. |
| Cohere-only generation/embeddings | LangChain Cohere clients | Replace | Capability-driven hosted provider interfaces and curated dynamic catalogs; no provider owns runtime semantics. |
| One giant prompt | `PromptTemplate` combines every signal | Replace | Typed context assembly, independently validated non-executable effects and a sanitized spoken stream. |
| Per-character Python voice modules | dynamically imported `voice.py` | Replace | TTS adapter + provider-neutral voice binding. Data selects a voice ID/trait; no executable profile code. |
| Generated `temp.py` execution | four response generators | Remove | Responses are data only. Typed TTS requests cross a versioned process boundary. |
| Edge TTS voice diversity | male/female JSON voice lists | Preserve intent, replace categorization | Deterministic voice traits such as register, texture, pace and locale; user override and licensing metadata. |
| Full SadTalker render | `create_facial_animation.py` | Remove | Optional generic tracked mouth residual only after benchmark and license gates; no native-rig/mod path. |
| Frozen frame with rectangular face video | nested OpenCV playback | Remove | Fresh-frame, GPU-native, masked composition with confidence/occlusion/staleness guard; otherwise no patch. |
| OpenCV mirror UI | notebook window | Replace | Tauri Response Console plus transparent native overlay; foreground game retains focus. |
| GDI/PrintWindow capture | two `grabscreen.py` variants | Replace | WGC by HWND with DXGI fallback and explicit mode compatibility. |
| Fixed 1920×1080 coordinates | notebook constants | Remove | Per-frame transform and per-monitor DPI/HDR metadata. |
| Fixed temp files | `temp/`, `video_temp/`, root `temp.py` | Remove | Bounded shared-memory rings/textures and per-session safe staging for artifacts that truly require disk. |
| Complete-response playback | wait for LLM/TTS/video | Replace | Clause-level TTS scheduling and streaming PCM; animation consumes timing/viseme events incrementally. |
| No barge-in | nested playback loop | Replace | Cancellation generation cuts LLM, TTS queue, playback and animation; late events are ignored. |
| Notebook configuration | cell constants and JSON edits | Remove | Onboarding and global/game/character inheritance in validated settings. |
| Implicit provider/data privacy | no UI boundary | Remove | Per-feature disclosure of audio, transcript, screenshot, webcam and game context egress; Offline means no provider calls. |
| Manual game directory convention | string `game_name` | Replace | Installed-game discovery, signed built-in profiles, manual executable fallback and experimental generic mode. |
| Cyberpunk content bundle | duplicated trees | Archive/import selectively | Do not ship unreviewed copied prose/media; convert only original/provenanced text after content review. |
| Vendored SadTalker source | 190 tracked files | Archive/remove from build | Preserve historical Git reference; only an explicitly selected, qualified generic lip-sync pack may add model weights later. |
| Console prints | ad hoc messages/errors | Replace | Structured local logs, user-facing diagnostics and secret-redacted export. |
| Magic-string errors | `NULL`, empty paths | Remove | Typed status/error with retryability, subsystem, user action and trace correlation. |

## Non-negotiable behavioral invariants

- Optional vision, emotion, identity, memory, TTS, or animation failures never crash the core session.
- Cloud fallback never occurs without prior authorization of the exact provider route.
- A turn becomes durable conversation history only to the extent that it was delivered/heard.
- Profiles and model outputs are data; they cannot introduce executable code.
- Online/protected/anti-cheat ambiguity disables capture/overlay rather than attempting bypass.
- Offscreen or obscured characters continue through explicit selection and audio/subtitles.

## Enforceable 2.0 emotion and webcam boundary

`emotion` in product preferences means optional **NPC output effects** only. It
does not authorize a webcam, infer the player's inner state, or label a person.
Malformed optional effect fields are neutralized independently from the spoken
response, so speech remains usable.

Webcam presence has a separate `webcamPresence` preference and defaults to
false for every execution/performance preset. A legacy screen-presence setting
migrates only to game-screen vision and cannot grant webcam consent. Persisting
the preference records consent intent; it does not open a device, start a route,
or prove that a webcam producer is installed. The current product registers no
live webcam capture command. Any future producer must remain local, show an
active/revocable indicator and preview/device choice, avoid retention by
default, and degrade to a neutral no-op. It may expose bounded authored signals
such as presence or action-unit activity, but never age, race, gender, protected
traits, or a claim about the user's inner emotion.
