# Version 1 behavior comparison

Status: reviewed against immutable revision
`503ef3b64a921b6a11efa9e3e0432a0c3de3b619` using `git show` and
`docs/legacy/` only. No notebook/generator was executed, no pickle was loaded,
no credential was read, and `temp.py` was not run.

## Preserve the behavior, replace the mechanism

| Useful v1 behavior | V1 method and failure | Modern disposition |
| --- | --- | --- |
| Push-to-talk and visible response status | Focus-bound OpenCV `t`, blocking microphone/STT, fixed delays | Global/system-owned PTT with authoritative key-up, streaming events, cancellation, and an accessible main-app trace |
| Audio-only fallback | Useful branch, but still webcam-first, serial, file-based, and likely mismatched MP3/WAV playback | Audio/subtitles are the reliable baseline; vision and visual response never block a turn |
| Known-character identity, lore, style, memory, and voice | Single-frame DeepFace search, mutable files, executable `voice.py`, mixed authority | Explicit game/character IDs; optional temporally locked identity evidence; typed prompt/lore/voice/memory data with provenance |
| Background NPC variety and continuity | One destructive `default` slot; guessed age/gender/race; ten-minute random replacement | Stable encounter IDs, deterministic game-authored archetypes/names/voice traits, manual correction, neutral unknown state, no demographic inference |
| Layered world/public/character/recent/long-term context | Full prompt injection, random examples, pickle-backed Chroma, mutable summaries | Provenance/spoiler-aware retrieval, deterministic examples, versioned rebuildable indexes, immutable delivered-turn sources |
| Conversation continuity | Player/NPC lines persisted before delivery; failures become false history | Commit only delivered/heard text with audio/subtitle receipts; raw turns remain authoritative |
| Stable/diverse voices | Model text interpolated into Python and executed; binary demographic voice lists | Provider-neutral voice traits resolved to approved stock voices; all configuration remains typed data |
| Optional facial presence | Full SadTalker render before playback; unchecked subprocess; pasted rectangle on frozen frame | Optional actor-locked, same-frame, mouth-only residual with immediate untouched-frame fallback; no candidate ships before quality/performance/license gates |
| In-game feedback | Opaque 1920x1080 OpenCV mirror, self-capture, focus loss, fixed geometry | WGC by selected HWND, system PTT, click-through overlay, PMv2/HDR-aware transforms, bottom-center subtitle fallback |
| Cloud conversation/retrieval | Cohere-only clients, plaintext rotating keys, no cancellation/retry contract | Explicit typed providers, OS credential references, bounded retries, measured latency, and no silent fallback |

## Modern identity decision

Tracking owns continuity; recognition never does. The pipeline is:

1. WGC capture session issues monotonic frame IDs and QPC timestamps.
2. A detector produces candidate heads/faces.
3. ByteTrack is the low-cost temporal baseline; BoT-SORT with optional ReID is
   the higher-cost comparison for camera motion and occlusion.
4. Addressed-actor selection combines manual target, persistent screen
   position, subtitle/name-tag OCR when profile-supported, speaking/turn timing,
   and optional embedding similarity.
5. Known identity requires multi-frame consensus, calibrated per-game
   threshold, second-best margin, provenance-aware references, and a sticky
   confidence lock.
6. Ambiguity preserves the current explicit selection or asks the user. One
   weak frame cannot switch identity.
7. Prompt, lore, stock voice, and memory bind only after the actor lock.
8. Offscreen/occluded turns continue through audio/subtitles until explicit
   contrary evidence arrives.

Public InsightFace weights are not automatically product-redistributable; code
license and model license are separate. Any detector/embedding pack requires a
verified production license and content-addressed manifest.

## Data migration boundary

Only provenance-reviewed, human-readable lore/biography/style data is a
candidate for deterministic import. Never execute or deserialize legacy
notebooks, Python orchestration, per-character voice modules, `temp.py`,
Chroma/pickle representations, generated biographies, stored credentials, or
mutable conversation state.

