# Model and dependency license gate ledger

This is a candidate ledger, not legal advice or redistribution approval. `Candidate` means the component may be benchmarked. `Blocked pending review` means no pack/public artifact may include it until the exact revision, weights, auxiliary assets, runtime and intended use have been reviewed together.

| Component | Task | Upstream license signal | Current disposition | Required before packaging |
| --- | --- | --- | --- | --- |
| Hosted LLM/STT/TTS/retrieval services | Conversation | Provider/model-specific service terms | API-first product route; not redistributed | Refresh model availability, pricing/quota, privacy/retention, region, and use terms in the signed catalog. |
| Qwen3-4B-Instruct-2507 Q4, Moonshine v2 Tiny/Small, Kokoro-82M INT8, BGE-small-en-v1.5 | Optional local LLM/STT/TTS/embedding | Model, weights, tokenizer/dictionary, voice and runtime licenses are separate | Qualification candidates; never first-run defaults and not yet curated downloads | Pin exact revisions/assets; audit every transitive license; measure Windows latency, quality, size, RAM, resident/p99 VRAM and load time; activate only through measured whole-loadout admission. |
| sqlite-vec | Optional local vector extension, not a model pack | MIT upstream signal | Candidate library dependency only | Pin commit, reproducible extension, security/performance test and notices. |
| TTS visemes / causal audio-to-viseme → project-owned 2D mouth warp | Generic screen-space lip-sync control signal | Provider timestamps or exact local model/runtime terms | Selected non-generative baseline architecture; no native game-rig path or qualified pack claim yet | Validate current-frame landmarks, cancellation, dynamic mask/occlusion, temporal stability, language/phoneme coverage, RAM/VRAM and game impact. |
| NVIDIA LipSync NIM | Generic direct-video lip-sync | Private-access NVIDIA software/AI product/open-model terms; ordinary Developer API key does not prove entitlement | Enterprise/offline benchmark only; not interactive default | H4M documents 30-frame lookahead; generic service still needs access, first-output/cancel, media conversion, VRAM, quality and game-impact measurement. Linux/Triton packaging is outside the Windows local-pack baseline. |
| MuseTalk 1.5 | Generic screen-space neural residual experiment | MIT code/weights signal plus independently audited transitive models/assets | Offline comparator and refactor spike only; standalone Windows batch path functionally passed, but **blocked** for live/product use | The 2026-08-29 synthetic image + real stock-audio run attested 4.175 GiB of inputs and produced a 1.579 s/39-frame MP4, but took about 102 s end-to-end through the upstream avatar path. Retain outside the catalog unless a current-frame ROI runner passes first-output, cancel, identity, temporal, license and game-contention gates. |
| EfficientSync; FlashLips | Generic screen-space lip-sync research | Papers only in this decision snapshot | Watchlist only; not pack candidates | Require public immutable code/weights, exact licenses, Windows runtime, cancellation, local latency/resource results and rendered current-frame behavior before candidacy. |
| Ditto | Generic screen-space lip-sync | Code/model/runtime terms require review | Deferred | Reconsider only after the baseline and leading candidates are qualified. |
| LatentSync | Offline lip-sync rendering | Code/model/runtime terms require review | Offline comparison only; rejected for live product path | Do not offer as a live-game pack; any research must remain isolated and provenance-complete. |
| Native game-rig animation | Rig animation | Game/vendor-specific integration terms | Rejected product path | Requires game/mod/rig integration, which is outside the generic external architecture. |
| Wav2Lip, SadTalker, LivePortrait | Full-frame/talking-head animation | Project/model-specific | Rejected live product paths | Offline/full-frame designs do not satisfy the fresh-frame masked-residual and game-impact contract. |
| Game/profile lore and biographies | Content | Author/source-specific | Author new/provenanced only | Per-record provenance, originality/license, spoiler tier, no copied wiki prose. |
| Game screenshots/audio/actor voices | Media/voice | Publisher/performer-specific | Not distributed by default | Explicit rights; otherwise synthetic/user-local assets only. No game-audio voice cloning. |

## Pack admission checklist

1. Immutable upstream URL and revision, exact file list, SHA-256 and size.
2. Code, weights, tokenizer, voice, dictionary, sample and runtime licenses reviewed separately.
3. Redistribution and commercial-use compatibility with the MIT core and intended public binary confirmed.
4. Required attribution/notices and machine-readable SPDX expression recorded.
5. Provenance and training/use restrictions reviewed; prohibited use is not routed around with direct download.
6. Windows 10/11 runtime, offline self-test, security scan and rollback pass.
7. Reference and 12 GB game-contention measurements stored with fixture/tool versions; disclose exact download/installed size and peak RAM/VRAM.
8. Model card limitations are surfaced in Model Manager.

Research/personal-use models may appear only in a clearly segregated direct-from-upstream flow when their exact license permits the intended personal use. They never enter the curated redistributable catalog merely because the application itself is free.

Lip-sync reference snapshot: [NVIDIA LipSync NIM](https://docs.nvidia.com/nim/maxine/lipsync/latest/overview.html), [Audio2Face-3D](https://docs.nvidia.com/ace/audio2face-3d-microservice/latest/text/getting-started/overview.html), [MuseTalk](https://github.com/TMElyralab/MuseTalk), [EfficientSync](https://arxiv.org/abs/2608.18832), and [FlashLips](https://arxiv.org/abs/2512.20033), refreshed 2026-08-30. Candidate names and upstream descriptions are research inputs, not product benchmarks, package qualification or availability claims.
