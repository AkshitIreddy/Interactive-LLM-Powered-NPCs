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

Current research lanes are: Audio2Face-3D regression v2.3 as a coefficient source for a project-owned tracked 2D mouth residual; NVIDIA Maxine AR SDK LipSync as an access-controlled, direct-video experiment; and MuseTalk 1.5 as an offline comparator after its measured batch path took approximately 102 seconds for 1.579 seconds of output. Audio2Face-3D is not connected to a native game rig, and none of these lanes is a qualified or available pack. EfficientSync and FlashLips remain paper watchlist entries pending code, weights, license and Windows qualification. Ditto remains deferred, LatentSync remains offline-only, and Wav2Lip, SadTalker and LivePortrait remain rejected as live product paths.

## Model manager contract

The base installer contains no AI model, Python pack, CUDA toolkit or FFmpeg requirement. After explicit user selection, TUF-protected catalog metadata may resolve a qualified lip-sync pack; the manager stages, resumes, verifies size/hash/signature, safely extracts, self-tests, atomically activates, repairs, rolls back, removes and reference-counts shared files.

The resource broker uses live DXGI budget, a configured game reserve, measured warm and p99 worker envelopes, load/unload cost, user VRAM ceiling and target FPS. It grants at most one optional local visual lease, drops stale frames, and suspends or unloads lip-sync before affecting hosted conversation or audio/subtitles. A captured source frame remains immutable; only a validated mouth residual may be applied to a presentation copy of the newest compatible frame.

Local LLM, STT, TTS and embedding packs remain outside the current product policy. If that policy changes, a separate architecture decision and co-residency matrix must classify combinations before download/activation, reserve live game headroom, serialize incompatible GPU work, expose cold/warm switching cost, and prohibit silent device/provider substitution.

## Consequences

- API-only users never need a model download or local model-capable GPU.
- Lip-sync pack/runtime ABI, reproducible builds, licenses and per-hardware benchmark data become visual-feature gates.
- The same candidate may need separate vendor/backend packs; catalog recommendations must explain disk/RAM/VRAM, pre-roll, warm-load cost, quality, and experimental status.
- Research/non-commercial/restricted models are excluded from normal redistribution unless exact terms permit it; direct user downloads remain segregated and cannot route around prohibited use.

## Evidence

- Audio2Face-3D: <https://github.com/NVIDIA/Audio2Face-3D>
- NVIDIA AR SDK LipSync: <https://docs.nvidia.com/maxine/ar/latest/API/Architecture/using-ar-features.html#lipsync>
- MuseTalk: <https://github.com/TMElyralab/MuseTalk>
- EfficientSync: <https://arxiv.org/abs/2608.18832>
- FlashLips: <https://arxiv.org/abs/2512.20033>
- TUF specification: <https://theupdateframework.github.io/specification/latest/>
