# NPC 2.0 deterministic simulator

This dependency-free harness exercises the turn state machine without a microphone, GPU, network, real provider, model download, or wall clock. Every run is driven by a versioned scenario manifest, a seeded PRNG, and a stable virtual-time queue. Output is canonical JSON Lines so traces can be hashed and compared across Windows and CI.

Run from this directory with Node 20 or newer:

```powershell
npm test
npm run sim -- list
npm run sim -- run happy-path
npm run sim -- run-all --output .artifacts/sim
npm run sim -- benchmark --output .artifacts/sim/virtual-benchmark.json
npm run sim -- verify
```

`verify` validates manifests, resource profiles, expected outcomes, deterministic trace hashes, and the fixture integrity ledger. Its single-line `npc.sim.verification.v1` JSON result includes every assertion, actual/expected value, trace hash, and virtual timing. `run` writes JSONL to stdout unless `--output` is supplied. `run-all` accepts an output directory and creates one `<scenario>.events.jsonl` file per scenario. `benchmark` aggregates mocked virtual-time metrics and repeats the power/boost/competing-workload context in every observation.

## Recovery and quality corpus

The corpus includes direct replay evidence for:

- a successful typed turn with current-frame mouth patch and one durable delivery commit;
- cancellation/barge-in with every queued old-generation event, including a commit attempt, rejected as stale;
- retryable provider timeout followed only by an explicit manual retry;
- runtime crash/restart with the abandoned generation unable to commit;
- ambiguous identity evidence, a missed track, and explicit manual resolution without guessing;
- lip-sync patches dropped for an old source frame and old actor-track epoch;
- low-resource visual degradation that preserves voice; and
- byte-identical same-seed replay plus a different trace for a different seed.

Delivery is modeled separately from streamed text and audio. Only a `delivery.commit` received after mocked audio completion in the current cancellation generation produces `delivery.committed`. Retry and restart advance the generation, so delayed work from an abandoned attempt is machine-verifiably ignored.

Live benchmark measurements are deliberately outside this harness. Resource manifests record power mode, CPU boost, and competing workload state, and mark whether observations are canonical. The checked-in `silent-shared-machine` profile documents the current constrained machine state; it must never be used as a release benchmark.

Scenario manifests and resource profiles are under `../../fixtures/sim`. Refreshing golden hashes is an explicit maintainer action:

```powershell
npm run sim -- update-hashes
```

The update command only rewrites the simulator fixture ledger. Review the diff before committing it.
