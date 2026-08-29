# Model and dependency license gate ledger

This is a candidate ledger, not legal advice or redistribution approval. `Candidate` means the component may be benchmarked. `Blocked pending review` means no pack/public artifact may include it until the exact revision, weights, auxiliary assets, runtime and intended use have been reviewed together.

| Component | Task | Upstream license signal | Current disposition | Required before packaging |
| --- | --- | --- | --- | --- |
| Hosted LLM/STT/TTS/retrieval services | Conversation | Provider/model-specific service terms | API-first product route; not redistributed | Refresh model availability, pricing/quota, privacy/retention, region, and use terms in the signed catalog. |
| Local LLM/STT/TTS/embedding weights | Conversation | Model-specific | Not a product download route | Conversation remains API-first; the pack manager must reject these task kinds. |
| sqlite-vec | Optional local vector extension, not a model pack | MIT upstream signal | Candidate library dependency only | Pin commit, reproducible extension, security/performance test and notices. |
| Lightweight tracked viseme/mouth-warp | Generic screen-space lip-sync | Project implementation plus ordinary runtime dependencies | Low-resource baseline; no model pack claim | Rendered tracking/occlusion/mask/latency/game-impact evidence across the display matrix. |
| NVIDIA AR SDK LipSync / private NGC package | Generic screen-space lip-sync | Private-access NVIDIA terms; normal NIM API key does not grant access | Conditional candidate only | Verify exact artifact/license/access; Windows 10/11 and Ada path; per-frame image + 16 kHz mono contract; region/tracking integration; fixed 14-frame pre-roll; size, VRAM, quality, latency, and game impact. |
| MuseTalk 1.5 | Generic screen-space lip-sync | Code repository license plus transitive model/assets | Public experimental comparator; blocked pending review | Audit every weight/dependency, Windows pack, exact size/VRAM, identity/privacy implications, visual quality, latency, and game-impact gate. |
| Ditto | Generic screen-space lip-sync | Code/model/runtime terms require review | Deferred | Reconsider only after the baseline and leading candidates are qualified. |
| LatentSync | Offline lip-sync rendering | Code/model/runtime terms require review | Offline comparison only; rejected for live product path | Do not offer as a live-game pack; any research must remain isolated and provenance-complete. |
| Audio2Face/native rigs | Rig animation | NVIDIA/game-specific integration terms | Rejected product path | Requires game/mod/rig integration, which is outside the generic external architecture. |
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

Lip-sync reference snapshot: [NVIDIA LipSync model card](https://build.nvidia.com/nvidia/lipsync/modelcard), [MuseTalk](https://github.com/TMElyralab/MuseTalk), and [LatentSync](https://github.com/bytedance/LatentSync), checked 2026-08-28. Candidate names and upstream descriptions are research inputs, not product benchmarks or availability claims.
