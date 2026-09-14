import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  status: vi.fn(),
}));

vi.mock("./tauriBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauriBridge")>()),
  readSyntheticReviewTargetStatus: bridge.status,
}));

import { TestGameLauncher } from "./TestGameLauncher";

const available = {
  available: true as const,
  commandName: "prepare_synthetic_review_target",
};

const ready = {
  schemaVersion: 1 as const,
  state: "readyToLaunch" as const,
  displayName: "Eclipse Harbor",
  executableName: "interactive-npcs-synthetic-target.exe",
  executablePath:
    "E:\\review\\local-app-data\\test-game\\interactive-npcs-synthetic-target.exe",
  expectedRelativePath:
    "local-app-data\\test-game\\interactive-npcs-synthetic-target.exe",
  detail: "Installed and ready.",
};

describe("TestGameLauncher", () => {
  beforeEach(() => {
    bridge.status.mockReset();
    bridge.status.mockResolvedValue(ready);
  });

  it("shows the verified included path and launches from one user action", async () => {
    const user = userEvent.setup();
    const onLaunchAndConnect = vi.fn().mockResolvedValue(undefined);
    render(
      <TestGameLauncher
        availability={available}
        connected={false}
        busy={false}
        onLaunchAndConnect={onLaunchAndConnect}
      />,
    );

    expect(await screen.findByText("Ready to launch")).toBeInTheDocument();
    expect(screen.getByTitle(ready.executablePath)).toHaveTextContent(
      "local-app-data\\test-game",
    );
    await user.click(screen.getByRole("button", { name: "Launch & connect" }));
    expect(onLaunchAndConnect).toHaveBeenCalledOnce();
  });

  it("calls a missing game a packaging problem and does not offer a dead launch", async () => {
    bridge.status.mockResolvedValue({
      ...ready,
      state: "missing",
      executablePath: null,
      detail: "The included practice game is missing from this app folder.",
    });
    render(
      <TestGameLauncher
        availability={available}
        connected={false}
        busy={false}
        onLaunchAndConnect={() => undefined}
      />,
    );

    expect(await screen.findByText("Game files missing")).toBeInTheDocument();
    expect(
      screen.getByText(/missing from this app folder/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Launch & connect" }),
    ).toBeDisabled();
  });

  it("explains that browser preview cannot launch the native practice game", () => {
    render(
      <TestGameLauncher
        availability={{ available: false, reason: "browserPreview" }}
        connected={false}
        busy={false}
        onLaunchAndConnect={() => undefined}
      />,
    );

    expect(screen.getByText("Native app required")).toBeInTheDocument();
    expect(screen.getByText(/Open the native review app/)).toBeInTheDocument();
    expect(bridge.status).not.toHaveBeenCalled();
  });

  it("retries a status read without launching anything", async () => {
    const user = userEvent.setup();
    bridge.status
      .mockRejectedValueOnce(new Error("Native status unavailable"))
      .mockResolvedValueOnce(ready);
    render(
      <TestGameLauncher
        availability={available}
        connected={false}
        busy={false}
        onLaunchAndConnect={() => undefined}
      />,
    );

    expect(
      await screen.findByText("Native status unavailable"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(bridge.status).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Ready to launch")).toBeInTheDocument();
  });
});
