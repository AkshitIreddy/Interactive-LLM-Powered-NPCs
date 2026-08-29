import type { NativeSimulationEvent } from "./tauriBridge";

export type SimulationEvidenceSource =
  | "none"
  | "awaitingNative"
  | "browserFixture"
  | "nativeRuntime";

export type SimulationEvidencePhase =
  | "idle"
  | "running"
  | "sentenceReady"
  | "delivered"
  | "cancelled";

export interface SimulationEvidence {
  source: SimulationEvidenceSource;
  phase: SimulationEvidencePhase;
  simulationId: string | null;
  generation: number | null;
  sequence: number;
  measurementBasis:
    | "deterministicFixture"
    | "trustedRuntimeFixture"
    | "controlledBenchmark"
    | null;
  sentenceReadyText: string | null;
  deliveredText: string | null;
  fixtureFirstAudioMs: number | null;
  cancellationReason: string | null;
}

export const EMPTY_SIMULATION_EVIDENCE: SimulationEvidence = {
  source: "none",
  phase: "idle",
  simulationId: null,
  generation: null,
  sequence: -1,
  measurementBasis: null,
  sentenceReadyText: null,
  deliveredText: null,
  fixtureFirstAudioMs: null,
  cancellationReason: null,
};

export function awaitingNativeEvidence(): SimulationEvidence {
  return {
    ...EMPTY_SIMULATION_EVIDENCE,
    source: "awaitingNative",
    phase: "running",
  };
}

export function browserFixtureEvidence(
  phase: SimulationEvidencePhase = "running",
): SimulationEvidence {
  return {
    ...EMPTY_SIMULATION_EVIDENCE,
    source: "browserFixture",
    phase,
  };
}

export function applyNativeSimulationEvent(
  current: SimulationEvidence,
  event: NativeSimulationEvent,
): SimulationEvidence {
  const sameTurn =
    current.simulationId === event.simulationId &&
    current.generation === event.generation;
  if (sameTurn && event.sequence <= current.sequence) return current;

  const base: SimulationEvidence = sameTurn
    ? current
    : {
        ...EMPTY_SIMULATION_EVIDENCE,
        source: "nativeRuntime",
        phase: "running",
        simulationId: event.simulationId,
        generation: event.generation,
      };
  const ordered = {
    ...base,
    source: "nativeRuntime" as const,
    sequence: event.sequence,
  };

  switch (event.type) {
    case "started":
      return {
        ...ordered,
        phase: "running",
        measurementBasis: event.measurementBasis,
      };
    case "sentenceReady":
      return {
        ...ordered,
        phase: "sentenceReady",
        sentenceReadyText: event.text,
      };
    case "completed":
      return {
        ...ordered,
        phase: "delivered",
        deliveredText: event.deliveredText,
        fixtureFirstAudioMs: event.fixtureFirstAudioMs,
      };
    case "cancelled":
      return {
        ...ordered,
        phase: "cancelled",
        cancellationReason: event.reason,
      };
    case "stageStarted":
    case "stageCompleted":
      return ordered;
  }
}

export function visibleSimulationText(
  evidence: SimulationEvidence,
): string | null {
  return evidence.deliveredText ?? evidence.sentenceReadyText;
}

export function simulationEvidenceLabel(evidence: SimulationEvidence): string {
  if (evidence.source === "nativeRuntime") {
    if (evidence.phase === "delivered") return "NATIVE RUNTIME · DELIVERED";
    if (evidence.phase === "sentenceReady")
      return "NATIVE RUNTIME · SENTENCE READY";
    return "NATIVE RUNTIME · DETERMINISTIC FIXTURE";
  }
  if (evidence.source === "awaitingNative") return "AWAITING NATIVE RUNTIME";
  if (evidence.source === "browserFixture") return "BROWSER FIXTURE PREVIEW";
  return "NO LIVE RUNTIME EVIDENCE";
}

export function nativeCompletionNotice(
  fixtureFirstAudioMs: number | null | undefined,
): string {
  if (
    typeof fixtureFirstAudioMs !== "number" ||
    !Number.isFinite(fixtureFirstAudioMs) ||
    fixtureFirstAudioMs < 0
  )
    return "Native simulation complete · first-audio timing unavailable";
  return `Native simulation complete · runtime fixture first-audio field ${(fixtureFirstAudioMs / 1_000).toFixed(2)} s`;
}
