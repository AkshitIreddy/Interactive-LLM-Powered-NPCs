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
  lower-jaw-biased source-pixel motion, a lip-local feathered mask, and a
  source-derived cavity shadow. The PCM fallback no longer paints procedural
  teeth or tongue. Its headless proof supports acquired face ROI carry and
  adaptive 10/15 Hz tracking while compositing at 30 FPS.
- Corrected the source-preserving compositor after user review found that its
  geometric seam and symmetric cavity cut through the upper lip. The fallback
  now refines the contact seam from current-frame pixels, keeps the upper lip
  stationary, opens downward through the lower jaw, and gates protected-upper-
  lip darkening independently from whole-mouth motion.
- Upgraded the mouth signal and geometry path to preserve the complete ordered
  OpenSeeFace 18-point contour. The compositor now produces distinct curved
  open, rounded, spread, and closure shapes, locks the exact source upper-lip
  surface, and uses only conservative source-colour oral depth plus gated
  tongue/enamel hints when full contour evidence is present.
- Preserved provider TTS viseme durations end to end. The authenticated playback
  producer and native broker carry at most 128 ordered canonical cues without
  changing audio frame accounting; command 29 binds them to the WASAPI sample
  clock and applies bounded anticipation/release with decisive bilabial closure.
  Providers without cues retain a new pre-emphasized seven-band PCM classifier,
  with amplitude-only motion as the final fail-safe.
- Added explicit NVIDIA Magpie stock-voice selection to the provider smoke
  runner. It rejects ZeroShot/cloning identifiers and defaults deterministically
  to Jason, Leo, then Ray instead of preferring Aria/Sofia.
- Completed local-only credential and synthetic provider smoke tests for Cohere, ElevenLabs, AssemblyAI, and the qualified NVIDIA NIM routes. Only redacted ignored evidence was retained; keys were never placed in tracked source or logs.
- Passed a local Debug NSIS install/launch/forced-parent-termination/uninstall smoke with authenticated runtime/media-broker health, direct child topology, protected AppData ACLs, and no orphan process, file, shortcut, or registry residue. Runtime doctor was degraded only by the intentionally unsigned development catalog. The package is permanently classified `unchecked-development-package`, remains local and unsigned, and does not replace the clean Windows 10/11 VM matrix or count as RC evidence.
- Regenerated a source-clean local Debug package at `cb43a7f` with the then-current 185,856-byte CPU mouth worker, silently installed it without launching the app, and passed closed-world reconciliation for 756 installed files, 755 manifested payload files, exact four-sidecar hashes, legal resources, and unclassified-file rejection. Later visual review rejected that mouth sidecar, so the v9 package is no longer valid lip-sync quality evidence. A v10 local directory overlays the replacement audited sidecar but is not newly reconciled installer evidence. Unsafe Windows broker GUI smoke tests remained excluded, so desktop lip-sync presentation is still unaccepted.
- Built a clean-source `573f84a` Debug-review package after the sample-clocked
  cue, spectral/full-contour, and stronger-jaw upgrades. The v13 loose review directory and all
  four sidecars are SHA-256/size matched to their manifests and pass GUI-
  subsystem/import audit. The app and installer were not launched or installed;
  v13 remains `unchecked-development-package` and is not reconciliation or
  desktop lip-sync presentation evidence.

### Verified locally

- Passed the full locked Rust workspace tests, root formatting, and strict Clippy.
- Passed 124 Response Console tests, TypeScript typecheck, and the production Vite build.
- Passed 30 worker tests, 21 deterministic benchmark-harness tests, all 20 authored profile validations plus the synthetic review profile, and 22 deterministic simulation tests with 127 assertions across 13 scenarios.
- Passed the five native media-broker non-GUI CTests plus the headless synthetic renderer/review-game contract. Unsafe Windows broker GUI smoke tests were not run; no live-game or performance result is claimed.
- Validated 777 local documentation links across 150 files and passed current-tree secret scanning, strict license/provenance checks, and deterministic complete CycloneDX SBOM generation.
- Rejected both the first isolated mouth proof, which had a detached dark slit,
  flat synthetic teeth, and a female Aria fixture, and the later v16 proof,
  whose cavity crossed the upper lip despite passing its numeric motion gate.
  The current 960x720 candidate uses stock male Jason audio, full-contour
  geometry, source-derived contact-seam placement, a locked upper-lip surface,
  and curved lower-jaw opening. It measured 34.817 ms p95 moving tracking at
  10 Hz, 3.043 ms p95 compositing at 30 FPS, 0 GPU VRAM, 28 visibly changed
  frames, a 19.2% maximum articulated aperture relative to mouth width, and
  zero protected-upper-lip darkening.

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
