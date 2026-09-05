import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fromNativeSnapshot,
  nativeDeactivateScope,
  nativeDiscoverTtsStockVoices,
  nativeAcknowledgePrivateEvaluation,
  nativePrivateEvaluationAcknowledgement,
  nativePrivateEvaluationPolicy,
  nativeReview,
  toNativeLoadout,
  type NativeLoadoutDocument,
  type NativeLoadoutSnapshot,
} from "./providerLoadoutBridge";
import {
  readBrowserLoadouts,
  resetBrowserLoadoutsForTests,
  writeBrowserLoadouts,
} from "./providerLoadouts";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("canonical provider loadout migration", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    resetBrowserLoadoutsForTests();
  });

  it("accepts a legacy retrieval browser route and writes canonical embeddings", () => {
    const legacy = readBrowserLoadouts()[0] as unknown as {
      routes: Record<string, unknown>;
      fallbacks: Record<string, unknown>;
    };
    legacy.routes.retrieval = legacy.routes.embeddings;
    delete legacy.routes.embeddings;
    legacy.routes.tts = {
      providerId: "nvidia-nim-magpie",
      modelId: "magpie-tts-multilingual",
    };
    window.localStorage.setItem(
      "npc2.provider-loadouts.v1",
      JSON.stringify([legacy]),
    );

    const migrated = readBrowserLoadouts();
    expect(migrated[0].routes.embeddings).toEqual({
      providerId: "fts-only",
      modelId: "sqlite-fts5",
    });
    expect(migrated[0].routes.vision.providerId).toBe("disabled");
    expect(migrated[0].routes.tts.voiceId).toBe(
      "Magpie-Multilingual.EN-US.Aria",
    );
    writeBrowserLoadouts(migrated);
    const persisted = window.localStorage.getItem("npc2.provider-loadouts.v1")!;
    expect(persisted).toContain('"embeddings"');
    expect(persisted).not.toContain('"retrieval"');
  });

  it("reads legacy native retrieval but emits embeddings, vision, lipsync, and voice_id", () => {
    const document: NativeLoadoutDocument = {
      format: "npc-provider-loadouts",
      schema_version: 1,
      loadouts: {
        legacy: {
          id: "legacy",
          name: "Legacy route",
          scope: { kind: "global" },
          parent: null,
          roles: {
            retrieval: {
              mode: "route",
              route: {
                primary: {
                  provider_id: "nvidia-nim",
                  model_id: "nvidia/nemotron-3-embed-1b",
                  credential: {
                    provider_id: "nvidia-nim",
                    reference_id: "personal",
                  },
                  disclosure: {
                    catalog_revision: 7,
                    execution: "hosted",
                    egress: "provider_cloud",
                    privacy_summary: "Legacy",
                    cost_summary: "Preview",
                    transmitted_data: ["memory_context"],
                  },
                  explicit_user_selection: true,
                },
                fallbacks: [],
              },
            },
          },
        },
      },
      activation: { global: "legacy", games: {}, characters: {} },
    };
    const snapshot: NativeLoadoutSnapshot = {
      document,
      catalogRevision: 8,
      persistenceHealth: "healthy",
      detail: "Loaded",
      credentialsChecked: false,
      networkRequestPerformed: false,
    };
    const browser = fromNativeSnapshot(snapshot)[0];
    expect(browser.routes.embeddings.providerId).toBe("nvidia-nim");
    browser.routes.tts.voiceId = "stock-voice-1";

    const emitted = toNativeLoadout(
      browser,
      document,
      document.loadouts.legacy,
      snapshot.catalogRevision,
    );
    expect(emitted.roles).toHaveProperty("embeddings");
    expect(emitted.roles).toHaveProperty("vision");
    expect(emitted.roles).toHaveProperty("lipsync");
    expect(emitted.roles).not.toHaveProperty("retrieval");
    expect(
      emitted.roles.embeddings?.mode === "route"
        ? emitted.roles.embeddings.route.primary.credential
        : null,
    ).toEqual({ provider_id: "nvidia-nim", reference_id: "personal" });
    expect(
      emitted.roles.tts?.mode === "route"
        ? emitted.roles.tts.route.primary.credential
        : null,
    ).toEqual({ provider_id: "cartesia", reference_id: "personal" });
    expect(
      emitted.roles.tts?.mode === "route"
        ? emitted.roles.tts.route.primary.voice_id
        : null,
    ).toBe("stock-voice-1");
    expect(
      emitted.roles.llm?.mode === "route"
        ? emitted.roles.llm.route.primary.disclosure.catalog_revision
        : null,
    ).toBe(8);
  });

  it("serializes local lip-sync processing with no network transmitted_data", () => {
    const loadout = readBrowserLoadouts()[0];
    loadout.routes.lipSync = {
      providerId: "local-visual-worker",
      modelId: "a2f3d-regression",
    };
    const document: NativeLoadoutDocument = {
      format: "npc-provider-loadouts",
      schema_version: 1,
      loadouts: {},
      activation: { global: loadout.id, games: {}, characters: {} },
    };

    const emitted = toNativeLoadout(loadout, document);
    const route =
      emitted.roles.lipsync?.mode === "route"
        ? emitted.roles.lipsync.route.primary
        : null;
    expect(route).not.toBeNull();
    expect(route?.disclosure.execution).toBe("local");
    expect(route?.disclosure.catalog_revision).toBe(9);
    expect(route?.disclosure.egress).toBe("none");
    expect(route?.disclosure.transmitted_data).toEqual([]);
    expect(route?.credential).toBeNull();
  });

  it("serializes the NVIDIA Magpie preset with its qualified stock voice and shared credential", () => {
    const loadout = readBrowserLoadouts().find(
      (candidate) => candidate.id === "character-mara-cinematic",
    )!;
    const document: NativeLoadoutDocument = {
      format: "npc-provider-loadouts",
      schema_version: 1,
      loadouts: {},
      activation: { global: "global-balanced-api", games: {}, characters: {} },
    };

    const emitted = toNativeLoadout(loadout, document);
    const route =
      emitted.roles.tts?.mode === "route"
        ? emitted.roles.tts.route.primary
        : null;
    expect(route?.provider_id).toBe("nvidia-nim-magpie");
    expect(route?.voice_id).toBe("Magpie-Multilingual.EN-US.Aria");
    expect(route?.credential).toEqual({
      provider_id: "nvidia-nim",
      reference_id: "personal",
    });
  });

  it("reviews the exact character context and deactivates its canonical scope", async () => {
    const character = readBrowserLoadouts().find(
      (candidate) => candidate.scope === "character",
    )!;
    invokeMock.mockResolvedValueOnce({ resolved: {} }).mockResolvedValueOnce({
      document: {},
    });

    await nativeReview(character, true);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "review_provider_loadout", {
      context: {
        game_id: "eclipse-harbor",
        character_id: "mara-venn",
      },
      offline: true,
    });

    await nativeDeactivateScope(character);
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "deactivate_provider_loadout_scope",
      {
        scope: {
          kind: "character",
          game_id: "eclipse-harbor",
          character_id: "mara-venn",
        },
      },
    );
  });

  it("refuses to deactivate the required global base route before invoking native", async () => {
    const global = readBrowserLoadouts().find(
      (candidate) => candidate.scope === "global",
    )!;

    await expect(nativeDeactivateScope(global)).rejects.toThrow(
      "global provider route cannot be deactivated",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("invokes authenticated stock-voice discovery with only the refresh decision", async () => {
    invokeMock.mockResolvedValueOnce({ status: "available", voices: [] });

    await nativeDiscoverTtsStockVoices(true);
    expect(invokeMock).toHaveBeenCalledWith("discover_tts_stock_voices", {
      forceRefresh: true,
    });
  });

  it("reads native Magpie policy and persists its exact displayed revision", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    invokeMock
      .mockResolvedValueOnce({
        schemaVersion: 1,
        providerId: "nvidia-nim",
        mode: "privateEvaluationOnly",
        termsRevision:
          "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
        termsUrl: "https://provider.example/terms.pdf",
        catalogRevision: 8,
        applicationNamespace: "io.github.akshitireddy.interactive-npcs.review",
        namespaceEligible: true,
        acknowledgement: null,
      })
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({
        schemaVersion: 1,
        providerId: "nvidia-nim",
        mode: "privateEvaluationOnly",
        termsRevision:
          "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
        catalogRevision: 8,
        applicationNamespace: "io.github.akshitireddy.interactive-npcs.review",
        acknowledgedAtEpochMs: 42,
        promotionSupported: false,
        publicationSupported: false,
      });

    await expect(nativePrivateEvaluationPolicy()).resolves.toMatchObject({
      applicationNamespace: "io.github.akshitireddy.interactive-npcs.review",
      namespaceEligible: true,
      termsUrl: "https://provider.example/terms.pdf",
    });
    await expect(nativePrivateEvaluationAcknowledgement()).resolves.toBeNull();
    await nativeAcknowledgePrivateEvaluation(
      "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
    );
    expect(invokeMock.mock.calls).toEqual([
      ["provider_private_evaluation_policy", undefined],
      ["provider_private_evaluation_acknowledgement", undefined],
      [
        "acknowledge_provider_private_evaluation",
        {
          request: {
            providerId: "nvidia-nim",
            termsRevision:
              "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
            explicitUserConfirmation: true,
          },
        },
      ],
    ]);
    delete (window as Window & { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__;
  });
});
