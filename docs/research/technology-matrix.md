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
| LLM | explicitly configured hosted providers | Qwen3-4B-Instruct-2507 Q4_K_M through llama.cpp, CPU-first | API-first default; an explicit local pack activates only after a measured whole-loadout fit. No silent provider fallback. |
| STT | explicitly configured hosted providers | Moonshine v2 Tiny/Small streaming on CPU | API-first default; PTT key-up remains authoritative and the exact Windows pack must pass latency/accuracy qualification. |
| TTS | explicitly configured hosted providers | Kokoro-82M INT8 ONNX/sherpa-onnx on CPU | API-first default; stock voices only, with exact voice assets and licenses in the pack ledger. No actor imitation. |
| Retrieval | SQLite FTS/recent context + optional hosted embeddings/reranking | lexical-only fallback | Hosted semantic text egress requires explicit route authorization. |
| Screen-space baseline | TTS-native visemes or causal audio-to-viseme → tracked current-frame mouth warp | optional tiny teeth/lip residual | **Selected engineering direction**; no native rig, immutable current frame, actor/frame/generation addressing, queue depth one, strict masked bypass. |
| NVIDIA animation experiments | Public MIT Audio2Face-3D SDK + regression Mark v2.3 | Private-access NVIDIA LipSync; deprecated portrait-oriented Audio2Face-2D; self-hosted A2F3D NIM | The SDK is the first NVIDIA local low-latency coefficient/geometry candidate for Windows x64, not a hosted route or pixel compositor. It still requires a prepared animation target plus a project-owned tracked ROI mapper/compositor. NVIDIA's faster-than-60-FPS statement is unverified on the target RTX 4080 beside a game. |
| Neural residual spike | MuseTalk 1.5, heavily refactored for current ROIs | LatentSync offline-only | MuseTalk may be measured as a bounded residual experiment; its stock avatar pipeline and current 4 GB consumer result are far too slow to claim as a live pack. |
| Watchlist | EfficientSync; FlashLips | Ditto deferred | Paper results only until code, weights, license, Windows, cancellation and local resource/visual gates pass. |
| Rejected live paths | Native game-rig integration; Wav2Lip; SadTalker; LivePortrait | — | No per-game integration or full-frame live replacement. |

## Decision guardrails

NVIDIA provider and animation status in this matrix was last verified against official sources on **2026-08-30**: [trial terms](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf), [NIM FAQ](https://docs.api.nvidia.com/nim/docs/product), [LipSync model card](https://build.nvidia.com/nvidia/lipsync/modelcard), [Audio2Face-2D](https://build.nvidia.com/nvidia/audio2face-2d/deploy), the [public Audio2Face-3D SDK](https://github.com/NVIDIA/Audio2Face-3D-SDK), the [Audio2Face-3D collection](https://github.com/NVIDIA/Audio2Face-3D), the [Mark v2.3 model](https://huggingface.co/nvidia/Audio2Face-3D-v2.3-Mark), and the [current self-hosted NIM](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html). Hosted NIM is a single-key evaluation option with model-specific entitlements and limits, not unlimited access or a redistributable runtime. The SDK is MIT, its model weights use separate NVIDIA Open Model terms, and its Windows requirements are CUDA `>=12.8,<13.0` (12.9 recommended) plus TensorRT `>=10.13,<11.0`.

- No vendor benchmark becomes a product claim without reproduction.
- No model/library is packed before exact source revision, all transitive assets and intended redistribution/commercial terms are recorded.
- A faster optional visual feature that destabilizes the game or delays audio loses to audio/subtitles.
- Backend diversity is a catalog/pack concern; core contracts do not expose CUDA-specific assumptions.
- The captured source frame is immutable. Visual output is a bounded, frame/generation-addressed mouth residual applied only to a presentation copy; invalid or stale work reveals the untouched current frame.
- API-first mode permits one explicit generic visual pack. Advanced local conversation packs require the separate measured-local policy: complete loadout co-residency, live game reserve, p99 workspace, RAM/VRAM safety margins and load/unload accounting; no silent device or cloud substitution.
