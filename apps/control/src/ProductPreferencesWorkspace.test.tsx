import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProductPreferencesWorkspace } from "./ProductPreferencesWorkspace";
import type {
  NativeEffectiveConfigurationSnapshot,
  NativeProductPreferenceScope,
  NativeProductPreferenceSnapshot,
  NativeSubtitlePreferenceSnapshot,
} from "./tauriBridge";

const bridge = vi.hoisted(() => ({
  readEffectiveConfiguration: vi.fn(),
  readProductPreferences: vi.fn(),
  readSubtitlePreferences: vi.fn(),
  saveProductPreferences: vi.fn(),
  saveSubtitlePreferences: vi.fn(),
  resetProductPreferences: vi.fn(),
  resetSubtitlePreferences: vi.fn(),
}));

vi.mock("./tauriBridge", () => bridge);

const effectiveValue = <T,>(
  value: T,
  sourceScope: NativeProductPreferenceScope = { kind: "global" },
  sourceKind: "preset" | "override" | "inherited" = "preset",
) => ({ value, sourceScope, sourceKind });

function preferenceSnapshot(
  scope: NativeProductPreferenceScope = { kind: "global" },
  revision = 4,
): NativeProductPreferenceSnapshot {
  const global = {
    scope: { kind: "global" as const },
    executionPreset: "cloud" as const,
    performancePreset: "balanced" as const,
    overrides: {
      subtitles: true,
      inputMode: "ptt" as const,
      vision: false,
      webcamPresence: false,
    },
  };
  return {
    schemaVersion: 1,
    revision,
    entries: [global],
    effective: {
      scope,
      executionPreset: effectiveValue(
        "cloud",
        { kind: "global" },
        scope.kind === "global" ? "preset" : "inherited",
      ),
      performancePreset: effectiveValue(
        "balanced",
        { kind: "global" },
        scope.kind === "global" ? "preset" : "inherited",
      ),
      verbosity: effectiveValue("standard"),
      creativity: effectiveValue(50),
      responseLength: effectiveValue("medium"),
      interruptionMode: effectiveValue("finishSentence"),
      inputMode: effectiveValue("ptt"),
      subtitles: effectiveValue(true),
      overlay: effectiveValue(false),
      memory: effectiveValue(true),
      emotion: effectiveValue(true),
      vision: effectiveValue(false),
      webcamPresence: effectiveValue(false),
      egress: {
        transcript: effectiveValue("selectedProviderRoute"),
        microphoneAudio: effectiveValue("selectedProviderRoute"),
        capturedGameImage: effectiveValue("denied"),
        localMemoryContext: effectiveValue("selectedProviderRoute"),
      },
      automaticProviderFallback: false,
    },
    migration: {
      state: "migratedLegacyOnboarding",
      fromSchemaVersion: 1,
      detail:
        "Native product preferences were initialized from legacy onboarding intent.",
    },
    routeSnapshot: {
      sourceLoadoutId: "native-balanced-api",
      generation: null,
      sha256: "a".repeat(64),
      activationPerformed: false,
    },
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

function subtitleSnapshot(
  scope: NativeProductPreferenceScope = { kind: "global" },
  revision = 8,
): NativeSubtitlePreferenceSnapshot {
  return {
    schemaVersion: 1,
    revision,
    entries: [],
    effective: {
      requestedScope: scope,
      selectedStyleId: {
        value: "studio-default",
        source: { kind: "bundledManifestDefault", styleId: "studio-default" },
      },
      safeAreaDp: {
        value: 28,
        source: { kind: "bundledStyle", styleId: "studio-default" },
      },
      textScale: { value: 1, source: { kind: "rendererDefault" } },
      backplateEnabled: {
        value: true,
        source: { kind: "bundledStyle", styleId: "studio-default" },
      },
      opacity: { value: 1, source: { kind: "rendererDefault" } },
      rendererParameters: {
        styleId: "studio-default",
        safeAreaDp: 28,
        bodySizeDp: 34,
        speakerSizeDp: 22,
        backplateEnabled: true,
        globalOpacity: 1,
      },
    },
    availableStyles: [
      {
        styleId: "studio-default",
        bodyFontRole: "subtitle-body",
        speakerFontRole: "subtitle-speaker",
        safeAreaDp: 28,
        bodySizeDp: 34,
        speakerSizeDp: 22,
        backplateEnabled: true,
      },
    ],
    assets: {
      noFontBinariesBundled: true,
      generatedAssetPolicy: "No font files are generated or downloaded.",
      fontRoles: [
        {
          roleId: "subtitle-body",
          fallbackChain: [
            {
              family: "Segoe UI",
              platforms: ["windows"],
              scripts: ["latin"],
              availability: "systemLookupRequired",
              licenseId: "LicenseRef-Windows-System-Font",
              binaryBundled: false,
              redistribution: "not bundled",
              usageBasis: "system lookup",
              reference: null,
            },
          ],
          genericFallback: "sansSerif",
          shapingFeatures: ["kern"],
        },
      ],
    },
    supportedOverrideFields: [
      "safeAreaDp",
      "textScale",
      "backplateEnabled",
      "opacity",
    ],
    migration: {
      state: "current",
      fromSchemaVersion: null,
      detail: "Native subtitle preferences use the current scoped schema v1.",
    },
  };
}

function configurationSnapshot(
  scope: NativeProductPreferenceScope = { kind: "global" },
): NativeEffectiveConfigurationSnapshot {
  return {
    schemaVersion: 1,
    scope,
    preferenceRevision: 4,
    readOnly: true,
    mutationAuthorityAdded: false,
    automaticProviderFallback: false,
    entries: [
      {
        key: "provider.tts",
        category: "providerRoute",
        label: "TTS route",
        owner: "providerLoadouts",
        value: {
          kind: "route",
          providerId: "elevenlabs",
          modelId: "eleven_flash_v2_5",
          voiceId: "aria",
          execution: "providerCloud",
          egress: "audio",
          transmittedData: ["text"],
          manualFallbackCount: 1,
          automaticFallback: false,
        },
        defaultValue: { kind: "disabled" },
        winningScope: { kind: "global" },
        sourceKind: "activeLoadout",
        persistence: {
          source: "nativePersisted",
          detail: "Native loadout is healthy.",
        },
        explanation: "Resolved from the active loadout chain.",
        runtimeConsequence: "Pinned per turn.",
        unavailableReason: null,
      },
    ],
  };
}

describe("native product preferences", () => {
  beforeEach(() => {
    bridge.readEffectiveConfiguration.mockReset();
    bridge.readProductPreferences.mockReset();
    bridge.readSubtitlePreferences.mockReset();
    bridge.saveProductPreferences.mockReset();
    bridge.saveSubtitlePreferences.mockReset();
    bridge.resetProductPreferences.mockReset();
    bridge.resetSubtitlePreferences.mockReset();
    bridge.readSubtitlePreferences.mockResolvedValue(subtitleSnapshot());
    bridge.readEffectiveConfiguration.mockResolvedValue(
      configurationSnapshot(),
    );
  });

  it("does not substitute browser preset fixtures", () => {
    render(
      <ProductPreferencesWorkspace
        nativeAvailable={false}
        gameProfileId="skyrim-special-edition"
        characterId="lydia"
      />,
    );
    expect(
      screen.getByText("Your preferences live in the desktop app"),
    ).toBeVisible();
    expect(screen.queryByLabelText("Execution preset")).not.toBeInTheDocument();
    expect(bridge.readProductPreferences).not.toHaveBeenCalled();
  });

  it("renders effective inheritance, egress, migration, and authority truth", async () => {
    const snapshot = preferenceSnapshot();
    bridge.readProductPreferences.mockResolvedValue(snapshot);
    const onSnapshot = vi.fn();
    render(
      <ProductPreferencesWorkspace
        nativeAvailable
        gameProfileId="skyrim-special-edition"
        characterId="lydia"
        onSnapshot={onSnapshot}
      />,
    );

    expect(await screen.findByLabelText("Execution preset")).toHaveValue(
      "cloud",
    );
    expect(screen.getByLabelText("Performance preset")).toHaveValue("balanced");
    expect(screen.getByText("native-balanced-api")).toBeVisible();
    expect(screen.getByText("No admission receipt")).toBeVisible();
    expect(
      screen.getByText("Captured game image").parentElement,
    ).toHaveTextContent("Denied");
    expect(
      screen.getByText(/initialized from legacy onboarding intent/i),
    ).toBeVisible();
    expect(document.body).toHaveTextContent(
      "Webcam presence is not available yet; this stores your preference.",
    );
    expect(document.body).toHaveTextContent("Automatic fallback off");
    expect(onSnapshot).toHaveBeenCalledWith(snapshot);
    expect(
      await screen.findByText("Validated renderer parameters"),
    ).toBeVisible();
    expect(screen.getByText("TTS route")).toBeVisible();
    expect(document.body).toHaveTextContent("Mutation authority added: no");
  });

  it("saves only bounded subtitle overrides and exposes native font provenance", async () => {
    const user = userEvent.setup();
    bridge.readProductPreferences.mockResolvedValue(preferenceSnapshot());
    bridge.saveSubtitlePreferences.mockResolvedValue({
      ...subtitleSnapshot(),
      revision: 9,
    });
    render(
      <ProductPreferencesWorkspace
        nativeAvailable
        gameProfileId="skyrim-special-edition"
        characterId="lydia"
      />,
    );

    await screen.findByLabelText("Bundled subtitle style");
    await user.selectOptions(
      screen.getByLabelText("Text scale source"),
      "override",
    );
    await user.clear(screen.getByLabelText("Text scale"));
    await user.type(screen.getByLabelText("Text scale"), "1.25");
    await user.selectOptions(screen.getByLabelText("Native backplate"), "off");
    await user.click(
      screen.getByRole("button", { name: "Save subtitle intent" }),
    );

    expect(bridge.saveSubtitlePreferences).toHaveBeenCalledWith(8, {
      scope: { kind: "global" },
      selectedStyleId: undefined,
      overrides: { textScale: 1.25, backplateEnabled: false },
    });
    expect(document.body).toHaveTextContent("Segoe UI");
    expect(document.body).toHaveTextContent("systemLookupRequired");
    expect(document.body).toHaveTextContent(
      "no font or style asset was downloaded",
    );
  });

  it("saves an exact character override against the visible revision", async () => {
    const user = userEvent.setup();
    const global = preferenceSnapshot();
    const characterScope = {
      kind: "character" as const,
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
    };
    const character = preferenceSnapshot(characterScope);
    bridge.readProductPreferences.mockImplementation(
      async (scope: NativeProductPreferenceScope) =>
        scope.kind === "character" ? character : global,
    );
    bridge.saveProductPreferences.mockResolvedValue({
      ...character,
      revision: 5,
    });
    render(
      <ProductPreferencesWorkspace
        nativeAvailable
        gameProfileId="skyrim-special-edition"
        characterId="lydia"
      />,
    );
    await screen.findByLabelText("Execution preset");
    await user.click(
      screen.getByRole("button", { name: "Selected character" }),
    );
    await waitFor(() =>
      expect(bridge.readProductPreferences).toHaveBeenLastCalledWith(
        characterScope,
      ),
    );
    await user.selectOptions(
      screen.getByLabelText("Execution preset"),
      "hybrid",
    );
    await user.selectOptions(
      screen.getByLabelText("Performance preset"),
      "immersive",
    );
    await user.selectOptions(screen.getByLabelText("Verbosity"), "detailed");
    await user.selectOptions(screen.getByLabelText("Subtitles"), "off");
    await user.selectOptions(
      screen.getByLabelText("Webcam presence intent"),
      "on",
    );
    await user.click(
      screen.getByRole("button", { name: /Save Character .* intent/i }),
    );

    expect(bridge.saveProductPreferences).toHaveBeenCalledWith(4, {
      scope: characterScope,
      executionPreset: "hybrid",
      performancePreset: "immersive",
      overrides: {
        verbosity: "detailed",
        subtitles: false,
        webcamPresence: true,
      },
    });
    expect(
      await screen.findByText(
        /No provider route, local pack, egress fallback/i,
      ),
    ).toBeVisible();
  });

  it("requires a second explicit action before reset and preserves native errors", async () => {
    const user = userEvent.setup();
    bridge.readProductPreferences.mockResolvedValue(preferenceSnapshot());
    bridge.resetProductPreferences.mockRejectedValue(
      new Error("product preference revision changed; refresh before saving"),
    );
    render(
      <ProductPreferencesWorkspace
        nativeAvailable
        gameProfileId="skyrim-special-edition"
        characterId={null}
      />,
    );
    await screen.findByLabelText("Execution preset");
    await user.click(screen.getByRole("button", { name: "Reset this scope" }));
    expect(bridge.resetProductPreferences).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Confirm reset" }));
    expect(bridge.resetProductPreferences).toHaveBeenCalledWith(4, {
      kind: "global",
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "revision changed",
    );
  });

  it("shows a retryable native load error without stale ready state", async () => {
    bridge.readProductPreferences.mockRejectedValue(
      new Error("native preference store unavailable"),
    );
    render(
      <ProductPreferencesWorkspace
        nativeAvailable
        gameProfileId="skyrim-special-edition"
        characterId={null}
      />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "native preference store unavailable",
    );
    expect(screen.queryByLabelText("Execution preset")).not.toBeInTheDocument();
  });
});
