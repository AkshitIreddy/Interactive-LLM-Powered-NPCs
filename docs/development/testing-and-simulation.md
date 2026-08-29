# Testing and deterministic simulation

## Full checks

```powershell
./dev.ps1 lint
./dev.ps1 test
```

These are complete developer gates rather than broad recursive guesses. `test` runs
the frontend suite exactly once, the simulation tests and verification exactly once,
the locked root Rust workspace with all targets and features, worker unittest
discovery, and CLI validation of exactly 20 game
profiles. It also runs the isolated README-demo unit test, dry-run, and artifact
verification sequence.

On Windows, `test` additionally runs the separately locked Tauri workspace,
configures/builds/tests the media broker, and runs the game-load harness smoke script
without `-Live`. The latter tests its CMake policy
target and creates only a dry-run manifest; it does not create D3D pressure or a
visible workload. Windows-only Tauri/native gates are visibly skipped off Windows,
so a portable run is not Windows acceptance evidence. No test command launches the Response Console,
downloads dependencies or models, contacts a provider, reads secrets, changes power
or CPU-boost settings, benchmarks, packages, or publishes.

`lint` runs frontend type checking and Prettier in check-only mode; root and nested
Tauri Rustfmt/Clippy; repository JSON parsing; and local Markdown file/anchor checks.
Markdown validation is deterministic and never requests remote URLs. Clippy normally
uses the pinned toolchain. The Windows-only `+stable` rustup alias fallback is allowed
only when `rustc -vV` proves its release and commit hash exactly match the active
pinned compiler. Nested Tauri Rustfmt still runs off Windows, but nested Tauri Clippy
is visibly skipped because it targets the Windows shell.

Missing prerequisites are blocking and actionable. Read the final failure list; an
explicit `SKIP` is not a pass for that platform component.

## Simulator

The Node 20+ simulator uses versioned manifests, seeded randomness, virtual time, and canonical JSONL. It needs no microphone, GPU, game, network, or wall clock.

```powershell
cd tools/sim
npm test
npm run sim -- list
npm run sim -- run happy-path
npm run sim -- run-all --output .artifacts/sim
npm run sim -- benchmark --output .artifacts/sim/virtual-benchmark.json
npm run sim -- verify
```

`verify` checks schemas, resource profiles, expected outcomes, trace hashes, and fixture integrity. Refreshing golden hashes is an explicit reviewed action:

```powershell
npm run sim -- update-hashes
```

Checked-in scenarios are `happy-path`, `offscreen-named-npc`, `barge-in`, `provider-error`, `animation-worker-crash`, `low-vram`, and `privacy-offline`. They cover partial/final STT, identity/retrieval concurrency, complete-sentence release, interruption/late events, worker faults, provider failure, local resource degradation, audio/visual fallback, offline policy, and delivered-only memory commit.

## Focused Rust tests

Run a crate directly when iterating, for example:

```powershell
cargo test --manifest-path crates/provider-catalog/Cargo.toml
```

Use the root dispatcher before handoff to catch cross-component failures.

The exact root and nested commands are:

```powershell
cargo test --workspace --all-targets --all-features --locked --offline
cargo test --manifest-path apps/control/src-tauri/Cargo.toml --all-targets --locked --offline
```

The profile gate enumerates the files rather than relying on an implicit directory
default:

```powershell
$profiles = Get-ChildItem profiles/games -Filter profile.json -File -Recurse | Sort-Object FullName
if ($profiles.Count -ne 20) { throw "Expected exactly 20 profiles" }
cargo run --locked --offline -p npc-game-profile --bin validate-profile -- $profiles.FullName
```

Worker protocol coverage uses standard-library discovery and no model downloads:

```powershell
python -m unittest discover -s workers/tests -p test_*.py -v
```

## Visual and Windows tests

UI tests require rendered inspection at wide/narrow, light/dark/high-contrast, reduced motion, error/loading/active, and 200% scaling. Capture tests require native Windows evidence across DPI, display origins, resize, alt-tab, HDR, ultrawide, and device loss. DOM, compilation, or WSL output alone cannot certify visuals/native behavior.

The Response Console exposes deterministic visual routes for review: `?page=<id>&state=ready|active|loading|empty|error|degraded`, `?onboarding=1&step=0..9`, `?contrast=high`, `?motion=reduce`, and `?demoControls=1`. Values in these fixtures are visibly illustrative and cannot be published as hardware or benchmark results.

## Benchmarks

Deterministic traces and the virtual benchmark validate semantics, not microphone/GPU/network/wall-clock performance. Every virtual observation is noncanonical. Resource fixtures include constrained shared-machine, reference 12 GB, and low-VRAM 6 GB configurations; none is a measured release result. Live runs must include an environment manifest and follow [benchmark honesty](../guides/performance-and-benchmarking.md).
