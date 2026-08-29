# Technology comparison matrix

Decisions are for a Windows gaming companion, not a general SaaS or research notebook.

## Desktop shell and orchestration

| Option | Strengths | Costs/risks | Disposition |
| --- | --- | --- | --- |
| Tauri 2 + Rust + native broker | Polished web UI, Rust control plane, small app model, NSIS/updater support | WebView lifecycle/overhead; multi-language boundary | **Selected**, release-spike gated; no realtime media in WebView. |
| WinUI 3 | Deep Windows-native UI/integration | More UI-specific complexity and slower cross-skilled iteration | Fallback if Tauri spike fails measurable gates. |
| Electron | Mature UI ecosystem | Larger baseline memory/process footprint for a game companion | Rejected for initial architecture. |
| Python desktop/notebook | Direct ML ecosystem | Packaging, GIL/event loop, crash/runtime/reproducibility and consumer UX problems | Rejected as orchestrator; isolated pack only. |
| LangChain/LangGraph | Useful for broad agent graphs/integrations | Abstraction/provider/runtime weight for a bounded streaming turn | Rejected from production live path. |
| Custom Tokio state machine | Explicit deadlines, streams, cancellation and typed state | Project owns implementation/test coverage | **Selected**. |

## Capture and presentation

| Option | Game suitability | Key limitations | Disposition |
| --- | --- | --- | --- |
| Windows Graphics Capture by HWND | Modern D3D-native window capture, target selection | Protected/exclusive/minimized behavior varies | **Primary external capture**. |
| DXGI Desktop Duplication | Efficient display frames and metadata | Display-level rather than semantic HWND; compatibility/HDR considerations | Qualified fallback/harness input. |
| GDI BitBlt/PrintWindow | Easy prototype | Black/stale accelerated content, CPU copies, no presentation solution | Removed. |
| DirectComposition transparent overlay | GPU composition, native window semantics | Lifecycle/DPI/HDR/anti-cheat testing required | **Selected external presentation**. |
| OpenCV opaque mirror | Simple debug display | Self-capture, focus, frozen copy, latency | Removed. |
| Code injection, hooking, mods, native-rig adapters | Potentially deeper game access | Brittle, protected-online conflict, per-build burden, defeats broad compatibility | Rejected; the supported path remains external and non-injecting. |

## Memory

| Option | Packaging/latency | Persistence/migrations | Disposition |
| --- | --- | --- | --- |
| SQLite STRICT + WAL + FTS5 + sqlite-vec | In-process, single file, low overhead | Strong transactions/migrations; vector extension must be pinned | **Selected**. |
| Chroma embedded | Familiar vector abstraction | Python/library/index/pickle legacy burden and mixed authority | Replaced. |
| Separate vector database | Scales beyond local need | Server lifecycle, ports, memory, installer complexity | Rejected for v2 scale. |
| JSON files | Transparent | Weak atomicity/query/migration/concurrency | Import source only, not runtime authority. |

## Conversation APIs and optional local lip-sync

| Area | Primary | Alternative/candidate | Rule |
| --- | --- | --- | --- |
| LLM | explicitly configured hosted providers | configurable hosted OpenAI-compatible endpoint | API-first; no local LLM pack or silent provider fallback. |
| STT | explicitly configured hosted providers | provider endpointing plus PTT | API-first; PTT key-up remains authoritative. |
| TTS | explicitly configured hosted providers | provider stock voices | API-first; no actor imitation or local TTS pack. |
| Retrieval | SQLite FTS/recent context + optional hosted embeddings/reranking | lexical-only fallback | Hosted semantic text egress requires explicit route authorization. |
| Screen-space baseline | Audio2Face-3D regression v2.3 coefficient source → project-owned tracked 2D mouth residual | audio-derived viseme/mouth warp | Research architecture only; no native rig, no pack claim, immutable current frame and strict masked fail-open composition. |
| Direct-video experiment | NVIDIA Maxine AR SDK LipSync | none qualified | Access-controlled NGC feature; synchronized frame/audio contract; exact access, license, Windows, latency, VRAM, quality and game-impact qualification required. |
| Offline comparator | MuseTalk 1.5 | LatentSync offline-only | MuseTalk's measured batch path took ~102 s for 1.579 s output; neither is a live pack candidate. |
| Watchlist | EfficientSync; FlashLips | Ditto deferred | Paper results only until code, weights, license, Windows, cancellation and local resource/visual gates pass. |
| Rejected live paths | Native game-rig integration; Wav2Lip; SadTalker; LivePortrait | — | No per-game integration or full-frame live replacement. |

## Decision guardrails

- No vendor benchmark becomes a product claim without reproduction.
- No model/library is packed before exact source revision, all transitive assets and intended redistribution/commercial terms are recorded.
- A faster optional visual feature that destabilizes the game or delays audio loses to audio/subtitles.
- Backend diversity is a catalog/pack concern; core contracts do not expose CUDA-specific assumptions.
- The captured source frame is immutable. Visual output is a bounded, frame/generation-addressed mouth residual applied only to a presentation copy; invalid or stale work reveals the untouched current frame.
- Current admission permits one optional local visual lease. Any future local conversation-model policy requires measured co-residency, live game reserve, p99 workspace and load/unload accounting; no silent device or cloud substitution.
