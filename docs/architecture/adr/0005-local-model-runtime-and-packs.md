# ADR-0005: Optional local packs are limited to generic lip-sync

Status: Accepted direction; no lip-sync model is qualified yet  
Date: 2026-08-28

## Context

Conversation is API-first: LLM, STT, TTS, and retrieval use hosted providers explicitly configured by the user. The base application must remain model-free and must not automatically download Python, CUDA, FFmpeg, runtimes, or weights. Generic screen-space lip-sync is the one optional feature that may justify a local AI pack because captured game frames should not be sent to a hosted animation service by default.

## Decision

Reserve the signed pack lifecycle for generic screen-space lip-sync candidates only. Each eligible pack must disclose before download:

- exact model and immutable revision;
- download and installed sizes;
- peak RAM/VRAM and supported Windows/GPU/backend constraints;
- license, access, use, and redistribution terms;
- measured latency, game impact, and visual-quality evidence;
- experimental limitations, privacy behavior, and immediate audio/subtitle fallback.

No pack is bundled, auto-selected, auto-downloaded, auto-activated as a default/dependency/migration/game requirement/fallback, or represented as available until its license and Windows/game-load/visual gates pass. Activation additionally requires a valid attestation and an explicit user choice. A CPython runtime may accompany an explicitly selected pack only when the qualified implementation requires it and the pack remains self-contained.

Current research lanes are: a lightweight tracked viseme/mouth-warp baseline; MuseTalk 1.5 as the public experimental comparator; and NVIDIA AR SDK LipSync as a conditional candidate if its private NGC access, Windows/Ada support, fixed pre-roll, licensing, and integration constraints can be qualified. A normal NVIDIA NIM key does not unlock that private SDK. Ditto remains deferred, and LatentSync is an offline-rendering comparison only. Native rigs/Audio2Face, Wav2Lip, SadTalker, and LivePortrait are rejected as live product paths.

## Model manager contract

The base installer contains no AI model, Python pack, CUDA toolkit or FFmpeg requirement. After explicit user selection, TUF-protected catalog metadata may resolve a qualified lip-sync pack; the manager stages, resumes, verifies size/hash/signature, safely extracts, self-tests, atomically activates, repairs, rolls back, removes and reference-counts shared files.

The resource broker uses live DXGI budget, measured worker envelope, user VRAM ceiling and target FPS. It drops stale frames, grants only the declared optional visual lease, and disables lip-sync before affecting conversation audio/subtitles.

## Consequences

- API-only users never need a model download or local model-capable GPU.
- Lip-sync pack/runtime ABI, reproducible builds, licenses and per-hardware benchmark data become visual-feature gates.
- The same candidate may need separate vendor/backend packs; catalog recommendations must explain disk/RAM/VRAM, pre-roll, warm-load cost, quality, and experimental status.
- Research/non-commercial/restricted models are excluded from normal redistribution unless exact terms permit it; direct user downloads remain segregated and cannot route around prohibited use.

## Evidence

- NVIDIA LipSync model card: <https://build.nvidia.com/nvidia/lipsync/modelcard>
- MuseTalk: <https://github.com/TMElyralab/MuseTalk>
- LatentSync: <https://github.com/bytedance/LatentSync>
- TUF specification: <https://theupdateframework.github.io/specification/latest/>
