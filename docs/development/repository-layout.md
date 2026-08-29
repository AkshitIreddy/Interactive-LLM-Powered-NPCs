# Repository layout

The 2.0 product lives alongside legacy migration evidence during the overhaul.

| Path | Responsibility |
| --- | --- |
| `apps/control/` | React/TypeScript Response Console and Tauri control-plane shell. |
| `crates/protocol/` | Versioned envelopes and canonical IPC/events. |
| `crates/runtime-core/` | Turn state machine, providers, policy, degradation, timing. |
| `crates/memory/` | SQLite migrations, retrieval, memories, provenance/import. |
| `crates/game-profile/` | `GameProfileV2` loading/semantic validation. |
| `crates/game-discovery/` | Safe store/process/build discovery contracts. |
| `crates/provider-catalog/` | Provider/model/voice catalog loading and trust policy. |
| `crates/model-manager/` | Verified pack lifecycle. |
| `crates/diagnostics/` | Content-safe structured diagnostics. |
| `native/media-broker/` | C++20/WinRT audio/capture/overlay broker. |
| `schemas/` | Machine-readable public data contracts. |
| `profiles/games/` | Declarative authored game profiles. |
| `catalog/` | Development provider/model catalog; no keys or payloads. |
| `tools/sim/`, `fixtures/sim/` | Deterministic virtual-time simulator and fixtures. |
| `packaging/`, `scripts/` | Local packaging policy and canonical developer commands. |
| `docs/` | User, architecture, development, troubleshooting, and legal documentation. |

Legacy root notebooks, `functions/`, `SadTalker/`, duplicated `Cyberpunk_2077/` trees, Chroma/pickle data, and plaintext-key examples are not 2.0 runtime modules. Their eventual removal/archive follows the reversible import map.

Generated build/cache/artifact directories are not source. Do not commit secrets, downloads, weights, recordings, proprietary captures, SQLite user data, or raw benchmark outputs unless a reviewed result/provenance policy explicitly calls for them.
