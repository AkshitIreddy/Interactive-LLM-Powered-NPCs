# Realistic captured-face qualification update

Status: local review evidence, not release or production admission  
Measured: 2026-09-01 UTC  
Target PC: Windows 11, NVIDIA GeForce RTX 4080 Laptop GPU  
Large artifacts: `E:\temp\InteractiveNPCs`

## Why this run exists

The earlier pixel-art synthetic actor could validate capture plumbing but could
not answer whether the product's face pipeline works on the intended domain. It
has been replaced by an original generated photorealistic fictional character,
Mara Venn, in a realistic harbor-control scene. The source target deliberately
keeps Mara's mouth pixels static while rain and peripheral HUD elements move.
That makes any observed mouth motion attributable to the tested renderer rather
than to the source fixture.

This preserves the useful version-1 intent without reviving its unsafe method:

1. detect a face in the currently selected game window;
2. match it against a game-scoped character gallery;
3. bind the locked character to prompt, voice, memory, and provider loadout;
4. synthesize speech;
5. animate only the same temporally locked on-screen actor; and
6. fall open to untouched frames plus audio/subtitles when confidence or timing
   fails.

Version 1 applied a full talking-head render to one captured still. Version 2
separates identity observations, actor authority, landmarks, audio-driven pixel
generation, and final compositing so one weak detection or a memory match can
never select a face by itself.

## Generated source fixture

The built-in image generator produced an original 1672x941 portrait plus two
reference angles. Exact prompts and SHA-256 values are in
`scripts/synthetic-game-replay/assets/PROVENANCE.md`. The target embeds the main
portrait as a verified resource; the fixture self-test proves the mouth guard
region is invariant across source frames while the rest of the scene advances.

## Live hosted routes

All requests used native/provider credential sources. No secret value or raw
credential was written to source, reports, screenshots, or command lines.

| Route | Bounded result | Decision |
| --- | ---: | --- |
| Cohere chat | 492 ms; constrained `READY` response | Usable API-first LLM option for review |
| ElevenLabs Flash v2.5 | 921 ms request; 1.579 s valid non-silent WAV | Usable API-first TTS option for review |
| AssemblyAI | Correct transcript; confidence 0.8914; 7.172 s job | Functional non-streaming review route; not a low-latency result |
| NVIDIA Nemotron 3.5 Lightning 30B A3B | 700 ms bounded chat | Usable private-evaluation LLM option |
| NVIDIA Nemotron 3 Embed 1B | 232 ms; 2048 finite dimensions | Usable private-evaluation embedding option |
| NVIDIA Magpie stock Aria | 918 ms; 1.3 s valid non-silent WAV; 86 voices discovered | Usable private-evaluation stock-voice option |
| NVIDIA hosted rerank | HTTP 404 | Not selectable |
| NVIDIA ASR HTTP probe | HTTP 404; current official route is NVCF/Riva gRPC | Keep blocked until the in-app gRPC route passes |

These are functional probes, not permanent availability, reliability, game-load,
production-entitlement, or cost guarantees.

## Real local model results

| Component | Result | Product role |
| --- | --- | --- |
| MuseTalk 1.5 | 39 frames / 1.56 s at 1672x940; 87.873 s wall; 7,836 MiB peak GPU used; visually distinct mouth shapes with stable identity/background | Realistic-face offline comparator only; stock batch path fails live latency |
| OpenSeeFace MNV3 + LM1 | 30/30 detections; p95 29.61 ms CPU; detector 0.824, landmarks 0.887, mouth 0.933 | Generic human-face tracking/landmark signal only; generates no pixels and recognizes no identity |
| OpenCV YuNet + SFace | same-identity cosine min 0.8439; negative max 0.4370; margin 0.4069; about 200 ms per large gallery image | Game-scoped known-character observation source; no actor-selection authority |
| BGE Small English v1.5 | Existing 24-sample CPU evidence: p99 load 1,669 ms, reload 1,462 ms, operation 32 ms, RAM about 233 MB, zero VRAM, 6/6 top-1 semantic suite | Only local conversation pack with a current non-production review envelope |
| Qwen3 4B Q4_K_M | Existing one-run Vulkan evidence: 17.47 s load, 7.80 s TTFT, 18.47 tok/s, 3,623 MiB resident VRAM delta | Functional standalone local LLM; single stale-manifest sample is not admission evidence |
| Moonshine v2 Medium Streaming | Existing 21-cycle evidence: p99 RTF 4.123 and approximately 80.7% CPU | Functional but fails the low-latency local STT target |
| Kokoro v1.0 INT8 | Pack and worker implemented; no accepted real Windows qualification | Not selectable |

The app remains API-first for LLM, STT, and TTS. BGE and OpenSeeFace may be
installed in the isolated review namespace only through the signed catalog and
explicit user action. No complete local lip-sync renderer is admitted.

## What “works on any face” can honestly mean

No current model supports every possible face from every image source. The
supported domain is a visible, sufficiently large, mostly frontal human face
with one addressed actor and usable mouth detail. Stylized characters, masks,
helmets, animals, creatures, extreme profile, heavy occlusion, multiple complete
faces, motion blur, and protected capture must be detected as unsupported or
ambiguous. The safe behavior is immediate fallback to the untouched captured
frame plus speech/subtitles—not a guessed identity or distorted face.

OpenSeeFace is useful inside that supported domain because it observes the
current frame at interactive CPU latency. It cannot make the face talk. The
renderer must independently consume the locked actor, fresh source frame and
delivered audio, and its output must pass identity, mask, freshness, cancellation
and resource gates before presentation.

## Best current renderer direction

The current [NVIDIA LipSync model card](https://build.nvidia.com/nvidia/lipsync/modelcard)
is the closest documented Windows-local match for one realistic complete human
face plus PCM audio, but its [deployment path](https://build.nvidia.com/nvidia/lipsync/deploy)
requires NVIDIA AI for Media/private access. It is not an ordinary hosted
one-key endpoint and has not been qualified beside a game.

MuseTalk 1.5 remains the best reproduced realistic-frame comparator in this
checkout. A persistent precomputed avatar/reference cache, bounded mouth-only
crop, CUDA graphs/TensorRT where compatible, latest-frame-wins queues, and
audio-clock scheduling are the engineering path to test next; the measured
stock batch path itself is not shippable. Ditto, JoyVASA/LivePortrait, and newer
streaming heads remain research candidates until an exact Windows stack,
license, artifacts, latency, VRAM, cancellation and visual evidence pass.

## Evidence locations

- fixture validation: `artifacts/synthetic-replay/validation`
- hosted probes: `E:\temp\InteractiveNPCs\provider-smoke\2026-09-01-realistic-fixture`
- MuseTalk: `E:\temp\InteractiveNPCs\local-model-tests\musetalk-mara-v2`
- OpenSeeFace: `E:\temp\InteractiveNPCs\local-model-tests\openseeface-mara-v2`
- identity gallery: `E:\temp\InteractiveNPCs\local-model-tests\identity-mara-v1`

Every external report is local-review evidence. It is not bundled, published,
or promoted by this document.
