// @ts-check

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const test = require("node:test");
const { canonicalJson } = require("../src/canonical.ts");
const { fixtureRoot, readJson, loadScenarios, loadScenario, computeFixtureLedger } = require("../src/manifest.ts");
const { SPINE, simulate, evaluateExpected, assertExpected } = require("../src/simulator.ts");
const { nearestRank, buildBenchmarkReport } = require("../src/benchmark.ts");

function run(id) {
  const { scenario, resourceProfile } = loadScenario(id);
  const result = simulate(scenario, resourceProfile);
  assertExpected(scenario, result);
  return result;
}

test("all versioned scenarios replay byte-for-byte and match golden trace hashes", () => {
  const loaded = loadScenarios();
  assert.equal(loaded.length, 13);
  for (const { scenario, resourceProfile } of loaded) {
    const first = simulate(scenario, resourceProfile);
    const replay = simulate(scenario, resourceProfile);
    assertExpected(scenario, first);
    assert.equal(first.jsonl, replay.jsonl, scenario.id);
    assert.equal(first.traceSha256, scenario.expected.traceSha256, scenario.id);
    assert.deepEqual(Object.keys(first.spine), SPINE, scenario.id);
  }
});

test("happy path overlaps response, voice, and animation and records constrained machine state", () => {
  const result = run("happy-path");
  assert.equal(result.status, "completed");
  assert.equal(result.metrics.speechEndToFirstAudioMs, 240);
  assert.ok(result.events.some((event) => event.kind === "audio.chunk" && event.virtualTimeMs < 680));
  const resource = result.events.find((event) => event.kind === "resource.snapshot");
  assert.deepEqual(
    {
      powerMode: resource.payload.powerMode,
      cpuBoostEnabled: resource.payload.cpuBoostEnabled,
      competingAgents: resource.payload.competingAgents,
      canonicalBenchmark: resource.payload.canonicalBenchmark,
    },
    { powerMode: "silent", cpuBoostEnabled: false, competingAgents: true, canonicalBenchmark: false },
  );
});

test("offscreen named NPC completes with audio and an explicit animation fallback", () => {
  const result = run("offscreen-named-npc");
  assert.equal(result.spine.animating, "skipped");
  assert.ok(result.events.some((event) => event.kind === "identity.resolved" && event.payload.strategy === "explicit_name"));
  assert.ok(result.events.some((event) => event.kind === "audio.complete"));
});

test("barge-in advances cancellation generation and rejects all stale queued chunks", () => {
  const result = run("barge-in");
  assert.equal(result.status, "cancelled");
  const cancellation = result.events.find((event) => event.kind === "turn.cancelled");
  assert.equal(cancellation.cancellationGeneration, 1);
  const stale = result.events.filter((event) => event.kind === "late_event.ignored");
  assert.ok(stale.length >= 5);
  assert.ok(stale.every((event) => event.payload.reason === "stale_cancellation_generation"));
  assert.equal(result.events.filter((event) => event.kind === "audio.chunk").length, 1);
  assert.equal(result.metrics.deliveryCommitCount, 0);
  assert.equal(result.events.some((event) => event.kind === "delivery.committed"), false);
});

test("typed input completes through audio, current-frame lip-sync, and one delivery commit", () => {
  const result = run("typed-turn");
  assert.equal(result.status, "completed");
  assert.equal(result.metrics.inputReadyToFirstAudioMs, 205);
  assert.equal(result.metrics.deliveryCommitCount, 1);
  assert.equal(result.events.filter((event) => event.kind === "animation.patch_applied").length, 1);
  assert.deepEqual([result.spine.listening, result.spine.transcribing], ["skipped", "skipped"]);
});

test("provider timeout requires an explicit manual retry on a new generation", () => {
  const result = run("provider-timeout-manual-retry");
  const error = result.events.find((event) => event.kind === "turn.error");
  const retry = result.events.find((event) => event.kind === "turn.retry_started");
  const commit = result.events.find((event) => event.kind === "delivery.committed");
  assert.equal(error.payload.code, "provider_timeout");
  assert.equal(error.payload.retryable, true);
  assert.equal(retry.cancellationGeneration, 1);
  assert.equal(commit.cancellationGeneration, 1);
  assert.ok(commit.sequence > retry.sequence);
  assert.equal(result.metrics.failureToRecoveryMs, 100);
});

test("runtime crash and restart cannot falsely commit the pre-crash generation", () => {
  const result = run("runtime-crash-restart");
  assert.equal(result.metrics.runtimeRestartCount, 1);
  assert.equal(result.metrics.deliveryCommitCount, 1);
  assert.equal(result.metrics.rejectedDeliveryCommitCount, 0);
  const staleCommit = result.events.find(
    (event) => event.kind === "late_event.ignored" && event.source === "delivery" && event.payload.kind === "commit",
  );
  assert.equal(staleCommit.payload.reason, "stale_cancellation_generation");
  const committed = result.events.filter((event) => event.kind === "delivery.committed");
  assert.equal(committed.length, 1);
  assert.equal(committed[0].payload.sentenceId, "post-restart");
});

test("identity ambiguity and a missed track never guess before manual resolution", () => {
  const result = run("actor-ambiguity-miss");
  const evidence = result.events.filter((event) => event.kind.startsWith("identity."));
  assert.deepEqual(evidence.map((event) => event.kind), ["identity.ambiguous", "identity.missed", "identity.resolved"]);
  assert.equal(evidence[0].payload.action, "do_not_guess");
  assert.equal(evidence[2].payload.strategy, "manual_target");
});

test("stale lip-sync patches are dropped by frame and actor epoch while current work applies", () => {
  const result = run("stale-lipsync-drop");
  const dropped = result.events.filter((event) => event.kind === "animation.patch_dropped");
  assert.deepEqual(dropped.map((event) => event.payload.reason), ["captured_frame_not_current", "actor_track_epoch_mismatch"]);
  assert.equal(result.events.filter((event) => event.kind === "animation.patch_applied").length, 1);
  assert.equal(result.metrics.staleLipSyncDropCount, 2);
});

test("seed fixture replays byte-identically and a different seed changes the canonical trace", () => {
  const { scenario, resourceProfile } = loadScenario("seed-reproducibility");
  const first = simulate(scenario, resourceProfile);
  const replay = simulate(structuredClone(scenario), resourceProfile);
  const otherSeed = structuredClone(scenario);
  otherSeed.seed += 1;
  const divergent = simulate(otherSeed, resourceProfile);
  assert.equal(first.jsonl, replay.jsonl);
  assert.equal(first.traceSha256, replay.traceSha256);
  assert.notEqual(first.traceSha256, divergent.traceSha256);
  assert.notEqual(first.events[0].payload.deterministicRunId, divergent.events[0].payload.deterministicRunId);
});

test("provider failure is retryable and never commits mocked audio", () => {
  const result = run("provider-error");
  assert.equal(result.status, "failed");
  assert.equal(result.metrics.firstAudioMs, null);
  assert.ok(result.events.some((event) => event.kind === "turn.error" && event.payload.retryable === true));
  assert.equal(result.events.some((event) => event.kind === "audio.chunk"), false);
});

test("animation crash quarantines visuals while voice finishes", () => {
  const result = run("animation-worker-crash");
  assert.equal(result.status, "degraded");
  assert.deepEqual(result.degradationReasons, ["animation_worker_quarantined"]);
  assert.ok(result.events.some((event) => event.kind === "quarantined_event.ignored"));
  assert.ok(result.events.some((event) => event.kind === "audio.complete"));
});

test("low VRAM follows declared visual degradation order and preserves local audio", () => {
  const result = run("low-vram");
  const reasons = result.events
    .filter((event) => event.kind === "runtime.degraded")
    .map((event) => event.payload.reason);
  assert.deepEqual(reasons, ["low_vram", "continuous_vision_disabled", "screen_space_lipsync_disabled"]);
  assert.equal(result.spine.animating, "skipped");
  assert.ok(result.events.some((event) => event.kind === "audio.complete"));
});

test("offline privacy fixture is fully local and makes zero network attempts", () => {
  const result = run("privacy-offline");
  assert.equal(result.metrics.networkAttempts, 0);
  assert.equal(result.events.some((event) => event.kind.startsWith("network.")), false);
  const policy = result.events.find((event) => event.kind === "privacy.policy");
  assert.equal(policy.payload.mode, "offline");
  assert.equal(policy.payload.networkAllowed, false);
  assert.deepEqual(new Set(Object.values(policy.payload.providers)), new Set(["mock-local"]));
});

test("fixture integrity ledger covers every scenario, schema, and resource manifest", () => {
  const expected = readJson(path.join(fixtureRoot(), "fixture-hashes.v1.json"));
  const actual = computeFixtureLedger();
  assert.equal(canonicalJson(actual), canonicalJson(expected));
  assert.equal(new Set(actual.fixtures.map((fixture) => fixture.path)).size, actual.fixtures.length);
});

test("virtual benchmark report keeps power constraints and refuses canonical status", () => {
  assert.equal(nearestRank([9, 1, 5, 3], 0.5), 3);
  assert.equal(nearestRank([], 0.95), null);
  const report = buildBenchmarkReport();
  assert.equal(report.schemaVersion, "npc.sim.benchmark.v1");
  assert.equal(report.canonicalReleaseBenchmark, false);
  assert.equal(report.observations.length, 13);
  assert.ok(report.observations.every((observation) => observation.canonicalMeasurement === false));
  const constrained = report.observations.find((observation) => observation.resourceProfileId === "silent-shared-machine");
  assert.deepEqual(
    [constrained.powerMode, constrained.cpuBoostEnabled, constrained.competingAgents],
    ["silent", false, true],
  );
});

test("CLI returns canonical JSONL and verifies the complete corpus", () => {
  const cli = path.resolve(__dirname, "../src/cli.ts");
  const runResult = spawnSync(process.execPath, [cli, "run", "privacy-offline"], { encoding: "utf8" });
  assert.equal(runResult.status, 0, runResult.stderr);
  assert.ok(runResult.stdout.endsWith("\n"));
  for (const line of runResult.stdout.trimEnd().split("\n")) {
    assert.equal(canonicalJson(JSON.parse(line)), line);
  }

  const verifyResult = spawnSync(process.execPath, [cli, "verify"], { encoding: "utf8" });
  assert.equal(verifyResult.status, 0, verifyResult.stderr);
  const verification = JSON.parse(verifyResult.stdout);
  assert.equal(verification.status, "ok");
  assert.equal(verification.schemaVersion, "npc.sim.verification.v1");
  assert.equal(verification.scenarioCount, 13);
  assert.ok(verification.assertionCount >= 80);
  assert.ok(verification.scenarios.every((scenario) => scenario.assertions.every((assertion) => assertion.passed)));
  assert.ok(verification.scenarios.every((scenario) => typeof scenario.metrics.eventCountBeforeSummary === "number"));

  const benchmarkResult = spawnSync(process.execPath, [cli, "benchmark"], { encoding: "utf8" });
  assert.equal(benchmarkResult.status, 0, benchmarkResult.stderr);
  assert.equal(JSON.parse(benchmarkResult.stdout).measurementKind, "deterministic_virtual_time");
});
