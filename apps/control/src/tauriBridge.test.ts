import { afterEach, describe, expect, it, vi } from "vitest";
import {
  loadNativeBootstrapHealth,
  normalizeNativeSimulationEvent,
  readDiagnosticSummary,
  readMediaBrokerDiagnostics,
  readRuntimeDoctor,
  readRuntimeProfileSummaries,
  saveOnboarding,
  startNativeSimulation,
  syntheticReplayCaptureAvailability,
  type NativeBootstrapSnapshot,
  type OnboardingSnapshot,
} from "./tauriBridge";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMocks.invoke,
  Channel: class {
    onmessage?: (message: unknown) => void;
  },
}));

const enableTauri = () =>
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });

afterEach(() => {
  tauriMocks.invoke.mockReset();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
});

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
      runtimeFixtureOnly: true,
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
    if (event?.type === "completed") {
      expect(event.fixtureFirstAudioMs).toBeNull();
      expect(event.runtimeFixtureOnly).toBe(true);
    }
  });

  it("preserves explicit live-audio evidence on current completion events", () => {
    expect(
      normalizeNativeSimulationEvent({
        type: "completed",
        simulationId: "native-live-14",
        generation: 4,
        sequence: 12,
        fixtureFirstAudioMs: 287,
        runtimeFixtureOnly: false,
        deliveredText: "Live audio reached the device.",
      }),
    ).toMatchObject({
      type: "completed",
      runtimeFixtureOnly: false,
    });
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
    expect(wait).toHaveBeenCalledWith(25, undefined);
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

  it("keeps polling beyond the old fifth attempt until startup becomes ready", async () => {
    let invocation = 0;
    const invoke = vi.fn(async () => {
      invocation += 1;
      return invocation < 8 ? snapshot(false, false) : snapshot(true, true);
    });
    const progress: number[] = [];

    const result = await loadNativeBootstrapHealth(
      {
        maxAttempts: 12,
        maxElapsedMs: 20_000,
        initialDelayMs: 0,
        sleep: async () => undefined,
        onProgress: (health) => progress.push(health.attempts),
      },
      invoke,
    );

    expect(invoke).toHaveBeenCalledTimes(8);
    expect(progress).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(result.kind).toBe("snapshot");
    if (result.kind === "snapshot") {
      expect(result.attempts).toBe(8);
      expect(result.snapshot.runtime.connected).toBe(true);
      expect(result.snapshot.mediaBroker.connected).toBe(true);
    }
  });

  it("cancels startup polling without publishing a stale final result", async () => {
    const controller = new AbortController();
    const invoke = vi.fn().mockResolvedValue(snapshot(false, false));

    await expect(
      loadNativeBootstrapHealth(
        {
          maxAttempts: 30,
          signal: controller.signal,
          sleep: async () => controller.abort(),
        },
        invoke,
      ),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("exposes the exact capture command only with native debug capability", () => {
    enableTauri();
    expect(syntheticReplayCaptureAvailability(false)).toEqual({
      available: false,
      reason: "releaseBuild",
    });
    expect(syntheticReplayCaptureAvailability(true)).toEqual({
      available: true,
      commandName: "debug_select_synthetic_replay_capture_target",
    });
  });
});

describe("Tauri command bridge", () => {
  const onboarding: OnboardingSnapshot = {
    schemaVersion: 1,
    completed: false,
    currentStep: "game",
    selectedGameId: "eclipse-harbor",
    preferences: {
      execution: "hybrid",
      performance: "balanced",
      subtitles: true,
      ptt: true,
      localOnly: false,
      screenPresence: false,
      diagnostics: true,
    },
    updatedAtEpochMs: 0,
  };

  it("keeps native-only setup and diagnostics calls inert in browser preview", async () => {
    await expect(saveOnboarding(onboarding)).resolves.toBeNull();
    await expect(readRuntimeDoctor()).resolves.toBeNull();
    await expect(readRuntimeProfileSummaries()).resolves.toEqual([]);
    await expect(readMediaBrokerDiagnostics()).resolves.toBeNull();
    await expect(readDiagnosticSummary()).resolves.toBeNull();
    expect(tauriMocks.invoke).not.toHaveBeenCalled();
  });

  it("passes onboarding through the narrow native persistence command", async () => {
    enableTauri();
    const saved = {
      onboarding: { ...onboarding, updatedAtEpochMs: 42 },
      persistence: { health: "healthy" as const, detail: "Saved." },
    };
    tauriMocks.invoke.mockResolvedValueOnce(saved);

    await expect(saveOnboarding(onboarding)).resolves.toEqual(saved);
    expect(tauriMocks.invoke).toHaveBeenCalledWith("save_onboarding", {
      onboarding,
    });
  });

  it("invokes each current native evidence command without invented arguments", async () => {
    enableTauri();
    tauriMocks.invoke
      .mockResolvedValueOnce({ status: "ready" })
      .mockResolvedValueOnce([
        { id: "eclipse-harbor", displayName: "Eclipse Harbor" },
      ])
      .mockResolvedValueOnce({ state: "ready" })
      .mockResolvedValueOnce({ overall: "readyForSimulation" });

    await readRuntimeDoctor();
    await readRuntimeProfileSummaries();
    await readMediaBrokerDiagnostics();
    await readDiagnosticSummary();

    expect(tauriMocks.invoke.mock.calls.map(([command]) => command)).toEqual([
      "runtime_doctor",
      "runtime_profile_summaries",
      "media_broker_diagnostics",
      "diagnostic_summary",
    ]);
  });

  it("preserves simulation defaults when no selection is supplied", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({});

    await expect(startNativeSimulation("hybrid", vi.fn())).resolves.toBe(true);

    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_simulation", {
      request: {
        gameProfileId: "eclipse-harbor",
        characterName: "Mara Venn",
        transcript: "Did you ever make it to the old lighthouse?",
        executionMode: "hybrid",
      },
      events: expect.anything(),
    });
  });

  it("passes selected identity, transcript, and explicitly authorized live TTS", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({});

    await startNativeSimulation("cloud", vi.fn(), {
      gameProfileId: "synthetic-replay",
      characterName: "Mara Venn",
      characterId: "mara-venn",
      transcript: "Can you hear this live route?",
      devLiveTts: {
        providerId: "elevenlabs",
        modelId: "eleven_flash_v2_5",
        voiceId: "EXAVITQu4vr4xnSDxMaL",
        explicitUserAuthorization: true,
      },
    });

    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_simulation", {
      request: {
        gameProfileId: "synthetic-replay",
        characterName: "Mara Venn",
        characterId: "mara-venn",
        transcript: "Can you hear this live route?",
        executionMode: "cloud",
        devLiveTts: {
          providerId: "elevenlabs",
          modelId: "eleven_flash_v2_5",
          voiceId: "EXAVITQu4vr4xnSDxMaL",
          explicitUserAuthorization: true,
        },
      },
      events: expect.anything(),
    });
  });
});
