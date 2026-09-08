import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderLoadoutEditor } from "./ProviderLoadoutEditor";
import { resetBrowserLoadoutsForTests } from "./providerLoadouts";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("provider and model loadouts", () => {
  beforeEach(() => {
    window.localStorage.clear();
    invokeMock.mockReset();
    resetBrowserLoadoutsForTests();
  });

  afterEach(() => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__;
  });

  it("creates overrides for the selected world instead of a hard-coded fixture", async () => {
    const user = userEvent.setup();
    render(
      <ProviderLoadoutEditor
        gameProfileId="skyrim-special-edition"
        gameProfileLabel="Skyrim Special Edition"
        characterId="aela"
        characterLabel="Aela the Huntress"
      />,
    );

    await user.click(
      screen.getByRole("button", { name: /GAME OVERRIDE Game/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Create game loadout" }),
    );

    expect(
      screen.getAllByText("Skyrim Special Edition").length,
    ).toBeGreaterThan(0);
    expect(document.body).not.toHaveTextContent("Cyberpunk 2077");

    await user.click(
      screen.getByRole("button", { name: /CHARACTER OVERRIDE Character/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Create character loadout" }),
    );

    expect(
      screen.getAllByText("Skyrim Special Edition · Aela the Huntress").length,
    ).toBeGreaterThan(0);
    expect(document.body).not.toHaveTextContent("Mara Venn");
  });

  it("blocks scoped creation until World supplies an exact target", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    await user.click(
      screen.getByRole("button", { name: /GAME OVERRIDE Game/ }),
    );
    expect(
      screen.getByRole("button", { name: "Create game loadout" }),
    ).toBeDisabled();
    expect(document.body).toHaveTextContent(
      /Select a game in World to create its override/i,
    );

    await user.click(
      screen.getByRole("button", { name: /CHARACTER OVERRIDE Character/ }),
    );
    expect(
      screen.getByRole("button", { name: "Create character loadout" }),
    ).toBeDisabled();
    expect(document.body).toHaveTextContent(
      /Select a game and character in World to create an override/i,
    );
  });

  it("edits one role at a time and hands hosted account setup to the host", async () => {
    const user = userEvent.setup();
    const onManageProvider = vi.fn();
    render(<ProviderLoadoutEditor onManageProvider={onManageProvider} />);

    expect(screen.getByLabelText("Reply model provider")).toBeInTheDocument();
    expect(
      screen.queryByLabelText("Speech recognition provider"),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Connect or check Groq" }),
    );
    expect(onManageProvider).toHaveBeenCalledWith("groq");

    await user.click(
      screen.getByRole("button", { name: /Speech recognition.*AssemblyAI/i }),
    );
    expect(
      screen.getByLabelText("Speech recognition provider"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Connect or check OpenAI" }),
    ).not.toBeInTheDocument();
  });

  it("hides catalog-only speech routes and keeps AssemblyAI selectable", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor initialRole="stt" />);

    expect(
      screen
        .getByLabelText("Speech recognition provider")
        .querySelector('option[value="openai"]'),
    ).toBeNull();
    expect(screen.getByRole("option", { name: "AssemblyAI" })).toBeEnabled();
    await user.selectOptions(
      screen.getByLabelText("Speech recognition provider"),
      "assemblyai",
    );
    expect(screen.getByLabelText("Speech recognition provider")).toHaveValue(
      "assemblyai",
    );
  });

  it("shows explicit role routes and a next-turn boundary", () => {
    render(<ProviderLoadoutEditor />);

    expect(
      screen.getByRole("heading", {
        name: /Choose the route for the next conversation/,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Swaps begin next turn")).toBeInTheDocument();
    expect(screen.getByLabelText("Reply model provider")).toHaveValue("groq");
  });

  it("keeps every essential route reachable from the role picker", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    await user.click(
      screen.getByRole("button", { name: /Speech recognition.*AssemblyAI/i }),
    );
    expect(screen.getByLabelText("Speech recognition provider")).toHaveValue(
      "assemblyai",
    );
    expect(screen.getByLabelText("Speech recognition model")).toHaveAttribute(
      "title",
      "Universal-3 Pro Streaming",
    );
    await user.click(
      screen.getByRole("button", { name: /Character voice.*Cartesia/i }),
    );
    expect(screen.getByLabelText("Character voice provider")).toHaveValue(
      "cartesia",
    );
    expect(screen.getByLabelText("Character voice stock voice ID")).toHaveValue(
      "a0e99841-438c-4a64-b679-ae501e7d6091",
    );
    await user.click(
      screen.getByRole("button", { name: /Memory embeddings.*Local keyword/i }),
    );
    expect(screen.getByLabelText("Memory embeddings provider")).toHaveValue(
      "fts-only",
    );
    expect(document.body).not.toHaveTextContent("API key value");
  });

  it("pins the qualified stock voice when a hosted voice provider changes", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor initialRole="tts" />);

    await user.selectOptions(
      screen.getByLabelText("Character voice provider"),
      "deepgram",
    );
    expect(screen.getByLabelText("Character voice model")).toHaveValue(
      "aura-2-arcas-en",
    );
    expect(screen.getByLabelText("Character voice stock voice ID")).toHaveValue(
      "Arcas",
    );

    await user.selectOptions(
      screen.getByLabelText("Character voice provider"),
      "inworld",
    );
    expect(screen.getByLabelText("Character voice model")).toHaveValue(
      "inworld-tts-2-flash",
    );
    expect(screen.getByLabelText("Character voice stock voice ID")).toHaveValue(
      "Dennis",
    );
  });

  it("offers Cohere's current generation model instead of a rejected alias", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    await user.selectOptions(
      screen.getByLabelText("Reply model provider"),
      "cohere",
    );

    expect(screen.getByLabelText("Reply model model")).toHaveValue(
      "command-a-plus-05-2026",
    );
    expect(
      screen.getByRole("option", { name: "Command A+" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Cohere" })).toBeEnabled();
  });

  it("offers the exact measured Gemini, Groq, Mistral, and OpenRouter reply routes", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor onManageProvider={vi.fn()} />);

    await user.selectOptions(
      screen.getByLabelText("Reply model provider"),
      "groq",
    );
    expect(screen.getByLabelText("Reply model model")).toHaveValue(
      "openai/gpt-oss-20b",
    );
    expect(screen.getByRole("option", { name: "GPT-OSS 20B" })).toBeEnabled();
    await user.selectOptions(
      screen.getByLabelText("Reply model model"),
      "qwen/qwen3.6-27b",
    );
    expect(
      screen.getByText(/exact qualified Groq non-reasoning request profile/i),
    ).toBeInTheDocument();

    await user.selectOptions(
      screen.getByLabelText("Reply model provider"),
      "mistral",
    );
    expect(screen.getByLabelText("Reply model model")).toHaveValue(
      "ministral-8b-2512",
    );
    expect(
      screen.getByRole("option", { name: "Ministral 3B 25.12" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Connect or check Mistral AI" }),
    ).toBeEnabled();

    await user.selectOptions(
      screen.getByLabelText("Reply model provider"),
      "gemini",
    );
    expect(screen.getByLabelText("Reply model model")).toHaveValue(
      "gemini-3.1-flash-lite",
    );
    expect(document.body).not.toHaveTextContent("Gemini 2.5");

    await user.selectOptions(
      screen.getByLabelText("Reply model provider"),
      "openrouter",
    );
    expect(screen.getByLabelText("Reply model model")).toHaveValue(
      "liquid/lfm-2.5-2.6b:free",
    );
    expect(
      screen.getByText(/fallback routing is disabled/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connect or check OpenRouter" }),
    ).toBeEnabled();
  });

  it("creates, renames, clones, and activates an initially inactive scoped loadout", async () => {
    const user = userEvent.setup();
    render(
      <ProviderLoadoutEditor
        gameProfileId="cyberpunk-2077"
        gameProfileLabel="Cyberpunk 2077"
      />,
    );

    await user.click(
      screen.getByRole("button", { name: /GAME OVERRIDE Game/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Create game loadout" }),
    );
    const name = screen.getByRole("textbox", { name: "Loadout name" });
    await user.clear(name);
    await user.type(name, "Quiet night route");
    expect(name).toHaveValue("Quiet night route");

    await user.click(screen.getByRole("button", { name: "Clone" }));
    expect(screen.getByRole("textbox", { name: "Loadout name" })).toHaveValue(
      "Quiet night route copy",
    );
    await user.click(
      screen.getByRole("button", { name: "Activate for next turn" }),
    );
    expect(
      screen.getByRole("button", { name: "Active for next turn" }),
    ).toBeDisabled();
    expect(
      screen.getByText(/turn already listening.*keeps its original route/i),
    ).toBeInTheDocument();
  });

  it("requires explicit authorization for manual recovery and never calls it automatic", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    const fallback = screen.getByLabelText(
      "Reply model manual fallback provider",
    );
    expect(fallback).toBeDisabled();
    await user.click(
      screen.getByRole("checkbox", { name: /LLM manual retry/ }),
    );
    expect(fallback).toBeEnabled();
    expect(screen.getByText("Automatic fallback disabled")).toBeInTheDocument();
    expect(screen.getByText(/still require your action/i)).toBeInTheDocument();
  });

  it("keeps the optional model off and hands mouth motion to the game setup", async () => {
    const user = userEvent.setup();
    const onManageMouthMotion = vi.fn();
    render(
      <ProviderLoadoutEditor
        initialRole="lipSync"
        onManageMouthMotion={onManageMouthMotion}
      />,
    );

    expect(
      screen
        .getByLabelText("Optional lip-sync provider")
        .querySelector('option[value="local-visual-worker"]'),
    ).toBeNull();
    expect(screen.getByLabelText("Optional lip-sync provider")).toHaveValue(
      "disabled",
    );
    expect(screen.getByLabelText("Optional lip-sync model")).toHaveValue(
      "disabled",
    );
    expect(document.body).not.toHaveTextContent("Download pack");
    expect(document.body).toHaveTextContent(
      "Game mouth motion is set up in Games. A separate lip-sync model is optional and unavailable in this build.",
    );
    await user.click(
      screen.getByRole("button", { name: "Set up mouth motion" }),
    );
    expect(onManageMouthMotion).toHaveBeenCalledOnce();
  });

  it("keeps Magpie unavailable and hides unrelated discovery controls", () => {
    render(<ProviderLoadoutEditor initialRole="tts" />);

    expect(
      screen.getByRole("option", {
        name: /NVIDIA NIM Magpie · private evaluation only · discover stock voices first/i,
      }),
    ).toBeDisabled();
    expect(
      screen.queryByText("NVIDIA Magpie stock voices"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refresh NVIDIA stock voices" }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("Character voice provider")).toHaveValue(
      "cartesia",
    );
  });

  it("persists the first authenticated discovered Magpie voice atomically", async () => {
    const onNativeLoadoutsChange = vi.fn();
    const snapshot = {
      document: {
        format: "npc-provider-loadouts" as const,
        schema_version: 1 as const,
        loadouts: {
          "global-balanced-api": {
            id: "global-balanced-api",
            name: "Balanced API",
            scope: { kind: "global" as const },
            parent: null,
            roles: {},
          },
        },
        activation: {
          global: "global-balanced-api",
          games: {},
          characters: {},
        },
      },
      catalogRevision: 7,
      persistenceHealth: "healthy" as const,
      detail: "Loaded native routes.",
      credentialsChecked: false as const,
      networkRequestPerformed: false as const,
    };
    invokeMock.mockImplementation(
      (command: string, args?: Record<string, unknown>) => {
        if (command === "provider_loadout_snapshot") return snapshot;
        if (command === "provider_private_evaluation_policy")
          return {
            schemaVersion: 1,
            providerId: "nvidia-nim",
            mode: "privateEvaluationOnly",
            termsRevision:
              "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
            termsUrl: "https://provider.example/nvidia-trial-terms.pdf",
            catalogRevision: 7,
            applicationNamespace:
              "io.github.akshitireddy.interactive-npcs.review",
            namespaceEligible: true,
            trialPurposesOnly: true,
            productionUseSupported: false,
            promotionSupported: false,
            publicationSupported: false,
            confidentialSensitiveOrPersonalDataSupported: false,
            limitsApply: true,
            separateSubscriptionRequiredForProduction: true,
            accessScope: "Private evaluation only.",
            rateLimitNote: "Provider limits apply.",
            prohibitedData: [
              "confidential",
              "controlled_or_sensitive",
              "personal",
              "game_secrets",
            ],
            securityAbuseLogging: true,
            productImprovementCollectionDisclosed: true,
            serviceSpecificDisclosuresApply: true,
            exactModelTermsApply: true,
            acknowledgement: null,
          };
        if (command === "provider_private_evaluation_acknowledgement")
          return null;
        if (command === "discover_tts_stock_voices")
          return {
            schemaVersion: 1,
            providerId: "nvidia-nim-magpie",
            modelId: "magpie-tts-multilingual",
            status: "available",
            voices: [
              {
                voiceId: "provider-returned-aria-id",
                displayName: "Aria",
                language: "en-US",
                styles: ["neutral"],
                provenance: "providerStockDiscovery",
              },
            ],
            provenance: "providerStockDiscovery",
            refresh: {
              requested: false,
              performed: true,
              cacheHit: false,
              refreshedAtEpochMs: 100,
              expiresAtEpochMs: Date.now() + 60_000,
            },
          };
        if (command === "update_provider_loadout")
          return {
            ...snapshot,
            document: {
              ...snapshot.document,
              loadouts: {
                "global-balanced-api": args?.loadout,
              },
            },
          };
        if (command === "acknowledge_provider_private_evaluation")
          return {
            schemaVersion: 1,
            providerId: "nvidia-nim",
            mode: "privateEvaluationOnly",
            termsRevision:
              "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
            catalogRevision: 7,
            applicationNamespace:
              "io.github.akshitireddy.interactive-npcs.review",
            acknowledgedAtEpochMs: 42,
            promotionSupported: false,
            publicationSupported: false,
          };
        throw new Error(`Unexpected command ${command}`);
      },
    );
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const user = userEvent.setup();
    render(
      <ProviderLoadoutEditor
        initialRole="tts"
        onNativeLoadoutsChange={onNativeLoadoutsChange}
      />,
    );

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("discover_tts_stock_voices", {
        forceRefresh: false,
      }),
    );
    expect(onNativeLoadoutsChange).toHaveBeenCalledWith([
      expect.objectContaining({ id: "global-balanced-api", active: true }),
    ]);
    expect(
      screen.getByRole("option", {
        name: "NVIDIA NIM Magpie · private evaluation only",
      }),
    ).toBeEnabled();
    await user.selectOptions(
      screen.getByLabelText("Character voice provider"),
      "nvidia-nim-magpie",
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "update_provider_loadout",
        expect.objectContaining({
          loadout: expect.objectContaining({
            roles: expect.objectContaining({
              tts: expect.objectContaining({
                route: expect.objectContaining({
                  primary: expect.objectContaining({
                    provider_id: "nvidia-nim-magpie",
                    model_id: "magpie-tts-multilingual",
                    voice_id: "provider-returned-aria-id",
                  }),
                }),
              }),
            }),
          }),
        }),
      ),
    );
    expect(
      await screen.findByText("NVIDIA private evaluation only"),
    ).toBeInTheDocument();
    await user.click(screen.getByText("NVIDIA private evaluation only"));
    expect(document.body).toHaveTextContent(
      "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
    );
    expect(
      screen.getByRole("link", { name: "Open NVIDIA API Trial Terms" }),
    ).toHaveAttribute(
      "href",
      "https://provider.example/nvidia-trial-terms.pdf",
    );
    expect(document.body).toHaveTextContent(
      "io.github.akshitireddy.interactive-npcs.review · eligible for private evaluation",
    );
    expect(document.body).toHaveTextContent(
      "Magpie TTS → NVIDIA provider cloud",
    );
    expect(
      screen.getByRole("button", { name: "Active for next turn" }),
    ).toBeDisabled();
    const consent = screen.getByRole("checkbox", {
      name: /I reviewed the exact current NVIDIA trial terms/i,
    });
    expect(consent).not.toBeChecked();
    expect(
      screen.getByRole("button", { name: "Review and acknowledge terms" }),
    ).toBeDisabled();
    await user.click(consent);
    await user.click(
      screen.getByRole("button", { name: "Review and acknowledge terms" }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith(
      "acknowledge_provider_private_evaluation",
      expect.anything(),
    );
    await user.click(
      screen.getByRole("button", {
        name: "Confirm private-evaluation-only terms",
      }),
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "acknowledge_provider_private_evaluation",
      {
        request: {
          providerId: "nvidia-nim",
          termsRevision:
            "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
          explicitUserConfirmation: true,
        },
      },
    );
    expect(
      await screen.findByRole("button", { name: "Current terms acknowledged" }),
    ).toBeDisabled();
    expect(document.body).toHaveTextContent(
      /promotion and publication remain unsupported/i,
    );
    expect(document.body).toHaveTextContent(
      "nonproduction namespace io.github.akshitireddy.interactive-npcs.review",
    );
  });

  it("does not pre-apply browser activation when native protected state rejects it", async () => {
    const snapshot = {
      document: {
        format: "npc-provider-loadouts" as const,
        schema_version: 1 as const,
        loadouts: {
          "inactive-route": {
            id: "inactive-route",
            name: "Inactive route",
            scope: { kind: "game" as const, game_id: "eclipse-harbor" },
            parent: "global-balanced-api",
            roles: {},
          },
        },
        activation: {
          global: "global-balanced-api",
          games: {},
          characters: {},
        },
      },
      catalogRevision: 7,
      persistenceHealth: "healthy" as const,
      detail: "Loaded native routes.",
      credentialsChecked: false as const,
      networkRequestPerformed: false as const,
    };
    invokeMock.mockImplementation((command: string) => {
      if (command === "provider_loadout_snapshot") return snapshot;
      if (command === "provider_private_evaluation_acknowledgement")
        return null;
      if (command === "discover_tts_stock_voices")
        return { status: "unavailable", voices: [], refresh: {} };
      if (command === "activate_provider_loadout")
        throw new Error("native activation rejected");
      throw new Error(`Unexpected command ${command}`);
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor initialRole="tts" />);

    await screen.findByDisplayValue("Inactive route");
    await user.click(
      screen.getByRole("button", { name: "Activate for next turn" }),
    );
    expect(
      await screen.findByText(/was not committed by the native runtime/i),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Activate for next turn" }),
    ).toBeEnabled();
  });

  it("rejects stale acknowledgement evidence in the base production namespace", async () => {
    const snapshot = {
      document: {
        format: "npc-provider-loadouts" as const,
        schema_version: 1 as const,
        loadouts: {
          "global-balanced-api": {
            id: "global-balanced-api",
            name: "Balanced API",
            scope: { kind: "global" as const },
            parent: null,
            roles: {},
          },
        },
        activation: {
          global: "global-balanced-api",
          games: {},
          characters: {},
        },
      },
      catalogRevision: 9,
      persistenceHealth: "healthy" as const,
      detail: "Loaded native routes.",
      credentialsChecked: false as const,
      networkRequestPerformed: false as const,
    };
    invokeMock.mockImplementation(
      (command: string, args?: Record<string, unknown>) => {
        if (command === "provider_loadout_snapshot") return snapshot;
        if (command === "provider_private_evaluation_policy")
          return {
            schemaVersion: 1,
            providerId: "nvidia-nim",
            mode: "privateEvaluationOnly",
            termsRevision: "current-private-evaluation-terms-v2",
            termsUrl: "https://provider.example/current-terms.pdf",
            catalogRevision: 9,
            applicationNamespace: "io.github.akshitireddy.interactive-npcs",
            namespaceEligible: false,
            trialPurposesOnly: true,
            productionUseSupported: false,
            promotionSupported: false,
            publicationSupported: false,
            confidentialSensitiveOrPersonalDataSupported: false,
            limitsApply: true,
            separateSubscriptionRequiredForProduction: true,
            accessScope: "Private evaluation only.",
            rateLimitNote: "Provider limits apply.",
            prohibitedData: ["personal"],
            securityAbuseLogging: true,
            productImprovementCollectionDisclosed: true,
            serviceSpecificDisclosuresApply: true,
            exactModelTermsApply: true,
            acknowledgement: null,
          };
        if (command === "provider_private_evaluation_acknowledgement")
          return {
            schemaVersion: 1,
            providerId: "nvidia-nim-magpie",
            mode: "privateEvaluationOnly",
            termsRevision: "stale-private-evaluation-terms-v1",
            catalogRevision: 8,
            applicationNamespace:
              "io.github.akshitireddy.interactive-npcs.review",
            acknowledgedAtEpochMs: 1,
            promotionSupported: false,
            publicationSupported: false,
          };
        if (command === "discover_tts_stock_voices")
          return {
            schemaVersion: 1,
            providerId: "nvidia-nim-magpie",
            modelId: "magpie-tts-multilingual",
            status: "available",
            voices: [
              {
                voiceId: "provider-returned-aria-id",
                displayName: "Aria",
                language: "en-US",
                styles: [],
                provenance: "providerStockDiscovery",
              },
            ],
            provenance: "providerStockDiscovery",
            refresh: {
              requested: false,
              performed: false,
              cacheHit: true,
              expiresAtEpochMs: Date.now() + 60_000,
            },
          };
        if (command === "update_provider_loadout")
          return {
            ...snapshot,
            document: {
              ...snapshot.document,
              loadouts: { "global-balanced-api": args?.loadout },
            },
          };
        throw new Error(`Unexpected command ${command}`);
      },
    );
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor initialRole="tts" />);

    await waitFor(() =>
      expect(
        screen.getByRole("option", {
          name: "NVIDIA NIM Magpie · private evaluation only",
        }),
      ).toBeEnabled(),
    );
    await user.selectOptions(
      screen.getByLabelText("Character voice provider"),
      "nvidia-nim-magpie",
    );
    await user.click(screen.getByText("NVIDIA private evaluation only"));
    expect(
      await screen.findByText(/ineligible; acknowledgement blocked/i),
    ).toBeVisible();
    expect(document.body).toHaveTextContent(
      "current-private-evaluation-terms-v2",
    );
    expect(document.body).not.toHaveTextContent(
      "stale-private-evaluation-terms-v1",
    );
    expect(
      screen.getByRole("checkbox", {
        name: /I reviewed the exact current NVIDIA trial terms/i,
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Review and acknowledge terms" }),
    ).toBeDisabled();
    expect(invokeMock).not.toHaveBeenCalledWith(
      "acknowledge_provider_private_evaluation",
      expect.anything(),
    );
  });
});
