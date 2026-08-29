import { describe, expect, it } from "vitest";
import {
  applyNativeSimulationEvent,
  awaitingNativeEvidence,
  isRetainedDeliveredNativeTurn,
  nativeCompletionNotice,
  simulationEvidenceLabel,
  visibleSimulationText,
} from "./simulationEvidence";

describe("simulation evidence", () => {
  it("keeps sentence-ready and delivered native text in sequence", () => {
    const started = applyNativeSimulationEvent(awaitingNativeEvidence(), {
      type: "started",
      simulationId: "sim-7",
      generation: 4,
      sequence: 1,
      measurementBasis: "trustedRuntimeFixture",
    });
    const sentence = applyNativeSimulationEvent(started, {
      type: "sentenceReady",
      simulationId: "sim-7",
      generation: 4,
      sequence: 3,
      text: "The harbor light is still burning.",
    });
    const delivered = applyNativeSimulationEvent(sentence, {
      type: "completed",
      simulationId: "sim-7",
      generation: 4,
      sequence: 5,
      fixtureFirstAudioMs: 811,
      runtimeFixtureOnly: true,
      deliveredText: "The harbor light is still burning. Follow the seawall.",
    });

    expect(sentence.source).toBe("nativeRuntime");
    expect(simulationEvidenceLabel(sentence)).toBe(
      "NATIVE RUNTIME · SENTENCE READY",
    );
    expect(visibleSimulationText(sentence)).toBe(
      "The harbor light is still burning.",
    );
    expect(simulationEvidenceLabel(delivered)).toBe(
      "NATIVE RUNTIME · DELIVERED",
    );
    expect(visibleSimulationText(delivered)).toBe(
      "The harbor light is still burning. Follow the seawall.",
    );
    expect(delivered.fixtureFirstAudioMs).toBe(811);
    expect(isRetainedDeliveredNativeTurn(delivered)).toBe(true);
    expect(isRetainedDeliveredNativeTurn(sentence)).toBe(false);
  });

  it("ignores replayed or reordered events for the same native turn", () => {
    const delivered = applyNativeSimulationEvent(awaitingNativeEvidence(), {
      type: "completed",
      simulationId: "sim-9",
      generation: 2,
      sequence: 8,
      fixtureFirstAudioMs: 920,
      runtimeFixtureOnly: true,
      deliveredText: "Delivered once.",
    });
    const stale = applyNativeSimulationEvent(delivered, {
      type: "sentenceReady",
      simulationId: "sim-9",
      generation: 2,
      sequence: 7,
      text: "Stale sentence.",
    });

    expect(stale).toBe(delivered);
    expect(visibleSimulationText(stale)).toBe("Delivered once.");
  });

  it("never renders NaN when native first-audio timing is absent", () => {
    expect(nativeCompletionNotice(undefined)).toBe(
      "Native simulation complete · first-audio timing unavailable",
    );
    expect(nativeCompletionNotice(Number.NaN)).not.toContain("NaN");
    expect(nativeCompletionNotice(1310)).toBe(
      "Native simulation complete · runtime fixture first-audio field 1.31 s",
    );
  });
});
