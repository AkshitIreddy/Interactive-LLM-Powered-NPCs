import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  loadBootstrap: vi.fn(),
  inspectCharacter: vi.fn(),
  readPreferences: vi.fn(),
  savePreferences: vi.fn(),
  startSimulation: vi.fn(),
  cancelSimulation: vi.fn(),
}));

const nativeLoadouts = vi.hoisted(() => ({
  snapshot: vi.fn(),
}));

vi.mock("./tauriBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauriBridge")>()),
  LOADING_NATIVE_BOOTSTRAP: { kind: "loading", attempts: 0 },
  loadNativeBootstrapHealth: bridge.loadBootstrap,
  inspectCharacterDatabase: bridge.inspectCharacter,
  readProductPreferences: bridge.readPreferences,
  saveProductPreferences: bridge.savePreferences,
  startNativeSimulation: bridge.startSimulation,
  cancelNativeSimulation: bridge.cancelSimulation,
}));

vi.mock("./providerLoadoutBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./providerLoadoutBridge")>()),
  nativeSnapshot: nativeLoadouts.snapshot,
}));

import { ProductConsole } from "./ProductConsole";
import type {
  NativeProductPreferenceScope,
  NativeProductPreferenceSnapshot,
} from "./tauriBridge";

const onboardingPreferences = {
  execution: "cloud" as const,
  performance: "balanced" as const,
  subtitles: true,
  ptt: true,
  localOnly: false,
  screenPresence: false,
  diagnostics: true,
};

const bootstrap = {
  kind: "snapshot" as const,
  attempts: 1,
  snapshot: {
    contractVersion: 1,
    appVersion: "2.0.0-test",
    onboarding: {
      schemaVersion: 1,
      completed: true,
      currentStep: "ready" as const,
      selectedGameId: "eclipse-harbor",
      preferences: onboardingPreferences,
      updatedAtEpochMs: 1,
    },
    onboardingPersistence: {
      health: "healthy" as const,
      detail: "Loaded test onboarding.",
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
    providers: [],
    gameProfiles: [
      {
        id: "eclipse-harbor",
        displayName: "Eclipse Harbor",
        wave: "review",
        safety: "singlePlayerOnly" as const,
        catalogState: "bundled" as const,
        defaultFallback: "audioOnly" as const,
      },
    ],
    models: [],
    capabilities: {},
  },
};

const character = {
  schemaVersion: 1,
  gameProfileId: "eclipse-harbor",
  gameDisplayName: "Eclipse Harbor",
  selectedCharacterId: "mara-venn",
  character: {
    id: "mara-venn",
    displayName: "Mara Venn",
    aliases: ["Mara"],
    biography: "Keeper of the harbor light.",
    personality: "Measured and watchful.",
    dialogueStyle: "Short, grounded answers.",
    styleExamples: [],
    openingLines: [],
    backgroundNpc: false,
    promptRole: "Lighthouse keeper",
    promptObjectives: [],
    promptConstraints: [],
    knowledgeRefs: [],
    voice: {
      description: "Calm voice",
      locale: "en-US",
      styleTags: [],
      providerVoiceId: null,
      adapterId: null,
      catalogVersion: null,
      license: null,
      userOverrideAllowed: true,
    },
    identity: {
      strategy: "explicit_selection",
      evidence: ["authored profile"],
      fallback: "keep_selected_character",
      automaticFaceRecognitionClaimed: false,
    },
  },
  authoredKnowledge: [],
  provenance: [],
  deliveredMemory: [],
  memoryScope: {
    userId: "local",
    profileId: "eclipse-harbor",
    gameId: "eclipse-harbor",
    characterId: "mara-venn",
    sessionId: null,
    saveId: null,
    crossGameWideningAllowed: false,
  },
};

const characterScope: NativeProductPreferenceScope = {
  kind: "character",
  gameProfileId: "eclipse-harbor",
  characterId: "mara-venn",
};

function preferenceSnapshot(
  scope: NativeProductPreferenceScope,
  subtitles: boolean,
  revision = 1,
): NativeProductPreferenceSnapshot {
  const effective = <T,>(value: T) => ({
    value,
    sourceScope: scope,
    sourceKind: "override" as const,
  });
  return {
    schemaVersion: 1,
    revision,
    entries: [
      {
        scope,
        executionPreset: "hybrid",
        performancePreset: "immersive",
        overrides: {
          verbosity: "detailed",
          inputMode: "ptt",
          subtitles,
          overlay: true,
        },
      },
    ],
    effective: {
      scope,
      executionPreset: effective("hybrid"),
      performancePreset: effective("immersive"),
      verbosity: effective("detailed"),
      creativity: effective(70),
      responseLength: effective("long"),
      interruptionMode: effective("finishSentence"),
      inputMode: effective("ptt"),
      subtitles: effective(subtitles),
      overlay: effective(true),
      memory: effective(true),
      emotion: effective(false),
      vision: effective(false),
      webcamPresence: effective(false),
      egress: {
        transcript: effective("selectedProviderRoute"),
        microphoneAudio: effective("selectedProviderRoute"),
        capturedGameImage: effective("denied"),
        localMemoryContext: effective("denied"),
      },
      automaticProviderFallback: false,
    },
    migration: {
      state: "current",
      fromSchemaVersion: null,
      detail: "Current preference document.",
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

describe("ProductConsole native session preferences", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/?page=session");
    bridge.loadBootstrap.mockReset().mockResolvedValue(bootstrap);
    bridge.inspectCharacter.mockReset().mockResolvedValue(character);
    bridge.savePreferences.mockReset();
    bridge.startSimulation.mockReset();
    bridge.cancelSimulation.mockReset().mockResolvedValue(null);
    nativeLoadouts.snapshot.mockReset().mockResolvedValue(null);
  });

  it("keeps the current character effective values when Settings inspects another scope", async () => {
    const user = userEvent.setup();
    bridge.readPreferences
      .mockReset()
      .mockImplementation((scope) =>
        Promise.resolve(
          preferenceSnapshot(scope, scope.kind === "character" ? false : true),
        ),
      );

    render(<ProductConsole />);

    const subtitles = await screen.findByRole("checkbox", {
      name: /Show delivered subtitles/i,
    });
    await waitFor(() => expect(subtitles).toBeEnabled());
    expect(subtitles).not.toBeChecked();

    await user.click(screen.getByRole("button", { name: "Typed message" }));
    expect(
      screen.getByRole("button", { name: "Typed message" }),
    ).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Settings & help" }));
    await waitFor(() =>
      expect(bridge.readPreferences).toHaveBeenCalledWith({ kind: "global" }),
    );
    await user.click(screen.getByRole("button", { name: "Session" }));

    expect(
      screen.getByRole("button", { name: "Typed message" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("checkbox", { name: /Show delivered subtitles/i }),
    ).not.toBeChecked();
    expect(bridge.readPreferences).toHaveBeenCalledWith(characterScope);
  });

  it("saves the subtitle toggle at the current character revision and preserves other overrides", async () => {
    const user = userEvent.setup();
    const current = preferenceSnapshot(characterScope, true, 7);
    const saved = preferenceSnapshot(characterScope, false, 8);
    bridge.readPreferences.mockReset().mockResolvedValue(current);
    bridge.savePreferences.mockResolvedValue(saved);

    render(<ProductConsole />);

    const subtitles = await screen.findByRole("checkbox", {
      name: /Show delivered subtitles/i,
    });
    await waitFor(() => expect(subtitles).toBeEnabled());
    expect(subtitles).toBeChecked();
    await user.click(subtitles);

    await waitFor(() =>
      expect(bridge.savePreferences).toHaveBeenCalledWith(7, {
        scope: characterScope,
        executionPreset: "hybrid",
        performancePreset: "immersive",
        overrides: {
          verbosity: "detailed",
          inputMode: "ptt",
          subtitles: false,
          overlay: true,
        },
      }),
    );
    await waitFor(() => expect(subtitles).not.toBeChecked());
    expect(
      screen.getByText("Subtitles hidden for Eclipse Harbor · Mara Venn."),
    ).toBeVisible();
  });

  it("keeps subtitles unchanged and reports a native revision failure", async () => {
    const user = userEvent.setup();
    bridge.readPreferences
      .mockReset()
      .mockResolvedValue(preferenceSnapshot(characterScope, true, 11));
    bridge.savePreferences.mockRejectedValue(
      new Error("Preference revision changed; reload and try again."),
    );

    render(<ProductConsole />);

    const subtitles = await screen.findByRole("checkbox", {
      name: /Show delivered subtitles/i,
    });
    await waitFor(() => expect(subtitles).toBeEnabled());
    await user.click(subtitles);

    const failure = await screen.findByText(
      "Preference revision changed; reload and try again.",
    );
    expect(failure).toHaveAttribute("role", "alert");
    expect(subtitles).toBeChecked();
  });

  it("disables the native subtitle control in browser preview", async () => {
    bridge.loadBootstrap.mockResolvedValue({
      kind: "browserPreview",
      attempts: 0,
    });
    bridge.readPreferences.mockReset();

    render(<ProductConsole />);

    const subtitles = await screen.findByRole("checkbox", {
      name: /Show delivered subtitles/i,
    });
    expect(subtitles).toBeDisabled();
    expect(screen.getByText("Installed app required")).toBeVisible();
    expect(bridge.readPreferences).not.toHaveBeenCalled();
    expect(bridge.savePreferences).not.toHaveBeenCalled();
  });

  it("ignores a cancelled turn's late completion after a newer turn starts", async () => {
    const user = userEvent.setup();
    bridge.readPreferences
      .mockReset()
      .mockResolvedValue(preferenceSnapshot(characterScope, true, 3));
    const callbacks: Array<(event: Record<string, unknown>) => void> = [];
    bridge.startSimulation.mockImplementation(
      (_execution, onEvent: (event: Record<string, unknown>) => void) => {
        callbacks.push(onEvent);
        return Promise.resolve(true);
      },
    );

    render(<ProductConsole />);

    await screen.findByText("For Eclipse Harbor · Mara Venn");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send typed turn/i }));
    await waitFor(() => expect(callbacks).toHaveLength(1));

    await user.click(screen.getByRole("button", { name: "Cancel generation" }));
    await waitFor(() => expect(bridge.cancelSimulation).toHaveBeenCalledOnce());
    await user.click(screen.getByRole("button", { name: /Send typed turn/i }));
    await waitFor(() => expect(callbacks).toHaveLength(2));

    act(() => {
      callbacks[1]?.({
        type: "completed",
        simulationId: "new-turn",
        generation: 2,
        sequence: 2,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: false,
        deliveredText: "The newer turn won.",
      });
      callbacks[0]?.({
        type: "completed",
        simulationId: "stale-turn",
        generation: 1,
        sequence: 3,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: false,
        deliveredText: "The stale turn overwrote it.",
      });
    });

    expect(await screen.findByText("The newer turn won.")).toBeVisible();
    expect(
      screen.queryByText("The stale turn overwrote it."),
    ).not.toBeInTheDocument();
  });
});
