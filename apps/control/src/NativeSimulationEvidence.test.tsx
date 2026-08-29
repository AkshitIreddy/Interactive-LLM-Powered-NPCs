import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NativeSimulationEvent } from "./tauriBridge";

const bridge = vi.hoisted(() => ({
  callback: null as ((event: NativeSimulationEvent) => void) | null,
  start: vi.fn(),
  cancel: vi.fn(),
  loadBootstrap: vi.fn(),
  saveOnboarding: vi.fn(),
  doctor: vi.fn(),
  broker: vi.fn(),
  diagnostics: vi.fn(),
}));

vi.mock("./tauriBridge", () => ({
  LOADING_NATIVE_BOOTSTRAP: { kind: "loading", attempts: 0 },
  startNativeSimulation: bridge.start,
  cancelNativeSimulation: bridge.cancel,
  loadNativeBootstrapHealth: bridge.loadBootstrap,
  saveOnboarding: bridge.saveOnboarding,
  readRuntimeDoctor: bridge.doctor,
  readMediaBrokerDiagnostics: bridge.broker,
  readDiagnosticSummary: bridge.diagnostics,
  syntheticReplayCaptureAvailability: () => ({
    available: true,
    commandName: "debug_select",
  }),
  runSyntheticReplayCapture: vi.fn(),
}));

import { App } from "./App";

const preferences = {
  execution: "cloud" as const,
  performance: "balanced" as const,
  subtitles: true,
  ptt: true,
  localOnly: false,
  screenPresence: false,
  diagnostics: true,
};
const authenticatedBootstrap = {
  kind: "snapshot" as const,
  attempts: 2,
  snapshot: {
    contractVersion: 1,
    appVersion: "2.0.0-test",
    onboarding: {
      schemaVersion: 1,
      completed: true,
      currentStep: "ready" as const,
      selectedGameId: "eclipse-harbor",
      preferences,
      updatedAtEpochMs: 1,
    },
    runtime: {
      state: "ready" as const,
      connected: true,
      backend: "nativeRuntime" as const,
      processId: 100,
      restartCount: 0,
      recentFailureCount: 0,
      protocolVersion: "1",
      fixtureOnly: false,
      detail: "Authenticated runtime ready.",
    },
    mediaBroker: {
      state: "ready" as const,
      connected: true,
      processId: 101,
      restartCount: 0,
      recentFailureCount: 0,
      protocolVersion: 1,
      fixtureOnly: false,
      brokerState: "ready",
      captureAvailable: true,
      overlayAvailable: true,
      captureAudioAvailable: true,
      renderAudioAvailable: true,
      detail: "Authenticated broker ready.",
    },
    providers: [
      {
        providerId: "elevenlabs",
        displayName: "ElevenLabs",
        credentialReference: "vault-ref",
        status: "present" as const,
        detail: "Credential reference present.",
      },
    ],
    capabilities: { debugSyntheticReplayCapture: true },
  },
};

describe("native evidence in the product console", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/");
    bridge.callback = null;
    bridge.cancel.mockReset().mockResolvedValue(true);
    bridge.loadBootstrap.mockReset().mockResolvedValue(authenticatedBootstrap);
    bridge.saveOnboarding.mockReset().mockResolvedValue(null);
    bridge.doctor.mockReset().mockResolvedValue({
      status: "ready",
      profileCount: 1,
      providerCount: 1,
      modelCount: 0,
    });
    bridge.broker.mockReset().mockResolvedValue({
      framesReceived: 42,
      framesDropped: 0,
      deviceGeneration: 3,
    });
    bridge.diagnostics.mockReset().mockResolvedValue({
      overall: "readyForSimulation",
      generatedAtEpochMs: 1,
      measurements: {
        state: "unmeasured",
        reason: "Controlled benchmark only",
        currentResultsAreReleaseEvidence: false,
      },
      checks: [],
    });
    bridge.start.mockReset().mockImplementation(async (_execution, onEvent) => {
      bridge.callback = onEvent;
      onEvent({
        type: "started",
        simulationId: "native-1",
        generation: 7,
        sequence: 1,
        measurementBasis: "controlledBenchmark",
      });
      onEvent({
        type: "sentenceReady",
        simulationId: "native-1",
        generation: 7,
        sequence: 5,
        text: "The runtime returned a sentence-ready clause.",
      });
      return true;
    });
  });

  it("shows authenticated health and explicit live-call authorization", async () => {
    render(<App />);
    expect(
      await screen.findByText("Runtime and media broker authenticated"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", {
        name: /Authorize one live stock-voice call/i,
      }),
    ).toBeEnabled();
  });

  it("commits completed fixture text without calling it spoken audio", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: /Run spoken turn/i }));
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: true,
        deliveredText: "Delivered by the runtime fixture.",
      }),
    );
    expect(
      screen.getByText("Delivered by the runtime fixture."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/no audible delivery claimed/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(
      "Live provider audio delivered",
    );
  });

  it("passes the bounded live route only after explicit authorization", async () => {
    const user = userEvent.setup();
    render(<App />);
    const authorization = await screen.findByRole("checkbox", {
      name: /Authorize one live stock-voice call/i,
    });
    await user.click(authorization);
    await user.click(screen.getByRole("button", { name: /Run spoken turn/i }));
    expect(bridge.start).toHaveBeenCalledWith(
      "cloud",
      expect.any(Function),
      expect.objectContaining({
        devLiveTts: expect.objectContaining({
          providerId: "elevenlabs",
          explicitUserAuthorization: true,
        }),
      }),
    );
  });

  it("refreshes native diagnostic commands instead of fixtures", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: /Diagnostics/i }));
    await user.click(
      screen.getByRole("button", { name: "Refresh native checks" }),
    );
    expect(await screen.findByText("42")).toBeInTheDocument();
    expect(bridge.doctor).toHaveBeenCalledOnce();
    expect(bridge.broker).toHaveBeenCalledOnce();
    expect(bridge.diagnostics).toHaveBeenCalledOnce();
  });
});
