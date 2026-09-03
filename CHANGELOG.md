# Changelog

Notable 2.0 changes are recorded here. No 2.0 version has been published; version numbers remain provisional.

## [Unreleased]

### Added

- Added `GameProfileV2` schema/semantic validation and 20 authored declarative profile documents. All 20 validate in the current checkout; individual capabilities are labeled `replay_verified`, `experimental`, or `unsupported`, and none are represented as `live_certified` without live-game evidence.
- Added a development provider/model catalog with explicit privacy, consent, discovery, integrity, and deterministic voice-intent rules. The catalog is unsigned and development-only.
- Added deterministic virtual-time simulation for happy path, offscreen naming, interruption, provider failure, animation-worker failure, low VRAM, and offline privacy. Virtual benchmark output is explicitly noncanonical.
- Added SQLite memory schema/store/retrieval foundations with WAL/STRICT/FTS5, provenance, retention, deterministic hybrid ranking, embedding work queue, import, backup, and reindex behavior.
- Added a production-capable Model Manager core with bounded resumable HTTPS, strong ETag handling, streaming size/SHA-256 verification, Ed25519 catalog signatures, safe ZIP/TAR/TAR.GZ extraction, durable same-volume staging, atomic activation, repair, rollback, and reference-counted removal. Approved TUF roots, public catalogs, pack-specific self-tests, and UI integration remain gated.
- Added local packaging policy, canonical PowerShell developer commands, security hooks, and a disabled update boundary.
- Added typed runtime-core contracts for cancellation/barge-in, streaming sentence delivery, independent effects validation, provider/privacy policy, circuit breakers, structured timing, degradation, and delivered-only commits. Real provider, device, persistence, and game integrations remain separate.
- Added `EnvelopeV1` and versioned Protobuf contracts for negotiation, identity/order/deadline/cancellation validation, STT/LLM/TTS streams, effects, typed errors, provider metadata, model packs, and profiles.
- Added the C++20/WinRT media broker with real HWND-targeted free-threaded WGC, event-driven `IAudioClient3` capture/render, bounded PCM rings, transparent no-activate DirectComposition residual presentation, Desktop Duplication fallback, DPI-aware geometry, device-loss recovery, and fail-open visual gating. Authenticated one-time PCM playback leases and exact current-frame source leases now cross the process boundary; qualified visual-worker/landmark-pack presentation, HDR shaders, and live-game evidence remain integration gates.
- Added the complete Tauri 2/React Response Console, ten-step onboarding, all planned product areas and states, high-contrast/reduced-motion support, typed commands, atomic onboarding persistence, restricted CSP/capabilities, tray lifecycle, a supervised authenticated runtime sidecar, Job Object parent-death cleanup, and native Windows Credential Manager commands.
- Added tested hosted adapter cores for OpenAI, Anthropic, Gemini, Groq/OpenAI-compatible, and Cohere LLMs; Deepgram, AssemblyAI, ElevenLabs, and OpenAI STT; and Cartesia, ElevenLabs, Inworld, and Deepgram TTS, all behind mockable transports with cancellation, privacy, and redaction tests. Adapter-core coverage is not ordinary-runtime availability: the selected-route TTS runtime constructs ElevenLabs plus experimental NVIDIA Magpie only, while Cartesia, Deepgram, and Inworld TTS remain catalog/core-only and fail closed if selected.
- Added explicit NVIDIA NIM routes under one user-owned key: dedicated chat and 2048-dimensional embedding adapters with successful bounded synthetic endpoint smoke evidence; an experimental stock-voice Magpie TTS adapter with successful HTTP audio evidence, an authorized live Aria stream through the concrete fixed-origin Riva gRPC transport, and an ordinary selected-route builder for authenticated broker PCM delivery; and a Nemotron streaming-ASR adapter that remains disabled after two bounded live timeouts. Magpie remains explicit and experimental until an authorized end-to-end normal-turn, reliability, physical-endpoint, and game-load run passes; reranking remains non-selectable because the tested hosted endpoints were unavailable. No NVIDIA route is automatically selected, used as a fallback, or represented as production-ready.
- Added `npc-runtime.exe` with privacy-safe doctor, 20-profile validation, deterministic offline turn simulation, delivered-only SQLite commits, bounded authenticated control IPC, Windows named-pipe ACLs, and child-supervision policy.
- Added deterministic local-worker protocol stubs and pack descriptors for llama.cpp, Moonshine, whisper.cpp, Kokoro, ONNX embeddings/vision, and experimental lip-sync without bundling third-party binaries or models.
- Added game discovery parsers and evidence merging for Steam, Epic, GOG, common/manual selections, a privacy-safe diagnostics/export core, and Windows Credential Manager storage.
- Added the local debug NSIS packaging path with all 20 profiles, provider catalog, project-owned runtime/media sidecars, disabled updater metadata, SHA-256 manifest, and no publication step.
- Added the bounded D3D12/CPU/RAM game-load harness with DXGI budget awareness, thermal/device aborts, PresentMon-observable output, explicit power-profile metadata, and a dry-run default that never changes Windows or G-Helper power settings.
- Added a CPU-only exact-frame mouth compositor with causal PCM smoothing,
  lower-jaw-biased motion, shallow-cavity softening, and restrained
  exposure-matched teeth/tongue detail. Its headless proof supports acquired
  face ROI carry and adaptive 10/15 Hz tracking while compositing at 30 FPS.
- Completed local-only credential and synthetic provider smoke tests for Cohere, ElevenLabs, AssemblyAI, and the qualified NVIDIA NIM routes. Only redacted ignored evidence was retained; keys were never placed in tracked source or logs.
- Passed a local Debug NSIS install/launch/forced-parent-termination/uninstall smoke with authenticated runtime/media-broker health, direct child topology, protected AppData ACLs, and no orphan process, file, shortcut, or registry residue. Runtime doctor was degraded only by the intentionally unsigned development catalog. The package is permanently classified `unchecked-development-package`, remains local and unsigned, and does not replace the clean Windows 10/11 VM matrix or count as RC evidence.

### Verified locally

- Passed the full locked Rust workspace tests, root formatting, and strict Clippy.
- Passed 28 Response Console tests, TypeScript typecheck, and the production Vite build.
- Passed 22 worker tests, all 20 profile validations/replays, and 15 deterministic simulation checks across seven scenarios.
- Passed 2/2 native media-broker CTests and the inert game-load harness path; no live-game or performance result is claimed.
- Validated 152 local documentation links and passed current-tree secret scanning, strict license/provenance checks, and deterministic complete CycloneDX SBOM generation.
- Passed the isolated mouth core on two license-safe moving real-person clips
  with a real NVIDIA Magpie WAV. The primary 960x720 proof measured 28.825 ms
  p95 tracking at 10 Hz, 2.027 ms p95 compositing at 30 FPS, and 0 GPU VRAM;
  unsupported close-up, low-landmark-confidence, and stylized inputs failed
  closed without weakening production gates.

### Remaining integration gates

- Qualify the existing authenticated current-frame transport with an admitted landmark/visual pack, HDR presentation, and live-game capture/composition evidence; keep every stale, occluded, protected, or invalid visual result fail-open to the untouched frame.
- Exercise local adapters with approved Model Manager packs; no credential value, third-party runtime, or model weight is included in the repository.
- Live-qualify NVIDIA Nemotron ASR audio before making that route selectable. Keep Magpie explicit and experimental: its fixed-origin gRPC transport is live-qualified and the ordinary selected-route runtime can construct it for authenticated broker PCM delivery, but authorized end-to-end normal-turn, reliability, physical-endpoint, and game-load evidence remain separate gates. Keep NVIDIA reranking unavailable until an eligible hosted endpoint is demonstrated.
- Qualify separately installed generic talking-head/lip-sync candidates that accept an image plus audio, then expose only eligible packs as explicit user choices with model-specific quality, Windows runtime, storage/RAM/VRAM, GPU-contention, latency, game-impact, and license evidence. There is no hosted Audio2Face one-key route and no qualified generic lip-sync pack today.
- Complete clean-VM install/uninstall/update recovery, live-game certification, controlled power-profile benchmarks, production signing, and protected/anti-cheat refusal review.
- Promote model/runtime catalogs from unsigned development fixtures only after TUF roots, exact artifacts, licenses, and self-tests are approved.
- Resolve the reachable-history `apikeys.json` filename scan only with explicit approval for any history rewrite; the current source tree passes secret scanning.
- Provision production catalog/TUF trust roots, an approved production pack runner, qualified optional lip-sync packs, and signing/update infrastructure. No production pack is currently selectable or bundled.

### Documentation

- Replaced prototype setup and universal-compatibility claims with a truthful 2.0 pre-release overview.
- Added user, developer, reference, troubleshooting, security, support, contribution, legal, and roadmap documentation.
- Distinguished authored, replay-verified, and live-game-certified capabilities.
- Documented that acceptance thresholds are targets, not results.

### Security direction

- Deprecated plaintext keys, executable character voices, model-generated Python execution, pickle/Chroma migration, and demographic inference.
- Established protected-mode refusal, explicit provider routing, credential isolation, declarative profiles, signed adapters, and verified pack requirements.

### Legacy status

- 1.x notebooks and bundled dependencies are unsupported migration evidence, not a 2.0 execution path.

## Release status

There are no 2.0 releases, public installers, update feeds, model catalogs, or official benchmark results. A local RC must pass documented gates and receive explicit approval before public release action.
