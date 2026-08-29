// @ts-check

const { loadScenarios } = require("./manifest.ts");
const { simulate, assertExpected } = require("./simulator.ts");

/** @param {number[]} values @param {number} percentile */
function nearestRank(values, percentile) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.max(0, Math.ceil(percentile * sorted.length) - 1);
  return sorted[index];
}

function buildBenchmarkReport() {
  const observations = [];
  for (const { scenario, resourceProfile } of loadScenarios()) {
    const result = simulate(scenario, resourceProfile);
    assertExpected(scenario, result);
    observations.push({
      scenarioId: scenario.id,
      executionMode: scenario.privacy.mode,
      resourceProfileId: resourceProfile.id,
      powerMode: resourceProfile.power.mode,
      cpuBoostEnabled: resourceProfile.power.cpuBoostEnabled,
      competingAgents: resourceProfile.workload.competingAgents,
      competingWorkloadLabel: resourceProfile.workload.competingWorkloadLabel,
      canonicalMeasurement: false,
      status: result.status,
      virtualMetrics: result.metrics,
      traceSha256: result.traceSha256,
    });
  }

  const audibleLatencies = observations
    .map((observation) => observation.virtualMetrics.speechEndToFirstAudioMs)
    .filter((value) => value !== null);
  const statusCounts = {};
  for (const observation of observations) {
    statusCounts[observation.status] = (statusCounts[observation.status] || 0) + 1;
  }

  return {
    schemaVersion: "npc.sim.benchmark.v1",
    measurementKind: "deterministic_virtual_time",
    canonicalReleaseBenchmark: false,
    warning: "These are mocked functional timings, not live performance measurements.",
    summary: {
      scenarioCount: observations.length,
      audibleScenarioCount: audibleLatencies.length,
      speechEndToFirstAudioVirtualMs: {
        p50: nearestRank(audibleLatencies, 0.5),
        p95: nearestRank(audibleLatencies, 0.95),
        maximum: audibleLatencies.length ? Math.max(...audibleLatencies) : null,
      },
      statusCounts,
    },
    observations,
  };
}

module.exports = { nearestRank, buildBenchmarkReport };
