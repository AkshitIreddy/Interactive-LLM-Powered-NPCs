import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const productBridge = vi.hoisted(() => ({
  loadBootstrap: vi.fn(),
}));

vi.mock("./tauriBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauriBridge")>()),
  loadNativeBootstrapHealth: productBridge.loadBootstrap,
}));

import { App } from "./App";

describe("first-run onboarding", () => {
  it("opens automatically when a clean native bootstrap has no onboarding state", async () => {
    window.history.replaceState(null, "", "/");
    window.localStorage.clear();
    productBridge.loadBootstrap.mockReset().mockResolvedValue({
      kind: "snapshot",
      attempts: 1,
      snapshot: {
        contractVersion: 1,
        appVersion: "2.0.0-test",
        onboarding: null,
        runtime: {
          state: "ready",
          connected: true,
          backend: "nativeRuntime",
          processId: 100,
          restartCount: 0,
          recentFailureCount: 0,
          protocolVersion: "1",
          fixtureOnly: false,
          detail: "Authenticated runtime ready.",
        },
        mediaBroker: {
          state: "ready",
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
        providers: [],
        gameProfiles: [],
        capabilities: {},
      },
    });

    render(<App />);

    expect(
      await screen.findByRole("dialog", { name: /get you connected/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("Step 1 of 4")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Close setup" }),
    ).toHaveTextContent("Finish later");
  });
});
