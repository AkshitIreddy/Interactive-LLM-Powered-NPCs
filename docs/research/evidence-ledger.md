# Research evidence ledger

Snapshot date: 2026-08-29. This ledger records decision evidence, not a permanent claim that provider catalogs, model IDs, pricing or licenses remain unchanged. Volatile facts must be refreshed into signed catalogs before a release candidate.

Focused qualification: [generic captured-game lip-sync](generic-captured-game-lipsync-qualification-2026-08-30.md).

## Evidence quality rules

- Prefer official platform/provider documentation, upstream repositories, model cards and license texts.
- Treat vendor-published latency/quality as candidate evidence, not a product benchmark.
- Reproduce critical Windows/resource/visual claims on the release matrix.
- Record source revision/hash and exact license for every shipped artifact.
- “Candidate” means no release promise until benchmark, Windows, reliability and licensing gates pass.

## Platform and storage evidence

| ID | Source | Relevant evidence | Decision use | Remaining validation |
| --- | --- | --- | --- | --- |
| E-001 | [Tauri architecture](https://v2.tauri.app/concept/architecture/) | Rust core and WebView communicate through message passing. | Tauri can serve as bounded control plane. | Release overhead and recovery spike. |
| E-002 | [Tauri channels](https://v2.tauri.app/develop/calling-frontend/#channels) | Channels are intended for ordered streaming-style messages versus generic events. | Runtime stage/status channel, no realtime media. | Reconnect/order/load tests. |
| E-003 | [Tauri Windows installer](https://v2.tauri.app/distribute/windows-installer/) and [updater](https://v2.tauri.app/plugin/updater/) | NSIS/MSI and signed updater artifacts are supported. | NSIS base app and locally testable signed-update pipeline. | Clean Windows 10/11 non-admin VMs; feed stays off. |
| E-004 | [Windows Graphics Capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture) | Supported frame capture APIs for display/window content. | Primary external capture by HWND. | Game-mode/protected/minimized/HDR matrix. |
| E-005 | [Desktop Duplication API](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/desktop-duplication-api) | DXGI provides desktop-frame/dirty/move/pointer metadata. | Qualified display fallback and deterministic capture tests. | Contention and HDR behavior. |
| E-006 | [DirectComposition architecture](https://learn.microsoft.com/en-us/windows/win32/directcomp/architecture-and-components) | GPU-backed composition is managed by the desktop composition engine. | Native transparent presentation rather than OpenCV mirror. | Click-through, frame pacing and capture exclusion tests. |
| E-007 | [WASAPI](https://learn.microsoft.com/en-us/windows/win32/coreaudio/wasapi) | Windows event-driven audio APIs expose endpoint streams. | Media broker owns low-latency capture/playback. | Device format, unplug, exclusive/shared and underrun tests. |
| E-008 | [Named-pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights) and [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) | Windows supports ACL-controlled IPC and process-group lifetime/resource management. | SID-restricted pipe + supervised worker boundaries. | Cross-user/stale-client and kill-stage tests. |
| E-009 | [SQLite WAL](https://www.sqlite.org/wal.html), [STRICT](https://www.sqlite.org/stricttables.html), [FTS5](https://www.sqlite.org/fts5.html) | Local transactions, concurrent read/write pattern, rigid tables and built-in text indexing. | One authoritative local database. | Checkpoint/disk-full/migration/copy recovery. |
| E-010 | [sqlite-vec](https://github.com/asg017/sqlite-vec) | Embedded SQLite vector search without a separate server. | Pinned optional semantic index. | Pin/build/security/performance gate. |
| E-011 | [Windows Credential Manager](https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credwritew) | OS credential API writes protected credential entries. | Store API secrets outside JSON/config/logs. | Account lifecycle and canary test. |
| E-012 | [TUF specification](https://theupdateframework.github.io/specification/latest/) | Threshold roles/version/expiry/hash metadata defend update repositories. | Model/profile catalog integrity and rollback protection. | Key custody, rotation and offline bootstrap procedure. |

## Provider and optional visual-pack evidence

| ID | Source | Relevant evidence | Decision use | Caveat |
| --- | --- | --- | --- | --- |
| E-020 | [OpenAI model docs](https://developers.openai.com/api/docs/models) and [realtime transcription](https://developers.openai.com/api/docs/guides/realtime-transcription) | Discoverable hosted models and streaming transcription. | Initial LLM/STT adapter. | Model IDs/pricing/retention are catalog-time facts. |
| E-021 | [Gemini models](https://ai.google.dev/gemini-api/docs/models) | Provider publishes model capabilities/endpoints. | Initial LLM adapter and capability discovery. | Preview/stable status changes. |
| E-022 | [Anthropic models](https://platform.claude.com/docs/en/about-claude/models/overview) | Provider publishes model/capability lifecycle. | Initial LLM adapter. | Structured/streaming features must be probed per model. |
| E-023 | [Groq text](https://console.groq.com/docs/text-chat) and [models API](https://console.groq.com/docs/api-reference) | Streaming text and model listing are available. | Initial LLM adapter and dynamic catalog. | OpenAI-like shape is not universal semantic compatibility. |
| E-024 | [Deepgram docs](https://developers.deepgram.com/docs), [AssemblyAI streaming](https://www.assemblyai.com/docs/speech-to-text/streaming), [ElevenLabs speech-to-text](https://elevenlabs.io/docs/capabilities/speech-to-text) | Hosted streaming speech options expose different endpoint/timing features. | Initial STT candidates behind one capability interface. | Benchmark identical audio/noise fixtures and privacy terms. |
| E-025 | [ElevenLabs TTS](https://elevenlabs.io/docs/overview/capabilities/text-to-speech), [Cartesia TTS](https://docs.cartesia.ai/build-with-cartesia/tts-models), [Deepgram TTS](https://developers.deepgram.com/docs/tts-models) | Hosted TTS supports streaming/model/voice variants. | Initial hosted TTS candidates. | Vendor latency is not end-to-end game-load evidence. |
| E-026 | API-first product decision | Local LLM/STT/TTS/embedding downloads are not required or offered. | Keep the base app model-free; hosted providers own conversation inference. | Verify UI/catalog cannot auto-download or imply local conversation packs. |

## Lip-sync, composition and accessibility evidence

| ID | Source | Relevant evidence | Decision use | Remaining validation |
| --- | --- | --- | --- | --- |
| E-040 | [Audio2Face-3D collection](https://github.com/NVIDIA/Audio2Face-3D) and [SDK executor documentation](https://github.com/NVIDIA/Audio2Face-3D-SDK/blob/main/docs/README.md) | Regression models produce facial animation from audio and the SDK exposes geometry/blend-shape execution. | Candidate coefficient source for a project-owned tracked 2D mouth residual; no native game-rig path. | Exact model/SDK/license, Windows runtime, coefficient mapping, cancellation, latency, RAM/VRAM, quality and game impact. |
| E-041 | [NVIDIA AR SDK LipSync processing contract](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/using-ar-features.html#lipsync), [installation](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/InstalltheARSDK.html), and [vendor performance reference](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/PerformanceReference.html) | Separate NGC-distributed feature consumes synchronized video/audio and emits modified frames; vendor reports device-specific latency. | Access-controlled direct-video experiment only. | Exact entitlement/license, supported Windows/GPU path, startup/pre-roll, local latency, residual isolation, size/VRAM, quality and game impact. |
| E-042 | V1 repository audit and `single-monitor-root-cause.md` | Opaque mirror, fixed geometry and frozen rectangle fail structurally. | Remove mirror/full-frame/pasted-rectangle designs. | Rendered WGC/DComp acceptance matrix. |
| E-043 | [WCAG 2.2](https://www.w3.org/TR/WCAG22/) and [Xbox Accessibility Guidelines](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/) | Web/Windows gaming accessibility criteria and test guidance. | Keyboard/Narrator/contrast/scaling/reduced-motion UI gates. | Screen reader/controller and rendered review. |
| E-044 | [LatentSync](https://github.com/bytedance/LatentSync) | Offline-oriented lip-sync research implementation. | Offline comparison only; not a live candidate. | Keep outside the live pack catalog. |
| E-045 | [MuseTalk](https://github.com/TMElyralab/MuseTalk) and local attested report | Upstream has prepared-avatar/realtime-oriented code, but the measured standalone Windows batch path took ~102 s for 1.579 s output. | Offline comparator only; measured path fails live admission. | No persistent worker, app integration, redistribution or live-game qualification. |
| E-046 | [EfficientSync](https://arxiv.org/abs/2608.18832) and [FlashLips](https://arxiv.org/abs/2512.20033) | Papers describe localized deformation/reconstruction intended to reduce full-frame generation cost. | Architecture watchlist only. | Public code/weights, exact licenses, Windows runtime, cancellation and reproduced visual/resource evidence. |

## Conclusions supported by multiple sources

1. Use Tauri only for control UI, not the continuous media path (E-001–E-008).
2. Keep authoritative memory in SQLite and treat vector data as rebuildable (E-009–E-010 plus the legacy audit).
3. Provider models are capability-discovered and cataloged, not hard-coded forever (E-020–E-025).
4. Conversation is API-first; the only optional model-pack lane is generic local screen-space lip-sync (E-026, E-040–E-046).
5. Reject native-rig/per-game integration and require generic screen-space animation to yield immediately to audio/subtitles (E-040–E-046).
6. Release claims require local end-to-end measurements and rendered evidence; upstream/vendor claims only choose prototypes.
