import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SyntheticReplayCaptureControl } from "./SyntheticReplayCaptureControl";

describe("SyntheticReplayCaptureControl", () => {
  const nativeResult = {
    targetProcessId: 7331,
    targetWindowHandle: 880055,
    targetExecutableBasename: "npc2-synthetic-replay-player.exe",
    diagnostics: {
      state: "running",
      captureBackend: "windowsGraphicsCapture",
      overlayBackend: "directComposition",
      captureAudio: "stopped",
      renderAudio: "stopped",
      targetState: "capturing",
      deviceGeneration: 1,
      audioDeviceGeneration: 0,
      cancellationGeneration: 0,
      framesReceived: 84,
      framesPresented: 81,
      framesDropped: 3,
      overlaysSuppressed: 0,
    },
  };

  it("labels the browser-only harness without pretending audio or lip-sync", () => {
    render(
      <SyntheticReplayCaptureControl
        availability={{ available: false, reason: "browserPreview" }}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Replay a still as a game window" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Run synthetic capture probe" }),
    ).toBeDisabled();
    expect(screen.getByRole("note")).toHaveTextContent(
      "Not a gameplay, audio, or lip-sync test.",
    );
    expect(screen.getByText(/Browser preview only/)).toBeInTheDocument();
  });

  it("invokes only the explicitly supplied native command binding", async () => {
    const user = userEvent.setup();
    const run = vi.fn().mockResolvedValue(nativeResult);
    const availability = {
      available: true as const,
      commandName: "test-owned-command",
    };
    render(
      <SyntheticReplayCaptureControl availability={availability} run={run} />,
    );

    await user.click(
      screen.getByRole("button", { name: "Run synthetic capture probe" }),
    );

    expect(run).toHaveBeenCalledWith(availability);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Native synthetic target selected",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "npc2-synthetic-replay-player.exe",
    );
    expect(screen.getByRole("status")).toHaveTextContent("84 received");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Audio and lip-sync remain outside this probe",
    );
  });
});
