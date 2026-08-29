import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NativeSimulationEvent } from "./tauriBridge";

const bridge = vi.hoisted(() => ({
  callback: null as ((event: NativeSimulationEvent) => void) | null,
  start: vi.fn(),
  cancel: vi.fn(),
  loadBootstrap: vi.fn(),
}));

vi.mock("./tauriBridge", () => ({
  LOADING_NATIVE_BOOTSTRAP: { kind: "loading", attempts: 0 },
  startNativeSimulation: bridge.start,
  cancelNativeSimulation: bridge.cancel,
  loadNativeBootstrapHealth: bridge.loadBootstrap,
  syntheticReplayCaptureAvailability: () => ({
    available: false,
    reason: "releaseBuild",
  }),
  runSyntheticReplayCapture: vi.fn(),
  readSyntheticReplayCaptureDiagnostics: vi.fn(),
  clearSyntheticReplayCaptureTarget: vi.fn(),
}));

import { App } from "./App";

const authenticatedBootstrap = {
  kind: "snapshot" as const,
  attempts: 2,
  snapshot: {
    contractVersion: 1,
    appVersion: "2.0.0-test",
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
    capabilities: { debugSyntheticReplayCapture: true },
  },
};

describe("native simulation evidence in the control UI", () => {
  beforeEach(() => {
    bridge.callback = null;
    bridge.cancel.mockReset().mockResolvedValue(true);
    bridge.loadBootstrap.mockReset().mockResolvedValue(authenticatedBootstrap);
    bridge.start.mockReset().mockImplementation(async (_execution, onEvent) => {
      bridge.callback = onEvent;
      onEvent({
        type: "started",
        simulationId: "native-ui-1",
        generation: 7,
        sequence: 1,
        measurementBasis: "trustedRuntimeFixture",
      });
      onEvent({
        type: "sentenceReady",
        simulationId: "native-ui-1",
        generation: 7,
        sequence: 5,
        text: "The runtime returned this sentence-ready clause.",
      });
      return true;
    });
  });

  it("shows authenticated bootstrap health and native sentence text on Home", async () => {
    const user = userEvent.setup();
    render(<App />);

    expect(
      await screen.findByText("Runtime and media broker authenticated"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("synthetic-replay-capture-control"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Run a private simulation" }),
    );

    expect(screen.getByLabelText("Response pipeline")).toHaveTextContent(
      "NATIVE RUNTIME TURN · FIXTURE INPUT",
    );
    expect(
      screen.getAllByText("The runtime returned this sentence-ready clause."),
    ).not.toHaveLength(0);
    expect(document.body).toHaveTextContent("NATIVE RUNTIME · SENTENCE READY");
    expect(document.body).not.toHaveTextContent("virtual first audio NaN");
  });

  it("carries delivered native text into Conversation without relabeling it as browser fixture", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(
      screen.getByRole("button", { name: "Run a private simulation" }),
    );
    act(() => {
      bridge.callback?.({
        type: "completed",
        simulationId: "native-ui-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: null,
        deliveredText: "Delivered by the native runtime event channel.",
      });
    });
    await user.click(screen.getByRole("button", { name: /^Conversation/ }));

    expect(
      screen.getByText("Delivered by the native runtime event channel."),
    ).toBeInTheDocument();
    expect(screen.getAllByText("NATIVE RUNTIME · DELIVERED")).not.toHaveLength(
      0,
    );
    expect(
      screen.getByText(
        "Dialogue below is sourced from the desktop runtime event channel.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("No audio timing event yet")).toBeInTheDocument();
  });

  it("shows native runtime and broker rows in Diagnostics", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: /^Diagnostics/ }));

    expect(
      screen.getByRole("heading", { name: "Desktop runtime health." }),
    ).toBeInTheDocument();
    expect(screen.getByText("NATIVE PROCESS HEALTH")).toBeInTheDocument();
    expect(
      screen.getAllByText(/Authenticated · ready · protocol 1/),
    ).toHaveLength(2);
  });
});
