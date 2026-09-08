import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  read: vi.fn(),
  save: vi.fn(),
}));

vi.mock("./tauriBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauriBridge")>()),
  readProductPreferences: bridge.read,
  saveProductPreferences: bridge.save,
}));

import { GameOverlaySetup } from "./GameOverlaySetup";
import type {
  NativeProductPreferenceScope,
  NativeProductPreferenceSnapshot,
} from "./tauriBridge";

const effectiveValue = <T,>(
  value: T,
  sourceScope: NativeProductPreferenceScope,
) => ({ value, sourceScope, sourceKind: "override" as const });

function snapshot(
  gameProfileId: string,
  overlay: boolean,
  revision = 7,
): NativeProductPreferenceSnapshot {
  const scope = { kind: "game" as const, gameProfileId };
  return {
    schemaVersion: 1,
    revision,
    entries: [
      {
        scope,
        executionPreset: "hybrid",
        performancePreset: "immersive",
        overrides: {
          subtitles: false,
          vision: true,
          responseLength: "long",
          overlay,
        },
      },
    ],
    effective: {
      scope,
      executionPreset: effectiveValue("hybrid", scope),
      performancePreset: effectiveValue("immersive", scope),
      verbosity: effectiveValue("standard", scope),
      creativity: effectiveValue(50, scope),
      responseLength: effectiveValue("long", scope),
      interruptionMode: effectiveValue("finishSentence", scope),
      inputMode: effectiveValue("ptt", scope),
      subtitles: effectiveValue(false, scope),
      overlay: effectiveValue(overlay, scope),
      memory: effectiveValue(true, scope),
      emotion: effectiveValue(true, scope),
      vision: effectiveValue(true, scope),
      webcamPresence: effectiveValue(false, scope),
      egress: {
        transcript: effectiveValue("selectedProviderRoute", scope),
        microphoneAudio: effectiveValue("selectedProviderRoute", scope),
        capturedGameImage: effectiveValue("denied", scope),
        localMemoryContext: effectiveValue("selectedProviderRoute", scope),
      },
      automaticProviderFallback: false,
    },
    migration: {
      state: "current",
      fromSchemaVersion: null,
      detail: "Current preferences.",
    },
    routeSnapshot: null,
    resourceSnapshot: {
      selectionId: null,
      admissionStatus: null,
      admissionReceiptPresent: false,
      exactTargetPid: null,
      activationPerformed: false,
    },
    automaticProviderFallback: false,
    mutationActivatedRoutesOrPacks: false,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("GameOverlaySetup", () => {
  beforeEach(() => {
    bridge.read.mockReset();
    bridge.save.mockReset();
  });

  it("keeps browser preview read-only", () => {
    render(
      <GameOverlaySetup
        nativeAvailable={false}
        gameProfileId="cyberpunk-2077"
      />,
    );

    expect(screen.getByText("Windows app required")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Open Windows app to change" }),
    ).toBeDisabled();
    expect(bridge.read).not.toHaveBeenCalled();
    expect(bridge.save).not.toHaveBeenCalled();
  });

  it("persists only the overlay override and preserves the game entry", async () => {
    const user = userEvent.setup();
    const initial = snapshot("cyberpunk-2077", false);
    const saved = snapshot("cyberpunk-2077", true, 8);
    bridge.read.mockResolvedValue(initial);
    bridge.save.mockResolvedValue(saved);
    const onSnapshot = vi.fn();
    render(
      <GameOverlaySetup
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        onSnapshot={onSnapshot}
      />,
    );

    expect(await screen.findByText("Off")).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Enable game overlay" }),
    );

    expect(bridge.save).toHaveBeenCalledWith(7, {
      scope: { kind: "game", gameProfileId: "cyberpunk-2077" },
      executionPreset: "hybrid",
      performancePreset: "immersive",
      overrides: {
        subtitles: false,
        vision: true,
        responseLength: "long",
        overlay: true,
      },
    });
    expect(await screen.findByText("On")).toBeVisible();
    expect(onSnapshot).toHaveBeenNthCalledWith(1, initial);
    expect(onSnapshot).toHaveBeenNthCalledWith(2, saved);
  });

  it("keeps the visible state unchanged when revisioned persistence fails", async () => {
    const user = userEvent.setup();
    bridge.read.mockResolvedValue(snapshot("cyberpunk-2077", false));
    bridge.save.mockRejectedValue(
      new Error("product preference revision changed; refresh before saving"),
    );
    render(<GameOverlaySetup nativeAvailable gameProfileId="cyberpunk-2077" />);

    expect(await screen.findByText("Off")).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Enable game overlay" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "revision changed",
    );
    expect(
      screen.getByRole("button", { name: "Enable game overlay" }),
    ).toBeEnabled();
  });

  it("ignores a late preference response from the previous game scope", async () => {
    const cyberpunk = deferred<NativeProductPreferenceSnapshot>();
    bridge.read.mockImplementation((scope: NativeProductPreferenceScope) =>
      scope.kind === "game" && scope.gameProfileId === "cyberpunk-2077"
        ? cyberpunk.promise
        : Promise.resolve(snapshot("eclipse-harbor", true, 12)),
    );
    const onSnapshot = vi.fn();
    const { rerender } = render(
      <GameOverlaySetup
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        onSnapshot={onSnapshot}
      />,
    );

    rerender(
      <GameOverlaySetup
        nativeAvailable
        gameProfileId="eclipse-harbor"
        onSnapshot={onSnapshot}
      />,
    );
    expect(await screen.findByText("On")).toBeVisible();
    cyberpunk.resolve(snapshot("cyberpunk-2077", false));
    await waitFor(() => expect(bridge.read).toHaveBeenCalledTimes(2));

    expect(screen.getByText("On")).toBeVisible();
    expect(onSnapshot).toHaveBeenCalledTimes(1);
    expect(onSnapshot).toHaveBeenCalledWith(
      expect.objectContaining({ revision: 12 }),
    );
  });

  it("rejects a snapshot resolved for a different game", async () => {
    bridge.read.mockResolvedValue(snapshot("eclipse-harbor", true));
    const onSnapshot = vi.fn();
    render(
      <GameOverlaySetup
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        onSnapshot={onSnapshot}
      />,
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "different game scope",
    );
    expect(onSnapshot).not.toHaveBeenCalled();
    expect(bridge.save).not.toHaveBeenCalled();
  });
});
