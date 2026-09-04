# ADR-0007: Use only generic external screen-space lip-sync

Status: Accepted architecture; CPU headless component candidate implemented, no app route or neural pack qualified
Date: 2026-08-28
Updated: 2026-09-04

## Context

SadTalker renders a complete talking-head video before playback and cannot meet the latency, resource or anchoring needs of a live game. Per-game facial-rig integration could improve quality, but it requires mods, hooks, game-specific adapters, exact-build work or publisher cooperation. That conflicts with the product goal of one broadly usable Windows companion.

Conversation remains API-first: the current product has no local LLM, STT, TTS or embedding-model download route. Generic screen-space lip-sync is the only contemplated local AI workload, is optional, and must never delay or disable audio/subtitles.

## Decision

The product supports no native-rig, mod, hook, DLL-injection or per-game facial adapter path. Lip-sync, when enabled, is a game-agnostic **screen-space** capability over externally captured frames.

The default low-latency control source is a **provider-neutral, playback-clocked
canonical viseme timeline**. Exact TTS phoneme/viseme events are preferred when
available; a bounded local spectral classifier handles PCM-only providers and
amplitude is the final fail-safe. Audio2Face-3D remains an optional future
audio-to-animation control source. It produces facial geometry or blend-shape
animation rather than a modified game image; only approved mouth/jaw controls
could be mapped onto the tracked 2D mouth mesh. It does not connect to a game
rig, write game state or receive game memory.

**NVIDIA Maxine AR SDK LipSync** is a separate, experimental direct-video candidate. Its documented contract consumes synchronized video frames and audio and returns modified video frames. Evaluation requires access to the NGC-distributed feature, exact license and redistribution review, supported Windows/GPU qualification, startup/pre-roll measurement, and reproduction of latency, quality, VRAM and game-impact results. Vendor measurements select a spike; they are not product benchmarks. A normal hosted NIM model key is not evidence that this separate SDK feature is entitled or usable.

**MuseTalk 1.5 is offline/comparator-only.** The attested standalone Windows batch-path run took approximately 102 seconds to produce 1.579 seconds (39 frames) of output. That run proves only that the isolated upstream path produced a valid file; it fails live-latency admission and does not qualify a persistent worker, app integration or redistributable pack.

**EfficientSync and FlashLips remain paper watchlist entries.** Their deformation/reconstruction approaches are relevant to preserving the current frame while localizing mouth edits, but paper-reported speed is not local evidence. Code, weights, licenses, Windows support, cancellation, resource use and visual behavior must all exist and pass the same gates before either can become a pack candidate.

## Immutable-current-frame contract

Each captured source frame is immutable. A visual worker may return only a bounded mouth-region residual associated with the exact actor ID, capture-frame ID, QPC timestamp and cancellation generation that produced it. The compositor applies an accepted residual to a presentation copy of the newest compatible frame; it never mutates or recursively feeds a generated frame back into tracking or inference.

Tracking, pose, dynamic mask, optical flow, occlusion, identity confidence,
color/lighting, frame age and resource budget gate composition. Provider-neutral
TTS alignment/visemes use bounded start and duration samples on the exact
playback stream generation. The native broker rejects malformed, unordered,
overlapping, excess or lease-escaping cues; audio-derived features are the
explicit fallback. A late, stale, low-confidence, wrong-identity, out-of-mask or
over-budget result is discarded. The untouched current game frame is presented
within one display refresh, while conversation continues through audio/subtitles.

Never freeze the full game frame, paste a rectangular face or display a generated full-frame replacement.

## Resource admission and residency

The current application admits at most one optional local visual lease. Admission uses live DXGI budget/headroom, a configured game reserve, measured warm and p99 workspace VRAM/RAM, backend/driver compatibility, frame-time target and the candidate's load/unload cost. Total advertised VRAM alone is not sufficient. The scheduler may keep a proven visual worker warm only while its lease remains safe; under pressure it drops stale work, suspends or unloads animation, and leaves hosted conversation and audio playback unaffected.

If future scope adds local LLM, STT, TTS or embedding models, that requires a new architecture decision and measured co-residency matrix. Before download or activation, every requested combination must be classified as safe resident, serialized/cold-load only, CPU-only, conflicting or unverified. Admission must account for the running game's reserve and per-model p99 workspace, use exclusive leases for incompatible GPU stages, and expose load/unload latency. It must not silently co-reside, switch execution devices, evict a user-selected route or fall back to a cloud provider.

## Go/no-go gates

Screen-space animation must sustain at least 30 generated FPS or a visually validated 15 FPS temporal mode, p95 capture-to-composite at most 50 ms, p95 mouth-anchor drift at most 3% face width, at most 0.1% pixel changes outside the permitted mask, clean occlusion/stale recovery, and blind preference over unmodified/audio-only presentation. It must pass the 12 GB VRAM contention scenario without violating the configured game-impact budget.

Candidates that fail remain research-only or are removed; they do not block conversation or the release.

## Consequences

- Profiles advertise only external audio/subtitles and, when evidence exists, experimental generic screen-space animation.
- The coefficient-driven baseline and direct-video experiment share the same residual-validation and fail-open compositor boundary.
- Visual workers can crash, unload or quarantine without affecting playback.
- Tests require legally sourced or synthetic moving-face sequences, exact current-frame comparison and rendered human review—not isolated model FPS.
- No profile requires a mod or executable component, and no visual candidate receives game memory, rig access or code-injection authority.

## Evidence

- [Audio2Face-3D models and tools](https://github.com/NVIDIA/Audio2Face-3D)
- [Audio2Face-3D SDK executor documentation](https://github.com/NVIDIA/Audio2Face-3D-SDK/blob/main/docs/README.md)
- [NVIDIA AR SDK LipSync processing contract](https://docs.nvidia.com/maxine/ar/latest/API/Architecture/using-ar-features.html#lipsync)
- [NVIDIA AR SDK Windows installation](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/InstalltheARSDK.html)
- [NVIDIA AR SDK vendor performance reference](https://docs.nvidia.com/maxine/ar/latest/WindowsARSDK/PerformanceReference.html)
- [MuseTalk](https://github.com/TMElyralab/MuseTalk)
- [EfficientSync paper](https://arxiv.org/abs/2608.18832)
- [FlashLips paper](https://arxiv.org/abs/2512.20033)
