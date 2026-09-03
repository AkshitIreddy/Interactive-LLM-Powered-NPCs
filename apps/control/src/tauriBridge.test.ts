import { afterEach, describe, expect, it, vi } from "vitest";
import {
  admitSelectedLocalLoadout,
  backupAllLocalMemory,
  clearGameTarget,
  cancelThisPcBenchmark,
  cancelManualActorPicker,
  correctEncounterToAuthoredCharacter,
  deleteLocalMemoryBackup,
  discoverGameTargets,
  enumerateAudioInputs,
  enumerateAudioOutputs,
  eraseCharacterMemory,
  exportDiagnosticsV2,
  inspectCharacterDatabase,
  enrollIdentityReference,
  loadNativeBootstrapHealth,
  normalizeNativeSimulationEvent,
  readDiagnosticSummary,
  readDiagnosticsV2,
  readDiagnosticsMatrix,
  readDiagnosticsSettings,
  readEffectiveConfiguration,
  readExperimentalVisualPackStatus,
  readIdentityReferenceEnrollmentStatus,
  readManualActorPickerStatus,
  readLocalResourceSettings,
  readLocalResourceTelemetry,
  listLocalMemoryBackups,
  readSelectedGameTarget,
  readSelectedLocalLoadoutPlanner,
  readThisPcBenchmarkReport,
  readThisPcBenchmarkStatus,
  readTrustedLocalPackCatalog,
  readCharacterDatabaseCatalog,
  readCharacterMemoryStatus,
  readProductPreferences,
  readSubtitlePreferences,
  readSelectedAudioOutput,
  readSelectedAudioInput,
  removeAllLocalMemory,
  restoreLocalMemoryBackup,
  runSyntheticReplayCapture,
  saveOnboarding,
  selectAudioOutput,
  selectAudioInput,
  saveLocalResourceSettings,
  saveDiagnosticsSettings,
  saveProductPreferences,
  saveSubtitlePreferences,
  selectGameTarget,
  persistSelectedCharacter,
  mutateExperimentalVisualPack,
  mergeUnknownEncounters,
  startNativeSimulation,
  startThisPcBenchmark,
  startManualActorPicker,
  syntheticReplayCaptureAvailability,
  type NativeBootstrapSnapshot,
  type OnboardingSnapshot,
  verifySelectedGameCapture,
  resetProductPreferences,
  resetSubtitlePreferences,
} from "./tauriBridge";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMocks.invoke,
  Channel: class {
    onmessage?: (message: unknown) => void;
  },
}));

const enableTauri = () =>
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });

afterEach(() => {
  tauriMocks.invoke.mockReset();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
});

const snapshot = (
  runtimeConnected: boolean,
  brokerConnected: boolean,
): NativeBootstrapSnapshot => ({
  contractVersion: 1,
  appVersion: "2.0.0-test",
  runtime: {
    state: runtimeConnected ? "ready" : "starting",
    connected: runtimeConnected,
    backend: "nativeRuntime",
    processId: runtimeConnected ? 41 : null,
    restartCount: 0,
    recentFailureCount: 0,
    protocolVersion: runtimeConnected ? "1" : null,
    fixtureOnly: false,
    detail: runtimeConnected ? "Authenticated runtime ready." : "Starting.",
  },
  mediaBroker: {
    state: brokerConnected ? "ready" : "starting",
    connected: brokerConnected,
    processId: brokerConnected ? 42 : null,
    restartCount: 0,
    recentFailureCount: 0,
    protocolVersion: brokerConnected ? 1 : null,
    fixtureOnly: false,
    brokerState: brokerConnected ? "ready" : null,
    captureAvailable: brokerConnected,
    overlayAvailable: brokerConnected,
    captureAudioAvailable: brokerConnected,
    renderAudioAvailable: brokerConnected,
    detail: brokerConnected ? "Authenticated broker ready." : "Starting.",
  },
});

const selectedSttInputEvidence = () => ({
  mode: "pushToTalk",
  pushToTalkState: "transcriptReady",
  selectedSttReceipt: {
    schemaVersion: 1,
    receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
    receiptSha256: "a".repeat(64),
    captureSessionId: "018f47c2-7202-7c8d-8e2a-9e2791d69540",
    captureTurnId: "018f47c2-7202-7c8d-8e2a-9e2791d69541",
    captureGeneration: 21,
    gameId: "skyrim-special-edition",
    characterId: "lydia",
    sourceLoadoutId: "nvidia-review-loadout",
    route: {
      providerId: "assemblyai",
      modelId: "u3-rt-pro",
      credentialReference: "providers/assemblyai",
      egress: "microphone_audio_and_optional_non_secret_context",
      generation: 21,
      inputEndpointId: "windows-microphone-7",
      inputEndpointGeneration: 27,
      manualRetry: false,
      automaticFallback: false,
      capturedFrames: 9600,
      pttVirtualKey: 119,
      pttPressTransitionSequence: 101,
      pttPressedQpc: 1000,
      pttReleaseTransitionSequence: 102,
      pttReleasedQpc: 2200,
    },
    chunksSent: 8,
    pcmBytesSent: 19200,
    partialEvents: 2,
  },
});

const turnExecutionWithInput = (input: unknown) => ({
  consumedRoute: {
    schemaVersion: 1,
    sourceLoadoutId: "nvidia-review-loadout",
    generation: 21,
    sha256: "b".repeat(64),
    llm: null,
    tts: null,
  },
  input,
  deliveryState: "delivered",
  commitState: "committed",
  degradations: [],
  success: {
    llmProviderLive: false,
    ttsProviderLive: false,
    sttSkipped: false,
    subtitleDelivered: false,
    subtitleReceiptCount: 0,
    audioSubmitted: false,
    audioDrained: false,
    audioReceiptCount: 0,
  },
  audioReceipts: [],
  subtitlePresentationReceipts: [],
});

describe("Tauri bridge normalization", () => {
  it("retains pending provider provenance without treating start as delivery proof", () => {
    expect(
      normalizeNativeSimulationEvent({
        type: "started",
        simulationId: "native-pending",
        generation: 2,
        sequence: 1,
        measurementBasis: "pendingProviderEvidence",
      }),
    ).toEqual({
      type: "started",
      simulationId: "native-pending",
      generation: 2,
      sequence: 1,
      measurementBasis: "pendingProviderEvidence",
    });
  });

  it("preserves strict terminal character identity and prompt provenance", () => {
    const event = normalizeNativeSimulationEvent({
      type: "failed",
      simulationId: "character-proof",
      generation: 3,
      sequence: 9,
      reason: "fail closed",
      characterContext: {
        profileId: "skyrim-special-edition",
        characterId: "lydia",
        identitySource: "manual_character_id",
        explicitSelection: true,
        selection: {
          status: "known",
          character_id: "lydia",
          reason: "explicit",
        },
        prompt: {
          schemaVersion: "1",
          profileId: "skyrim-special-edition",
          characterId: "lydia",
          authorities: ["core_canon", "character_profile", "retrieved_memory"],
          recordCount: 2,
          retrievalProvenance: {
            schema_version: "character-db/1.0.0",
            policy_fingerprint_sha256: "a".repeat(64),
            query_sha256: "b".repeat(64),
            selected_profile_knowledge_ids: ["lore-1"],
            selected_memory_item_ids: ["memory-1"],
            embedding_metadata_ids: [],
          },
          scopedMemoryItemIds: ["memory-1"],
          scopedMemoryClasses: ["recent_delivered_turns"],
        },
      },
    });
    expect(
      event?.type === "failed" ? event.characterContext : null,
    ).toMatchObject({
      profileId: "skyrim-special-edition",
      characterId: "lydia",
      identitySource: "manual_character_id",
      explicitSelection: true,
      selection: { status: "known", character_id: "lydia", reason: "explicit" },
      prompt: {
        authorities: ["core_canon", "character_profile", "retrieved_memory"],
        recordCount: 2,
        scopedMemoryItemIds: ["memory-1"],
      },
    });
  });

  it("normalizes failed and cancelled terminal evidence without calling either completed", () => {
    const turnExecution = {
      consumedRoute: {
        schemaVersion: 1,
        sourceLoadoutId: "test-loadout",
        generation: 2,
        sha256: "a".repeat(64),
        llm: null,
        tts: null,
      },
      deliveryState: "manualRetryRequired",
      commitState: "notCommitted",
      degradations: [
        {
          type: "manualRetryRequired",
          failedRole: "tts",
          providerId: "elevenlabs",
          reason: "Provider timed out.",
          retryable: true,
        },
      ],
      success: {
        llmProviderLive: true,
        ttsProviderLive: false,
        sttSkipped: true,
        subtitleDelivered: false,
        subtitleReceiptCount: 0,
        audioSubmitted: false,
        audioDrained: false,
        audioReceiptCount: 0,
      },
      audioReceipts: [],
      subtitlePresentationReceipts: [],
    };
    for (const type of ["failed", "cancelled"] as const) {
      const event = normalizeNativeSimulationEvent({
        type,
        simulationId: `native-${type}`,
        generation: 2,
        sequence: 7,
        reason: "Provider timed out.",
        turnExecution,
      });
      expect(event?.type).toBe(type);
      expect(
        event?.type === "failed" || event?.type === "cancelled"
          ? event.turnExecution?.degradations[0]
          : null,
      ).toEqual(turnExecution.degradations[0]);
      expect(event?.type).not.toBe("completed");
    }
  });

  it("normalizes current snake-case completion fields without producing NaN", () => {
    expect(
      normalizeNativeSimulationEvent({
        type: "completed",
        simulation_id: "native-12",
        generation: 3,
        sequence: 9,
        fixture_first_audio_ms: 1310,
        runtime_fixture_only: true,
        delivered_text: "Native runtime reply.",
      }),
    ).toEqual({
      type: "completed",
      simulationId: "native-12",
      generation: 3,
      sequence: 9,
      fixtureFirstAudioMs: 1310,
      runtimeFixtureOnly: true,
      deliveredText: "Native runtime reply.",
    });
  });

  it("retains bounded native visual presentation proof on completion", () => {
    const event = normalizeNativeSimulationEvent({
      type: "completed",
      simulationId: "native-visible-speech",
      generation: 8,
      sequence: 17,
      fixtureFirstAudioMs: null,
      runtimeFixtureOnly: false,
      deliveredText: "The harbor can finally hear me.",
      visualPresentation: {
        schemaVersion: 1,
        sourceFrameSequence: 441,
        residualProposed: true,
        presented: true,
        degraded: false,
        pixelSource: "WindowsGraphicsCaptureTexture",
        pixelScope: "ExactSelectedWindow",
        detail: "review mouth residual presented",
      },
    });

    expect(
      event?.type === "completed" ? event.visualPresentation : undefined,
    ).toEqual({
      schemaVersion: 1,
      sourceFrameSequence: 441,
      residualProposed: true,
      presented: true,
      degraded: false,
      pixelSource: "WindowsGraphicsCaptureTexture",
      pixelScope: "ExactSelectedWindow",
      detail: "review mouth residual presented",
    });
  });

  it("uses a null timing fallback when the native completion omits timing", () => {
    const event = normalizeNativeSimulationEvent({
      type: "completed",
      simulation_id: "native-13",
      generation: 3,
      sequence: 9,
      delivered_text: "Text still arrived.",
    });

    expect(event?.type).toBe("completed");
    if (event?.type === "completed") {
      expect(event.fixtureFirstAudioMs).toBeNull();
      expect(event.runtimeFixtureOnly).toBe(true);
    }
  });

  it("preserves a non-fixture flag without fabricating audio receipt evidence", () => {
    const event = normalizeNativeSimulationEvent({
      type: "completed",
      simulationId: "native-live-14",
      generation: 4,
      sequence: 12,
      fixtureFirstAudioMs: 287,
      runtimeFixtureOnly: false,
      deliveredText: "Runtime text completed.",
    });
    expect(event).toMatchObject({
      type: "completed",
      runtimeFixtureOnly: false,
    });
    expect(
      event?.type === "completed" ? event.turnExecution : undefined,
    ).toBeUndefined();
  });

  it("preserves the accepted one-time STT receipt binding without transcript, PCM, or token data", () => {
    const input = selectedSttInputEvidence();
    const event = normalizeNativeSimulationEvent({
      type: "completed",
      simulationId: "native-stt-accepted",
      generation: 21,
      sequence: 12,
      runtimeFixtureOnly: false,
      deliveredText: "Native response after opaque STT receipt consumption.",
      turnExecution: turnExecutionWithInput(input),
    });

    expect(
      event?.type === "completed" ? event.turnExecution?.input : undefined,
    ).toEqual(input);
    const collectKeys = (value: unknown): string[] => {
      if (Array.isArray(value)) return value.flatMap(collectKeys);
      if (typeof value !== "object" || value === null) return [];
      return Object.entries(value).flatMap(([key, nested]) => [
        key,
        ...collectKeys(nested),
      ]);
    };
    expect(collectKeys(event)).not.toEqual(
      expect.arrayContaining([
        "transcript",
        "providerToken",
        "pcmPayload",
        "credential",
      ]),
    );
  });

  it("drops turn execution when selected STT receipt evidence is forged, replay-shaped, or malformed", () => {
    const invalidInputs = [
      {
        ...selectedSttInputEvidence(),
        transcript: "WebView text must never be accepted.",
      },
      {
        ...selectedSttInputEvidence(),
        selectedSttReceipt: {
          ...selectedSttInputEvidence().selectedSttReceipt,
          pcmPayload: "AAAA",
        },
      },
      {
        ...selectedSttInputEvidence(),
        selectedSttReceipt: {
          ...selectedSttInputEvidence().selectedSttReceipt,
          route: {
            ...selectedSttInputEvidence().selectedSttReceipt.route,
            credentialReference: "providers/forged",
          },
        },
      },
      {
        ...selectedSttInputEvidence(),
        selectedSttReceipt: {
          ...selectedSttInputEvidence().selectedSttReceipt,
          captureGeneration: 0,
        },
      },
      {
        ...selectedSttInputEvidence(),
        selectedSttReceipt: {
          ...selectedSttInputEvidence().selectedSttReceipt,
          gameId: "skyrim\nforged",
        },
      },
      {
        ...selectedSttInputEvidence(),
        selectedSttReceipt: {
          ...selectedSttInputEvidence().selectedSttReceipt,
          route: {
            ...selectedSttInputEvidence().selectedSttReceipt.route,
            pttReleaseTransitionSequence: 101,
          },
        },
      },
    ];

    for (const [index, input] of invalidInputs.entries()) {
      const event = normalizeNativeSimulationEvent({
        type: "completed",
        simulationId: "native-stt-invalid-" + index,
        generation: 21,
        sequence: 12,
        runtimeFixtureOnly: false,
        deliveredText: "Completion remains visible without forged evidence.",
        turnExecution: turnExecutionWithInput(input),
      });
      expect(event?.type).toBe("completed");
      expect(
        event?.type === "completed" ? event.turnExecution : undefined,
      ).toBeUndefined();
    }
  });

  it("normalizes explicit live-provider, subtitle, submission, drain, and receipt evidence", () => {
    const event = normalizeNativeSimulationEvent({
      type: "completed",
      simulationId: "native-live-15",
      generation: 4,
      sequence: 13,
      fixtureFirstAudioMs: 287,
      runtimeFixtureOnly: false,
      deliveredText: "Receipt-backed completion.",
      turnExecution: {
        consumedRoute: {
          schemaVersion: 1,
          sourceLoadoutId: "native-loadout",
          generation: 3,
          sha256: "b".repeat(64),
          llm: { providerId: "nvidia-nim", modelId: "nemotron", voiceId: null },
          tts: {
            providerId: "elevenlabs",
            modelId: "eleven_flash_v2_5",
            voiceId: "stock-voice",
          },
          privateEvaluationAcknowledgement: {
            providerId: "nvidia-nim",
            termsRevision:
              "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
            catalogRevision: 8,
            applicationNamespace:
              "io.github.akshitireddy.interactive-npcs.review",
            acknowledgementSha256: "c".repeat(64),
            modalities: ["llm", "tts"],
            promotionSupported: false,
            publicationSupported: false,
          },
        },
        deliveryState: "delivered",
        commitState: "committed",
        degradations: [{ type: "typedInput", reason: "Typed turn." }],
        success: {
          llmProviderLive: true,
          ttsProviderLive: true,
          sttSkipped: true,
          subtitleDelivered: true,
          subtitleReceiptCount: 1,
          audioSubmitted: true,
          audioDrained: true,
          audioReceiptCount: 1,
        },
        audioReceipts: [
          {
            receiptId: "audio-receipt-live-1",
            submitted: true,
            drained: true,
            outputSelectionMode: "endpointId",
            outputEndpointId: "windows-endpoint-7",
            outputEndpointGeneration: 22,
          },
        ],
        subtitlePresentationReceipts: [
          {
            receiptId: "subtitle-v1-live-1",
            sentenceId: 1,
            provenance: "trustedNativeCapture",
            presentationId: 101,
            targetGeometryEpoch: 3,
            captureSequence: 44,
            graphicsGeneration: 7,
            layerHashHex: "0123456789abcdef",
            presentedQpcTicks: "987654321",
            desktopXPx: -1200,
            desktopYPx: 700,
            widthPx: 600,
            heightPx: 140,
            dpiX: 144,
            dpiY: 144,
            direction: "leftToRight",
            bidiShapingApplied: true,
            graphemeClustersPreserved: true,
            usedBottomCenterFallback: false,
            colorTreatment: "windowsCompositorSdrWhiteMapping",
            committed: true,
          },
        ],
      },
    });

    expect(event?.type === "completed" ? event.turnExecution : null).toEqual({
      consumedRoute: {
        schemaVersion: 1,
        sourceLoadoutId: "native-loadout",
        generation: 3,
        sha256: "b".repeat(64),
        llm: { providerId: "nvidia-nim", modelId: "nemotron", voiceId: null },
        tts: {
          providerId: "elevenlabs",
          modelId: "eleven_flash_v2_5",
          voiceId: "stock-voice",
        },
        privateEvaluationAcknowledgement: {
          providerId: "nvidia-nim",
          termsRevision:
            "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1",
          catalogRevision: 8,
          applicationNamespace:
            "io.github.akshitireddy.interactive-npcs.review",
          acknowledgementSha256: "c".repeat(64),
          modalities: ["llm", "tts"],
          promotionSupported: false,
          publicationSupported: false,
        },
      },
      deliveryState: "delivered",
      commitState: "committed",
      degradations: [{ type: "typedInput", reason: "Typed turn." }],
      success: {
        llmProviderLive: true,
        ttsProviderLive: true,
        sttSkipped: true,
        subtitleDelivered: true,
        subtitleReceiptCount: 1,
        audioSubmitted: true,
        audioDrained: true,
        audioReceiptCount: 1,
      },
      audioReceipts: [
        {
          receiptId: "audio-receipt-live-1",
          submitted: true,
          drained: true,
          outputSelectionMode: "endpointId",
          outputEndpointId: "windows-endpoint-7",
          outputEndpointGeneration: 22,
        },
      ],
      subtitlePresentationReceipts: [
        {
          receiptId: "subtitle-v1-live-1",
          sentenceId: 1,
          provenance: "trustedNativeCapture",
          presentationId: 101,
          targetGeometryEpoch: 3,
          captureSequence: 44,
          graphicsGeneration: 7,
          layerHashHex: "0123456789abcdef",
          presentedQpcTicks: "987654321",
          desktopXPx: -1200,
          desktopYPx: 700,
          widthPx: 600,
          heightPx: 140,
          dpiX: 144,
          dpiY: 144,
          direction: "leftToRight",
          bidiShapingApplied: true,
          graphemeClustersPreserved: true,
          usedBottomCenterFallback: false,
          colorTreatment: "windowsCompositorSdrWhiteMapping",
          committed: true,
        },
      ],
    });
  });

  it("retries a startup snapshot and returns authenticated connections", async () => {
    const invoke = vi
      .fn<() => Promise<NativeBootstrapSnapshot>>()
      .mockResolvedValueOnce(snapshot(false, false))
      .mockResolvedValueOnce(snapshot(true, true));
    const wait = vi.fn().mockResolvedValue(undefined);

    const result = await loadNativeBootstrapHealth(
      { maxAttempts: 4, initialDelayMs: 25, sleep: wait },
      invoke,
    );

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(wait).toHaveBeenCalledWith(25, undefined);
    expect(result.kind).toBe("snapshot");
    if (result.kind === "snapshot") {
      expect(result.attempts).toBe(2);
      expect(result.snapshot.runtime.connected).toBe(true);
      expect(result.snapshot.mediaBroker.connected).toBe(true);
    }
  });

  it("returns the last bounded degraded snapshot instead of inventing health", async () => {
    const invoke = vi
      .fn<() => Promise<NativeBootstrapSnapshot>>()
      .mockResolvedValue(snapshot(true, false));

    const result = await loadNativeBootstrapHealth(
      { maxAttempts: 3, initialDelayMs: 0, sleep: async () => undefined },
      invoke,
    );

    expect(invoke).toHaveBeenCalledTimes(3);
    expect(result.kind).toBe("snapshot");
    if (result.kind === "snapshot")
      expect(result.snapshot.mediaBroker.connected).toBe(false);
  });

  it("keeps polling beyond the old fifth attempt until startup becomes ready", async () => {
    let invocation = 0;
    const invoke = vi.fn(async () => {
      invocation += 1;
      return invocation < 8 ? snapshot(false, false) : snapshot(true, true);
    });
    const progress: number[] = [];

    const result = await loadNativeBootstrapHealth(
      {
        maxAttempts: 12,
        maxElapsedMs: 20_000,
        initialDelayMs: 0,
        sleep: async () => undefined,
        onProgress: (health) => progress.push(health.attempts),
      },
      invoke,
    );

    expect(invoke).toHaveBeenCalledTimes(8);
    expect(progress).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(result.kind).toBe("snapshot");
    if (result.kind === "snapshot") {
      expect(result.attempts).toBe(8);
      expect(result.snapshot.runtime.connected).toBe(true);
      expect(result.snapshot.mediaBroker.connected).toBe(true);
    }
  });

  it("cancels startup polling without publishing a stale final result", async () => {
    const controller = new AbortController();
    const invoke = vi.fn().mockResolvedValue(snapshot(false, false));

    await expect(
      loadNativeBootstrapHealth(
        {
          maxAttempts: 30,
          signal: controller.signal,
          sleep: async () => controller.abort(),
        },
        invoke,
      ),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("exposes the exact capture command only with native debug capability", () => {
    enableTauri();
    expect(syntheticReplayCaptureAvailability(false)).toEqual({
      available: false,
      reason: "releaseBuild",
    });
    expect(syntheticReplayCaptureAvailability(true)).toEqual({
      available: true,
      commandName: "debug_select_synthetic_replay_capture_target",
    });
  });

  it("selects the synthetic review target through the registered debug-only command", async () => {
    enableTauri();
    const selected = {
      targetProcessId: 7331,
      targetWindowHandle: 880055,
      targetExecutableBasename: "interactive-npcs-synthetic-target.exe",
      diagnostics: { framesReceived: 4 },
    };
    tauriMocks.invoke.mockResolvedValueOnce(selected);

    await expect(
      runSyntheticReplayCapture({
        available: true,
        commandName: "debug_select_synthetic_replay_capture_target",
      }),
    ).resolves.toEqual(selected);
    expect(tauriMocks.invoke.mock.calls).toEqual([
      ["debug_select_synthetic_replay_capture_target"],
    ]);
  });

  it("preserves a native synthetic verification error", async () => {
    enableTauri();
    tauriMocks.invoke.mockRejectedValueOnce(
      new Error("capture receipt did not advance"),
    );
    await expect(verifySelectedGameCapture()).rejects.toThrow(
      "capture receipt did not advance",
    );
  });
});

describe("Tauri command bridge", () => {
  const onboarding: OnboardingSnapshot = {
    schemaVersion: 1,
    completed: false,
    currentStep: "game",
    selectedGameId: "eclipse-harbor",
    preferences: {
      execution: "hybrid",
      performance: "balanced",
      subtitles: true,
      ptt: true,
      localOnly: false,
      screenPresence: false,
      diagnostics: true,
    },
    updatedAtEpochMs: 0,
  };

  it("keeps native-only setup and diagnostics calls inert in browser preview", async () => {
    await expect(saveOnboarding(onboarding)).resolves.toBeNull();
    await expect(readDiagnosticSummary()).resolves.toBeNull();
    expect(tauriMocks.invoke).not.toHaveBeenCalled();
  });

  it("passes onboarding through the narrow native persistence command", async () => {
    enableTauri();
    const saved = {
      onboarding: { ...onboarding, updatedAtEpochMs: 42 },
      persistence: { health: "healthy" as const, detail: "Saved." },
    };
    tauriMocks.invoke.mockResolvedValueOnce(saved);

    await expect(saveOnboarding(onboarding)).resolves.toEqual(saved);
    expect(tauriMocks.invoke).toHaveBeenCalledWith("save_onboarding", {
      onboarding,
    });
  });

  it("invokes the current native diagnostic summary without invented arguments", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({ overall: "readyForSimulation" });

    await readDiagnosticSummary();

    expect(tauriMocks.invoke.mock.calls.map(([command]) => command)).toEqual([
      "diagnostic_summary",
    ]);
  });

  it("uses the frozen product command names and exact argument nesting", async () => {
    enableTauri();
    tauriMocks.invoke.mockImplementation(async (command: string) =>
      command === "export_diagnostics_v2"
        ? { fileName: "interactive-npcs-diagnostics.json" }
        : {},
    );
    const resourceSettings = {
      schemaVersion: 1,
      governor: {
        schema: "policy",
        vram_soft_ceiling_basis_points: 9000,
        ram_soft_ceiling_basis_points: 8500,
        minimum_vram_safety_bytes: 1,
        proportional_vram_safety_basis_points: 1500,
        minimum_ram_safety_bytes: 1,
        maximum_snapshot_age_millis: 2000,
        keep_warm_millis: 30000,
        unload_ttl_millis: 120000,
      },
      gameReserveVramBytes: 4,
      gameAdditionalReserveRamBytes: 2,
      preferredResidency: "cpu_resident_gpu_cold" as const,
    };
    const packState = { packId: "openseeface", revision: "1" };
    const loadoutSelection = {
      selection_id: "selected-local-loadout-1",
      roles: [
        {
          role: "language_model" as const,
          identity: { pack_id: "local.llm.test", revision: "r1" },
          preferred_residency: "cpu_resident_gpu_cold" as const,
        },
      ],
      expected_idle_millis: 1_000,
    };
    const preferenceScope = {
      kind: "character" as const,
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
    };
    const preferenceEntry = {
      scope: preferenceScope,
      executionPreset: "hybrid" as const,
      performancePreset: "immersive" as const,
      overrides: {
        verbosity: "detailed" as const,
        creativity: 67,
        inputMode: "ptt" as const,
        subtitles: true,
        vision: false,
        webcamPresence: false,
      },
    };
    await discoverGameTargets("skyrim-special-edition");
    await selectGameTarget("skyrim-special-edition", 55, true);
    await readSelectedGameTarget();
    await clearGameTarget();
    await verifySelectedGameCapture();
    await readCharacterDatabaseCatalog("skyrim-special-edition");
    await inspectCharacterDatabase("skyrim-special-edition", "lydia");
    await persistSelectedCharacter("skyrim-special-edition", "lydia");
    await readCharacterMemoryStatus("skyrim-special-edition", "lydia");
    await backupAllLocalMemory();
    await listLocalMemoryBackups();
    await deleteLocalMemoryBackup("backup-7");
    await eraseCharacterMemory("skyrim-special-edition", "lydia", true);
    await restoreLocalMemoryBackup("backup-7");
    await removeAllLocalMemory(true);
    await correctEncounterToAuthoredCharacter({
      gameProfileId: "skyrim-special-edition",
      encounterId: "11111111-1111-4111-8111-111111111111",
      characterId: "lydia",
      explicitUserConfirmation: true,
    });
    await mergeUnknownEncounters({
      gameProfileId: "skyrim-special-edition",
      sourceEncounterId: "11111111-1111-4111-8111-111111111111",
      destinationEncounterId: "22222222-2222-4222-8222-222222222222",
      explicitUserConfirmation: true,
    });
    await readLocalResourceSettings();
    await enumerateAudioOutputs();
    await readSelectedAudioOutput();
    await selectAudioOutput({
      mode: "endpointId",
      endpoint_id: "windows-endpoint-7",
    });
    await selectAudioOutput({ mode: "systemDefault" });
    await enumerateAudioInputs();
    await readSelectedAudioInput();
    await selectAudioInput({
      mode: "endpointId",
      endpoint_id: "windows-microphone-7",
    });
    await selectAudioInput({ mode: "systemDefault" });
    await readIdentityReferenceEnrollmentStatus();
    await enrollIdentityReference({
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
      referenceId: "lydia-private-reference-1",
      subjectDisplayName: "Lydia",
      sourceClass: "user_private",
      ownerUserId: "local-user",
      explicitUserConsent: true,
    });
    await readProductPreferences(preferenceScope);
    await saveProductPreferences(9, preferenceEntry);
    await resetProductPreferences(10, preferenceScope);
    await saveLocalResourceSettings(resourceSettings);
    await readLocalResourceTelemetry();
    await readSelectedLocalLoadoutPlanner();
    await readTrustedLocalPackCatalog();
    await admitSelectedLocalLoadout(loadoutSelection);
    await readExperimentalVisualPackStatus();
    await mutateExperimentalVisualPack("install", packState);
    await readDiagnosticsV2(100);
    await readDiagnosticsMatrix();
    await readDiagnosticsSettings();
    await saveDiagnosticsSettings({ schemaVersion: 1, verbosity: "essential" });
    await exportDiagnosticsV2(100);
    await startThisPcBenchmark({
      requestedIterations: 10,
      timeoutMillis: 120_000,
      baselineWindowMillis: 3_000,
    });
    await cancelThisPcBenchmark();
    await readThisPcBenchmarkStatus();
    await readThisPcBenchmarkReport("report-1");
    expect(tauriMocks.invoke.mock.calls).toEqual([
      ["discover_game_targets", { gameProfileId: "skyrim-special-edition" }],
      [
        "select_game_target",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            nativeWindowHint: 55,
            explicitUserConfirmedOfflineSinglePlayer: true,
          },
        },
      ],
      ["selected_game_target", {}],
      ["clear_game_target", {}],
      ["verify_selected_game_capture", {}],
      [
        "character_database_catalog",
        { gameProfileId: "skyrim-special-edition" },
      ],
      [
        "character_database_inspection",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            characterId: "lydia",
            recentTurnLimit: 20,
          },
        },
      ],
      [
        "select_game_character",
        { gameProfileId: "skyrim-special-edition", characterId: "lydia" },
      ],
      [
        "character_memory_status",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            characterId: "lydia",
          },
        },
      ],
      [
        "backup_all_local_memory",
        { request: { explicitUserConfirmation: true } },
      ],
      ["list_local_memory_backups", {}],
      [
        "delete_local_memory_backup",
        {
          request: {
            backupId: "backup-7",
            explicitUserConfirmation: true,
          },
        },
      ],
      [
        "erase_character_memory",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            characterId: "lydia",
            explicitUserConfirmation: true,
            backupBeforeErasure: true,
          },
        },
      ],
      [
        "restore_local_memory_backup",
        {
          request: {
            backupId: "backup-7",
            explicitUserConfirmation: true,
          },
        },
      ],
      [
        "remove_all_local_memory",
        {
          request: {
            explicitUserConfirmation: true,
            includeBackups: true,
          },
        },
      ],
      [
        "correct_encounter_to_authored_character",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            encounterId: "11111111-1111-4111-8111-111111111111",
            characterId: "lydia",
            explicitUserConfirmation: true,
          },
        },
      ],
      [
        "merge_unknown_encounters",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            sourceEncounterId: "11111111-1111-4111-8111-111111111111",
            destinationEncounterId: "22222222-2222-4222-8222-222222222222",
            explicitUserConfirmation: true,
          },
        },
      ],
      ["local_resource_settings", {}],
      ["enumerate_audio_outputs", {}],
      ["selected_audio_output", {}],
      [
        "select_audio_output",
        {
          selection: {
            mode: "endpointId",
            endpoint_id: "windows-endpoint-7",
          },
        },
      ],
      ["select_audio_output", { selection: { mode: "systemDefault" } }],
      ["enumerate_audio_inputs", {}],
      ["selected_audio_input", {}],
      [
        "select_audio_input",
        {
          selection: {
            mode: "endpointId",
            endpoint_id: "windows-microphone-7",
          },
        },
      ],
      ["select_audio_input", { selection: { mode: "systemDefault" } }],
      ["identity_reference_enrollment_status", {}],
      [
        "enroll_identity_reference",
        {
          request: {
            gameProfileId: "skyrim-special-edition",
            characterId: "lydia",
            referenceId: "lydia-private-reference-1",
            subjectDisplayName: "Lydia",
            sourceClass: "user_private",
            ownerUserId: "local-user",
            explicitUserConsent: true,
          },
        },
      ],
      ["product_preferences_snapshot", { scope: preferenceScope }],
      [
        "save_product_preferences",
        { request: { expectedRevision: 9, entry: preferenceEntry } },
      ],
      [
        "reset_product_preferences",
        {
          request: {
            expectedRevision: 10,
            scope: preferenceScope,
            explicitUserConfirmation: true,
          },
        },
      ],
      ["save_local_resource_settings", { settings: resourceSettings }],
      ["local_resource_telemetry", {}],
      ["selected_local_loadout_planner", {}],
      ["trusted_local_pack_catalog", {}],
      ["admit_selected_local_loadout", { selection: loadoutSelection }],
      ["experimental_visual_pack_status", {}],
      [
        "install_experimental_model_pack",
        {
          request: {
            packId: "openseeface",
            revision: "1",
            explicitUserConfirmation: true,
          },
        },
      ],
      ["diagnostics_v2_snapshot", { maxEvents: 100 }],
      ["diagnostics_v2_matrix", {}],
      ["diagnostics_v2_settings", {}],
      [
        "save_diagnostics_v2_settings",
        { settings: { schemaVersion: 1, verbosity: "essential" } },
      ],
      [
        "export_diagnostics_v2",
        { request: { maxEvents: 100, explicitUserConfirmation: true } },
      ],
      [
        "start_this_pc_benchmark",
        {
          request: {
            requestedIterations: 10,
            timeoutMillis: 120_000,
            baselineWindowMillis: 3_000,
          },
        },
      ],
      ["cancel_this_pc_benchmark", {}],
      ["this_pc_benchmark_status", {}],
      ["this_pc_benchmark_report", { reportId: "report-1" }],
    ]);
  });

  it("accepts only a bounded diagnostics export basename", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({
      fileName: "interactive-npcs-diagnostics-20260830.json",
      preview: {},
      uploaded: false,
    });
    await expect(exportDiagnosticsV2(25)).resolves.toMatchObject({
      fileName: "interactive-npcs-diagnostics-20260830.json",
      uploaded: false,
    });
    expect(tauriMocks.invoke).toHaveBeenLastCalledWith(
      "export_diagnostics_v2",
      { request: { maxEvents: 25, explicitUserConfirmation: true } },
    );

    for (const fileName of [
      "../diagnostics.json",
      "nested/diagnostics.json",
      "nested\\diagnostics.json",
      "diagnostics\u0000.json",
      "x".repeat(129),
    ]) {
      tauriMocks.invoke.mockResolvedValueOnce({
        fileName,
        preview: {},
        uploaded: false,
      });
      await expect(exportDiagnosticsV2()).rejects.toThrow("invalid file name");
    }
  });

  it("keeps effective configuration read-only and sends exact subtitle preference DTOs", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValue({});
    const scope = {
      kind: "character" as const,
      gameProfileId: "skyrim-special-edition",
      characterId: "lydia",
    };
    const entry = {
      scope,
      selectedStyleId: "studio-default",
      overrides: {
        safeAreaDp: 32,
        textScale: 1.15,
        backplateEnabled: true,
        opacity: 0.9,
      },
    };

    await readEffectiveConfiguration(scope);
    await readSubtitlePreferences(scope);
    await saveSubtitlePreferences(12, entry);
    await resetSubtitlePreferences(13, scope);

    expect(tauriMocks.invoke.mock.calls).toEqual([
      ["effective_configuration_snapshot", { request: { scope } }],
      ["read_subtitle_preferences", { request: { scope } }],
      [
        "save_subtitle_preferences",
        { request: { expectedRevision: 12, entry } },
      ],
      [
        "reset_subtitle_preferences",
        {
          request: {
            expectedRevision: 13,
            scope,
            explicitUserConfirmation: true,
          },
        },
      ],
    ]);
  });

  it("exposes only singleton start status and cancel intents for native actor selection", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValue({
      schemaVersion: 1,
      state: "unavailable",
      unavailableReason: "noAdmittedNativeCandidateSet",
      detail: "No current admitted native visual candidate set is available.",
    });

    await startManualActorPicker();
    await readManualActorPickerStatus();
    await cancelManualActorPicker();

    expect(tauriMocks.invoke.mock.calls).toEqual([
      ["start_manual_actor_picker", {}],
      ["manual_actor_picker_status", {}],
      ["cancel_manual_actor_picker", {}],
    ]);
    for (const [, args] of tauriMocks.invoke.mock.calls) {
      expect(args).toEqual({});
      expect(args).not.toHaveProperty("candidate");
      expect(args).not.toHaveProperty("coordinates");
      expect(args).not.toHaveProperty("nativeWindow");
    }
  });

  it("requires and sends the explicit selected identity for an ordinary turn", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({});

    await expect(
      startNativeSimulation("hybrid", vi.fn(), {
        gameProfileId: "eclipse-harbor",
        characterId: "mara-venn",
      }),
    ).resolves.toBe(true);

    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_simulation", {
      request: {
        gameProfileId: "eclipse-harbor",
        characterId: "mara-venn",
        transcript: "Did you ever make it to the old lighthouse?",
        enabledSpoilerTiers: [],
        executionMode: "hybrid",
      },
      events: expect.anything(),
    });
  });

  it("passes selected identity, transcript, and explicitly authorized live TTS", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({});

    await startNativeSimulation("cloud", vi.fn(), {
      gameProfileId: "synthetic-replay",
      characterName: "Mara Venn",
      characterId: "mara-venn",
      transcript: "Can you hear this live route?",
      devLiveTts: {
        providerId: "elevenlabs",
        modelId: "eleven_flash_v2_5",
        voiceId: "EXAVITQu4vr4xnSDxMaL",
        explicitUserAuthorization: true,
      },
    });

    expect(tauriMocks.invoke).toHaveBeenCalledWith("start_simulation", {
      request: {
        gameProfileId: "synthetic-replay",
        characterName: "Mara Venn",
        characterId: "mara-venn",
        transcript: "Can you hear this live route?",
        enabledSpoilerTiers: [],
        executionMode: "cloud",
        devLiveTts: {
          providerId: "elevenlabs",
          modelId: "eleven_flash_v2_5",
          voiceId: "EXAVITQu4vr4xnSDxMaL",
          explicitUserAuthorization: true,
        },
      },
      events: expect.anything(),
    });
  });

  it("passes only an opaque selected-STT receipt when native transcript consumption is requested", async () => {
    enableTauri();
    tauriMocks.invoke.mockResolvedValueOnce({});

    await startNativeSimulation("cloud", vi.fn(), {
      gameProfileId: "skyrim-special-edition",
      characterName: "Lydia",
      characterId: "lydia",
      transcript: "forged text must be stripped",
      selectedSttReceipt: {
        receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
        generation: 14,
      },
    });

    const [, args] = tauriMocks.invoke.mock.calls[0];
    expect(args).toEqual({
      request: {
        gameProfileId: "skyrim-special-edition",
        characterName: "Lydia",
        characterId: "lydia",
        selectedSttReceipt: {
          receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
          generation: 14,
        },
        enabledSpoilerTiers: [],
        executionMode: "cloud",
      },
      events: expect.anything(),
    });
    expect((args as { request: object }).request).not.toHaveProperty(
      "transcript",
    );
  });
});
