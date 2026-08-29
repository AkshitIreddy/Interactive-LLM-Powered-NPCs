import { describe, expect, it, vi } from "vitest";
import {
  loadNativeBootstrapHealth,
  normalizeNativeSimulationEvent,
  syntheticReplayCaptureAvailability,
  type NativeBootstrapSnapshot,
} from "./tauriBridge";

const snapshot = (
  runtimeConnected: boolean,
  brokerConnected: boolean,
): NativeBootstrapSnapshot => ({
  contractVersion: 1,
  appVersion: "2.0.0-test",
  runtime: {
    state: runtimeConnected ? "ready" : "starting",
    connected: runtimeConnected,
    backend: "nativeRuntime",
    processId: runtimeConnected ? 41 : null,
    restartCount: 0,
    recentFailureCount: 0,
    protocolVersion: runtimeConnected ? "1" : null,
    fixtureOnly: false,
    detail: runtimeConnected ? "Authenticated runtime ready." : "Starting.",
  },
  mediaBroker: {
    state: brokerConnected ? "ready" : "starting",
    connected: brokerConnected,
    processId: brokerConnected ? 42 : null,
    restartCount: 0,
    recentFailureCount: 0,
    protocolVersion: brokerConnected ? 1 : null,
    fixtureOnly: false,
    brokerState: brokerConnected ? "ready" : null,
    captureAvailable: brokerConnected,
    overlayAvailable: brokerConnected,
    captureAudioAvailable: brokerConnected,
    renderAudioAvailable: brokerConnected,
    detail: brokerConnected ? "Authenticated broker ready." : "Starting.",
  },
});

describe("Tauri bridge normalization", () => {
  it("normalizes current snake-case completion fields without producing NaN", () => {
    expect(
      normalizeNativeSimulationEvent({
        type: "completed",
        simulation_id: "native-12",
        generation: 3,
        sequence: 9,
        fixture_first_audio_ms: 1310,
        runtime_fixture_only: true,
        delivered_text: "Native runtime reply.",
      }),
    ).toEqual({
      type: "completed",
      simulationId: "native-12",
      generation: 3,
      sequence: 9,
      fixtureFirstAudioMs: 1310,
      deliveredText: "Native runtime reply.",
    });
  });

  it("uses a null timing fallback when the native completion omits timing", () => {
    const event = normalizeNativeSimulationEvent({
      type: "completed",
      simulation_id: "native-13",
      generation: 3,
      sequence: 9,
      delivered_text: "Text still arrived.",
    });

    expect(event?.type).toBe("completed");
    if (event?.type === "completed")
      expect(event.fixtureFirstAudioMs).toBeNull();
  });

  it("retries a startup snapshot and returns authenticated connections", async () => {
    const invoke = vi
      .fn<() => Promise<NativeBootstrapSnapshot>>()
      .mockResolvedValueOnce(snapshot(false, false))
      .mockResolvedValueOnce(snapshot(true, true));
    const wait = vi.fn().mockResolvedValue(undefined);

    const result = await loadNativeBootstrapHealth(
      { maxAttempts: 4, initialDelayMs: 25, sleep: wait },
      invoke,
    );

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(wait).toHaveBeenCalledWith(25);
    expect(result.kind).toBe("snapshot");
    if (result.kind === "snapshot") {
      expect(result.attempts).toBe(2);
      expect(result.snapshot.runtime.connected).toBe(true);
      expect(result.snapshot.mediaBroker.connected).toBe(true);
    }
  });

  it("returns the last bounded degraded snapshot instead of inventing health", async () => {
    const invoke = vi
      .fn<() => Promise<NativeBootstrapSnapshot>>()
      .mockResolvedValue(snapshot(true, false));

    const result = await loadNativeBootstrapHealth(
      { maxAttempts: 3, initialDelayMs: 0, sleep: async () => undefined },
      invoke,
    );

    expect(invoke).toHaveBeenCalledTimes(3);
    expect(result.kind).toBe("snapshot");
    if (result.kind === "snapshot")
      expect(result.snapshot.mediaBroker.connected).toBe(false);
  });

  it("exposes the exact capture command only with native debug capability", () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    expect(syntheticReplayCaptureAvailability(false)).toEqual({
      available: false,
      reason: "releaseBuild",
    });
    expect(syntheticReplayCaptureAvailability(true)).toEqual({
      available: true,
      commandName: "debug_select_synthetic_replay_capture_target",
    });
    delete (window as Window & { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__;
  });
});
