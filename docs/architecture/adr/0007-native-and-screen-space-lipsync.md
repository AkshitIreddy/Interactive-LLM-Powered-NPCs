# ADR-0007: Use only generic external screen-space lip-sync

Status: Accepted visual architecture; component candidates exist, no product visual route or pack is qualified

Date: 2026-08-28

Updated: 2026-09-05

Reconciled: 2026-09-07

## Context

SadTalker renders a complete talking-head video before playback and cannot meet the latency, resource or anchoring needs of a live game. Per-game facial-rig integration could improve quality, but it requires mods, hooks, game-specific adapters, exact-build work or publisher cooperation. That conflicts with the product goal of one broadly usable Windows companion.

The base product remains API-first and model-free. ADR-0005 now permits explicitly
selected, signed, measured local packs for language model, speech recognition, speech
synthesis, embedding, vision, and lip-sync roles. This decision is narrower: any lip-sync
implementation remains generic screen-space work and must never delay or disable
audio/subtitles.

Historical note: the 2026-08-28 revision called lip-sync the only contemplated local AI
workload. ADR-0005 superseded that pack-scope restriction on 2026-09-05. It did not change
the external, non-injecting visual boundary defined here.

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

Current visual orchestration, worker transport, atlas, and compositor code is component
infrastructure, not an accepted rendering method. The September 5 output was rejected
visually for painted-cavity appearance, missing teeth, and upper-lip damage. It must not
be promoted because motion or mask metrics pass.

Follow-up evidence replaces that output as the newest experiment, not as product
acceptance. The September 7 schema-3 Cyberpunk/Misty moderate-OH replay uses recorded
native YuNet/LM1 mouth geometry, periodic 12-frame detection with tracked-ROI updates,
smoothed state selection, and identity-bound photometric references. Its admission-event
digest matches v14, all 20 source-identical frames remain exact, and no pixel changes
outside the dynamic residual bounds. The state-7 reference makes rounded articulation
materially more restrained, but a photometric/pasted seam remains and its generated
anatomy is not observed game anatomy. Earlier opacity/EMA and rounded-reference scaling
experiments introduced double contours or spoke artifacts and remain rejected. The
moderate-OH artifact is accepted for local review only; it is not installed-provider,
live-capture, natural-animation, game-load, or end-to-end latency proof.

The target renderer is source-conditioned. It builds a recent high-confidence neutral
mouth reference for the locked actor, stabilizes a current-frame 2D/2.5D lower-face mesh,
warps source pixels for jaw/lip motion, and inpaints only newly exposed inner-mouth pixels
inside the dynamic mask. It follows the exact playback-clocked viseme timeline, reprojects
onto each newest compatible game frame, matches lighting/color, and discards work through
pose jumps, cuts, occlusion, identity uncertainty, or staleness. Semantic viseme updates
may use a visually qualified 15 Hz temporal mode, but presentation remains at display
cadence. A generic painted mouth atlas is not sufficient product output.

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

The visual coordinator and Model Manager define at most one optional local visual lease,
latest-frame scheduling, measured pack envelopes, and whole-loadout preflight. The target
admission decision uses native-stamped current DXGI budget/headroom, the configured game
reserve, measured warm and p99 workspace VRAM/RAM, backend/driver compatibility,
frame-time target, and load/reload cost. Total advertised VRAM is never sufficient.

This is not yet production enforcement. Runtime-core has an opt-in ResourceBroker path,
but the normal runtime host does not yet receive the trusted selected-game budget and
qualified whole-turn plan needed to drive it. Until that join exists, component policy
and simulated telemetry cannot qualify visual activation.

ADR-0005 governs co-residency with all local roles. Before download or activation, the
complete selected combination is classified as safe resident, serialized/cold-load only,
CPU-only, conflicting, or unverified. Admission accounts for the running game's reserve
and every role's p99 workspace, uses exclusive leases for incompatible stages, and shows
load/unload latency. It never silently co-resides, switches device/provider, evicts a
user-selected route, or falls back to cloud.

A proven visual worker may remain warm only while its current lease is safe. Under
pressure the scheduler drops stale work, suspends or unloads animation, and leaves
conversation audio/subtitles unaffected.

## Go/no-go gates

Screen-space animation must sustain at least 30 generated FPS or a visually validated 15 FPS temporal mode, p95 capture-to-composite at most 50 ms, p95 mouth-anchor drift at most 3% face width, at most 0.1% pixel changes outside the permitted mask, clean occlusion/stale recovery, and blind preference over unmodified/audio-only presentation. It must pass the 12 GB VRAM contention scenario without violating the configured game-impact budget.

Candidates that fail remain research-only or are removed; they do not block conversation or the release.

## Consequences

- Profiles advertise only external audio/subtitles and, when evidence exists, experimental generic screen-space animation.
- Every source-conditioned, coefficient-driven, or direct-video candidate shares the
  same residual-validation and fail-open compositor boundary; none bypasses rendered
  human review.
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
