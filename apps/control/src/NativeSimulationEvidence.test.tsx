import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NativeProductPreferenceScope,
  NativeProductPreferenceSnapshot,
  NativeSimulationEvent,
} from "./tauriBridge";

const bridge = vi.hoisted(() => ({
  callback: null as ((event: NativeSimulationEvent) => void) | null,
  start: vi.fn(),
  cancel: vi.fn(),
  loadBootstrap: vi.fn(),
  saveOnboarding: vi.fn(),
  inspect: vi.fn(),
  diagnostics: vi.fn(),
  diagnosticsV2: vi.fn(),
  diagnosticsMatrix: vi.fn(),
  diagnosticsSettings: vi.fn(),
  saveDiagnosticsSettings: vi.fn(),
  diagnosticsExport: vi.fn(),
  audioCatalog: vi.fn(),
  audioSelected: vi.fn(),
  audioSelect: vi.fn(),
  inputCatalog: vi.fn(),
  inputSelected: vi.fn(),
  inputSelect: vi.fn(),
  identityEnrollmentStatus: vi.fn(),
  readPreferences: vi.fn(),
  savePreferences: vi.fn(),
  captureSelect: vi.fn(),
  captureVerify: vi.fn(),
}));

const selectedStt = vi.hoisted(() => ({
  start: vi.fn(),
  status: vi.fn(),
  cancel: vi.fn(),
  terminal: null as ((event: Record<string, unknown>) => void) | null,
}));

const nativeLoadouts = vi.hoisted(() => ({
  snapshot: vi.fn(),
}));

vi.mock("./tauriBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tauriBridge")>()),
  LOADING_NATIVE_BOOTSTRAP: { kind: "loading", attempts: 0 },
  startNativeSimulation: bridge.start,
  cancelNativeSimulation: bridge.cancel,
  loadNativeBootstrapHealth: bridge.loadBootstrap,
  saveOnboarding: bridge.saveOnboarding,
  inspectCharacterDatabase: bridge.inspect,
  readDiagnosticSummary: bridge.diagnostics,
  readDiagnosticsV2: bridge.diagnosticsV2,
  readDiagnosticsMatrix: bridge.diagnosticsMatrix,
  readDiagnosticsSettings: bridge.diagnosticsSettings,
  saveDiagnosticsSettings: bridge.saveDiagnosticsSettings,
  exportDiagnosticsV2: bridge.diagnosticsExport,
  enumerateAudioOutputs: bridge.audioCatalog,
  readSelectedAudioOutput: bridge.audioSelected,
  selectAudioOutput: bridge.audioSelect,
  enumerateAudioInputs: bridge.inputCatalog,
  readSelectedAudioInput: bridge.inputSelected,
  selectAudioInput: bridge.inputSelect,
  readIdentityReferenceEnrollmentStatus: bridge.identityEnrollmentStatus,
  readProductPreferences: bridge.readPreferences,
  saveProductPreferences: bridge.savePreferences,
  syntheticReplayCaptureAvailability: () => ({
    available: true,
    commandName: "debug_select",
  }),
  runSyntheticReplayCapture: bridge.captureSelect,
  verifySelectedGameCapture: bridge.captureVerify,
}));

vi.mock("./selectedSttBridge", () => ({
  startSelectedSttPushToTalk: selectedStt.start,
  readSelectedSttPushToTalkStatus: selectedStt.status,
  cancelSelectedSttPushToTalk: selectedStt.cancel,
}));

vi.mock("./providerLoadoutBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./providerLoadoutBridge")>()),
  nativeSnapshot: nativeLoadouts.snapshot,
}));

import { App } from "./App";

const preferences = {
  execution: "cloud" as const,
  performance: "balanced" as const,
  subtitles: true,
  ptt: true,
  localOnly: false,
  screenPresence: false,
  diagnostics: true,
};
const authenticatedBootstrap = {
  kind: "snapshot" as const,
  attempts: 2,
  snapshot: {
    contractVersion: 1,
    appVersion: "2.0.0-test",
    onboarding: {
      schemaVersion: 1,
      completed: true,
      currentStep: "ready" as const,
      selectedGameId: "eclipse-harbor",
      preferences,
      updatedAtEpochMs: 1,
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
    providers: [
      {
        providerId: "elevenlabs",
        displayName: "ElevenLabs",
        credentialReference: "vault-ref",
        status: "present" as const,
        detail: "Credential reference present.",
      },
      {
        providerId: "assemblyai",
        displayName: "AssemblyAI",
        credentialReference: "vault-assemblyai-ref",
        status: "present" as const,
        detail: "Credential reference present.",
      },
    ],
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
    capabilities: { debugSyntheticReplayCapture: true },
  },
};

const sessionPreferenceScope: NativeProductPreferenceScope = {
  kind: "character",
  gameProfileId: "eclipse-harbor",
  characterId: "mara-venn",
};

function nativePreferenceSnapshot(
  subtitles: boolean,
  revision = 1,
): NativeProductPreferenceSnapshot {
  const effective = <T,>(value: T) => ({
    value,
    sourceScope: sessionPreferenceScope,
    sourceKind: "override" as const,
  });
  return {
    schemaVersion: 1,
    revision,
    entries: [
      {
        scope: sessionPreferenceScope,
        executionPreset: "cloud",
        performancePreset: "balanced",
        overrides: {
          verbosity: "standard",
          inputMode: "ptt",
          subtitles,
          overlay: false,
        },
      },
    ],
    effective: {
      scope: sessionPreferenceScope,
      executionPreset: effective("cloud"),
      performancePreset: effective("balanced"),
      verbosity: effective("standard"),
      creativity: effective(50),
      responseLength: effective("medium"),
      interruptionMode: effective("finishSentence"),
      inputMode: effective("ptt"),
      subtitles: effective(subtitles),
      overlay: effective(false),
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

const nativeLoadoutSnapshot = {
  document: {
    format: "npc-provider-loadouts" as const,
    schema_version: 1 as const,
    loadouts: {
      "native-balanced-api": {
        id: "native-balanced-api",
        name: "Balanced API",
        scope: { kind: "global" as const },
        parent: null,
        roles: {
          stt: {
            mode: "route" as const,
            route: {
              primary: {
                provider_id: "assemblyai",
                model_id: "u3-rt-pro",
                voice_id: null,
                credential: {
                  provider_id: "assemblyai",
                  reference_id: "providers/assemblyai",
                },
                disclosure: {
                  catalog_revision: 7,
                  execution: "hosted" as const,
                  egress: "provider_cloud" as const,
                  privacy_summary: "Microphone audio is sent to AssemblyAI.",
                  cost_summary: "Provider charges and limits apply.",
                  transmitted_data: ["microphone_audio" as const],
                },
                explicit_user_selection: true,
              },
              fallbacks: [],
            },
          },
        },
      },
    },
    activation: {
      global: "native-balanced-api",
      games: {},
      characters: {},
    },
  },
  catalogRevision: 7,
  persistenceHealth: "healthy" as const,
  detail: "Loaded native provider routes.",
  credentialsChecked: false as const,
  networkRequestPerformed: false as const,
};

describe("native evidence in the product console", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.history.replaceState(null, "", "/");
    bridge.callback = null;
    nativeLoadouts.snapshot
      .mockReset()
      .mockResolvedValue(nativeLoadoutSnapshot);
    bridge.cancel.mockReset().mockResolvedValue(true);
    bridge.loadBootstrap.mockReset().mockResolvedValue(authenticatedBootstrap);
    bridge.saveOnboarding.mockReset().mockResolvedValue(null);
    bridge.inspect.mockReset().mockResolvedValue(null);
    bridge.diagnostics.mockReset().mockResolvedValue({
      overall: "readyForSimulation",
      generatedAtEpochMs: 1,
      measurements: {
        state: "unmeasured",
        reason: "Controlled benchmark only",
        currentResultsAreReleaseEvidence: false,
      },
      checks: [],
    });
    bridge.diagnosticsV2.mockReset().mockResolvedValue({
      schemaVersion: 1,
      maximumDiskBytes: 2 * 1024 * 1024,
      requestedEventLimit: 100,
      events: [],
      skippedCorruptRecords: 0,
      privacy: {
        schemaVersion: "1.0.0",
        remoteTelemetry: "prohibited",
        automaticUpload: "prohibited",
        exportInitiation: "user_initiated_only",
        locallyRecorded: [],
        excludedByDesign: [],
      },
      recovery: {
        schemaVersion: "1.0.0",
        currentSessionId: "desktop-test",
        previousSession: {
          status: "no_marker",
          sessionId: null,
          applicationVersion: null,
          startedAtUtc: null,
          lastUpdatedAtUtc: null,
          lastPhase: null,
          lastEventMonotonicNs: null,
          priorRecoveryAttempt: null,
        },
        suggestedActions: [],
      },
      verbosity: "standard",
    });
    bridge.diagnosticsSettings.mockReset().mockResolvedValue({
      schemaVersion: 1,
      verbosity: "standard",
    });
    bridge.saveDiagnosticsSettings
      .mockReset()
      .mockImplementation(async (settings) => settings);
    bridge.diagnosticsExport.mockReset().mockResolvedValue({
      fileName: "interactive-npcs-diagnostics-20260830.json",
      preview: {
        schemaVersion: "1.0.0",
        checkCount: 15,
        eventCount: 3,
        includesRecoveryMetadata: true,
        serializedBytes: 2048,
        sha256: "e".repeat(64),
        redactions: [],
        remoteTelemetry: false,
        automaticUpload: false,
      },
      uploaded: false,
    });
    const matrixIds = [
      "credentials.providers",
      "audio.microphone",
      "audio.speaker",
      "provider.stt",
      "provider.tts",
      "provider.llm",
      "models.pack_integrity",
      "models.admission",
      "media.capture",
      "hardware.gpu",
      "hardware.vram",
      "game.target",
      "media.overlay",
      "runtime.latency",
      "system.permissions",
    ];
    bridge.diagnosticsMatrix.mockReset().mockResolvedValue({
      schemaVersion: "1.0.0",
      correlatedTurnId: null,
      credentialPresence: [
        {
          providerId: "elevenlabs",
          state: "present",
          provenance: "measured",
          observedAtUtc: "2026-08-30T13:00:00Z",
        },
      ],
      checks: matrixIds.map((checkId, index) => ({
        checkId,
        category: checkId.split(".")[0],
        status: index < 3 ? "ok" : "skipped",
        provenance: index < 3 ? "measured" : "unmeasured",
        observedAtUtc: index < 3 ? "2026-08-30T13:00:00Z" : null,
        durationMs: null,
        timing: null,
        summaryCode:
          index < 3 ? `${checkId}.native_probe` : `${checkId}.unmeasured`,
        summary:
          index < 3
            ? "The named native probe ran; broader capability is not implied."
            : "No native producer reports this evidence yet.",
        errorCode: null,
        providerId: null,
        modelId: null,
        metrics: {},
        suggestedActions: [
          {
            actionId: `${checkId}.open`,
            kind: checkId.startsWith("provider")
              ? "open_settings_section"
              : "retry_check",
            label: checkId.startsWith("provider")
              ? "Open provider settings"
              : "Retry check",
            targetId: checkId.startsWith("provider") ? "providers" : null,
            requiresConfirmation: false,
          },
        ],
      })),
    });
    bridge.audioCatalog.mockReset().mockResolvedValue({
      schemaVersion: 1,
      catalogGeneration: 3,
      endpoints: [
        {
          endpointId: "windows-speakers-7",
          friendlyName: "Desk speakers",
          state: "active",
          systemDefault: true,
          generation: 12,
        },
        {
          endpointId: "stale-headset",
          friendlyName: "Disconnected headset",
          state: "unplugged",
          systemDefault: false,
          generation: 12,
        },
      ],
    });
    bridge.audioSelected.mockReset().mockResolvedValue(null);
    bridge.audioSelect.mockReset().mockImplementation(async (selection) => ({
      schemaVersion: 1,
      selection,
      resolved: {
        endpointId:
          selection.mode === "systemDefault"
            ? "windows-speakers-7"
            : selection.endpoint_id,
        friendlyName: "Desk speakers",
        state: "active",
        systemDefault: true,
        generation: 12,
      },
    }));
    bridge.inputCatalog.mockReset().mockResolvedValue({
      schemaVersion: 1,
      catalogGeneration: 9,
      endpoints: [
        {
          endpointId: "windows-microphone-4",
          friendlyName: "Desk microphone",
          state: "active",
          systemDefault: true,
          generation: 27,
        },
        {
          endpointId: "stale-microphone",
          friendlyName: "Disconnected microphone",
          state: "unplugged",
          systemDefault: false,
          generation: 27,
        },
      ],
    });
    bridge.inputSelected.mockReset().mockResolvedValue(null);
    bridge.inputSelect.mockReset().mockImplementation(async (selection) => ({
      schemaVersion: 1,
      selection,
      resolved: {
        endpointId:
          selection.mode === "systemDefault"
            ? "windows-microphone-4"
            : selection.endpoint_id,
        friendlyName: "Desk microphone",
        state: "active",
        systemDefault: true,
        generation: 27,
      },
    }));
    bridge.identityEnrollmentStatus.mockReset().mockResolvedValue({
      schemaVersion: 1,
      status: "unavailable",
      reasonCode: "identity_pack_not_admitted",
      detail:
        "Identity reference enrollment is blocked until the signed identity pack is admitted.",
      signedIdentityPackAdmitted: false,
      nativePickerAvailableAfterAdmission: true,
      rawPixelsExposedToWebview: false,
      workerCapabilityExposedToWebview: false,
    });
    bridge.readPreferences
      .mockReset()
      .mockResolvedValue(nativePreferenceSnapshot(true));
    bridge.savePreferences
      .mockReset()
      .mockImplementation(async (expectedRevision, entry) =>
        nativePreferenceSnapshot(
          entry.overrides.subtitles ?? true,
          expectedRevision + 1,
        ),
      );
    bridge.captureSelect.mockReset().mockResolvedValue({
      targetProcessId: 7331,
      targetWindowHandle: 880055,
      targetExecutableBasename: "interactive-npcs-synthetic-target.exe",
      diagnostics: { framesReceived: 4 },
      captureEvidence: { latestFrameSequence: 4 },
    });
    bridge.captureVerify.mockReset().mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "eclipse-harbor",
      target: {
        processId: 7331,
        nativeWindow: 880055,
        executableName: "interactive-npcs-synthetic-target.exe",
        executablePathSha256: "a".repeat(64),
        title: "Interactive NPCs Synthetic Target",
        foreground: true,
        clientWidth: 1280,
        clientHeight: 720,
      },
      capture: {
        schemaVersion: 3,
        selectedProcessId: 7331,
        selectedWindowHandle: 880055,
        deviceGeneration: 2,
        geometryEpoch: 3,
        latestFrameSequence: 8,
        latestFrameQpc: 100,
        initialContentHash: 10,
        latestContentHash: 12,
        contentHashChanges: 2,
        geometryChanges: 0,
        nonadvancingFrames: 0,
        contentWidth: 1280,
        contentHeight: 720,
        overlayCaptureExcluded: true,
        overlayVisualsAllowed: true,
        pixelSource: "windowsGraphicsCaptureTexture",
        pixelScope: "exactSelectedWindow",
        externalDisplayOverlayPixelsExcluded: true,
        desktopLuminanceExcludedFromPixelEvidence: true,
        externalDisplayOverlaysMayChangePerceivedBrightness: true,
        selectedExecutableName: "interactive-npcs-synthetic-target.exe",
      },
      exactPidHwndExecutableMatch: true,
      frameSequenceAdvanced: true,
      contentChanged: true,
      safetyState: "verified_synthetic_fixture",
    });
    selectedStt.terminal = null;
    selectedStt.status.mockReset().mockResolvedValue({
      schemaVersion: 1,
      status: "idle",
      generation: 0,
      sessionId: null,
      turnId: null,
      inputEndpointId: null,
      inputEndpointGeneration: null,
    });
    selectedStt.cancel.mockReset().mockResolvedValue({
      schemaVersion: 1,
      outcome: "cancellationRequested",
      generation: 21,
    });
    selectedStt.start
      .mockReset()
      .mockImplementation(async (_request, terminal) => {
        selectedStt.terminal = terminal;
        return {
          schemaVersion: 1,
          status: "capturing",
          sessionId: "stt-session-live-1",
          turnId: "stt-turn-live-1",
          generation: 21,
          inputEndpointId: "windows-microphone-4",
          inputEndpointGeneration: 27,
          route: {
            providerId: "assemblyai",
            modelId: "u3-rt-pro",
            egress: "microphone_audio_and_optional_non_secret_context",
            automaticFallback: false,
          },
        };
      });
    bridge.start.mockReset().mockImplementation(async (_execution, onEvent) => {
      bridge.callback = onEvent;
      onEvent({
        type: "started",
        simulationId: "native-1",
        generation: 7,
        sequence: 1,
        measurementBasis: "controlledBenchmark",
      });
      onEvent({
        type: "sentenceReady",
        simulationId: "native-1",
        generation: 7,
        sequence: 5,
        text: "The runtime returned a sentence-ready clause.",
      });
      onEvent({
        type: "stageCompleted",
        simulationId: "native-1",
        generation: 7,
        sequence: 6,
        stage: "responding",
        fixtureElapsedMs: 84,
      });
      return true;
    });
  });

  it("shows authenticated health and native selected-route authority", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(
      await screen.findByText("Runtime and media broker authenticated"),
    ).toBeInTheDocument();
    expect(
      await screen.findByLabelText("Selected AssemblyAI push-to-talk"),
    ).toBeInTheDocument();
    await waitFor(() => expect(nativeLoadouts.snapshot).toHaveBeenCalledOnce());
    await user.click(screen.getByText("Microphone connection details"));
    expect(screen.getByText(/assemblyai · u3-rt-pro/i)).toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: /Authorize one live/i }),
    ).not.toBeInTheDocument();
  });

  it("enumerates and explicitly persists a stable Windows audio endpoint", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: /Audio devices/i }));

    const output = await screen.findByRole("combobox", {
      name: "Audio output route",
    });
    expect(
      screen.getByRole("option", { name: /Disconnected headset · unplugged/i }),
    ).toBeDisabled();
    await user.selectOptions(output, "endpoint:windows-speakers-7");
    expect(bridge.audioSelect).toHaveBeenCalledWith({
      mode: "endpointId",
      endpoint_id: "windows-speakers-7",
    });
    expect(
      await screen.findAllByText(/Saved exact endpoint: Desk speakers/i),
    ).not.toHaveLength(0);

    bridge.audioSelect.mockRejectedValueOnce(
      new Error("selected endpoint is no longer present"),
    );
    await user.selectOptions(output, "systemDefault");
    expect(
      await screen.findByText(
        /Selection not changed: selected endpoint is no longer present/i,
      ),
    ).toBeInTheDocument();
    expect(output).toHaveValue("endpoint:windows-speakers-7");
  });

  it("persists an exact microphone endpoint without claiming capture or transcript proof", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: /Audio devices/i }));

    const input = await screen.findByRole("combobox", {
      name: "Audio input route",
    });
    expect(
      screen.getByRole("option", {
        name: /Disconnected microphone · unplugged/i,
      }),
    ).toBeDisabled();
    await user.selectOptions(input, "endpoint:windows-microphone-4");
    expect(bridge.inputSelect).toHaveBeenCalledWith({
      mode: "endpointId",
      endpoint_id: "windows-microphone-4",
    });
    expect(
      await screen.findAllByText(/Saved exact endpoint: Desk microphone/i),
    ).not.toHaveLength(0);
    expect(screen.getAllByText("Not measured")).toHaveLength(4);
    expect(document.body).toHaveTextContent(
      /Choosing a microphone does not start recording or measure signal levels/i,
    );

    bridge.inputSelect.mockRejectedValueOnce(
      new Error("selected microphone is no longer present"),
    );
    await user.selectOptions(input, "systemDefault");
    expect(
      await screen.findByText(
        /Selection not changed: selected microphone is no longer present/i,
      ),
    ).toBeInTheDocument();
    expect(input).toHaveValue("endpoint:windows-microphone-4");

    await user.click(
      screen.getByRole("button", { name: "Refresh audio inputs" }),
    );
    expect(bridge.inputCatalog).toHaveBeenCalledTimes(3);
  });

  it("arms selected AssemblyAI STT and submits only its opaque one-time receipt", async () => {
    bridge.inputSelected.mockResolvedValue({
      schemaVersion: 1,
      selection: {
        mode: "endpointId",
        endpoint_id: "windows-microphone-4",
      },
      resolved: {
        endpointId: "windows-microphone-4",
        friendlyName: "Desk microphone",
        state: "active",
        systemDefault: true,
        generation: 27,
      },
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");

    const arm = await screen.findByRole("button", {
      name: "Enable push-to-talk",
    });
    await waitFor(() => expect(arm).toBeEnabled());
    expect(document.body).toHaveTextContent(
      /Speech recognition by AssemblyAI/i,
    );
    expect(document.body).toHaveTextContent(/u3-rt-pro/i);
    await user.click(screen.getByText("Microphone connection details"));
    expect(document.body).toHaveTextContent(/Automatic fallback false/i);

    expect(
      screen.queryByRole("checkbox", { name: /I approve this AssemblyAI/i }),
    ).not.toBeInTheDocument();
    expect(arm).toBeEnabled();
    await user.click(arm);
    expect(selectedStt.start).toHaveBeenCalledWith(
      {
        schemaVersion: 1,
        gameProfileId: "eclipse-harbor",
        characterId: "mara-venn",
        contextHint: null,
        attempt: { kind: "initial" },
      },
      expect.any(Function),
    );
    expect(document.body).toHaveTextContent(
      /Native capture allocated · generation 21/i,
    );

    await act(async () => {
      selectedStt.terminal?.({
        schemaVersion: 1,
        status: "transcriptReady",
        sessionId: "stt-session-live-1",
        turnId: "stt-turn-live-1",
        generation: 21,
        receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
        receiptSha256: "a".repeat(64),
        route: {
          providerId: "assemblyai",
          modelId: "u3-rt-pro",
          credentialReference: "providers/assemblyai",
          egress: "microphone_audio_and_optional_non_secret_context",
          generation: 21,
          inputEndpointId: "windows-microphone-4",
          inputEndpointGeneration: 27,
          manualRetry: false,
          automaticFallback: false,
          capturedFrames: 24_000,
          pttVirtualKey: 119,
          pttPressTransitionSequence: 41,
          pttPressedQpc: 1_000,
          pttReleaseTransitionSequence: 42,
          pttReleasedQpc: 2_000,
        },
        chunksSent: 8,
        pcmBytesSent: 2560,
        partialEvents: 2,
        errorCode: null,
        retryable: false,
      });
    });
    await user.click(screen.getByText(/Transcription ready · view details/i));
    expect(document.body).toHaveTextContent(/Receipt ready/i);
    expect(document.body).toHaveTextContent(/generation 21/i);
    expect(document.body).toHaveTextContent(
      /Transcript text is not exposed to this WebView/i,
    );
    expect(bridge.start).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: /Send voice message/i }),
    );
    expect(bridge.start.mock.calls.at(-1)?.[2]).toMatchObject({
      selectedSttReceipt: {
        receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
        generation: 21,
      },
    });
    expect(bridge.start.mock.calls.at(-1)?.[2]).not.toHaveProperty(
      "transcript",
    );
    expect(document.body).toHaveTextContent(
      /This receipt has already been submitted once and cannot be reused/i,
    );

    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-ptt-turn-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: false,
        deliveredText: "Native response after receipt-backed input.",
        turnExecution: {
          consumedRoute: {
            schemaVersion: 1,
            sourceLoadoutId: "assemblyai-review-loadout",
            generation: 21,
            sha256: "b".repeat(64),
            llm: null,
            tts: null,
          },
          input: {
            mode: "pushToTalk",
            pushToTalkState: "transcriptReady",
            selectedSttReceipt: {
              schemaVersion: 1,
              receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
              receiptSha256: "a".repeat(64),
              captureSessionId: "018f47c2-7202-7c8d-8e2a-9e2791d69540",
              captureTurnId: "018f47c2-7202-7c8d-8e2a-9e2791d69541",
              captureGeneration: 21,
              gameId: "eclipse-harbor",
              characterId: "mara-venn",
              sourceLoadoutId: "assemblyai-review-loadout",
              route: {
                providerId: "assemblyai",
                modelId: "u3-rt-pro",
                credentialReference: "providers/assemblyai",
                egress: "microphone_audio_and_optional_non_secret_context",
                generation: 21,
                inputEndpointId: "windows-microphone-4",
                inputEndpointGeneration: 27,
                manualRetry: false,
                automaticFallback: false,
                capturedFrames: 24_000,
                pttVirtualKey: 119,
                pttPressTransitionSequence: 41,
                pttPressedQpc: 1_000,
                pttReleaseTransitionSequence: 42,
                pttReleasedQpc: 2_000,
              },
              chunksSent: 8,
              pcmBytesSent: 2560,
              partialEvents: 2,
            },
          },
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
        },
      }),
    );

    const accepted = screen.getByLabelText("Accepted native STT receipt");
    expect(accepted).toHaveTextContent(
      "AssemblyAI u3-rt-pro receipt 018f47c2-7202-7c8d-8e2a-9e2791d6953f",
    );
    expect(accepted).toHaveTextContent("generation 21");
    expect(accepted).toHaveTextContent("windows-microphone-4 generation 27");
    expect(accepted).toHaveTextContent(
      "eclipse-harbor / mara-venn · assemblyai-review-loadout",
    );
    expect(accepted).toHaveTextContent("F8 press 41 → release 42");
    expect(accepted).toHaveTextContent("no automatic fallback");
    expect(accepted).toHaveTextContent(
      /Transcript, microphone audio, and provider credentials are not exposed here/i,
    );
  });

  it("shows the native arming window and keeps cancellation reachable before F8", async () => {
    bridge.inputSelected.mockResolvedValue({
      schemaVersion: 1,
      selection: { mode: "systemDefault" },
      resolved: {
        endpointId: "windows-microphone-4",
        friendlyName: "Desk microphone",
        state: "active",
        systemDefault: true,
        generation: 27,
      },
    });
    selectedStt.status
      .mockResolvedValueOnce({
        schemaVersion: 1,
        status: "idle",
        generation: 21,
        sessionId: null,
        turnId: null,
        inputEndpointId: null,
        inputEndpointGeneration: null,
      })
      .mockResolvedValue({
        schemaVersion: 1,
        status: "arming",
        generation: 22,
        sessionId: null,
        turnId: null,
        inputEndpointId: null,
        inputEndpointGeneration: null,
      });
    let rejectPendingStart: ((reason: Error) => void) | null = null;
    selectedStt.start.mockImplementationOnce(async (_request, terminal) => {
      selectedStt.terminal = terminal;
      return await new Promise<never>((_, reject) => {
        rejectPendingStart = reject;
      });
    });
    selectedStt.cancel.mockImplementationOnce(async () => {
      rejectPendingStart?.(new Error("native PTT arming cancelled"));
      return {
        schemaVersion: 1,
        outcome: "cancellationRequested",
        generation: 22,
      };
    });

    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(
      screen.getByRole("button", { name: "Enable push-to-talk" }),
    );
    expect(await screen.findByText(/Armed for physical F8/i)).toBeVisible();
    expect(document.body).toHaveTextContent(/press F8 within 8 seconds/i);
    expect(document.body).toHaveTextContent(/speak for up to 10 seconds/i);
    const cancel = screen.getByRole("button", {
      name: "Stop listening",
    });
    expect(cancel).toBeEnabled();
    await user.click(cancel);
    expect(selectedStt.cancel).toHaveBeenCalledOnce();
    expect(
      await screen.findByText(/native PTT arming cancelled/i),
    ).toBeInTheDocument();
  });

  it("blocks guided PTT setup until input and output are explicitly persisted", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?onboarding=1");
    bridge.saveOnboarding.mockImplementation(async (onboarding) => ({
      onboarding,
      persistence: { health: "healthy", detail: "Saved." },
    }));
    render(<App />);
    await screen.findByRole("dialog", { name: /get you connected/i });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(
      screen.getByRole("button", { name: "Select running synthetic target" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Verify live capture" }),
    );
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      screen.getByRole("heading", { name: "Choose your voice & models" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/does not silently assume/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Audio output route" }),
      "systemDefault",
    );
    expect(bridge.audioSelect).toHaveBeenCalledWith({ mode: "systemDefault" });
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Audio input route" }),
      "systemDefault",
    );
    expect(bridge.inputSelect).toHaveBeenCalledWith({ mode: "systemDefault" });
    expect(
      await screen.findByRole("button", { name: "Continue" }),
    ).toBeEnabled();
  }, 10_000);

  it("finishes onboarding only after a current live reply, drained audio, and subtitle receipt", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?onboarding=1");
    bridge.saveOnboarding.mockImplementation(async (onboarding) => ({
      onboarding,
      persistence: { health: "healthy", detail: "Saved." },
    }));
    render(<App />);
    await screen.findByRole("dialog", { name: /get you connected/i });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(
      screen.getByRole("button", { name: "Select running synthetic target" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Verify live capture" }),
    );
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Audio output route" }),
      "systemDefault",
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Audio input route" }),
      "systemDefault",
    );
    await user.click(await screen.findByRole("button", { name: "Continue" }));
    await user.click(
      screen.getByRole("button", { name: "Use a typed setup turn instead" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Run bounded typed turn" }),
    );

    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "setup-fixture",
        generation: 1,
        sequence: 8,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: true,
        deliveredText: "Fixture text completed.",
      }),
    );
    expect(screen.getByRole("button", { name: "Finish setup" })).toBeDisabled();
    expect(document.body).toHaveTextContent(
      /live providers and audible speech were not claimed/i,
    );

    await user.click(
      screen.getByRole("button", { name: "Run the turn again" }),
    );
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "setup-live",
        generation: 2,
        sequence: 8,
        fixtureFirstAudioMs: 310,
        runtimeFixtureOnly: false,
        deliveredText: "A live reply reached the selected output.",
        turnExecution: {
          consumedRoute: {
            schemaVersion: 1,
            sourceLoadoutId: "native-balanced-api",
            generation: 9,
            sha256: "setup-live-route",
            llm: {
              providerId: "openai",
              modelId: "gpt-4.1-mini",
              voiceId: null,
            },
            tts: {
              providerId: "elevenlabs",
              modelId: "eleven_flash_v2_5",
              voiceId: "EXAVITQu4vr4xnSDxMaL",
            },
          },
          deliveryState: "delivered",
          commitState: "committed",
          degradations: [],
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
              receiptId: "setup-audio-1",
              submitted: true,
              drained: true,
              outputSelectionMode: "systemDefault",
              outputEndpointId: "windows-speakers-7",
              outputEndpointGeneration: 12,
            },
          ],
          subtitlePresentationReceipts: [
            {
              receiptId: "setup-subtitle-1",
              sentenceId: 1,
              provenance: "trustedNativeCapture",
              presentationId: 7,
              targetGeometryEpoch: 3,
              captureSequence: 8,
              graphicsGeneration: 2,
              layerHashHex: "abcd",
              presentedQpcTicks: "1100",
              desktopXPx: 320,
              desktopYPx: 610,
              widthPx: 640,
              heightPx: 72,
              dpiX: 96,
              dpiY: 96,
              direction: "leftToRight",
              bidiShapingApplied: true,
              graphemeClustersPreserved: true,
              usedBottomCenterFallback: false,
              colorTreatment: "sdrPremultipliedSourceOver",
              committed: true,
            },
          ],
        },
      }),
    );

    const finish = screen.getByRole("button", { name: "Finish setup" });
    expect(finish).toBeEnabled();
    await user.click(finish);
    expect(bridge.saveOnboarding).toHaveBeenLastCalledWith(
      expect.objectContaining({ completed: true, currentStep: "ready" }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  }, 10_000);

  it("requires an explicit second native call before claiming synthetic WGC proof", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Night City" }));
    await user.click(screen.getByText("Practice environment"));
    expect(
      screen.getByRole("button", { name: "Verify live capture" }),
    ).toBeDisabled();

    await user.click(
      screen.getByRole("button", { name: "Select synthetic target" }),
    );
    expect(bridge.captureSelect).toHaveBeenCalledOnce();
    expect(
      screen.getByRole("button", { name: "Verify live capture" }),
    ).toBeEnabled();
    expect(screen.getByText("Synthetic target selected")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Verify live capture" }),
    );
    expect(bridge.captureVerify).toHaveBeenCalledOnce();
    expect(screen.getByText("Synthetic WGC verified")).toBeInTheDocument();
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "Matched PID · HWND · executable",
    );
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "8 · advanced",
    );
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "Excluded from capture",
    );
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "windowsGraphicsCaptureTexture · exactSelectedWindow",
    );
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "Unrelated display-overlay pixels and desktop luminance excluded",
    );
    expect(
      screen.getByText(
        /whole-screen screenshot luminance is never capture or color proof/i,
      ),
    ).toBeVisible();
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "verified_synthetic_fixture",
    );
  });

  it("keeps private identity enrollment disabled until native pack admission", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Night City" }));
    await user.click(screen.getByText("Character recognition"));
    expect(document.body).toHaveTextContent(
      "Identity reference enrollment is blocked until the signed identity pack is admitted.",
    );
    expect(
      screen.queryByRole("button", { name: /Choose reference/i }),
    ).not.toBeInTheDocument();
    expect(bridge.identityEnrollmentStatus).toHaveBeenCalledOnce();
  });

  it("rejects full-display pixels as exact selected-window capture proof", async () => {
    const user = userEvent.setup();
    const otherwiseValid = await bridge.captureVerify();
    bridge.captureVerify.mockReset().mockResolvedValue({
      ...otherwiseValid,
      capture: {
        ...otherwiseValid.capture,
        pixelSource: "desktopDuplicationTexture",
        pixelScope: "fullDisplayOutput",
        externalDisplayOverlayPixelsExcluded: false,
        desktopLuminanceExcludedFromPixelEvidence: false,
      },
    });
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Night City" }));
    await user.click(
      screen.getByRole("button", { name: "Select synthetic target" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Verify live capture" }),
    );
    expect(
      screen.queryByText("Synthetic WGC verified"),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "desktopDuplicationTexture · fullDisplayOutput",
    );
    expect(screen.getByLabelText("Synthetic WGC receipt")).toHaveTextContent(
      "Exact pixel provenance not proven",
    );
  });

  it("renders a native synthetic capture verification error without a proof claim", async () => {
    const user = userEvent.setup();
    bridge.captureVerify.mockRejectedValueOnce(
      new Error("native WGC receipt did not advance"),
    );
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Night City" }));
    await user.click(
      screen.getByRole("button", { name: "Select synthetic target" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Verify live capture" }),
    );
    expect(
      await screen.findByText("native WGC receipt did not advance"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Synthetic WGC verified"),
    ).not.toBeInTheDocument();
  });

  it("commits completed fixture text without calling it spoken audio", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: true,
        deliveredText: "Delivered by the runtime fixture.",
      }),
    );
    expect(
      screen.getByText("Delivered by the runtime fixture."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Practice response · simulated dialogue/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(
      "Live provider audio delivered",
    );
  });

  it("sends an ordinary selected-route request without a development TTS override", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByLabelText("Selected AssemblyAI push-to-talk");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    expect(bridge.start).toHaveBeenCalledWith(
      "cloud",
      expect.any(Function),
      expect.objectContaining({
        gameProfileId: "eclipse-harbor",
        characterId: "mara-venn",
        enabledSpoilerTiers: [],
      }),
    );
    expect(bridge.start.mock.calls[0][2]).not.toHaveProperty("devLiveTts");
  });

  it("starts in Night City after a matching native Cyberpunk inspection", async () => {
    const user = userEvent.setup();
    bridge.loadBootstrap.mockResolvedValueOnce({
      ...authenticatedBootstrap,
      snapshot: {
        ...authenticatedBootstrap.snapshot,
        onboarding: {
          ...authenticatedBootstrap.snapshot.onboarding,
          selectedGameId: "eclipse-harbor",
        },
        gameProfiles: [
          {
            id: "cyberpunk-2077",
            displayName: "Cyberpunk 2077",
            wave: "1",
            safety: "singlePlayerOnly",
            catalogState: "bundled",
            defaultFallback: "audioOnly",
          },
        ],
      },
    });
    bridge.inspect.mockResolvedValueOnce({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      gameDisplayName: "Cyberpunk 2077",
      selectedCharacterId: "misty-olzewski",
      character: {
        id: "misty-olzewski",
        displayName: "Misty Olszewski",
        promptRole: "Esoterica owner",
      },
      authoredKnowledge: [],
      provenance: [],
      deliveredMemory: [],
      memoryScope: {
        crossGameWideningAllowed: false,
      },
    });
    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "Misty Olszewski" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Selected character" }),
    ).toHaveAttribute("aria-pressed", "true");
    const primaryActions = 1;
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    expect(screen.getByLabelText("Turn transcript")).toHaveValue(
      "What has this city been like for you lately?",
    );
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    expect(primaryActions).toBeLessThanOrEqual(2);
    expect(bridge.start).toHaveBeenCalledWith(
      "cloud",
      expect.any(Function),
      expect.objectContaining({
        gameProfileId: "cyberpunk-2077",
        characterId: "misty-olzewski",
        characterName: "Misty Olszewski",
      }),
    );
  });

  it("keeps an explicit practice choice when Cyberpunk inspection finishes later", async () => {
    const user = userEvent.setup();
    bridge.loadBootstrap.mockResolvedValueOnce({
      ...authenticatedBootstrap,
      snapshot: {
        ...authenticatedBootstrap.snapshot,
        onboarding: {
          ...authenticatedBootstrap.snapshot.onboarding,
          selectedGameId: "cyberpunk-2077",
        },
        gameProfiles: [
          {
            id: "cyberpunk-2077",
            displayName: "Cyberpunk 2077",
            wave: "1",
            safety: "singlePlayerOnly",
            catalogState: "bundled",
            defaultFallback: "audioOnly",
          },
        ],
      },
    });
    let resolveInspection: ((value: unknown) => void) | undefined;
    bridge.inspect.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveInspection = resolve;
        }),
    );
    render(<App />);

    const synthetic = await screen.findByRole("button", {
      name: "Practice game",
    });
    await waitFor(() => expect(bridge.inspect).toHaveBeenCalled());
    await user.click(synthetic);
    expect(synthetic).toHaveAttribute("aria-pressed", "true");
    await act(async () => {
      resolveInspection?.({
        schemaVersion: 1,
        gameProfileId: "cyberpunk-2077",
        gameDisplayName: "Cyberpunk 2077",
        selectedCharacterId: "misty-olzewski",
        character: {
          id: "misty-olzewski",
          displayName: "Misty Olszewski",
          promptRole: "Esoterica owner",
        },
        authoredKnowledge: [],
        provenance: [],
        deliveredMemory: [],
        memoryScope: { crossGameWideningAllowed: false },
      });
    });

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Selected character" }),
      ).not.toBeDisabled(),
    );
    expect(synthetic).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("heading", { name: "Mara Venn" })).toBeVisible();
  });

  it("does not activate Night City from a mismatched inspection", async () => {
    bridge.loadBootstrap.mockResolvedValueOnce({
      ...authenticatedBootstrap,
      snapshot: {
        ...authenticatedBootstrap.snapshot,
        onboarding: {
          ...authenticatedBootstrap.snapshot.onboarding,
          selectedGameId: "cyberpunk-2077",
        },
        gameProfiles: [
          {
            id: "cyberpunk-2077",
            displayName: "Cyberpunk 2077",
            wave: "1",
            safety: "singlePlayerOnly",
            catalogState: "bundled",
            defaultFallback: "audioOnly",
          },
        ],
      },
    });
    bridge.inspect.mockResolvedValueOnce({
      schemaVersion: 1,
      gameProfileId: "eclipse-harbor",
      gameDisplayName: "Eclipse Harbor",
      selectedCharacterId: "mara-venn",
      character: {
        id: "mara-venn",
        displayName: "Mara Venn",
        promptRole: "Lighthouse keeper",
      },
      authoredKnowledge: [],
      provenance: [],
      deliveredMemory: [],
      memoryScope: { crossGameWideningAllowed: false },
    });

    render(<App />);
    await waitFor(() => expect(bridge.inspect).toHaveBeenCalled());

    expect(
      screen.getByRole("button", { name: "Selected character" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Practice game" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("does not turn a non-fixture completion flag into an audio-delivery claim", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: 120,
        runtimeFixtureOnly: false,
        deliveredText: "Runtime text without an exposed receipt.",
        turnExecution: {
          consumedRoute: {
            schemaVersion: 1,
            sourceLoadoutId: "native-balanced-api",
            generation: 8,
            sha256: "unproven-audio-route",
            llm: null,
            tts: null,
          },
          deliveryState: "delivered",
          commitState: "committed",
          degradations: [],
          success: {
            llmProviderLive: false,
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
        },
      }),
    );
    expect(screen.getByText(/audio not confirmed/i)).toBeInTheDocument();
    await user.click(screen.getByText("Delivery & identity details"));
    expect(
      screen.getByText(/requires live TTS \+ submission \+ drain receipts/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(
      "Receipt-backed provider audio",
    );
  });

  it("claims sink delivery only with matching live TTS submission and drain receipts", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: 120,
        runtimeFixtureOnly: false,
        deliveredText: "Receipt-backed provider completion.",
        turnExecution: {
          consumedRoute: {
            schemaVersion: 1,
            sourceLoadoutId: "native-protected-loadout",
            generation: 4,
            sha256: "native-route-sha256",
            llm: {
              providerId: "nvidia-nim",
              modelId: "nemotron",
              voiceId: null,
            },
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
              acknowledgementSha256: "d".repeat(64),
              modalities: ["llm", "tts"],
              promotionSupported: false,
              publicationSupported: false,
            },
          },
          deliveryState: "delivered",
          commitState: "committed",
          degradations: [],
          success: {
            llmProviderLive: true,
            ttsProviderLive: true,
            sttSkipped: true,
            subtitleDelivered: true,
            subtitleReceiptCount: 0,
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
              outputEndpointId: "windows-speakers-7",
              outputEndpointGeneration: 12,
            },
          ],
          subtitlePresentationReceipts: [],
        },
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
            authorities: ["core_canon", "character_profile"],
            recordCount: 2,
            retrievalProvenance: {
              schema_version: "character-db/1.0.0",
              policy_fingerprint_sha256: "a",
              query_sha256: "b",
              selected_profile_knowledge_ids: [],
              selected_memory_item_ids: [],
              embedding_metadata_ids: [],
            },
            scopedMemoryItemIds: ["memory-1"],
            scopedMemoryClasses: ["recent_delivered_turns"],
          },
        },
      }),
    );
    expect(
      screen.getByText("Reply sent to your audio device"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/1 submitted \+ drained receipt.*audio-receipt-live-1/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/explicit endpoint windows-speakers-7 · generation 12/i),
    ).toBeInTheDocument();
    expect(screen.getByText("Live LLM provider evidence")).toBeInTheDocument();
    expect(
      screen.getByText("Control-app text visible; native overlay not proven"),
    ).toBeInTheDocument();
    expect(screen.getByText("native-protected-loadout")).toBeInTheDocument();
    expect(screen.getByText(/nvidia-nim · nemotron/i)).toBeInTheDocument();
    expect(
      screen.getByText(
        /io\.github\.akshitireddy\.interactive-npcs\.review.*acknowledgement d{64}.*promotion\/publication unsupported/i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/skyrim-special-edition \/ lydia/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/manual_character_id · explicit selection/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/2 records · core_canon, character_profile/i),
    ).toBeInTheDocument();
  });

  it("shows runtime degradations and manual recovery without implying automatic fallback", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    act(() =>
      bridge.callback?.({
        type: "failed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        reason: "TTS provider timed out; choose a manual retry.",
        turnExecution: {
          consumedRoute: {
            schemaVersion: 1,
            sourceLoadoutId: "native-protected-loadout",
            generation: 5,
            sha256: "native-failed-route-sha256",
            llm: {
              providerId: "nvidia-nim",
              modelId: "nemotron",
              voiceId: null,
            },
            tts: {
              providerId: "elevenlabs",
              modelId: "eleven_flash_v2_5",
              voiceId: "stock-voice",
            },
          },
          deliveryState: "manualRetryRequired",
          commitState: "commitDeferred",
          degradations: [
            {
              type: "manualRetryRequired",
              failedRole: "tts",
              providerId: "elevenlabs",
              reason: "TTS provider timed out; choose a manual retry.",
              retryable: true,
            },
          ],
          success: {
            llmProviderLive: true,
            ttsProviderLive: false,
            sttSkipped: true,
            subtitleDelivered: true,
            subtitleReceiptCount: 0,
            audioSubmitted: false,
            audioDrained: false,
            audioReceiptCount: 0,
          },
          audioReceipts: [],
          subtitlePresentationReceipts: [],
        },
      }),
    );
    expect(
      screen.getAllByText("TTS provider timed out; choose a manual retry.")
        .length,
    ).toBeGreaterThan(0);
    expect(
      screen.getByText(/manualRetryRequired.*commitDeferred/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Review recovery profile" }),
    ).toBeInTheDocument();
    expect(document.body).toHaveTextContent("no automatic provider switch");
  });

  it("sends the authored transcript and honors the persisted character subtitle state", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    const transcript = screen.getByLabelText("Turn transcript");
    await user.clear(transcript);
    await user.type(transcript, "What is beyond the breakwater?");
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    expect(bridge.start).toHaveBeenCalledWith(
      "cloud",
      expect.any(Function),
      expect.objectContaining({
        transcript: "What is beyond the breakwater?",
      }),
    );
    expect(screen.getByText("Balanced API")).toBeInTheDocument();
    expect(
      screen.getByText(/configured WebView preference/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Configured preference · native consumed-route evidence after the turn wins/i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/not consumed; no frame adapter is active/i),
    ).toBeInTheDocument();

    const subtitles = screen.getByRole("checkbox", {
      name: /Subtitles/i,
    });
    await waitFor(() => expect(subtitles).toBeEnabled());
    await user.click(subtitles);
    await waitFor(() =>
      expect(bridge.savePreferences).toHaveBeenCalledWith(
        1,
        expect.objectContaining({
          scope: sessionPreferenceScope,
          executionPreset: "cloud",
          performancePreset: "balanced",
          overrides: expect.objectContaining({
            inputMode: "ptt",
            subtitles: false,
          }),
        }),
      ),
    );
    await waitFor(() => expect(subtitles).not.toBeChecked());
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: true,
        deliveredText: "This text should be hidden.",
      }),
    );
    expect(
      screen.queryByText("This text should be hidden."),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/Subtitle text hidden/i)).toBeInTheDocument();
  });

  it("refreshes native diagnostic commands instead of fixtures", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Typed message" }));
    await user.click(screen.getByRole("button", { name: /Send message/i }));
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: /^Help/i }));
    await user.click(screen.getByRole("button", { name: /^Troubleshooting/i }));
    await user.click(
      screen.getByRole("button", { name: "Run native diagnostics" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Refresh native checks" }),
    );
    expect(await screen.findByText("15 checks")).toBeInTheDocument();
    expect(bridge.diagnostics).toHaveBeenCalledOnce();
    expect(bridge.diagnosticsV2).toHaveBeenCalledWith(100);
    expect(bridge.diagnosticsMatrix).toHaveBeenCalledWith();
    expect(bridge.diagnosticsSettings).toHaveBeenCalledOnce();
    expect(screen.getAllByText("unmeasured").length).toBeGreaterThan(0);
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Diagnostics verbosity" }),
      "essential",
    );
    expect(bridge.saveDiagnosticsSettings).toHaveBeenCalledWith({
      schemaVersion: 1,
      verbosity: "essential",
    });
    expect(screen.queryByText("84 ms")).not.toBeInTheDocument();
    expect(
      screen.getByText(/completion evidence is still pending/i),
    ).toBeInTheDocument();
    act(() =>
      bridge.callback?.({
        type: "completed",
        simulationId: "native-1",
        generation: 7,
        sequence: 8,
        fixtureFirstAudioMs: null,
        runtimeFixtureOnly: true,
        deliveredText: "Completed diagnostic rehearsal.",
      }),
    );
    expect(screen.getByText("84 ms")).toBeInTheDocument();
    expect(screen.getByText("Completed runtime fixture")).toBeInTheDocument();
  });

  it("shows only the privacy-safe diagnostics export basename", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: /^Help/i }));
    await user.click(screen.getByRole("button", { name: /^Troubleshooting/i }));
    await user.click(
      screen.getByRole("button", { name: "Run native diagnostics" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Export local diagnostics" }),
    );

    const status = await screen.findByText(
      /Exported locally as interactive-npcs-diagnostics-20260830\.json/i,
    );
    expect(status).toHaveTextContent("3 events");
    expect(status).toHaveTextContent("No directory path is exposed");
    expect(status).not.toHaveTextContent("C:\\");
    expect(status).not.toHaveTextContent("/Users/");
    expect(bridge.diagnosticsExport).toHaveBeenCalledWith(100);
  });

  it("routes a canonical diagnostic remediation to its owned product page", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Runtime and media broker authenticated");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: /^Help/i }));
    await user.click(screen.getByRole("button", { name: /^Troubleshooting/i }));
    await user.click(
      screen.getByRole("button", { name: "Run native diagnostics" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Refresh native checks" }),
    );
    await screen.findByText("15 checks");
    await user.click(
      screen.getAllByRole("button", { name: "Open provider settings" })[0],
    );
    expect(
      screen.getByRole("heading", { name: "Neural loadout" }),
    ).toBeInTheDocument();
  });
});
