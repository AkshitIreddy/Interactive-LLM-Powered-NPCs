# Can mouth packs become optional?

Research date: 2026-09-20. This is a feasibility assessment, not a new quality qualification or an implemented renderer.

## Answer

Yes, manually prepared character packs are avoidable. That does not mean a closed-mouth screenshot contains a usable oral interior. The missing teeth/tongue/interior must come from a model, a shared approximation, or other frames of that actor. A deformation-only renderer can move visible lips cheaply but cannot reveal faithful hidden anatomy. The user's earlier rejection of flat dark holes is directly relevant.

The promising product distinction is **no manual character setup**, rather than pretending there is no reference data anywhere. A runtime could initialize from the current actor, animate conservatively immediately, and improve a small transient identity cache as useful frames become available. Downloadable authored packs would then be optional quality improvements. Whether the initial fallback meets the user's visual bar remains unproven.

## Candidates

| Approach | What it removes | Fit and remaining uncertainty |
| --- | --- | --- |
| NVIDIA LipSync / AR SDK | Manually enrolled character references | Direct image plus speech input. Plausible native Windows candidate; private access, GPU cost and game-character generalization remain unresolved. |
| Current-frame lip warp plus automatic oral-reference cache | Manual pack preparation | Preserves current pose, lighting and surrounding pixels. Observed open-mouth frames improve identity fidelity. An initially closed mouth still needs generation or an approximate interior. |
| MuseTalk 1.5 | Per-character model training | Supports unseen faces, but prepared-avatar throughput is not the cost of processing unpredictable moving game frames. Identity preservation and jitter remain concerns. |
| LatentSync / larger diffusion or causal models | Manual character packs | Stronger offline synthesis candidates, but their memory/throughput must coexist with the game. No evidence here supports them as the default live renderer. |
| New compact reconstruction / texture-mixing research | Potentially the heavy per-frame generator | Relevant architecture ideas, with implementation availability and actual consumer-GPU behavior still separate questions. |

### NVIDIA: concrete direct-frame alternative

The official [model card](https://build.nvidia.com/nvidia/lipsync/modelcard) describes image plus mono 16 kHz speech input, one complete human face, same-resolution output and Windows 10/11 support. It requires the private-access program and uses the NVIDIA Open Model License. These are vendor capabilities, not evidence that it preserves Cyberpunk facial implants or works beside this user's game.

The official [AR SDK performance table](https://docs.nvidia.com/maxine/ar/1.0.0/WindowsARSDK/PerformanceReference.html) lists LipSync latency of 42.7 ms on RTX 3080 and 35.3 ms on RTX 4090. Those desktop measurements do not predict this laptop's sustained performance under game load. They also do not establish full capture, tracking, audio buffering, compositing and presentation latency.

This is the strongest concrete direct-frame candidate found in this pass. An ordinary NVIDIA NIM key does not establish access to this downloadable model. No access application, download or provider call was made.

### MuseTalk and existing heavier methods

[MuseTalk](https://github.com/TMElyralab/MuseTalk) supports unseen 256×256 face regions and publishes its model/inference code. Its high-throughput avatar workflow preprocesses and caches input frames; the README also discusses identity and temporal limitations. That is materially different from preserving an arbitrary newly captured game frame every refresh. The project already retains earlier MuseTalk experiments; their measurements and exact pipeline scope matter more than an advertised FPS number.

[Wav2Lip](https://github.com/Rudrabha/Wav2Lip) is character-independent, but its official pretrained usage is non-commercial and this project's retained Wav2Lip output was visually rejected. [LatentSync](https://github.com/bytedance/LatentSync) and its [1.6 model](https://huggingface.co/ByteDance/LatentSync-1.6) are more suitable offline comparators than an assumed low-cost game runtime. [Lip Forcing](https://github.com/cvlab-kaist/LipForcing) is relevant causal research, but advertised small-model performance and the actually available model are not interchangeable. None was downloaded or benchmarked in this pass.

### Automatic references are plausible, but not a solved first-contact case

[EfficientSync](https://arxiv.org/abs/2608.18832) studies deformation-based reference texture mixing. It supports exploring a light renderer that reuses actual oral texture, but reference frames are still part of the method. A paper throughput figure does not qualify this application's native path.

For this app, a bounded actor-local cache could collect sharp, unoccluded mouth observations already visible in gameplay, indexed by pose and mouth shape. A candidate neural teacher could fill missing shapes once, then unload. This is an engineering proposal, not a claim from the paper or existing implementation. It would need identity binding, lighting adaptation, expiry on actor loss, and a way to avoid freezing head movement while using reference pixels.

The user's common case is an idle, closed-mouth NPC before the first response. Harvesting alone cannot solve that case. A shared oral approximation may work at modest on-screen sizes, but should not become the default simply because it is fast: the user has already rejected dark-hole and flat-teeth artifacts. A zero-shot generated reference or direct generator may be necessary for close-ups.

## Implication for this project

The existing native source-pixel route is a useful low-cost baseline, not proof of high-quality pack-free articulation. Current prepared packs remain optional private review material and are not naturally qualified. The working hypothesis is to make **manual preparation optional** while retaining a tiny automatic cache as an optimization, with direct-frame NVIDIA inference as a comparison if access becomes available.

A meaningful comparison would use the same near-frontal idle footage of several Cyberpunk characters, the same spoken audio/viseme schedule and enlarged temporal mouth views. It would distinguish first-contact quality from warmed-cache quality and measure complete frame cost/VRAM under game pressure. No new API key is needed for a source-warp/automatic-observation prototype. The first-contact generation question is still open.

This pass made no new live lip-sync, GPU performance, audible-output or natural-quality claim.
