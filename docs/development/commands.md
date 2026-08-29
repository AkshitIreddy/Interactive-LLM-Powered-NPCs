# Development commands

Run from the repository root in Windows PowerShell:

| Command | Purpose |
| --- | --- |
| `./dev.ps1 environment` | Report toolchain/platform readiness without installing tools. |
| `./dev.ps1 setup [-Offline]` | Restore present workspaces from pinned manifests. |
| `./dev.ps1 dev` | Start the available development surfaces. |
| `./dev.ps1 lint` | Run the complete non-mutating formatting, type, JSON, and local-link gate. |
| `./dev.ps1 test` | Run the complete offline functional gate, including Windows native tests when on Windows. |
| `./dev.ps1 benchmark [-Quick] [-OutputDirectory <path>]` | Run benchmark harness and preserve environment metadata; not automatically a publishable result. |
| `./dev.ps1 package [-Configuration Debug\|Release] [-SkipChecks] [-OutputDirectory <path>]` | Create local review artifacts; never signs or publishes. |

Security checks:

```powershell
./scripts/security/scan-secrets.ps1 -IncludeUntracked
./scripts/security/generate-sbom.ps1 -Strict
./scripts/security/check-licenses.ps1 -Strict
```

`-Strict` is required for a release candidate. Non-strict warnings can support early bootstrap but cannot satisfy the RC gate.

Focused checks currently include:

```powershell
corepack pnpm run test:frontend
corepack pnpm run test:sim
corepack pnpm run sim:verify
cargo test --workspace --all-targets --all-features --locked --offline
cargo test --manifest-path apps/control/src-tauri/Cargo.toml --all-targets --locked --offline
python -m unittest discover -s workers/tests -p test_*.py -v
```

`test:frontend` deliberately selects only `@npc2/control`; it does not recursively
run the simulation package. The dispatcher invokes the simulator separately, once,
then verifies its checked-in fixtures. It also enumerates `profiles/games/**/profile.json`,
requires exactly 20 files, and passes all 20 to the `validate-profile` CLI.

On Windows, the same command configures, builds, and runs the media-broker CMake
runs the separately locked nested-Tauri tests and the media-broker CMake tests. It
then invokes `tools/game-load/scripts/smoke.ps1` without `-Live`, which
runs the game-load CMake tests and an inert dry-run manifest check. Finally it runs
`npm test`, `npm run dry-run`, and `npm run verify` in `demo/readme`. It never starts
the desktop UI, applies game load, changes a power/G-Helper profile, accesses provider
credentials, calls a network provider, benchmarks, packages, signs, or publishes.
Missing prerequisites are blocking and named in the final failure list. On a
non-Windows host, the explicitly Windows-native Tauri, media, and game-load gates are
marked `SKIP`; such a run is not Windows certification.

`./dev.ps1 lint` runs frontend TypeScript checking and the package's real Prettier
`--check` script, root and nested-Tauri Rustfmt, root and nested-Tauri Clippy with
locked dependency graphs, all repository JSON parsing, and deterministic validation
of local Markdown targets and anchors. Remote links are recognized but never
requested. PSScriptAnalyzer remains optional and is reported as a warning when it is
not installed. Nested Tauri Rustfmt remains host-independent, while nested Tauri
Clippy is an explicit Windows gate and is marked `SKIP` elsewhere.

The repository's pinned Rust toolchain is always tried first. A known Windows rustup
installation issue can make plain `cargo clippy` unavailable even when the component
is installed. In that one case, the dispatcher permits the `+stable` alias only after
the active and stable `rustc -vV` release **and commit hash** match exactly. Any drift
or unverifiable identity is a hard failure; `+stable` is never used as an unchecked
upgrade path. Rustfmt does not resolve dependencies, while both Clippy invocations
use `--locked`.

Run the local-link validator directly without a network connection when editing docs:

```powershell
./scripts/check-doc-links.ps1
```

Run the Response Console alone with `pnpm --filter @npc2/control dev`; Vite listens on `http://127.0.0.1:1420`.

The provider catalog is at `catalog/v1/catalog.json`. Simulation scenarios and their integrity ledger are under `fixtures/sim`. See each component README before refreshing any generated/golden artifact.
