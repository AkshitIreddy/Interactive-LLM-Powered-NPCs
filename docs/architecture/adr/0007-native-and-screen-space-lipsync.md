# ADR-0007: Use only generic external screen-space lip-sync

Status: Accepted direction; candidates remain experimental until gates pass  
Date: 2026-08-28

## Context

SadTalker renders a complete talking-head video before playback and cannot meet the latency/resource/anchoring needs of a live game. Per-game facial-rig integration could improve quality, but it requires mods, hooks, game-specific adapters, exact-build work, or publisher cooperation. That conflicts with the product goal of one broadly usable Windows companion.

## Decision

The product supports no native-rig, mod, hook, DLL-injection, or per-game facial adapter path. Lip-sync, when enabled, is a single game-agnostic **screen-space** capability over externally captured frames.

Compare a lightweight tracked viseme/mouth-warp residual baseline with MuseTalk 1.5 as the public experimental comparator. NVIDIA AR SDK LipSync is a conditional candidate only if its private NGC access, Windows/Ada path, per-frame image plus 16 kHz mono input contract, region/tracking behavior, fixed 14-frame pre-roll, license, quality, and game-impact constraints can be qualified; a normal NVIDIA NIM key does not grant access. Ditto is deferred, and LatentSync is an offline-rendering comparison rather than a live candidate.

Generate only the mouth-region residual over the current live frame. Tracking, pose, dynamic mask, optical flow, occlusion, identity confidence, color/lighting, and frame age gate composition. The system uses provider-neutral TTS alignment/visemes when available and audio-derived features otherwise. Native rigs/Audio2Face, Wav2Lip, SadTalker, and LivePortrait are not live product paths.

Never freeze the full game frame or paste a full rectangular face. If track confidence, freshness, occlusion or resource budget fails, reveal the untouched frame within one display refresh and continue audio/subtitles.

## Go/no-go gates

Screen-space animation must sustain ≥30 generated FPS or a visually validated 15 FPS temporal mode, p95 capture-to-composite ≤50 ms, p95 mouth-anchor drift ≤3% face width, ≤0.1% pixel changes outside the permitted mask, clean occlusion/stale recovery, and blind preference over unmodified/audio-only presentation. It must pass the 12 GB VRAM contention scenario without violating the configured game-impact budget.

Candidates that fail remain experimental or are removed; they do not block conversation or the release.

## Consequences

- Profiles advertise only external audio/subtitles and, when evidence exists, experimental generic screen-space animation.
- TTS alignment/viseme support becomes a capability, with audio-derived fallback.
- Visual workers can crash/quarantine without affecting playback.
- Tests require legally sourced/synthetic video sequences, fresh-frame comparison and rendered human review—not model FPS alone.
- No profile requires a mod or executable component, and no visual candidate receives game memory, rig access, or code-injection authority.

## Evidence

- MuseTalk: <https://github.com/TMElyralab/MuseTalk>
- NVIDIA LipSync model card: <https://build.nvidia.com/nvidia/lipsync/modelcard>
- LatentSync: <https://github.com/bytedance/LatentSync>
