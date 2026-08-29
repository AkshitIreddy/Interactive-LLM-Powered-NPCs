# NPC 2.0 deterministic simulator

This dependency-free harness exercises the turn state machine without a microphone, GPU, network, or wall clock. Every run is driven by a versioned scenario manifest, a seeded PRNG, and a stable virtual-time queue. Output is canonical JSON Lines so traces can be hashed and compared across Windows and CI.

Run from this directory with Node 20 or newer:

```powershell
npm test
npm run sim -- list
npm run sim -- run happy-path
npm run sim -- run-all --output .artifacts/sim
npm run sim -- benchmark --output .artifacts/sim/virtual-benchmark.json
npm run sim -- verify
```

`verify` validates manifests, resource profiles, expected outcomes, deterministic trace hashes, and the fixture integrity ledger. `run` writes JSONL to stdout unless `--output` is supplied. `run-all` accepts an output directory and creates one `<scenario>.events.jsonl` file per scenario. `benchmark` aggregates mocked virtual-time metrics and repeats the power/boost/competing-workload context in every observation.

Live benchmark measurements are deliberately outside this harness. Resource manifests record power mode, CPU boost, and competing workload state, and mark whether observations are canonical. The checked-in `silent-shared-machine` profile documents the current constrained machine state; it must never be used as a release benchmark.

Scenario manifests and resource profiles are under `../../fixtures/sim`. Refreshing golden hashes is an explicit maintainer action:

```powershell
npm run sim -- update-hashes
```

The update command only rewrites the simulator fixture ledger. Review the diff before committing it.
