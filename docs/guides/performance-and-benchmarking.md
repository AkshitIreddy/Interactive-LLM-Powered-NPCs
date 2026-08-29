# Performance and benchmarking

The UI separates **Reference** results from **This PC** results. Estimates are labeled as estimates; simulation is labeled simulated. No 2.0 benchmark result is published yet.

## Acceptance targets are not results

Planning targets include response latency, retrieval, interruption, audio/visual sync, game-frame impact, STT accuracy, capture stability, and soak reliability. They become claims only after a pinned RC passes with raw evidence.

## Required result manifest

Record CPU/GPU/VRAM/RAM; OS/build/driver/power mode; displays/DPI/HDR; commit/build; game/profile/build/scene/resolution/settings; provider endpoint/region/model revisions; optional lip-sync pack/runtime revision; routing/performance modes; cold/warm state; sample count; fixture/input hashes; and network conditions.

Report p50, p95, error/cancellation counts, CPU/RSS, GPU/VRAM, game average and 1%-low/frame-time distribution, model load time, and degradation events. Separate cold load from warmed turns.

## Canonical spans

At minimum capture speech end, VAD commit, STT partial/final, identity/retrieval complete, LLM first token/first complete sentence, TTS first PCM, playback start, first visual frame, interruption request/silence, and completion. Span names remain stable and low-cardinality; prompt/audio content is excluded by default.

## Comparing configurations

Use the same fixture, scene, settings, warm-up, run count, and power state. An API-route test records network/provider variance. An optional lip-sync test records live GPU budget after game allocation. Never compare one cold run to a warmed median.

## Performance modes

Competitive prioritizes frame time; Fast prioritizes latency; Balanced mixes them; Immersive permits more perception/animation; Maximum Quality uses resources within hard caps; Custom exposes individual limits. Privacy/provider routing never changes as a performance optimization.

## Publishing rule

Every published table links to raw machine-readable results and the generator revision. Missing manifests, illustrative HUD values, hand timing, or unrepeatable anecdotes are not benchmarks.
