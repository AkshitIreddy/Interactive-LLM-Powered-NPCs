import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import type { AppPreferences } from "./types";
import {
  promptAndSaveProviderCredential,
  testProviderCredential,
} from "./providerCredentials";
import {
  cancelNativeSimulation,
  enumerateAudioInputs,
  enumerateAudioOutputs,
  exportDiagnosticsV2,
  inspectCharacterDatabase,
  loadNativeBootstrapHealth,
  LOADING_NATIVE_BOOTSTRAP,
  readDiagnosticSummary,
  readDiagnosticsV2,
  readDiagnosticsMatrix,
  readDiagnosticsSettings,
  readIdentityReferenceEnrollmentStatus,
  readProductPreferences,
  readSelectedAudioInput,
  readSelectedAudioOutput,
  runSyntheticReplayCapture,
  saveOnboarding,
  saveDiagnosticsSettings,
  selectAudioInput,
  selectAudioOutput,
  startNativeSimulation,
  syntheticReplayCaptureAvailability,
  verifySelectedGameCapture,
  type NativeBootstrapHealth,
  type NativeAudioInputSelection,
  type NativeAudioInputSnapshot,
  type NativeAudioOutputSelection,
  type NativeAudioOutputSnapshot,
  type NativeSelectedAudioInput,
  type NativeSelectedAudioOutput,
  type NativeDiagnosticSummary,
  type NativeDiagnosticsExportResult,
  type NativeDiagnosticsMatrix,
  type NativeDiagnosticsSettings,
  type NativeDiagnosticsV2Snapshot,
  type NativeCharacterInspection,
  type NativeCaptureEvidence,
  type NativeGameTargetSelection,
  type NativeGameProfileSummary,
  type NativeIdentityReferenceEnrollmentStatus,
  type NativeModelSummary,
  type NativeOnboardingStep,
  type NativeProviderCredentialSummary,
  type NativeProductPreferenceSnapshot,
  type NativeSimulationEvent,
  type NativeTurnExecutionEvidence,
  type OnboardingSnapshot,
} from "./tauriBridge";
import { ProviderLoadoutEditor } from "./ProviderLoadoutEditor";
import {
  activeBrowserLoadoutFor,
  modelFor,
  providerFor,
  ROLE_META,
  ROLE_ORDER,
  type ProviderLoadout,
} from "./providerLoadouts";
import {
  CharacterDatabase,
  EncounterLifecycleControls,
  GameTargetWorkspace,
  LocalResourcePlanner,
} from "./ProductWorkspaces";
import { ProductPreferencesWorkspace } from "./ProductPreferencesWorkspace";
import {
  cancelSelectedSttPushToTalk,
  readSelectedSttPushToTalkStatus,
  startSelectedSttPushToTalk,
  type SelectedSttCapturing,
  type SelectedSttStatus,
  type SelectedSttTerminal,
} from "./selectedSttBridge";
import { SUPPORT_GUIDES, type SupportGuideEntry } from "./supportGuides";

type ProductPage = "session" | "world" | "voice" | "diagnostics" | "settings";

type SyntheticCaptureProof = {
  pid: number;
  hwnd: number;
  executable: string;
  frames: number;
  receipt: {
    evidence: NativeCaptureEvidence;
    exactTargetMatch: boolean;
    frameSequenceAdvanced: boolean;
    contentChanged: boolean;
    safetyState: string;
    verified: boolean;
  } | null;
};

type SelectedSttScope = {
  gameProfileId: string;
  characterId: string;
  characterLabel: string;
};

type SelectedSttReadiness = {
  ready: boolean;
  reason: string;
  configuredProviderId: string;
  configuredModelId: string;
  credentialPresent: boolean;
  selectedInput: NativeSelectedAudioInput | null;
};

const PAGE_ALIASES: Record<string, ProductPage> = {
  home: "session",
  conversation: "session",
  games: "world",
  characters: "world",
  presence: "world",
  models: "voice",
  performance: "diagnostics",
  help: "settings",
};

const DEFAULT_PREFERENCES: AppPreferences = {
  execution: "cloud",
  performance: "balanced",
  subtitles: true,
  ptt: true,
  localOnly: false,
  screenPresence: false,
  diagnostics: true,
};

const NAV: Array<{ id: ProductPage; label: string; index: string }> = [
  { id: "session", label: "Session deck", index: "01" },
  { id: "world", label: "World", index: "02" },
  { id: "voice", label: "Voice & models", index: "03" },
  { id: "diagnostics", label: "Diagnostics", index: "04" },
  { id: "settings", label: "Settings & guide", index: "05" },
];

const ONBOARDING_STEPS: Array<{ id: NativeOnboardingStep; label: string }> = [
  { id: "scan", label: "System" },
  { id: "game", label: "World" },
  { id: "providers", label: "Voice" },
  { id: "simulation", label: "Test" },
];

const nowSnapshot = (
  completed: boolean,
  currentStep: NativeOnboardingStep,
  preferences: AppPreferences,
): OnboardingSnapshot => ({
  schemaVersion: 1,
  completed,
  currentStep,
  selectedGameId: "eclipse-harbor",
  preferences,
  updatedAtEpochMs: Date.now(),
});

function initialPage(): ProductPage {
  const requested = new URLSearchParams(window.location.search).get("page");
  if (!requested) return "session";
  if (NAV.some((item) => item.id === requested))
    return requested as ProductPage;
  return PAGE_ALIASES[requested] ?? "session";
}

function runtimeSummary(health: NativeBootstrapHealth) {
  if (health.kind === "loading")
    return { tone: "wait", label: "Starting native services" };
  if (health.kind === "browserPreview")
    return {
      tone: "muted",
      label: "Browser preview — native actions unavailable",
    };
  if (health.kind === "unavailable")
    return { tone: "bad", label: "Native control unavailable" };
  const connected =
    health.snapshot.runtime.connected && health.snapshot.mediaBroker.connected;
  return connected
    ? { tone: "good", label: "Runtime and media broker authenticated" }
    : { tone: "wait", label: "Native services are still connecting" };
}

function hasReceiptBackedAudio(
  completed: (NativeSimulationEvent & { type: "completed" }) | null,
) {
  const evidence = completed?.turnExecution;
  if (!evidence) return false;
  const { success, audioReceipts } = evidence;
  return (
    success.ttsProviderLive &&
    success.audioSubmitted &&
    success.audioDrained &&
    success.audioReceiptCount > 0 &&
    audioReceipts.length === success.audioReceiptCount &&
    audioReceipts.every((receipt) => receipt.submitted && receipt.drained)
  );
}

function hasReceiptBackedSubtitle(
  completed: (NativeSimulationEvent & { type: "completed" }) | null,
) {
  const evidence = completed?.turnExecution;
  if (!evidence) return false;
  const { success, subtitlePresentationReceipts } = evidence;
  return (
    success.subtitleDelivered &&
    success.subtitleReceiptCount > 0 &&
    subtitlePresentationReceipts.length === success.subtitleReceiptCount &&
    subtitlePresentationReceipts.every(
      (receipt) =>
        receipt.committed &&
        receipt.widthPx > 0 &&
        receipt.heightPx > 0 &&
        receipt.presentedQpcTicks !== "0",
    )
  );
}

function subtitleReceiptSurfaceLabel(
  receipts: NativeTurnExecutionEvidence["subtitlePresentationReceipts"],
) {
  const surfaces = new Set(
    receipts.map((receipt) => {
      switch (receipt.provenance) {
        case "trustedNativeCapture":
          return "trusted in-game native surface";
        case "consoleBottomCenterUnavailable":
          return "control-console bottom-center fallback";
        case "deterministicFixture":
          return "deterministic fixture surface";
      }
    }),
  );
  return [...surfaces].join(" + ");
}

function audioReceiptEndpointLabel(
  receipts: NativeTurnExecutionEvidence["audioReceipts"],
) {
  const proven = receipts.filter(
    (receipt) =>
      receipt.outputSelectionMode &&
      receipt.outputEndpointId &&
      receipt.outputEndpointGeneration !== undefined,
  );
  if (proven.length !== receipts.length || proven.length === 0)
    return "endpoint not reported";
  return [
    ...new Set(
      proven.map(
        (receipt) =>
          `${receipt.outputSelectionMode === "systemDefault" ? "system default" : "explicit endpoint"} ${receipt.outputEndpointId} · generation ${receipt.outputEndpointGeneration}`,
      ),
    ),
  ].join(" + ");
}

export function ProductConsole() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const [page, setPage] = useState<ProductPage>(initialPage);
  const [bootstrap, setBootstrap] = useState<NativeBootstrapHealth>(
    LOADING_NATIVE_BOOTSTRAP,
  );
  const [preferences, setPreferences] =
    useState<AppPreferences>(DEFAULT_PREFERENCES);
  const [setupOpen, setSetupOpen] = useState(query.get("onboarding") === "1");
  const [setupStep, setSetupStep] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [turnEvents, setTurnEvents] = useState<NativeSimulationEvent[]>([]);
  const [deliveredTurn, setDeliveredTurn] = useState<
    (NativeSimulationEvent & { type: "completed" }) | null
  >(null);
  const [selectedGameProfileId, setSelectedGameProfileId] =
    useState("eclipse-harbor");
  const [selectedCharacter, setSelectedCharacter] =
    useState<NativeCharacterInspection | null>(null);
  const [selectedTarget, setSelectedTarget] =
    useState<NativeGameTargetSelection | null>(null);
  const [turnWorldMode, setTurnWorldMode] = useState<
    "syntheticReview" | "selectedWorld"
  >("syntheticReview");
  const resumeWorldOnInspection = useRef(false);
  const [turnInputMode, setTurnInputMode] = useState<"typed" | "ptt">(
    DEFAULT_PREFERENCES.ptt ? "ptt" : "typed",
  );
  const [turnPrompt, setTurnPrompt] = useState(
    "Did you ever make it to the old lighthouse?",
  );
  const [turnError, setTurnError] = useState<string | null>(null);
  const [turnRoute, setTurnRoute] = useState<{
    loadout: ProviderLoadout;
    capturedAtEpochMs: number;
  } | null>(null);
  const [diagnostics, setDiagnostics] =
    useState<NativeDiagnosticSummary | null>(null);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [diagnosticsV2, setDiagnosticsV2] =
    useState<NativeDiagnosticsV2Snapshot | null>(null);
  const [diagnosticsExport, setDiagnosticsExport] =
    useState<NativeDiagnosticsExportResult | null>(null);
  const [diagnosticsMatrix, setDiagnosticsMatrix] =
    useState<NativeDiagnosticsMatrix | null>(null);
  const [diagnosticsSettings, setDiagnosticsSettings] =
    useState<NativeDiagnosticsSettings | null>(null);
  const [providerBusy, setProviderBusy] = useState<string | null>(null);
  const [captureProof, setCaptureProof] =
    useState<SyntheticCaptureProof | null>(null);
  const [audioOutputs, setAudioOutputs] =
    useState<NativeAudioOutputSnapshot | null>(null);
  const [selectedAudioOutput, setSelectedAudioOutput] =
    useState<NativeSelectedAudioOutput | null>(null);
  const [audioOutputBusy, setAudioOutputBusy] = useState(false);
  const [audioOutputError, setAudioOutputError] = useState<string | null>(null);
  const [audioInputs, setAudioInputs] =
    useState<NativeAudioInputSnapshot | null>(null);
  const [selectedAudioInput, setSelectedAudioInput] =
    useState<NativeSelectedAudioInput | null>(null);
  const [audioInputBusy, setAudioInputBusy] = useState(false);
  const [audioInputError, setAudioInputError] = useState<string | null>(null);
  const [identityEnrollmentStatus, setIdentityEnrollmentStatus] =
    useState<NativeIdentityReferenceEnrollmentStatus | null>(null);
  const [identityEnrollmentError, setIdentityEnrollmentError] = useState<
    string | null
  >(null);
  const [selectedSttCapture, setSelectedSttCapture] =
    useState<SelectedSttCapturing | null>(null);
  const [selectedSttStatus, setSelectedSttStatus] =
    useState<SelectedSttStatus | null>(null);
  const [selectedSttTerminal, setSelectedSttTerminal] =
    useState<SelectedSttTerminal | null>(null);
  const [selectedSttReceiptDisposition, setSelectedSttReceiptDisposition] =
    useState<"ready" | "submitted" | "rejected" | null>(null);
  const [selectedSttScope, setSelectedSttScope] =
    useState<SelectedSttScope | null>(null);
  const [selectedSttBusy, setSelectedSttBusy] = useState(false);
  const [selectedSttError, setSelectedSttError] = useState<string | null>(null);

  const applyNativeProductPreferences = useCallback(
    (productPreferences: NativeProductPreferenceSnapshot) => {
      const effective = productPreferences.effective;
      setPreferences((current) => ({
        ...current,
        execution:
          effective.executionPreset.value === "fullyLocal"
            ? "local"
            : effective.executionPreset.value,
        performance: effective.performancePreset.value,
        subtitles: effective.subtitles.value,
        ptt: effective.inputMode.value === "ptt",
        localOnly: effective.executionPreset.value === "fullyLocal",
        screenPresence: effective.vision.value,
      }));
      setTurnInputMode(effective.inputMode.value === "ptt" ? "ptt" : "typed");
    },
    [],
  );

  const refreshAudioOutputs = useCallback(async () => {
    setAudioOutputBusy(true);
    setAudioOutputError(null);
    try {
      const [catalog, selected] = await Promise.all([
        enumerateAudioOutputs(),
        readSelectedAudioOutput(),
      ]);
      setAudioOutputs(catalog);
      setSelectedAudioOutput(selected);
    } catch (error) {
      setAudioOutputError(
        error instanceof Error
          ? error.message
          : "Native audio outputs could not be read.",
      );
    } finally {
      setAudioOutputBusy(false);
    }
  }, []);

  const chooseAudioOutput = useCallback(
    async (selection: NativeAudioOutputSelection) => {
      setAudioOutputBusy(true);
      setAudioOutputError(null);
      try {
        const selected = await selectAudioOutput(selection);
        if (!selected) {
          setAudioOutputError(
            "Open the native desktop app to persist an audio output.",
          );
          return;
        }
        setSelectedAudioOutput(selected);
        setNotice(
          `Audio output saved: ${selected.resolved.friendlyName}. Future receipts must identify the drained endpoint.`,
        );
        const catalog = await enumerateAudioOutputs();
        if (catalog) setAudioOutputs(catalog);
      } catch (error) {
        setAudioOutputError(
          error instanceof Error
            ? error.message
            : "The selected audio output could not be persisted.",
        );
      } finally {
        setAudioOutputBusy(false);
      }
    },
    [],
  );

  const refreshAudioInputs = useCallback(async () => {
    setAudioInputBusy(true);
    setAudioInputError(null);
    try {
      const catalog = await enumerateAudioInputs();
      const selected = await readSelectedAudioInput();
      setAudioInputs(catalog);
      setSelectedAudioInput(selected);
    } catch (error) {
      setAudioInputError(
        error instanceof Error
          ? error.message
          : "Native audio inputs could not be read.",
      );
    } finally {
      setAudioInputBusy(false);
    }
  }, []);

  const chooseAudioInput = useCallback(
    async (selection: NativeAudioInputSelection) => {
      setAudioInputBusy(true);
      setAudioInputError(null);
      try {
        const selected = await selectAudioInput(selection);
        if (!selected) {
          setAudioInputError(
            "Open the native desktop app to persist an audio input.",
          );
          return;
        }
        setSelectedAudioInput(selected);
        setNotice(
          `Audio input saved: ${selected.resolved.friendlyName}. Endpoint routing is configured; microphone frames, signal quality, and a transcript are not yet proven.`,
        );
        const catalog = await enumerateAudioInputs();
        if (catalog) setAudioInputs(catalog);
      } catch (error) {
        setAudioInputError(
          error instanceof Error
            ? error.message
            : "The selected audio input could not be persisted.",
        );
      } finally {
        setAudioInputBusy(false);
      }
    },
    [],
  );

  useEffect(() => {
    const controller = new AbortController();
    void loadNativeBootstrapHealth({ signal: controller.signal })
      .then((health) => {
        setBootstrap(health);
        if (health.kind !== "snapshot") return;
        const onboarding = health.snapshot.onboarding;
        const requestedGameId = onboarding?.selectedGameId;
        const nativeGameId = health.snapshot.gameProfiles?.some(
          (profile) => profile.id === requestedGameId,
        )
          ? requestedGameId
          : health.snapshot.gameProfiles?.[0]?.id;
        if (nativeGameId) setSelectedGameProfileId(nativeGameId);
        resumeWorldOnInspection.current = Boolean(
          requestedGameId &&
            nativeGameId === requestedGameId &&
            requestedGameId !== "eclipse-harbor",
        );
        if (onboarding) {
          setPreferences(onboarding.preferences);
          setTurnInputMode(onboarding.preferences.ptt ? "ptt" : "typed");
          if (!onboarding.completed) {
            const knownStep = ONBOARDING_STEPS.findIndex(
              (item) => item.id === onboarding.currentStep,
            );
            setSetupStep(Math.max(0, knownStep));
            setSetupOpen(true);
          }
        }
      })
      .catch((error: unknown) => {
        if (!(error instanceof Error && error.name === "AbortError")) {
          setNotice("Native bootstrap did not complete.");
        }
      });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    if (
      bootstrap.kind !== "snapshot" ||
      !bootstrap.snapshot.mediaBroker.connected
    )
      return;
    void refreshAudioOutputs();
    void refreshAudioInputs();
  }, [bootstrap, refreshAudioInputs, refreshAudioOutputs]);

  useEffect(() => {
    if (bootstrap.kind !== "snapshot") return;
    let active = true;
    void readIdentityReferenceEnrollmentStatus()
      .then((status) => {
        if (!active) return;
        setIdentityEnrollmentStatus(status);
        setIdentityEnrollmentError(null);
      })
      .catch((error: unknown) => {
        if (!active) return;
        setIdentityEnrollmentStatus(null);
        setIdentityEnrollmentError(
          error instanceof Error
            ? error.message
            : "Identity reference enrollment status could not be read.",
        );
      });
    return () => {
      active = false;
    };
  }, [bootstrap.kind]);

  useEffect(() => {
    if (bootstrap.kind !== "snapshot") return;
    let active = true;
    void readSelectedSttPushToTalkStatus()
      .then((status) => {
        if (active) setSelectedSttStatus(status);
      })
      .catch((error: unknown) => {
        if (active)
          setSelectedSttError(
            error instanceof Error
              ? error.message
              : "Selected STT status could not be read.",
          );
      });
    return () => {
      active = false;
    };
  }, [bootstrap.kind]);

  useEffect(() => {
    if (bootstrap.kind !== "snapshot") return;
    void readProductPreferences({ kind: "global" })
      .then((next) => {
        if (next) applyNativeProductPreferences(next);
      })
      .catch((error: unknown) =>
        setNotice(
          error instanceof Error
            ? error.message
            : "Native product preferences could not be loaded.",
        ),
      );
  }, [applyNativeProductPreferences, bootstrap.kind]);

  useEffect(() => {
    if (bootstrap.kind !== "snapshot") return;
    if (
      !bootstrap.snapshot.gameProfiles?.some(
        (profile) => profile.id === selectedGameProfileId,
      )
    ) {
      setSelectedCharacter(null);
      return;
    }
    let active = true;
    void inspectCharacterDatabase(selectedGameProfileId)
      .then((inspection) => {
        if (active && inspection) {
          setSelectedCharacter(inspection);
          if (resumeWorldOnInspection.current) {
            setTurnWorldMode("selectedWorld");
            resumeWorldOnInspection.current = false;
          }
        }
      })
      .catch((error: unknown) => {
        if (active)
          setNotice(
            error instanceof Error
              ? error.message
              : "Selected character could not be loaded.",
          );
      });
    return () => {
      active = false;
    };
  }, [bootstrap.kind, selectedGameProfileId]);

  const snapshot = bootstrap.kind === "snapshot" ? bootstrap.snapshot : null;
  const nativeWorldReady = Boolean(
    snapshot?.gameProfiles?.some(
      (profile) => profile.id === selectedGameProfileId,
    ),
  );
  const provider = snapshot?.providers?.find(
    (item) => item.providerId === "elevenlabs",
  );
  const providerPresent = provider?.status === "present";
  const nvidiaProvider = snapshot?.providers?.find(
    (item) => item.providerId === "nvidia-nim",
  );
  const assemblyAiProvider = snapshot?.providers?.find(
    (item) => item.providerId === "assemblyai",
  );
  const currentSttScope: SelectedSttScope =
    turnWorldMode === "selectedWorld" && selectedCharacter
      ? {
          gameProfileId: selectedCharacter.gameProfileId,
          characterId: selectedCharacter.character.id,
          characterLabel: `${selectedCharacter.gameDisplayName} · ${selectedCharacter.character.displayName}`,
        }
      : {
          gameProfileId: "eclipse-harbor",
          characterId: "mara-venn",
          characterLabel: "Eclipse Harbor · Mara Venn",
        };
  const configuredSttRoute = activeBrowserLoadoutFor(
    currentSttScope.gameProfileId,
    currentSttScope.characterId,
  ).routes.stt;
  const selectedSttReadiness: SelectedSttReadiness = {
    ready: Boolean(
      bootstrap.kind === "snapshot" &&
        selectedAudioInput &&
        assemblyAiProvider?.status === "present" &&
        configuredSttRoute.providerId === "assemblyai" &&
        configuredSttRoute.modelId === "u3-rt-pro",
    ),
    reason:
      bootstrap.kind !== "snapshot"
        ? "Open the installed native app. Browser preview cannot allocate microphone capture."
        : !selectedAudioInput
          ? "Persist an active native microphone endpoint first."
          : assemblyAiProvider?.status !== "present"
            ? `AssemblyAI native credential is unavailable: ${assemblyAiProvider?.detail ?? "no vault status"}`
            : configuredSttRoute.providerId !== "assemblyai" ||
                configuredSttRoute.modelId !== "u3-rt-pro"
              ? `Selected preference is ${configuredSttRoute.providerId} · ${configuredSttRoute.modelId}; explicitly select AssemblyAI · u3-rt-pro. Native state is re-resolved again at capture start.`
              : "Ready to request native capture. A physical F8 press and release remains broker-authoritative.",
    configuredProviderId: configuredSttRoute.providerId,
    configuredModelId: configuredSttRoute.modelId,
    credentialPresent: assemblyAiProvider?.status === "present",
    selectedInput: selectedAudioInput,
  };
  const nvidiaPresent = nvidiaProvider?.status === "present";
  const captureDebugAvailable = syntheticReplayCaptureAvailability(
    Boolean(snapshot?.capabilities?.debugSyntheticReplayCapture),
  );
  const activeEvent = turnEvents.at(-1);
  const activeStage =
    activeEvent?.type === "stageStarted" ||
    activeEvent?.type === "stageCompleted"
      ? activeEvent.stage
      : activeEvent?.type === "sentenceReady"
        ? "voicing"
        : activeEvent?.type === "completed"
          ? "animating"
          : null;

  const navigate = (next: ProductPage) => {
    setPage(next);
    const url = new URL(window.location.href);
    url.searchParams.set("page", next);
    url.searchParams.delete("state");
    window.history.replaceState({}, "", url);
  };

  const persistSetup = useCallback(
    async (
      completed: boolean,
      step: NativeOnboardingStep,
      selectedPreferences = preferences,
    ) => {
      const result = await saveOnboarding(
        nowSnapshot(completed, step, selectedPreferences),
      );
      if (!result) return false;
      setPreferences(result.onboarding.preferences);
      return true;
    },
    [preferences],
  );

  const nextSetupStep = async () => {
    if (setupStep < ONBOARDING_STEPS.length - 1) {
      const next = setupStep + 1;
      const selectedPreferences =
        setupStep === 2
          ? { ...preferences, execution: "cloud" as const, localOnly: false }
          : preferences;
      const saved = await persistSetup(
        false,
        ONBOARDING_STEPS[next].id,
        selectedPreferences,
      );
      if (!saved) {
        setNotice(
          "Open the native desktop app to save and continue guided setup.",
        );
        return;
      }
      setSetupStep(next);
      return;
    }
    const saved = await persistSetup(true, "ready");
    if (!saved) {
      setNotice("Open the native desktop app to finish and save setup.");
      return;
    }
    setSetupOpen(false);
    setNotice("Setup saved. Session deck is ready.");
  };

  const runTurn = async () => {
    const selectedSttReceipt =
      turnInputMode === "ptt" &&
      selectedSttTerminal?.status === "transcriptReady" &&
      selectedSttReceiptDisposition === "ready" &&
      selectedSttTerminal.receiptId
        ? {
            receiptId: selectedSttTerminal.receiptId,
            generation: selectedSttTerminal.generation,
          }
        : null;
    const normalizedPrompt =
      turnInputMode === "typed" ? turnPrompt.trim() : null;
    if (turnInputMode === "ptt" && !selectedSttReceipt) {
      setTurnError(
        "Capture a new receipt-backed AssemblyAI PTT turn before starting the character response.",
      );
      return;
    }
    if (turnInputMode === "typed" && !normalizedPrompt) {
      setTurnError("Enter a transcript before starting a turn.");
      return;
    }
    setDeliveredTurn(null);
    setTurnEvents([]);
    setRunning(true);
    if (selectedSttReceipt) setSelectedSttReceiptDisposition("submitted");
    setNotice(null);
    setTurnError(null);
    const selectedWorldTurn =
      turnWorldMode === "selectedWorld" && selectedCharacter !== null;
    const turnGameProfileId = selectedWorldTurn
      ? selectedCharacter.gameProfileId
      : "eclipse-harbor";
    const turnCharacterId = selectedWorldTurn
      ? selectedCharacter.character.id
      : "mara-venn";
    const turnCharacterName = selectedWorldTurn
      ? selectedCharacter.character.displayName
      : "Mara Venn";
    setTurnRoute({
      loadout: structuredClone(
        activeBrowserLoadoutFor(turnGameProfileId, turnCharacterId),
      ),
      capturedAtEpochMs: Date.now(),
    });
    try {
      const started = await startNativeSimulation(
        preferences.execution,
        (event) => {
          setTurnEvents((current) => [...current, event].slice(-24));
          if (event.type === "completed") {
            setDeliveredTurn(event);
            setRunning(false);
          }
          if (event.type === "cancelled") {
            setRunning(false);
            setTurnError(event.reason);
          }
          if (event.type === "failed") {
            setRunning(false);
            setTurnError(event.reason);
          }
        },
        selectedSttReceipt
          ? {
              gameProfileId: turnGameProfileId,
              characterId: turnCharacterId,
              characterName: turnCharacterName,
              selectedSttReceipt,
              enabledSpoilerTiers: [],
            }
          : {
              gameProfileId: turnGameProfileId,
              characterId: turnCharacterId,
              characterName: turnCharacterName,
              transcript: normalizedPrompt ?? "",
              enabledSpoilerTiers: [],
            },
      );
      if (!started) {
        setRunning(false);
        const detail =
          "Native desktop runtime is required for a delivered turn.";
        setTurnError(detail);
        setNotice(detail);
      } else if (selectedSttReceipt) {
        setNotice(
          `Opaque STT receipt generation ${selectedSttReceipt.generation} was submitted once to the native turn. Capture again for any later turn.`,
        );
      }
    } catch (error) {
      setRunning(false);
      const detail =
        error instanceof Error ? error.message : "Turn failed before delivery.";
      if (selectedSttReceipt) {
        setSelectedSttReceiptDisposition("rejected");
        setSelectedSttError(
          `Receipt generation ${selectedSttReceipt.generation} was consumed or rejected by the native turn and cannot be replayed. Capture again.`,
        );
      }
      setTurnError(detail);
      setNotice(detail);
    }
  };

  const stopTurn = async () => {
    await cancelNativeSimulation();
    setRunning(false);
  };

  const applySelectedSttTerminal = (event: SelectedSttTerminal) => {
    setSelectedSttCapture(null);
    setSelectedSttStatus({
      schemaVersion: 1,
      status: "idle",
      generation: event.generation,
      sessionId: null,
      turnId: null,
      inputEndpointId: null,
      inputEndpointGeneration: null,
    });
    setSelectedSttTerminal(event);
    if (event.status === "transcriptReady" && event.receiptId) {
      setSelectedSttReceiptDisposition("ready");
      setTurnError(null);
      setSelectedSttError(null);
      setNotice(
        `AssemblyAI produced an opaque one-time receipt from ${event.chunksSent} native chunks for generation ${event.generation}. Its transcript stays native until explicit receipt consumption.`,
      );
    } else if (event.status === "cancelled") {
      setSelectedSttReceiptDisposition(null);
      setNotice(
        `Push-to-talk generation ${event.generation} cancelled without a receipt.`,
      );
    } else {
      setSelectedSttReceiptDisposition(null);
      setSelectedSttError(
        event.errorCode ?? "Selected STT failed without a receipt.",
      );
    }
  };

  const startSelectedSttCapture = async (manualRetryGeneration?: number) => {
    if (!selectedSttReadiness.ready) {
      setSelectedSttError(selectedSttReadiness.reason);
      return;
    }
    setSelectedSttBusy(true);
    setSelectedSttError(null);
    setSelectedSttTerminal(null);
    setSelectedSttReceiptDisposition(null);
    const scope = { ...currentSttScope };
    let terminalBeforeStartReturned: SelectedSttTerminal | null = null;
    let statusPoll: number | null = null;
    let pollActive = true;
    try {
      setNotice(
        `Arming PTT for ${scope.characterLabel}. After clicking Arm, press and hold physical F8 within 8 seconds, speak for up to 10 seconds, then release F8.`,
      );
      const capturePromise = startSelectedSttPushToTalk(
        {
          schemaVersion: 1,
          gameProfileId: scope.gameProfileId,
          characterId: scope.characterId,
          contextHint: null,
          attempt:
            manualRetryGeneration === undefined
              ? { kind: "initial" }
              : {
                  kind: "manualRetry",
                  priorGeneration: manualRetryGeneration,
                  userAuthorized: true,
                },
        },
        (event) => {
          terminalBeforeStartReturned = event;
          applySelectedSttTerminal(event);
        },
      );
      const refreshArmingStatus = async () => {
        try {
          const nextStatus = await readSelectedSttPushToTalkStatus();
          if (pollActive && nextStatus) setSelectedSttStatus(nextStatus);
        } catch {
          // The pending start invocation remains authoritative. A status poll
          // failure must not manufacture a terminal state or cancel capture.
        }
      };
      void refreshArmingStatus();
      statusPoll = window.setInterval(() => {
        void refreshArmingStatus();
      }, 250);
      const capture = await capturePromise;
      if (!capture) {
        setSelectedSttError(
          "Open the installed native app to start selected STT capture.",
        );
        return;
      }
      setSelectedSttScope(scope);
      if (!terminalBeforeStartReturned) {
        setSelectedSttCapture(capture);
        setSelectedSttStatus({
          schemaVersion: 1,
          status: "capturing",
          generation: capture.generation,
          sessionId: capture.sessionId,
          turnId: capture.turnId,
          inputEndpointId: capture.inputEndpointId,
          inputEndpointGeneration: capture.inputEndpointGeneration,
        });
      }
      setNotice(
        `Native PTT generation ${capture.generation} is capturing for ${scope.characterLabel}. Speak for up to 10 seconds, then release physical F8; this WebView cannot manufacture either transition.`,
      );
    } catch (error) {
      setSelectedSttCapture(null);
      setSelectedSttError(
        error instanceof Error
          ? error.message
          : "Selected STT capture could not start.",
      );
    } finally {
      pollActive = false;
      if (statusPoll !== null) window.clearInterval(statusPoll);
      setSelectedSttBusy(false);
    }
  };

  const cancelSelectedSttCapture = async () => {
    setSelectedSttBusy(true);
    setSelectedSttError(null);
    try {
      const result = await cancelSelectedSttPushToTalk();
      if (!result) {
        setSelectedSttError(
          "Open the installed native app to cancel selected STT capture.",
        );
        return;
      }
      if (result.outcome === "alreadyIdle") {
        setSelectedSttCapture(null);
        setSelectedSttStatus({
          schemaVersion: 1,
          status: "idle",
          generation: result.generation,
          sessionId: null,
          turnId: null,
          inputEndpointId: null,
          inputEndpointGeneration: null,
        });
      }
      setNotice(
        result.outcome === "cancellationRequested"
          ? `Cancellation requested for native PTT generation ${result.generation}; awaiting its terminal event.`
          : `Native PTT was already idle at generation ${result.generation}.`,
      );
    } catch (error) {
      setSelectedSttError(
        error instanceof Error
          ? error.message
          : "Selected STT cancellation failed.",
      );
    } finally {
      setSelectedSttBusy(false);
    }
  };

  const refreshDiagnostics = async () => {
    setDiagnosticsBusy(true);
    try {
      const [nextDiagnostics, nextDiagnosticsV2, nextMatrix, nextSettings] =
        await Promise.all([
          readDiagnosticSummary(),
          readDiagnosticsV2(100),
          readDiagnosticsMatrix(),
          readDiagnosticsSettings(),
        ]);
      setDiagnostics(nextDiagnostics);
      setDiagnosticsV2(nextDiagnosticsV2);
      setDiagnosticsMatrix(nextMatrix);
      setDiagnosticsSettings(nextSettings);
    } catch (error) {
      setNotice(
        error instanceof Error
          ? error.message
          : "Native diagnostics did not complete.",
      );
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const persistDiagnosticsVerbosity = async (
    verbosity: NativeDiagnosticsSettings["verbosity"],
  ) => {
    setDiagnosticsBusy(true);
    try {
      const next = await saveDiagnosticsSettings({
        schemaVersion: 1,
        verbosity,
      });
      if (!next)
        throw new Error("Native diagnostics settings returned no state.");
      setDiagnosticsSettings(next);
      setDiagnosticsV2((current) =>
        current ? { ...current, verbosity: next.verbosity } : current,
      );
      setNotice(
        `Local diagnostics verbosity saved: ${next.verbosity}. Warnings and errors remain recorded; this does not enable remote telemetry.`,
      );
    } catch (error) {
      setNotice(
        error instanceof Error
          ? error.message
          : "Diagnostics verbosity could not be saved.",
      );
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const exportNativeDiagnostics = async () => {
    setDiagnosticsBusy(true);
    try {
      const result = await exportDiagnosticsV2(100);
      if (!result) {
        setNotice("Open the native desktop app to export diagnostics.");
        return;
      }
      setDiagnosticsExport(result);
      setNotice(
        `Local diagnostics export written · ${result.preview.eventCount} events · no upload`,
      );
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Diagnostics export failed.",
      );
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const selectSyntheticTarget = async () => {
    try {
      const result = await runSyntheticReplayCapture(captureDebugAvailable);
      setCaptureProof({
        pid: result.targetProcessId,
        hwnd: result.targetWindowHandle,
        executable: result.targetExecutableBasename,
        frames: result.diagnostics.framesReceived,
        receipt: null,
      });
      setNotice(
        `Synthetic target selected · PID ${result.targetProcessId} · HWND ${result.targetWindowHandle}. Select Verify live capture to require advancing exact-target WGC evidence.`,
      );
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Synthetic capture failed.",
      );
    }
  };

  const verifySyntheticCapture = async () => {
    if (!captureProof) {
      setNotice("Select the native synthetic target before verifying capture.");
      return;
    }
    try {
      const result = await verifySelectedGameCapture();
      if (!result)
        throw new Error("Native capture verification returned no receipt.");
      const evidence = result.capture;
      const exactTargetMatch =
        result.exactPidHwndExecutableMatch &&
        result.target.processId === captureProof.pid &&
        result.target.nativeWindow === captureProof.hwnd &&
        result.target.executableName.toLocaleLowerCase() ===
          captureProof.executable.toLocaleLowerCase() &&
        evidence.selectedProcessId === captureProof.pid &&
        evidence.selectedWindowHandle === captureProof.hwnd &&
        evidence.selectedExecutableName.toLocaleLowerCase() ===
          captureProof.executable.toLocaleLowerCase();
      const frameSequenceAdvanced = result.frameSequenceAdvanced;
      const verified =
        exactTargetMatch &&
        frameSequenceAdvanced &&
        evidence.overlayCaptureExcluded &&
        evidence.pixelSource === "windowsGraphicsCaptureTexture" &&
        evidence.pixelScope === "exactSelectedWindow" &&
        evidence.externalDisplayOverlayPixelsExcluded &&
        evidence.desktopLuminanceExcludedFromPixelEvidence &&
        result.safetyState === "verified_synthetic_fixture";
      setCaptureProof({
        pid: result.target.processId,
        hwnd: result.target.nativeWindow,
        executable: result.target.executableName,
        frames: evidence.latestFrameSequence,
        receipt: {
          evidence,
          exactTargetMatch,
          frameSequenceAdvanced,
          contentChanged: result.contentChanged,
          safetyState: result.safetyState,
          verified,
        },
      });
      setNotice(
        verified
          ? `Synthetic WGC receipt verified · PID ${result.target.processId} · HWND ${result.target.nativeWindow} · frame sequence ${evidence.latestFrameSequence} · exact-window pixels · unrelated display overlays excluded.`
          : "Synthetic capture verification did not prove every required condition. Review the exact-target, advancing-frame, exact-window pixel-source, and overlay-exclusion receipt in World.",
      );
    } catch (error) {
      setNotice(
        error instanceof Error
          ? error.message
          : "Synthetic capture verification failed.",
      );
    }
  };

  const runProviderAction = async (
    providerId: "elevenlabs" | "nvidia-nim",
    action: "save" | "validate",
  ) => {
    setProviderBusy(providerId);
    try {
      const result =
        action === "save"
          ? await promptAndSaveProviderCredential(providerId)
          : await testProviderCredential(providerId);
      setNotice(result.detail);
      if (action === "save" && result.status === "present") {
        const health = await loadNativeBootstrapHealth({ maxAttempts: 1 });
        setBootstrap(health);
      }
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Credential action failed.",
      );
    } finally {
      setProviderBusy(null);
    }
  };

  return (
    <div className="product-shell">
      <aside className="product-rail" aria-label="Primary navigation">
        <button
          className="brand-lockup"
          onClick={() => navigate("session")}
          aria-label="Open session deck"
        >
          <span className="brand-mark">N2</span>
          <span>
            <b>NPC 2.0</b>
            <small>session instrument</small>
          </span>
        </button>
        <nav>
          {NAV.map((item) => (
            <button
              key={item.id}
              className={page === item.id ? "nav-item selected" : "nav-item"}
              onClick={() => navigate(item.id)}
            >
              <span>{item.index}</span>
              <b>{item.label}</b>
            </button>
          ))}
        </nav>
        <div className="rail-safety">
          <span className="status-dot good" />
          Single-player only<small>Anti-cheat modes stay blocked.</small>
        </div>
      </aside>

      <main className="product-main">
        <header className="product-topbar">
          <div>
            <span className="eyebrow">
              NPC 2.0 / {NAV.find((item) => item.id === page)?.label}
            </span>
          </div>
          <div className={`runtime-chip ${runtimeSummary(bootstrap).tone}`}>
            <span className="status-dot" />
            {runtimeSummary(bootstrap).label}
          </div>
          <button
            className="quiet-button"
            onClick={() => {
              setNotice(null);
              setSetupOpen(true);
            }}
          >
            Setup
          </button>
        </header>

        {notice && !setupOpen && (
          <div className="notice" role="status">
            <span>{notice}</span>
            <button onClick={() => setNotice(null)} aria-label="Dismiss notice">
              ×
            </button>
          </div>
        )}

        <SignalRail
          activeStage={activeStage}
          running={running}
          delivered={Boolean(deliveredTurn)}
          providerPresent={providerPresent}
          captureProof={captureProof}
          selectedCharacter={selectedCharacter}
          selectedTarget={selectedTarget}
          turnWorldMode={turnWorldMode}
        />

        {page === "session" && (
          <SessionPage
            running={running}
            deliveredTurn={deliveredTurn}
            events={turnEvents}
            providerPresent={providerPresent}
            captureProof={captureProof}
            selectedCharacter={selectedCharacter}
            selectedTarget={selectedTarget}
            turnWorldMode={turnWorldMode}
            setTurnWorldMode={(value) => {
              resumeWorldOnInspection.current = false;
              setTurnWorldMode(value);
            }}
            inputMode={turnInputMode}
            setInputMode={setTurnInputMode}
            prompt={turnPrompt}
            setPrompt={setTurnPrompt}
            turnError={turnError}
            turnRoute={turnRoute}
            subtitles={preferences.subtitles}
            setSubtitles={(subtitles) =>
              setPreferences((current) => ({ ...current, subtitles }))
            }
            nativeAvailable={nativeWorldReady}
            selectedSttReadiness={selectedSttReadiness}
            selectedSttCapture={selectedSttCapture}
            selectedSttStatus={selectedSttStatus}
            selectedSttTerminal={selectedSttTerminal}
            selectedSttReceiptDisposition={selectedSttReceiptDisposition}
            selectedSttScope={selectedSttScope}
            selectedSttBusy={selectedSttBusy}
            selectedSttError={selectedSttError}
            onStartSelectedStt={() => void startSelectedSttCapture()}
            onCancelSelectedStt={() => void cancelSelectedSttCapture()}
            onRetrySelectedStt={(generation) =>
              void startSelectedSttCapture(generation)
            }
            onRun={runTurn}
            onStop={stopTurn}
            onNavigate={navigate}
          />
        )}
        {page === "world" && (
          <WorldPage
            captureProof={captureProof}
            captureAvailable={captureDebugAvailable.available}
            onCapture={selectSyntheticTarget}
            onVerifyCapture={verifySyntheticCapture}
            nativeAvailable={bootstrap.kind === "snapshot"}
            gameProfiles={snapshot?.gameProfiles ?? []}
            gameProfileId={selectedGameProfileId}
            onGameProfileChange={(gameProfileId) => {
              resumeWorldOnInspection.current = false;
              setSelectedGameProfileId(gameProfileId);
              setSelectedCharacter(null);
            }}
            onTargetChange={setSelectedTarget}
            onCharacterChange={(inspection) => {
              setSelectedCharacter(inspection);
              setTurnWorldMode("selectedWorld");
            }}
            selectedCharacter={selectedCharacter}
            identityEnrollmentStatus={identityEnrollmentStatus}
            identityEnrollmentError={identityEnrollmentError}
          />
        )}
        {page === "voice" && (
          <VoicePage
            providerPresent={providerPresent}
            providerDetail={provider?.detail}
            nvidiaPresent={nvidiaPresent}
            nvidiaDetail={nvidiaProvider?.detail}
            models={snapshot?.models ?? []}
            nativeAvailable={bootstrap.kind === "snapshot"}
            providerBusy={providerBusy}
            audioInputs={audioInputs}
            selectedAudioInput={selectedAudioInput}
            audioInputBusy={audioInputBusy}
            audioInputError={audioInputError}
            onRefreshAudioInputs={refreshAudioInputs}
            onSelectAudioInput={chooseAudioInput}
            selectedSttReadiness={selectedSttReadiness}
            selectedSttCapture={selectedSttCapture}
            selectedSttStatus={selectedSttStatus}
            selectedSttTerminal={selectedSttTerminal}
            selectedSttReceiptDisposition={selectedSttReceiptDisposition}
            selectedSttScope={selectedSttScope}
            selectedSttBusy={selectedSttBusy}
            selectedSttError={selectedSttError}
            onStartSelectedStt={() => void startSelectedSttCapture()}
            onCancelSelectedStt={() => void cancelSelectedSttCapture()}
            onRetrySelectedStt={(generation) =>
              void startSelectedSttCapture(generation)
            }
            onProviderAction={runProviderAction}
            onSave={async () => {
              const next = {
                ...preferences,
                execution: "cloud",
                localOnly: false,
              } satisfies AppPreferences;
              const saved = await saveOnboarding(
                nowSnapshot(true, "ready", next),
              );
              if (!saved) {
                setNotice(
                  "Open the native desktop app to save the API-first default.",
                );
                return;
              }
              setPreferences(saved.onboarding.preferences);
              setNotice("API-first execution default saved.");
            }}
          />
        )}
        {page === "diagnostics" && (
          <DiagnosticsPage
            bootstrap={bootstrap}
            diagnostics={diagnostics ?? snapshot?.diagnostics ?? null}
            diagnosticsV2={diagnosticsV2}
            diagnosticsExport={diagnosticsExport}
            diagnosticsMatrix={diagnosticsMatrix}
            diagnosticsSettings={diagnosticsSettings}
            busy={diagnosticsBusy}
            nativeAvailable={bootstrap.kind === "snapshot"}
            onRefresh={refreshDiagnostics}
            onExport={exportNativeDiagnostics}
            onSaveVerbosity={persistDiagnosticsVerbosity}
            onNavigate={navigate}
            turnEvents={turnEvents}
          />
        )}
        {page === "settings" && (
          <SettingsPage
            audioOutputs={audioOutputs}
            selectedAudioOutput={selectedAudioOutput}
            audioOutputBusy={audioOutputBusy}
            audioOutputError={audioOutputError}
            onRefreshAudioOutputs={refreshAudioOutputs}
            onSelectAudioOutput={chooseAudioOutput}
            audioInputs={audioInputs}
            selectedAudioInput={selectedAudioInput}
            audioInputBusy={audioInputBusy}
            audioInputError={audioInputError}
            onRefreshAudioInputs={refreshAudioInputs}
            onSelectAudioInput={chooseAudioInput}
            gameProfileId={selectedGameProfileId}
            characterId={selectedCharacter?.character.id ?? null}
            onProductPreferenceSnapshot={applyNativeProductPreferences}
            onSetup={() => {
              setSetupStep(0);
              setSetupOpen(true);
            }}
            onDiagnostics={() => navigate("diagnostics")}
            onNavigate={navigate}
            nativeAvailable={bootstrap.kind === "snapshot"}
          />
        )}
      </main>

      {setupOpen && (
        <OnboardingOverlay
          step={setupStep}
          bootstrap={bootstrap}
          providerPresent={providerPresent}
          nvidiaPresent={nvidiaPresent}
          loadout={activeBrowserLoadoutFor(
            selectedCharacter?.gameProfileId ?? selectedGameProfileId,
            selectedCharacter?.character.id ?? "mara-venn",
          )}
          credentialStates={snapshot?.providers ?? []}
          nativeAvailable={bootstrap.kind === "snapshot"}
          audioOutputs={audioOutputs}
          selectedAudioOutput={selectedAudioOutput}
          audioOutputBusy={audioOutputBusy}
          audioOutputError={audioOutputError}
          captureAvailable={captureDebugAvailable.available}
          captureProof={captureProof}
          deliveredTurn={deliveredTurn}
          running={running}
          preferences={preferences}
          feedback={notice}
          setPreferences={setPreferences}
          onBack={() => setSetupStep((current) => Math.max(0, current - 1))}
          onNext={nextSetupStep}
          onCapture={selectSyntheticTarget}
          onProviderAction={runProviderAction}
          onRefreshAudioOutputs={refreshAudioOutputs}
          onSelectAudioOutput={chooseAudioOutput}
          audioInputs={audioInputs}
          selectedAudioInput={selectedAudioInput}
          audioInputBusy={audioInputBusy}
          audioInputError={audioInputError}
          onRefreshAudioInputs={refreshAudioInputs}
          onSelectAudioInput={chooseAudioInput}
          selectedSttReadiness={selectedSttReadiness}
          selectedSttCapture={selectedSttCapture}
          selectedSttStatus={selectedSttStatus}
          selectedSttTerminal={selectedSttTerminal}
          selectedSttReceiptDisposition={selectedSttReceiptDisposition}
          selectedSttScope={selectedSttScope}
          selectedSttBusy={selectedSttBusy}
          selectedSttError={selectedSttError}
          onStartSelectedStt={() => void startSelectedSttCapture()}
          onCancelSelectedStt={() => void cancelSelectedSttCapture()}
          onRetrySelectedStt={(generation) =>
            void startSelectedSttCapture(generation)
          }
          onUseTypedInput={() => {
            setPreferences((current) => ({ ...current, ptt: false }));
            setTurnInputMode("typed");
          }}
          onRun={runTurn}
          onClose={
            snapshot?.onboarding?.completed || query.get("onboarding") === "1"
              ? () => setSetupOpen(false)
              : undefined
          }
        />
      )}
    </div>
  );
}

function SignalRail({
  activeStage,
  running,
  delivered,
  providerPresent,
  captureProof,
  selectedCharacter,
  selectedTarget,
  turnWorldMode,
}: {
  activeStage: string | null;
  running: boolean;
  delivered: boolean;
  providerPresent: boolean;
  captureProof: SyntheticCaptureProof | null;
  selectedCharacter: NativeCharacterInspection | null;
  selectedTarget: NativeGameTargetSelection | null;
  turnWorldMode: "syntheticReview" | "selectedWorld";
}) {
  const selectedWorld =
    turnWorldMode === "selectedWorld" && selectedCharacter !== null;
  const nodes = [
    {
      label: "Target",
      value: selectedWorld
        ? selectedTarget
          ? `${selectedTarget.target.title} · PID ${selectedTarget.target.processId}`
          : `${selectedCharacter.gameDisplayName} · process not bound`
        : captureProof?.receipt?.verified
          ? `WGC verified · PID ${captureProof.pid}`
          : captureProof
            ? `Selected · PID ${captureProof.pid}`
            : "Eclipse Harbor · not selected",
      ready: selectedWorld
        ? Boolean(selectedTarget?.processInstanceBound)
        : Boolean(captureProof?.receipt?.verified),
    },
    {
      label: "Actor",
      value: selectedWorld
        ? selectedCharacter.character.displayName
        : "Mara Venn",
      ready: true,
    },
    {
      label: "TTS vault",
      value: providerPresent
        ? "ElevenLabs credential present"
        : "No credential",
      ready: providerPresent,
    },
    { label: "Voice", value: "Per-turn authorization", ready: false },
    {
      label: "Delivery",
      value: delivered
        ? "Text committed"
        : running
          ? (activeStage ?? "Starting")
          : "Standby",
      ready: delivered,
    },
  ];
  return (
    <section
      className={running ? "signal-rail running" : "signal-rail"}
      aria-label="Session signal rail"
    >
      {nodes.map((node, index) => (
        <div className="signal-node" key={node.label}>
          <span className={node.ready ? "signal-index ready" : "signal-index"}>
            {String(index + 1).padStart(2, "0")}
          </span>
          <span>
            <small>{node.label}</small>
            <b>{node.value}</b>
          </span>
        </div>
      ))}
    </section>
  );
}

function SelectedSttControl({
  readiness,
  capture,
  status,
  terminal,
  receiptDisposition,
  scope,
  busy,
  error,
  onStart,
  onCancel,
  onRetry,
  compact = false,
}: {
  readiness: SelectedSttReadiness;
  capture: SelectedSttCapturing | null;
  status: SelectedSttStatus | null;
  terminal: SelectedSttTerminal | null;
  receiptDisposition: "ready" | "submitted" | "rejected" | null;
  scope: SelectedSttScope | null;
  busy: boolean;
  error: string | null;
  onStart: () => void;
  onCancel: () => void;
  onRetry: (generation: number) => void;
  compact?: boolean;
}) {
  const startReasonId = useId();
  const [egressConsent, setEgressConsent] = useState(false);
  const capturing =
    capture?.status === "capturing" || status?.status === "capturing";
  const arming = status?.status === "arming";
  const active = arming || capturing;
  const generation = capture?.generation ?? status?.generation ?? null;
  return (
    <section
      className={`selected-stt-control ${compact ? "is-compact" : ""}`}
      aria-label="Selected AssemblyAI push-to-talk"
      aria-busy={busy}
    >
      <div className="selected-stt-control__heading">
        <div>
          <b>Live push-to-talk</b>
          <small>Selected AssemblyAI route · physical F8 authority</small>
        </div>
        <span
          className={`badge ${terminal?.status === "transcriptReady" && receiptDisposition !== "rejected" ? "good" : active ? "wait" : readiness.ready ? "good" : "bad"}`}
        >
          {terminal?.status === "transcriptReady"
            ? receiptDisposition === "submitted"
              ? "Receipt submitted once"
              : receiptDisposition === "rejected"
                ? "Receipt rejected · recapture"
                : "Receipt ready"
            : terminal?.status === "failed"
              ? "Failed"
              : terminal?.status === "cancelled"
                ? "Cancelled"
                : arming
                  ? "Armed · waiting for F8"
                  : capturing
                    ? "Capturing"
                    : readiness.ready
                      ? "Ready"
                      : "Blocked"}
        </span>
      </div>
      <dl className="selected-stt-readiness">
        <div>
          <dt>Selected route</dt>
          <dd>
            {readiness.configuredProviderId} · {readiness.configuredModelId}
          </dd>
        </div>
        <div>
          <dt>Input endpoint</dt>
          <dd>
            {readiness.selectedInput
              ? `${readiness.selectedInput.resolved.friendlyName} · gen ${readiness.selectedInput.resolved.generation}`
              : "Not selected"}
          </dd>
        </div>
        <div>
          <dt>Credential</dt>
          <dd>
            {readiness.credentialPresent
              ? "Native AssemblyAI vault reference present"
              : "Missing"}
          </dd>
        </div>
        <div>
          <dt>Fallback</dt>
          <dd>Automatic fallback false</dd>
        </div>
      </dl>
      <p className="selected-stt-physical-note">
        Click Arm, then press and hold physical F8 within 8 seconds. Speak for
        up to 10 seconds and release F8. The broker supplies both transitions,
        endpoint generation, PCM, and provider token; this WebView supplies none
        of them. VAD is unavailable.
      </p>
      <label className="selected-stt-consent">
        <input
          type="checkbox"
          checked={egressConsent}
          disabled={!readiness.ready || active || busy}
          onChange={(event) => setEgressConsent(event.target.checked)}
        />
        <span>
          <b>I approve this AssemblyAI cloud STT attempt</b>
          <small>
            Captured microphone audio and optional non-secret context are sent
            to AssemblyAI using the exact selected <code>u3-rt-pro</code> route
            and native vault credential. Provider handling, retention, and terms
            may apply. Automatic fallback is false. The transcript stays native
            and can enter a character turn only through an explicit one-time
            opaque receipt.
          </small>
        </span>
      </label>
      <div className="selected-stt-actions">
        <button
          className="secondary-action"
          disabled={!readiness.ready || !egressConsent || active || busy}
          aria-describedby={startReasonId}
          onClick={onStart}
        >
          {busy ? "Requesting native capture…" : "Arm live PTT capture"}
        </button>
        <button
          className="stop-action"
          disabled={!active || (busy && !arming)}
          onClick={onCancel}
        >
          Cancel native capture
        </button>
        {terminal?.status === "failed" && terminal.retryable && (
          <button
            className="secondary-action"
            disabled={!readiness.ready || busy}
            onClick={() => onRetry(terminal.generation)}
          >
            Manually retry generation {terminal.generation}
          </button>
        )}
      </div>
      <small id={startReasonId} className="control-reason">
        {readiness.reason}
      </small>
      {active && (
        <div className="selected-stt-receipt" role="status">
          <b>
            {arming ? "Armed for physical F8" : "Native capture allocated"} ·
            generation {generation}
          </b>
          <span>
            {arming
              ? "No audio stream allocated yet · press F8 within 8 seconds"
              : capture
                ? `${capture.sessionId} · ${capture.turnId} · ${capture.inputEndpointId} gen ${capture.inputEndpointGeneration}`
                : `${status?.sessionId} · ${status?.turnId} · recovered active native status`}
          </span>
          <small>
            {arming
              ? "Waiting for a strictly newer broker-authoritative F8 press. Timeout or cancellation allocates no stream."
              : "Waiting for broker-authoritative physical release and one terminal channel event. Allocation alone is not audio or receipt proof."}
          </small>
        </div>
      )}
      {terminal && (
        <div
          className={`selected-stt-receipt ${terminal.status === "failed" ? "is-error" : ""}`}
          role="status"
        >
          <b>
            {terminal.status} · generation {terminal.generation}
          </b>
          <span>
            {terminal.sessionId} · {terminal.turnId}
            {scope ? ` · ${scope.characterLabel}` : ""}
          </span>
          {terminal.status === "transcriptReady" && terminal.route ? (
            <>
              <code className="selected-stt-receipt-id">
                Receipt {terminal.receiptId}
              </code>
              <small>
                {terminal.route.providerId} · {terminal.route.modelId} ·
                endpoint {terminal.route.inputEndpointId} gen{" "}
                {terminal.route.inputEndpointGeneration}
                {" · "}
                {terminal.chunksSent} chunks · {terminal.pcmBytesSent} PCM bytes
                · {terminal.route.capturedFrames} captured frames · physical F8
                press sequence {terminal.route.pttPressTransitionSequence} →
                release {terminal.route.pttReleaseTransitionSequence} ·{" "}
                {terminal.partialEvents} partial events · exact native vault
                reference present · no automatic fallback
              </small>
              <small>
                Transcript text is not exposed to this WebView. Digest{" "}
                {terminal.receiptSha256}.{" "}
                {receiptDisposition === "ready"
                  ? "The next explicit PTT turn action sends only this receipt ID and generation."
                  : "No further turn action may reuse this receipt."}{" "}
                Expired, consumed, or replayed receipts require a new capture.
              </small>
              {receiptDisposition && receiptDisposition !== "ready" && (
                <small>
                  {receiptDisposition === "submitted"
                    ? "This receipt has already been submitted once and cannot be reused. Capture again for another turn."
                    : "Native consumption rejected this already-spent receipt. It cannot be replayed; capture again."}
                </small>
              )}
            </>
          ) : (
            <small>
              {terminal.errorCode ?? "No receipt was committed."}
              {terminal.retryable
                ? " · explicit manual retry available"
                : " · not retryable"}
            </small>
          )}
        </div>
      )}
      {error && (
        <p className="selected-stt-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function SessionPage({
  running,
  deliveredTurn,
  events,
  providerPresent,
  captureProof,
  selectedCharacter,
  selectedTarget,
  turnWorldMode,
  setTurnWorldMode,
  inputMode,
  setInputMode,
  prompt,
  setPrompt,
  turnError,
  turnRoute,
  subtitles,
  setSubtitles,
  nativeAvailable,
  selectedSttReadiness,
  selectedSttCapture,
  selectedSttStatus,
  selectedSttTerminal,
  selectedSttReceiptDisposition,
  selectedSttScope,
  selectedSttBusy,
  selectedSttError,
  onStartSelectedStt,
  onCancelSelectedStt,
  onRetrySelectedStt,
  onRun,
  onStop,
  onNavigate,
}: {
  running: boolean;
  deliveredTurn: (NativeSimulationEvent & { type: "completed" }) | null;
  events: NativeSimulationEvent[];
  providerPresent: boolean;
  captureProof: SyntheticCaptureProof | null;
  selectedCharacter: NativeCharacterInspection | null;
  selectedTarget: NativeGameTargetSelection | null;
  turnWorldMode: "syntheticReview" | "selectedWorld";
  setTurnWorldMode: (value: "syntheticReview" | "selectedWorld") => void;
  inputMode: "typed" | "ptt";
  setInputMode: (value: "typed" | "ptt") => void;
  prompt: string;
  setPrompt: (value: string) => void;
  turnError: string | null;
  turnRoute: { loadout: ProviderLoadout; capturedAtEpochMs: number } | null;
  subtitles: boolean;
  setSubtitles: (value: boolean) => void;
  nativeAvailable: boolean;
  selectedSttReadiness: SelectedSttReadiness;
  selectedSttCapture: SelectedSttCapturing | null;
  selectedSttStatus: SelectedSttStatus | null;
  selectedSttTerminal: SelectedSttTerminal | null;
  selectedSttReceiptDisposition: "ready" | "submitted" | "rejected" | null;
  selectedSttScope: SelectedSttScope | null;
  selectedSttBusy: boolean;
  selectedSttError: string | null;
  onStartSelectedStt: () => void;
  onCancelSelectedStt: () => void;
  onRetrySelectedStt: (generation: number) => void;
  onRun: () => void;
  onStop: () => void;
  onNavigate: (page: ProductPage) => void;
}) {
  const audioDelivered = hasReceiptBackedAudio(deliveredTurn);
  const subtitleDelivered = hasReceiptBackedSubtitle(deliveredTurn);
  const visualPresentation = deliveredTurn?.visualPresentation;
  const failedTurn = [...events]
    .reverse()
    .find(
      (
        event,
      ): event is Extract<
        NativeSimulationEvent,
        { type: "failed" | "cancelled" }
      > => event.type === "failed" || event.type === "cancelled",
    );
  const executionEvidence =
    deliveredTurn?.turnExecution ?? failedTurn?.turnExecution;
  const acceptedSttReceipt = executionEvidence?.input?.selectedSttReceipt;
  const characterContext =
    deliveredTurn?.characterContext ?? failedTurn?.characterContext;
  const execution = executionEvidence?.success;
  const manualRetry = executionEvidence?.degradations.find(
    (degradation) => degradation.type === "manualRetryRequired",
  );
  const visibleTurnError = turnError ?? manualRetry?.reason ?? null;
  const completionLabel = audioDelivered
    ? "Receipt-backed provider audio submitted and drained"
    : subtitleDelivered
      ? "Receipt-backed native subtitle surface committed"
      : deliveredTurn?.runtimeFixtureOnly
        ? "Deterministic runtime fixture — no live provider or audible delivery claimed"
        : deliveredTurn
          ? "Runtime text completed — audio delivery not proven by this event"
          : "No delivered turn in this session";
  return (
    <div className="page-grid session-grid">
      <section className="instrument-panel primary-instrument">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Selected conversation</span>
            <h1>
              {turnWorldMode === "selectedWorld" && selectedCharacter
                ? selectedCharacter.character.displayName
                : "Mara Venn"}
            </h1>
            <p>
              {turnWorldMode === "selectedWorld" && selectedCharacter
                ? `${selectedCharacter.gameDisplayName} · ${selectedCharacter.character.promptRole} · persisted manual selection`
                : "Eclipse Harbor · synthetic review fixture · explicit test identity"}
            </p>
          </div>
          <span className="identity-seal">MV</span>
        </div>
        <div className="turn-composer">
          <div
            className="segmented-control turn-world-mode"
            aria-label="Turn world source"
          >
            <button
              className={turnWorldMode === "syntheticReview" ? "active" : ""}
              aria-pressed={turnWorldMode === "syntheticReview"}
              onClick={() => setTurnWorldMode("syntheticReview")}
            >
              Synthetic review game
            </button>
            <button
              className={turnWorldMode === "selectedWorld" ? "active" : ""}
              aria-pressed={turnWorldMode === "selectedWorld"}
              disabled={!selectedCharacter}
              onClick={() => setTurnWorldMode("selectedWorld")}
            >
              Selected native character
            </button>
          </div>
          <small>
            {turnWorldMode === "syntheticReview"
              ? "Runnable task-owned review path. It is not a bundled commercial GameProfileV2."
              : "Uses the selected bundled character and native route. Commercial visual capture remains blocked; failures are reported without fallback claims."}
          </small>
          {turnWorldMode === "syntheticReview" && (
            <div className="authorization-control route-authority-status">
              <span>
                <b>
                  {captureProof?.receipt?.verified
                    ? "External lip-sync bridge armed"
                    : "External lip-sync awaits verified capture"}
                </b>
                <small>
                  {captureProof?.receipt?.verified
                    ? "The exact static-mouth Mara window can feed OpenSeeFace CPU landmarks and the causal post-WASAPI mouth compositor. Actual presentation is reported only by the completed-turn receipt below."
                    : "Start the test game, then select and verify its exact WGC window on the World page before speaking."}
                </small>
              </span>
            </div>
          )}
          <div
            className="segmented-control turn-mode"
            aria-label="Turn input mode"
          >
            <button
              className={inputMode === "typed" ? "active" : ""}
              aria-pressed={inputMode === "typed"}
              onClick={() => setInputMode("typed")}
            >
              Typed message
            </button>
            <button
              className={inputMode === "ptt" ? "active" : ""}
              aria-pressed={inputMode === "ptt"}
              onClick={() => setInputMode("ptt")}
            >
              PTT rehearsal
            </button>
          </div>
          <label>
            <span>
              {inputMode === "typed"
                ? "Message transcript"
                : "PTT native receipt"}
            </span>
            <textarea
              aria-label="Turn transcript"
              value={
                inputMode === "ptt"
                  ? selectedSttTerminal?.status === "transcriptReady"
                    ? `Receipt generation ${selectedSttTerminal.generation} ready — transcript remains native`
                    : ""
                  : prompt
              }
              maxLength={500}
              rows={3}
              disabled={running || inputMode === "ptt"}
              onChange={(event) => setPrompt(event.target.value)}
            />
          </label>
          <small>
            {inputMode === "typed"
              ? "Sent as the explicit transcript for this bounded turn."
              : selectedSttTerminal?.status === "transcriptReady"
                ? "Opaque receipt ready. The transcript is not exposed here; the turn sends only receipt ID and generation for one-time native consumption."
                : "Arm native capture below, then use physical F8. The WebView cannot supply PCM, provider tokens, a press, or a release event."}
          </small>
          {inputMode === "ptt" && (
            <SelectedSttControl
              readiness={selectedSttReadiness}
              capture={selectedSttCapture}
              status={selectedSttStatus}
              terminal={selectedSttTerminal}
              receiptDisposition={selectedSttReceiptDisposition}
              scope={selectedSttScope}
              busy={selectedSttBusy}
              error={selectedSttError}
              onStart={onStartSelectedStt}
              onCancel={onCancelSelectedStt}
              onRetry={onRetrySelectedStt}
            />
          )}
        </div>
        <div className="session-controls">
          <button
            className="primary-action"
            onClick={onRun}
            disabled={
              running ||
              (inputMode === "ptt" &&
                (selectedSttTerminal?.status !== "transcriptReady" ||
                  selectedSttReceiptDisposition !== "ready"))
            }
          >
            {running
              ? "Turn in progress"
              : inputMode === "typed"
                ? "Send typed turn"
                : selectedSttTerminal?.status === "transcriptReady" &&
                    selectedSttReceiptDisposition === "ready"
                  ? "Send receipt-backed PTT turn"
                  : selectedSttReceiptDisposition === "submitted" ||
                      selectedSttReceiptDisposition === "rejected"
                    ? "Receipt spent — capture again"
                    : "Capture a PTT receipt first"}
            <span>
              {nativeAvailable ? "Native runtime" : "Desktop runtime required"}
            </span>
          </button>
          {running && (
            <button className="stop-action" onClick={onStop}>
              Cancel generation
            </button>
          )}
          <div className="authorization-control route-authority-status">
            <span>
              <b>Native selected route is authoritative</b>
              <small>
                The runtime resolves the persisted loadout and vault references
                for this turn. No development voice override is sent.
              </small>
            </span>
          </div>
          <label className="subtitle-control">
            <input
              type="checkbox"
              checked={subtitles}
              onChange={(event) => setSubtitles(event.target.checked)}
            />
            <span>
              <b>Show delivered subtitles</b>
              <small>Session-only until saved in Settings.</small>
            </span>
          </label>
        </div>
        {visibleTurnError && (
          <div className="degradation-panel" role="alert">
            <div>
              <span className="eyebrow">
                Turn stopped · no automatic provider switch
              </span>
              <b>{visibleTurnError}</b>
              <small>
                The configured manual fallback choices remain visible below.
              </small>
            </div>
            <button
              className="secondary-action"
              disabled={!nativeAvailable || running}
              onClick={onRun}
            >
              Retry current route
            </button>
            <button
              className="quiet-button"
              onClick={() => onNavigate("voice")}
            >
              Review recovery profile
            </button>
          </div>
        )}
        <div className="delivery-proof">
          <span
            className={
              audioDelivered || subtitleDelivered
                ? "proof-icon delivered"
                : "proof-icon"
            }
          />
          <div>
            <small>Delivery ledger</small>
            <b>{completionLabel}</b>
            {deliveredTurn && subtitles ? (
              <p>{deliveredTurn.deliveredText}</p>
            ) : deliveredTurn ? (
              <p className="subtitle-hidden">
                Subtitle text hidden by session preference.
              </p>
            ) : null}
            <dl className="delivery-channels">
              <div>
                <dt>Response text</dt>
                <dd>
                  {!deliveredTurn
                    ? "Not delivered"
                    : execution?.llmProviderLive
                      ? "Live LLM provider evidence"
                      : deliveredTurn.runtimeFixtureOnly
                        ? "Deterministic runtime fixture"
                        : "Runtime completion; provider not proven"}
                </dd>
              </div>
              <div>
                <dt>Subtitle</dt>
                <dd>
                  {subtitleDelivered
                    ? `${execution?.subtitleReceiptCount} committed presentation receipt${execution?.subtitleReceiptCount === 1 ? "" : "s"} · ${subtitleReceiptSurfaceLabel(executionEvidence?.subtitlePresentationReceipts ?? [])} · ${executionEvidence?.subtitlePresentationReceipts.map((receipt) => receipt.receiptId).join(", ")}`
                    : deliveredTurn && subtitles
                      ? "Control-app text visible; native overlay not proven"
                      : "Not reported"}
                </dd>
              </div>
              <div>
                <dt>Audio</dt>
                <dd>
                  {audioDelivered
                    ? `${execution?.audioReceiptCount} submitted + drained receipt${execution?.audioReceiptCount === 1 ? "" : "s"} · ${executionEvidence?.audioReceipts.map((receipt) => receipt.receiptId).join(", ")} · ${audioReceiptEndpointLabel(executionEvidence?.audioReceipts ?? [])}`
                    : "Not proven — requires live TTS + submission + drain receipts"}
                </dd>
              </div>
              <div>
                <dt>Visible speech</dt>
                <dd>
                  {!deliveredTurn
                    ? "Not reported"
                    : visualPresentation?.presented
                      ? `${turnWorldMode === "syntheticReview" ? "OpenSeeFace CPU landmarks + local audio-driven mouth overlay" : "Admitted local mouth overlay"} presented on captured frame ${visualPresentation.sourceFrameSequence} · ${visualPresentation.pixelSource ?? "pixel source not reported"} / ${visualPresentation.pixelScope ?? "pixel scope not reported"}`
                      : visualPresentation
                        ? `Visual-only bypass — ${visualPresentation.detail}`
                        : "Not reported — no native visual presentation receipt"}
                </dd>
              </div>
              <div>
                <dt>Runtime state</dt>
                <dd>
                  {executionEvidence?.deliveryState ?? "Not reported"} ·{" "}
                  {executionEvidence?.commitState ?? "commit not reported"}
                </dd>
              </div>
            </dl>
            {acceptedSttReceipt && (
              <section
                className="accepted-stt-receipt-proof"
                aria-label="Accepted native STT receipt"
              >
                <div>
                  <small>Native input accepted</small>
                  <b>
                    AssemblyAI u3-rt-pro receipt {acceptedSttReceipt.receiptId}
                  </b>
                  <code>{acceptedSttReceipt.receiptSha256}</code>
                </div>
                <dl className="delivery-channels">
                  <div>
                    <dt>Capture binding</dt>
                    <dd>
                      generation {acceptedSttReceipt.captureGeneration} ·{" "}
                      {acceptedSttReceipt.route.inputEndpointId} generation{" "}
                      {acceptedSttReceipt.route.inputEndpointGeneration}
                    </dd>
                  </div>
                  <div>
                    <dt>Turn scope</dt>
                    <dd>
                      {acceptedSttReceipt.gameId} /{" "}
                      {acceptedSttReceipt.characterId ?? "no character"} ·{" "}
                      {acceptedSttReceipt.sourceLoadoutId}
                    </dd>
                  </div>
                  <div>
                    <dt>Physical PTT proof</dt>
                    <dd>
                      F8 press{" "}
                      {acceptedSttReceipt.route.pttPressTransitionSequence} →
                      release{" "}
                      {acceptedSttReceipt.route.pttReleaseTransitionSequence} ·{" "}
                      {acceptedSttReceipt.route.capturedFrames} captured frames
                    </dd>
                  </div>
                  <div>
                    <dt>Provider route</dt>
                    <dd>
                      {acceptedSttReceipt.route.providerId} /{" "}
                      {acceptedSttReceipt.route.modelId} ·{" "}
                      {acceptedSttReceipt.route.manualRetry
                        ? "manual retry"
                        : "initial attempt"}{" "}
                      · no automatic fallback
                    </dd>
                  </div>
                </dl>
                <small>
                  This completed native event proves the opaque capture receipt
                  was accepted for this turn. Transcript, microphone audio, and
                  provider credentials are not exposed here.
                </small>
              </section>
            )}
            {executionEvidence && executionEvidence.degradations.length > 0 && (
              <ul className="turn-degradations" aria-label="Turn degradations">
                {executionEvidence.degradations.map((degradation, index) => (
                  <li key={`${degradation.type}-${index}`}>
                    <b>{degradation.type}</b>
                    <span>{degradation.reason}</span>
                  </li>
                ))}
              </ul>
            )}
            {characterContext && (
              <>
                <dl className="delivery-channels character-context-proof">
                  <div>
                    <dt>Resolved character</dt>
                    <dd>
                      {characterContext.profileId} /{" "}
                      {characterContext.characterId}
                    </dd>
                  </div>
                  <div>
                    <dt>Identity source</dt>
                    <dd>
                      {characterContext.identitySource} ·{" "}
                      {characterContext.explicitSelection
                        ? "explicit selection"
                        : "not explicit"}
                    </dd>
                  </div>
                  <div>
                    <dt>Selection outcome</dt>
                    <dd>
                      {characterContext.selection?.status ?? "not reported"}
                      {characterContext.selection?.status === "ambiguous"
                        ? ` · ${characterContext.selection.candidate_ids.join(", ")}`
                        : ""}
                    </dd>
                  </div>
                  <div>
                    <dt>Prompt authority</dt>
                    <dd>
                      {characterContext.prompt
                        ? `${characterContext.prompt.recordCount} records · ${characterContext.prompt.authorities.join(", ") || "none"}`
                        : "not reported"}
                    </dd>
                  </div>
                  <div>
                    <dt>Scoped memory</dt>
                    <dd>
                      {characterContext.prompt
                        ? `${characterContext.prompt.scopedMemoryItemIds.length} items · ${characterContext.prompt.scopedMemoryClasses.join(", ") || "no classes"}`
                        : "not reported"}
                    </dd>
                  </div>
                  <div>
                    <dt>Background encounter</dt>
                    <dd>
                      {characterContext.encounter
                        ? `${characterContext.encounter.status} · ${characterContext.encounter.encounter_id}`
                        : "none reported"}
                    </dd>
                  </div>
                </dl>
                {characterContext.encounter && (
                  <EncounterLifecycleControls
                    nativeAvailable={nativeAvailable}
                    encounter={characterContext.encounter}
                    authoredCharacter={
                      selectedCharacter?.gameProfileId ===
                      characterContext.encounter.game_profile_id
                        ? {
                            gameProfileId: selectedCharacter.gameProfileId,
                            characterId: selectedCharacter.character.id,
                            displayName:
                              selectedCharacter.character.displayName,
                          }
                        : null
                    }
                  />
                )}
              </>
            )}
          </div>
        </div>
      </section>

      <aside className="session-side">
        <section className="instrument-panel compact-panel">
          <div className="panel-title">
            <h2>Ready decision</h2>
            <span className={providerPresent ? "badge good" : "badge wait"}>
              {providerPresent
                ? "Credential ready"
                : nativeAvailable
                  ? "Credential missing"
                  : "Browser preview"}
            </span>
          </div>
          <dl className="facts">
            <div>
              <dt>Capture</dt>
              <dd>
                {selectedTarget
                  ? `Bound PID ${selectedTarget.target.processId} · visuals ${selectedTarget.captureAuthorized ? "authorized" : "blocked"}`
                  : captureProof?.receipt?.verified
                    ? `Synthetic WGC verified · PID ${captureProof.pid}`
                    : captureProof
                      ? `Synthetic target selected · PID ${captureProof.pid}`
                      : "Not selected"}
              </dd>
            </div>
            <div>
              <dt>Identity</dt>
              <dd>
                {turnWorldMode === "selectedWorld" && selectedCharacter
                  ? `${selectedCharacter.character.displayName} · native selected`
                  : "Mara Venn · synthetic review"}
              </dd>
            </div>
            <div>
              <dt>Input</dt>
              <dd>Typed transcript or receipt-backed PTT</dd>
            </div>
            <div>
              <dt>Subtitles</dt>
              <dd>{subtitles ? "Visible" : "Hidden"}</dd>
            </div>
          </dl>
          <button className="text-action" onClick={() => onNavigate("world")}>
            Inspect world selection →
          </button>
        </section>
        <section className="instrument-panel trace-panel">
          <div className="panel-title">
            <h2>Turn trace</h2>
            <span className="mono">{events.length} events</span>
          </div>
          {events.length === 0 ? (
            <p className="empty-copy">
              No current trace. Run a turn to populate runtime-owned events.
            </p>
          ) : (
            <ol>
              {events.slice(-7).map((event) => (
                <li key={`${event.sequence}-${event.type}`}>
                  <span>{event.sequence.toString().padStart(2, "0")}</span>
                  <b>{event.type}</b>
                  <small>
                    {"stage" in event
                      ? event.stage
                      : "measurementBasis" in event
                        ? event.measurementBasis
                        : ""}
                  </small>
                </li>
              ))}
            </ol>
          )}
        </section>
        <section className="instrument-panel route-receipt-panel">
          <div className="panel-title">
            <h2>Route evidence</h2>
            <span className="badge wait">
              {executionEvidence?.consumedRoute
                ? "Native consumed route"
                : turnRoute
                  ? "Preference only"
                  : "Not captured"}
            </span>
          </div>
          {executionEvidence?.consumedRoute && (
            <div className="native-consumed-route">
              <div className="route-receipt-heading">
                <b>{executionEvidence.consumedRoute.sourceLoadoutId}</b>
                <small>
                  generation {executionEvidence.consumedRoute.generation} ·
                  sha256 {executionEvidence.consumedRoute.sha256}
                </small>
              </div>
              <dl className="facts">
                <div>
                  <dt>LLM actually consumed</dt>
                  <dd>
                    {executionEvidence.consumedRoute.llm
                      ? `${executionEvidence.consumedRoute.llm.providerId} · ${executionEvidence.consumedRoute.llm.modelId}`
                      : "No LLM route"}
                  </dd>
                </div>
                <div>
                  <dt>TTS actually consumed</dt>
                  <dd>
                    {executionEvidence.consumedRoute.tts
                      ? `${executionEvidence.consumedRoute.tts.providerId} · ${executionEvidence.consumedRoute.tts.modelId}${executionEvidence.consumedRoute.tts.voiceId ? ` · ${executionEvidence.consumedRoute.tts.voiceId}` : ""}`
                      : "No TTS route"}
                  </dd>
                </div>
                {executionEvidence.consumedRoute
                  .privateEvaluationAcknowledgement && (
                  <div>
                    <dt>Private-evaluation authority</dt>
                    <dd>
                      {
                        executionEvidence.consumedRoute
                          .privateEvaluationAcknowledgement.applicationNamespace
                      }
                      {" · acknowledgement "}
                      {
                        executionEvidence.consumedRoute
                          .privateEvaluationAcknowledgement
                          .acknowledgementSha256
                      }
                      {" · modalities "}
                      {executionEvidence.consumedRoute.privateEvaluationAcknowledgement.modalities.join(
                        ", ",
                      )}
                      {" · promotion/publication unsupported"}
                    </dd>
                  </div>
                )}
              </dl>
            </div>
          )}
          {!turnRoute ? (
            <p className="empty-copy">
              Start a turn to capture configured preference and wait for native
              consumed-route evidence.
            </p>
          ) : (
            <>
              <div className="route-receipt-heading">
                <b>{turnRoute.loadout.name}</b>
                <small>
                  {turnRoute.loadout.scope} preference · captured{" "}
                  {new Date(turnRoute.capturedAtEpochMs).toLocaleTimeString()}
                </small>
              </div>
              <ol className="route-receipt">
                {ROLE_ORDER.map((role) => {
                  const route = turnRoute.loadout.routes[role];
                  const provider = providerFor(role, route.providerId);
                  const model = modelFor(role, route);
                  const fallback = turnRoute.loadout.fallbacks[role];
                  return (
                    <li key={role}>
                      <span>{ROLE_META[role].short}</span>
                      <div>
                        <b>
                          {provider.name} · {model.name}
                        </b>
                        <small>
                          {role === "stt"
                            ? "Configured preference · native consumed-route evidence after the turn wins"
                            : role === "embeddings"
                              ? "Configured only · not consumed; SQLite retrieval is not in this turn receipt"
                              : role === "vision"
                                ? "Configured only · not consumed; no frame adapter is active"
                                : role === "lipSync"
                                  ? "Configured only · no qualified mouth worker is active"
                                  : "Configured route · consumption requires completed runtime evidence"}
                        </small>
                        <small>
                          {fallback?.authorized
                            ? `Manual retry: ${providerFor(role, fallback.providerId).name}`
                            : "No authorized manual fallback"}
                        </small>
                      </div>
                    </li>
                  );
                })}
              </ol>
              <p className="route-receipt-disclosure">
                This list is only the configured WebView preference. The native
                consumed-route record above wins whenever it is present.
              </p>
            </>
          )}
        </section>
      </aside>
    </div>
  );
}

function WorldPage({
  captureAvailable,
  captureProof,
  onCapture,
  onVerifyCapture,
  nativeAvailable,
  gameProfiles,
  gameProfileId,
  onGameProfileChange,
  onTargetChange,
  onCharacterChange,
  selectedCharacter,
  identityEnrollmentStatus,
  identityEnrollmentError,
}: {
  captureAvailable: boolean;
  captureProof: SyntheticCaptureProof | null;
  onCapture: () => void;
  onVerifyCapture: () => void;
  nativeAvailable: boolean;
  gameProfiles: NativeGameProfileSummary[];
  gameProfileId: string;
  onGameProfileChange: (gameProfileId: string) => void;
  onTargetChange: (selection: NativeGameTargetSelection | null) => void;
  onCharacterChange: (inspection: NativeCharacterInspection) => void;
  selectedCharacter: NativeCharacterInspection | null;
  identityEnrollmentStatus: NativeIdentityReferenceEnrollmentStatus | null;
  identityEnrollmentError: string | null;
}) {
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">Target and identity boundary</span>
        <h1>World</h1>
        <p>
          {nativeAvailable
            ? "Select a bundled profile, bind an eligible process instance, and inspect the native-owned character authority used by ordinary turns."
            : "Preview the synthetic review profile. Bundled game profiles, process binding, and native-owned character authority require the Windows desktop shell."}
        </p>
      </header>
      <GameTargetWorkspace
        nativeAvailable={nativeAvailable}
        gameProfiles={gameProfiles}
        gameProfileId={gameProfileId}
        onGameProfileChange={onGameProfileChange}
        onSelectionChange={onTargetChange}
      />
      <div className="world-layout">
        <section className="instrument-panel world-visual">
          <div className="world-horizon">
            <div className="moon" />
            <div className="lighthouse">
              <i />
              <span />
            </div>
            <div className="harbor-lines" />
          </div>
          <div className="world-overlay">
            <span
              className={
                captureProof?.receipt?.verified ? "badge good" : "badge wait"
              }
            >
              {captureProof?.receipt?.verified
                ? "Synthetic WGC verified"
                : captureProof
                  ? "Synthetic target selected"
                  : "Debug fixture only"}
            </span>
            <h2>Eclipse Harbor</h2>
            <p>
              Synthetic validation target · offline · task-owned GUI executable
            </p>
          </div>
        </section>
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>Capture contract</h2>
            <span className="badge">Debug only</span>
          </div>
          <dl className="facts spacious">
            <div>
              <dt>Executable</dt>
              <dd>interactive-npcs-synthetic-target.exe</dd>
            </div>
            <div>
              <dt>Policy</dt>
              <dd>Single-player only</dd>
            </div>
            <div>
              <dt>Evidence</dt>
              <dd>
                {captureProof
                  ? `PID ${captureProof.pid} · HWND ${captureProof.hwnd}`
                  : "PID · HWND · frame counters"}
              </dd>
            </div>
            <div>
              <dt>Backend</dt>
              <dd>Windows Graphics Capture</dd>
            </div>
          </dl>
          <div className="capture-verification-actions">
            <button
              className="secondary-action"
              disabled={!captureAvailable}
              onClick={onCapture}
            >
              {captureProof
                ? "Reselect synthetic target"
                : captureAvailable
                  ? "Select synthetic target"
                  : "Native debug target unavailable"}
            </button>
            <button
              className="primary-action"
              disabled={!captureAvailable || !captureProof}
              aria-describedby="verify-capture-reason"
              onClick={onVerifyCapture}
            >
              Verify live capture
            </button>
          </div>
          <small id="verify-capture-reason" className="control-reason">
            {!captureAvailable
              ? "Requires the native debug build and task-owned synthetic target. Ordinary commercial-game capture remains fail-closed."
              : !captureProof
                ? "Select the exact task-owned synthetic target first."
                : "Calls the canonical native selected-target verifier and requires exact PID, HWND, executable, an advancing frame sequence, and overlay exclusion."}
          </small>
          {captureProof?.receipt && (
            <dl className="capture-receipt" aria-label="Synthetic WGC receipt">
              <div>
                <dt>Exact target</dt>
                <dd>
                  {captureProof.receipt.exactTargetMatch
                    ? "Matched PID · HWND · executable"
                    : "Mismatch"}
                </dd>
              </div>
              <div>
                <dt>Frame sequence</dt>
                <dd>
                  {captureProof.receipt.evidence.latestFrameSequence} ·{" "}
                  {captureProof.receipt.frameSequenceAdvanced
                    ? "advanced"
                    : "not proven advancing"}
                </dd>
              </div>
              <div>
                <dt>Content / geometry</dt>
                <dd>
                  {captureProof.receipt.evidence.contentWidth}×
                  {captureProof.receipt.evidence.contentHeight} ·{" "}
                  {captureProof.receipt.contentChanged
                    ? `${captureProof.receipt.evidence.contentHashChanges} content changes`
                    : "content change not required for this receipt"}
                </dd>
              </div>
              <div>
                <dt>Overlay exclusion</dt>
                <dd>
                  {captureProof.receipt.evidence.overlayCaptureExcluded
                    ? "Excluded from capture"
                    : "Not proven"}
                </dd>
              </div>
              <div>
                <dt>Pixel source / scope</dt>
                <dd>
                  {captureProof.receipt.evidence.pixelSource} ·{" "}
                  {captureProof.receipt.evidence.pixelScope}
                </dd>
              </div>
              <div>
                <dt>Display provenance</dt>
                <dd>
                  {captureProof.receipt.evidence
                    .externalDisplayOverlayPixelsExcluded &&
                  captureProof.receipt.evidence
                    .desktopLuminanceExcludedFromPixelEvidence
                    ? "Unrelated display-overlay pixels and desktop luminance excluded"
                    : "Exact pixel provenance not proven"}
                </dd>
              </div>
              <div>
                <dt>Safety state</dt>
                <dd>{captureProof.receipt.safetyState}</dd>
              </div>
            </dl>
          )}
          {captureProof?.receipt && (
            <p className="source-disclosure capture-pixel-note">
              {captureProof.receipt.evidence
                .externalDisplayOverlaysMayChangePerceivedBrightness
                ? "External display overlays may change perceived brightness, but exact selected-window WGC pixels exclude unrelated display-overlay pixels. Desktop or whole-screen screenshot luminance is never capture or color proof."
                : "Native perceived-brightness caveat is missing; this receipt cannot support a display-color claim."}
            </p>
          )}
        </section>
      </div>
      <CharacterDatabase
        nativeAvailable={nativeAvailable}
        gameProfileId={gameProfileId}
        onSelectionChange={onCharacterChange}
      />
      <section className="instrument-panel identity-enrollment-panel">
        <div className="panel-title">
          <h2>Private identity reference</h2>
          <span className="badge wait">
            {identityEnrollmentStatus?.signedIdentityPackAdmitted
              ? "Admission ready"
              : "Unavailable"}
          </span>
        </div>
        <p className="body-copy">
          Enroll a user-owned private photo or original synthetic artwork only
          through the future native picker for the exact selected game and
          character. File paths, raw pixels, and worker capability never enter
          this WebView.
        </p>
        <dl className="facts spacious">
          <div>
            <dt>Scope</dt>
            <dd>
              {selectedCharacter
                ? `${selectedCharacter.gameDisplayName} · ${selectedCharacter.character.displayName}`
                : "Select an authored character first"}
            </dd>
          </div>
          <div>
            <dt>Accepted rights</dt>
            <dd>User-private ownership or licensed original synthetic work</dd>
          </div>
          <div>
            <dt>WebView exposure</dt>
            <dd>
              {identityEnrollmentStatus &&
              !identityEnrollmentStatus.rawPixelsExposedToWebview &&
              !identityEnrollmentStatus.workerCapabilityExposedToWebview
                ? "No raw pixels, paths, or worker tokens"
                : "No enrollment authority exposed"}
            </dd>
          </div>
        </dl>
        <button
          className="secondary-action"
          disabled={
            !selectedCharacter ||
            !identityEnrollmentStatus?.signedIdentityPackAdmitted
          }
          aria-describedby="identity-enrollment-reason"
        >
          Choose reference in native picker
        </button>
        <small id="identity-enrollment-reason" className="control-reason">
          {identityEnrollmentError
            ? `Enrollment status unavailable: ${identityEnrollmentError}`
            : (identityEnrollmentStatus?.detail ??
              "A signed, measured identity pack must be admitted before the native picker can open. Cancelling a future picker creates no gallery receipt.")}
        </small>
      </section>
    </div>
  );
}

function VoicePage({
  providerPresent,
  providerDetail,
  nvidiaPresent,
  nvidiaDetail,
  models,
  nativeAvailable,
  providerBusy,
  audioInputs,
  selectedAudioInput,
  audioInputBusy,
  audioInputError,
  onRefreshAudioInputs,
  onSelectAudioInput,
  selectedSttReadiness,
  selectedSttCapture,
  selectedSttStatus,
  selectedSttTerminal,
  selectedSttReceiptDisposition,
  selectedSttScope,
  selectedSttBusy,
  selectedSttError,
  onStartSelectedStt,
  onCancelSelectedStt,
  onRetrySelectedStt,
  onProviderAction,
  onSave,
}: {
  providerPresent: boolean;
  providerDetail?: string;
  nvidiaPresent: boolean;
  nvidiaDetail?: string;
  models: NativeModelSummary[];
  nativeAvailable: boolean;
  providerBusy: string | null;
  audioInputs: NativeAudioInputSnapshot | null;
  selectedAudioInput: NativeSelectedAudioInput | null;
  audioInputBusy: boolean;
  audioInputError: string | null;
  onRefreshAudioInputs: () => Promise<void>;
  onSelectAudioInput: (selection: NativeAudioInputSelection) => Promise<void>;
  selectedSttReadiness: SelectedSttReadiness;
  selectedSttCapture: SelectedSttCapturing | null;
  selectedSttStatus: SelectedSttStatus | null;
  selectedSttTerminal: SelectedSttTerminal | null;
  selectedSttReceiptDisposition: "ready" | "submitted" | "rejected" | null;
  selectedSttScope: SelectedSttScope | null;
  selectedSttBusy: boolean;
  selectedSttError: string | null;
  onStartSelectedStt: () => void;
  onCancelSelectedStt: () => void;
  onRetrySelectedStt: (generation: number) => void;
  onProviderAction: (
    providerId: "elevenlabs" | "nvidia-nim",
    action: "save" | "validate",
  ) => void;
  onSave: () => void;
}) {
  const providerCards = [
    {
      id: "nvidia-nim" as const,
      name: "NVIDIA NIM",
      present: nvidiaPresent,
      detail: nvidiaDetail,
      description:
        "One native NVIDIA credential can cover selected hosted reply, embedding, and Magpie voice routes only inside an eligible private-evaluation namespace with the current terms acknowledged.",
      data: "Selected text, audio, or memory query by route",
    },
    {
      id: "elevenlabs" as const,
      name: "ElevenLabs",
      present: providerPresent,
      detail: providerDetail,
      description:
        "Qualified stock-voice path for the current spoken-turn rehearsal. Voice cloning is not enabled.",
      data: "Generated reply text only",
    },
  ];
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">
          API-first route · local visuals optional
        </span>
        <h1>Voice & models</h1>
        <p>
          Start with hosted conversation models so the game keeps its GPU. Add a
          local role only after this PC proves that the complete loadout fits.
          Credentials remain in the native vault and never enter this WebView.
        </p>
      </header>
      <p className="source-disclosure">
        <b>Source:</b> recommended API-first starter, not an active-turn
        receipt. The Session deck freezes the configured per-turn route
        separately.
      </p>
      <section className="instrument-panel settings-output-panel">
        <div className="panel-title">
          <h2>Microphone endpoint</h2>
          <span className="badge wait">Routing only</span>
        </div>
        <AudioInputPicker
          nativeAvailable={nativeAvailable}
          inputs={audioInputs}
          selected={selectedAudioInput}
          busy={audioInputBusy}
          error={audioInputError}
          onRefresh={onRefreshAudioInputs}
          onSelect={onSelectAudioInput}
        />
        <SelectedSttControl
          compact
          readiness={selectedSttReadiness}
          capture={selectedSttCapture}
          status={selectedSttStatus}
          terminal={selectedSttTerminal}
          receiptDisposition={selectedSttReceiptDisposition}
          scope={selectedSttScope}
          busy={selectedSttBusy}
          error={selectedSttError}
          onStart={onStartSelectedStt}
          onCancel={onCancelSelectedStt}
          onRetry={onRetrySelectedStt}
        />
      </section>
      <section
        className="route-map"
        aria-label="Recommended API-first starter route"
      >
        {[
          {
            role: "Speech in",
            model: "Configured route",
            provider:
              "Consumed only by an exact receipt-backed native PTT turn",
          },
          {
            role: "Reply",
            model: "Streaming hosted LLM",
            provider: "NVIDIA NIM or another configured API",
          },
          {
            role: "Voice out",
            model: "Stock voice",
            provider: "NVIDIA Magpie or ElevenLabs · no cloning",
          },
          {
            role: "Lip-sync",
            model: "Off by default",
            provider:
              "A future qualified local pack will require explicit install and whole-loadout admission",
          },
        ].map((route) => (
          <article key={route.role} className="route-card">
            <span>{route.role}</span>
            <h2>{route.model}</h2>
            <p>{route.provider}</p>
          </article>
        ))}
      </section>
      <div className="provider-grid" aria-label="Provider accounts">
        {providerCards.map((account) => (
          <section
            className="instrument-panel provider-account"
            key={account.id}
          >
            <div className="panel-title">
              <h2>{account.name}</h2>
              <span className={account.present ? "badge good" : "badge bad"}>
                {account.present ? "Credential present" : "Credential missing"}
              </span>
            </div>
            <p className="body-copy">{account.description}</p>
            <dl className="facts spacious">
              <div>
                <dt>Data sent</dt>
                <dd>{account.data}</dd>
              </div>
              <div>
                <dt>Vault</dt>
                <dd>{account.detail ?? "Native status unavailable"}</dd>
              </div>
            </dl>
            <div className="provider-actions">
              <button
                className="secondary-action"
                disabled={!nativeAvailable || providerBusy !== null}
                onClick={() => onProviderAction(account.id, "save")}
              >
                {providerBusy === account.id
                  ? "Native prompt open…"
                  : account.present
                    ? "Replace credential"
                    : "Add credential securely"}
              </button>
              <button
                className="quiet-button"
                disabled={!account.present || providerBusy !== null}
                onClick={() => onProviderAction(account.id, "validate")}
              >
                Validate vault binding
              </button>
            </div>
          </section>
        ))}
      </div>
      <div className="voice-layout">
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>API-first starter</h2>
            <span className="badge good">Recommended</span>
          </div>
          <p className="body-copy">
            Cloud execution is preferred, while the selected six-role loadout
            remains independently configured and validated. This action does not
            create routes or imply that LLM, STT, or TTS credentials exist.
          </p>
          <button
            className="secondary-action route-save"
            disabled={!nativeAvailable}
            onClick={onSave}
          >
            Save cloud execution preference
          </button>
          <small className="control-reason">
            NVIDIA trial routes are private-evaluation services with provider-
            and model-specific limits, not production entitlement.
          </small>
        </section>
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>Local model inventory</h2>
            <span className="badge">
              {models.length || "No"} catalog entries
            </span>
          </div>
          {models.length > 0 ? (
            <div className="model-inventory">
              {models.slice(0, 6).map((model) => (
                <article key={model.id}>
                  <div>
                    <b>{model.displayName}</b>
                    <small>{model.purpose}</small>
                  </div>
                  <span className="badge wait">{model.lifecycle}</span>
                  <p>
                    {model.qualificationNote ??
                      "No measured install and device-fit evidence is available."}
                  </p>
                </article>
              ))}
            </div>
          ) : (
            <div className="blocked-state">
              <b>Local visual packs not qualified</b>
              <span>No download or enable control is shown yet.</span>
            </div>
          )}
          <div className="blocked-state">
            <b>Activation requires a complete fit result</b>
            <span>
              Game reserve + resident models + p99 workspace + safety margin
              must fit. Unknown measurements are treated as a conflict.
            </span>
          </div>
        </section>
      </div>
      <details className="advanced-profiles">
        <summary>
          <span>
            <b>Advanced profiles</b>
            <small>
              Named global, game, and character routes · swaps begin next turn
            </small>
          </span>
          <span className="badge">6 roles</span>
        </summary>
        <ProviderLoadoutEditor />
      </details>
      <LocalResourcePlanner models={models} nativeAvailable={nativeAvailable} />
    </div>
  );
}

function DiagnosticsPage({
  bootstrap,
  diagnostics,
  diagnosticsV2,
  diagnosticsExport,
  diagnosticsMatrix,
  diagnosticsSettings,
  busy,
  nativeAvailable,
  onRefresh,
  onExport,
  onSaveVerbosity,
  onNavigate,
  turnEvents,
}: {
  bootstrap: NativeBootstrapHealth;
  diagnostics: NativeDiagnosticSummary | null;
  diagnosticsV2: NativeDiagnosticsV2Snapshot | null;
  diagnosticsExport: NativeDiagnosticsExportResult | null;
  diagnosticsMatrix: NativeDiagnosticsMatrix | null;
  diagnosticsSettings: NativeDiagnosticsSettings | null;
  busy: boolean;
  nativeAvailable: boolean;
  onRefresh: () => void;
  onExport: () => void;
  onSaveVerbosity: (verbosity: NativeDiagnosticsSettings["verbosity"]) => void;
  onNavigate: (page: ProductPage) => void;
  turnEvents: NativeSimulationEvent[];
}) {
  const checks = diagnostics?.checks ?? [];
  const timingBasis = turnEvents.find((event) => event.type === "started");
  const completedTurn = turnEvents.find((event) => event.type === "completed");
  const stageTimings = completedTurn
    ? turnEvents.filter(
        (event): event is NativeSimulationEvent & { type: "stageCompleted" } =>
          event.type === "stageCompleted",
      )
    : [];
  const timingLabel = completedTurn
    ? completedTurn.turnExecution
      ? "Completed route evidence"
      : completedTurn.runtimeFixtureOnly
        ? "Completed runtime fixture"
        : "Completed · provider unproven"
    : timingBasis?.type === "started" &&
        timingBasis.measurementBasis === "pendingProviderEvidence"
      ? "Provider evidence pending"
      : timingBasis?.type === "started"
        ? timingBasis.measurementBasis
        : "No turn data";
  const runMatrixAction = (
    action: NativeDiagnosticsMatrix["checks"][number]["suggestedActions"][number],
  ) => {
    if (action.kind === "retry_check") {
      onRefresh();
      return;
    }
    const target = action.targetId ?? "";
    if (
      ["providers", "stt", "tts", "llm", "models"].includes(target) ||
      action.kind === "select_alternative_provider" ||
      action.kind === "reduce_local_model_load"
    ) {
      onNavigate("voice");
      return;
    }
    if (["game", "capture", "overlay"].includes(target)) {
      onNavigate("world");
      return;
    }
    if (target === "performance") {
      onRefresh();
      return;
    }
    onNavigate("settings");
  };
  return (
    <div className="page-stack">
      <header className="page-heading split">
        <div>
          <span className="eyebrow">Native truth, timestamped</span>
          <h1>Diagnostics</h1>
          <p>
            No illustrative CPU, GPU, latency, or provider values are rendered
            here.
          </p>
        </div>
        <div className="diagnostic-actions">
          <button
            className="secondary-action"
            onClick={onRefresh}
            disabled={busy || !nativeAvailable}
          >
            {busy
              ? "Refreshing…"
              : nativeAvailable
                ? "Refresh native checks"
                : "Native desktop required"}
          </button>
          <button
            className="quiet-button"
            onClick={onExport}
            disabled={busy || !nativeAvailable}
            aria-describedby="diagnostics-export-reason"
          >
            Export local diagnostics
          </button>
          <small id="diagnostics-export-reason">
            Explicit local export only · maximum 100 events · never uploaded
            automatically.
          </small>
        </div>
      </header>
      <div className="diagnostic-summary">
        <article>
          <span>Control bootstrap</span>
          <b>{bootstrap.kind}</b>
          <small>
            {bootstrap.kind === "snapshot"
              ? `${bootstrap.attempts} attempt${bootstrap.attempts === 1 ? "" : "s"}`
              : "No native measurement"}
          </small>
        </article>
        <article>
          <span>Evidence class</span>
          <b>
            {diagnostics?.measurements.currentResultsAreReleaseEvidence
              ? "Release"
              : "Not release"}
          </b>
          <small>
            {diagnostics?.measurements.reason ?? "No diagnostic summary"}
          </small>
        </article>
      </div>
      <section className="instrument-panel diagnostic-matrix-panel">
        <div className="panel-title">
          <div>
            <span className="eyebrow">Canonical 15-check matrix</span>
            <h2>Product readiness matrix</h2>
          </div>
          <span className="badge">
            {diagnosticsMatrix
              ? `${diagnosticsMatrix.checks.length} checks`
              : "Not loaded"}
          </span>
        </div>
        <p className="source-disclosure">
          Measured means the named native probe ran; it does not imply broader
          capability. Every missing producer remains an explicit unmeasured row
          instead of disappearing or inheriting a fixture value.
        </p>
        <label className="diagnostic-verbosity-control">
          <span>LOCAL EVENT DETAIL</span>
          <select
            aria-label="Diagnostics verbosity"
            value={diagnosticsSettings?.verbosity ?? ""}
            disabled={!nativeAvailable || busy || !diagnosticsSettings}
            onChange={(event) =>
              onSaveVerbosity(
                event.target.value as NativeDiagnosticsSettings["verbosity"],
              )
            }
          >
            {!diagnosticsSettings && <option value="">Not loaded</option>}
            <option value="essential">Essential · warnings/errors</option>
            <option value="standard">Standard · info and above</option>
            <option value="verbose">Verbose · all local events</option>
          </select>
          <small>
            Local structured-event retention only. It never enables content
            logging, credential capture, remote telemetry, or automatic upload.
          </small>
        </label>
        {!diagnosticsMatrix ? (
          <div className="empty-state">
            <b>No native matrix loaded</b>
            <p>
              Refresh native checks. Browser preview does not construct the 15
              rows or credential-presence state.
            </p>
          </div>
        ) : (
          <>
            <div className="diagnostic-credential-presence">
              <b>Credential presence only</b>
              {diagnosticsMatrix.credentialPresence.length ? (
                diagnosticsMatrix.credentialPresence.map((provider) => (
                  <span key={provider.providerId}>
                    {provider.providerId} · {provider.state} ·{" "}
                    {provider.provenance}
                  </span>
                ))
              ) : (
                <span>No provider credential references reported.</span>
              )}
            </div>
            <div className="diagnostic-matrix-list" role="table">
              {diagnosticsMatrix.checks.map((check) => (
                <article key={check.checkId} role="row">
                  <div>
                    <span>{check.category}</span>
                    <b>{check.checkId}</b>
                    <small>{check.summaryCode}</small>
                  </div>
                  <div>
                    <span
                      className={`badge ${check.status === "ok" ? "good" : check.status === "failed" ? "bad" : "wait"}`}
                    >
                      {check.status}
                    </span>
                    <b>{check.provenance}</b>
                    <small>
                      {check.observedAtUtc ?? "No measured timestamp"}
                    </small>
                  </div>
                  <p>{check.summary}</p>
                  <div className="diagnostic-matrix-actions">
                    {check.suggestedActions.map((action) => (
                      <button
                        key={action.actionId}
                        className="quiet-button"
                        onClick={() => runMatrixAction(action)}
                      >
                        {action.label}
                      </button>
                    ))}
                  </div>
                </article>
              ))}
            </div>
          </>
        )}
      </section>
      <section className="instrument-panel diagnostics-v2-panel">
        <div className="panel-title">
          <div>
            <span className="eyebrow">Bounded local event store</span>
            <h2>Diagnostics v2</h2>
          </div>
          <span className="badge">
            {diagnosticsV2
              ? `${diagnosticsV2.events.length} events`
              : "Not loaded"}
          </span>
        </div>
        {!diagnosticsV2 ? (
          <div className="empty-state">
            <b>No diagnostics-v2 snapshot loaded</b>
            <p>
              Refresh in the native desktop shell. Browser preview does not
              invent event, privacy, or recovery state.
            </p>
          </div>
        ) : (
          <>
            <dl className="facts spacious">
              <div>
                <dt>Local event detail</dt>
                <dd>{diagnosticsV2.verbosity}</dd>
              </div>
              <div>
                <dt>Remote telemetry</dt>
                <dd>{diagnosticsV2.privacy.remoteTelemetry}</dd>
              </div>
              <div>
                <dt>Automatic upload</dt>
                <dd>{diagnosticsV2.privacy.automaticUpload}</dd>
              </div>
              <div>
                <dt>Export initiation</dt>
                <dd>{diagnosticsV2.privacy.exportInitiation}</dd>
              </div>
              <div>
                <dt>Previous exit</dt>
                <dd>{diagnosticsV2.recovery.previousSession.status}</dd>
              </div>
              <div>
                <dt>Corrupt records skipped</dt>
                <dd>{diagnosticsV2.skippedCorruptRecords}</dd>
              </div>
              <div>
                <dt>Maximum local disk</dt>
                <dd>
                  {(diagnosticsV2.maximumDiskBytes / 1024).toFixed(0)} KiB
                </dd>
              </div>
            </dl>
            {diagnosticsV2.events.length === 0 ? (
              <p className="empty-copy">
                The bounded native event store is empty.
              </p>
            ) : (
              <ol className="diagnostics-event-list">
                {diagnosticsV2.events.slice(-20).map((stored) => (
                  <li key={`${stored.sessionId}-${stored.sequence}`}>
                    <span>{stored.sequence}</span>
                    <div>
                      <b>
                        {stored.event.component} · {stored.event.eventName}
                      </b>
                      <small>
                        {stored.event.status} · {stored.event.provenance} ·{" "}
                        {stored.event.severity}
                      </small>
                    </div>
                    <code>{stored.event.errorCode ?? "no error code"}</code>
                  </li>
                ))}
              </ol>
            )}
          </>
        )}
        {diagnosticsExport && (
          <p className="inline-status" role="status">
            Exported locally as {diagnosticsExport.fileName}:{" "}
            {diagnosticsExport.preview.eventCount} events ·{" "}
            {diagnosticsExport.preview.serializedBytes} bytes · SHA-256{" "}
            {diagnosticsExport.preview.sha256} · uploaded{" "}
            {diagnosticsExport.uploaded ? "yes" : "no"}. No directory path is
            exposed to the WebView.
          </p>
        )}
      </section>
      <div className="diagnostic-detail-grid">
        <section className="instrument-panel check-table">
          <div className="panel-title">
            <h2>Current checks</h2>
            <span className="mono">
              {diagnostics
                ? new Date(diagnostics.generatedAtEpochMs).toLocaleTimeString()
                : "not refreshed"}
            </span>
          </div>
          {checks.length === 0 ? (
            <div className="empty-state">
              <b>No native results yet</b>
              <p>
                Open the Windows desktop build and refresh. Browser preview
                deliberately invents nothing.
              </p>
            </div>
          ) : (
            <div role="table">
              {checks.map((check) => (
                <div className="check-row" role="row" key={check.id}>
                  <span className={`check-status ${check.status}`} />
                  <div>
                    <b>{check.title}</b>
                    <p>{check.detail}</p>
                    {check.remediation && <small>{check.remediation}</small>}
                  </div>
                  <code>{check.id}</code>
                </div>
              ))}
            </div>
          )}
        </section>
        <section className="instrument-panel turn-performance">
          <div className="panel-title">
            <div>
              <span className="eyebrow">Latest turn · event-reported only</span>
              <h2>Latency & degradation trace</h2>
            </div>
            <span className="badge wait">{timingLabel}</span>
          </div>
          {stageTimings.length === 0 ? (
            <div className="empty-state">
              <b>No stage timing events in this session</b>
              <p>
                {timingBasis?.type === "started" && !completedTurn
                  ? "A turn started, but completion evidence is still pending. Timing fields stay hidden until completion."
                  : "Run a turn first. The UI does not substitute illustrative latency bars when native events are absent."}
              </p>
            </div>
          ) : (
            <div className="stage-timing-list">
              {stageTimings.map((event) => (
                <article key={`${event.sequence}-${event.stage}`}>
                  <span>{event.stage}</span>
                  <b>{event.fixtureElapsedMs} ms</b>
                  <small>event field: fixtureElapsedMs</small>
                </article>
              ))}
              <p>
                Values labeled <code>fixtureElapsedMs</code> are runtime event
                fields, not a release benchmark. Provider retries and route
                changes are never inferred from timing alone.
              </p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function SettingsPage({
  audioOutputs,
  selectedAudioOutput,
  audioOutputBusy,
  audioOutputError,
  audioInputs,
  selectedAudioInput,
  audioInputBusy,
  audioInputError,
  onRefreshAudioOutputs,
  onSelectAudioOutput,
  onRefreshAudioInputs,
  onSelectAudioInput,
  gameProfileId,
  characterId,
  onProductPreferenceSnapshot,
  onSetup,
  onDiagnostics,
  onNavigate,
  nativeAvailable,
}: {
  audioOutputs: NativeAudioOutputSnapshot | null;
  selectedAudioOutput: NativeSelectedAudioOutput | null;
  audioOutputBusy: boolean;
  audioOutputError: string | null;
  audioInputs: NativeAudioInputSnapshot | null;
  selectedAudioInput: NativeSelectedAudioInput | null;
  audioInputBusy: boolean;
  audioInputError: string | null;
  onRefreshAudioOutputs: () => Promise<void>;
  onSelectAudioOutput: (selection: NativeAudioOutputSelection) => Promise<void>;
  onRefreshAudioInputs: () => Promise<void>;
  onSelectAudioInput: (selection: NativeAudioInputSelection) => Promise<void>;
  gameProfileId: string;
  characterId: string | null;
  onProductPreferenceSnapshot: (
    snapshot: NativeProductPreferenceSnapshot,
  ) => void;
  onSetup: () => void;
  onDiagnostics: () => void;
  onNavigate: (page: ProductPage) => void;
  nativeAvailable: boolean;
}) {
  const [guideQuery, setGuideQuery] = useState("");
  const [activeGuideId, setActiveGuideId] =
    useState<SupportGuideEntry["id"]>("setup");
  const normalizedGuideQuery = guideQuery.trim().toLowerCase();
  const filteredGuides = useMemo(
    () =>
      normalizedGuideQuery.length === 0
        ? SUPPORT_GUIDES
        : SUPPORT_GUIDES.filter((guide) =>
            [guide.title, guide.summary, guide.detail, ...guide.searchTerms]
              .join(" ")
              .toLowerCase()
              .includes(normalizedGuideQuery),
          ),
    [normalizedGuideQuery],
  );
  const activeGuide =
    SUPPORT_GUIDES.find((guide) => guide.id === activeGuideId) ??
    SUPPORT_GUIDES[0];
  const runGuideAction = (guide: SupportGuideEntry) => {
    if (guide.action.kind === "setup") {
      onSetup();
    } else if (guide.action.kind === "diagnostics") {
      onDiagnostics();
    } else {
      onNavigate(guide.action.page);
    }
  };
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">Persisted support only</span>
        <h1>Settings & guide</h1>
        <p>
          Every control below has a native persistence effect or a direct
          product destination.
        </p>
      </header>
      <div className="settings-layout">
        <ProductPreferencesWorkspace
          nativeAvailable={nativeAvailable}
          gameProfileId={gameProfileId}
          characterId={characterId}
          onSnapshot={onProductPreferenceSnapshot}
        />
        <section className="instrument-panel settings-output-panel">
          <div className="panel-title">
            <h2>Playback destination</h2>
            <span className="badge">Native broker</span>
          </div>
          <AudioOutputPicker
            nativeAvailable={nativeAvailable}
            outputs={audioOutputs}
            selected={selectedAudioOutput}
            busy={audioOutputBusy}
            error={audioOutputError}
            onRefresh={onRefreshAudioOutputs}
            onSelect={onSelectAudioOutput}
          />
          <AudioInputPicker
            nativeAvailable={nativeAvailable}
            inputs={audioInputs}
            selected={selectedAudioInput}
            busy={audioInputBusy}
            error={audioInputError}
            onRefresh={onRefreshAudioInputs}
            onSelect={onSelectAudioInput}
          />
        </section>
        <section className="instrument-panel guide-list">
          <div className="panel-title">
            <h2>Reviewed in-app guide</h2>
            <span className="badge good">{SUPPORT_GUIDES.length} topics</span>
          </div>
          <label className="guide-search">
            <span>Search the bundled guide</span>
            <input
              type="search"
              value={guideQuery}
              placeholder="Privacy, voice, game, overlay…"
              onChange={(event) => setGuideQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && filteredGuides[0]) {
                  event.preventDefault();
                  setActiveGuideId(filteredGuides[0].id);
                }
              }}
            />
          </label>
          <div className="guide-results" aria-label="Guide search results">
            {filteredGuides.length > 0 ? (
              filteredGuides.map((guide, index) => (
                <button
                  key={guide.id}
                  aria-pressed={activeGuide.id === guide.id}
                  onClick={() => setActiveGuideId(guide.id)}
                >
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  <div>
                    <b>{guide.title}</b>
                    <small>{guide.summary}</small>
                  </div>
                  <i>→</i>
                </button>
              ))
            ) : (
              <p className="guide-empty" role="status">
                No reviewed guide topic matches “{guideQuery}”. Try privacy,
                voice, game, overlay, setup, or diagnostics.
              </p>
            )}
          </div>
          <div
            className="guide-note guide-detail"
            aria-live="polite"
            aria-label="Selected guide topic"
          >
            <span className="eyebrow">
              Bundled review · {activeGuide.reviewedRevision}
            </span>
            <b>{activeGuide.title}</b>
            <p>{activeGuide.detail}</p>
            <button
              className="secondary-action"
              onClick={() => runGuideAction(activeGuide)}
            >
              {activeGuide.actionLabel}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}

function AudioOutputPicker({
  nativeAvailable,
  outputs,
  selected,
  busy,
  error,
  onRefresh,
  onSelect,
  compact = false,
}: {
  nativeAvailable: boolean;
  outputs: NativeAudioOutputSnapshot | null;
  selected: NativeSelectedAudioOutput | null;
  busy: boolean;
  error: string | null;
  onRefresh: () => Promise<void>;
  onSelect: (selection: NativeAudioOutputSelection) => Promise<void>;
  compact?: boolean;
}) {
  const selectedValue = selected
    ? selected.selection.mode === "systemDefault"
      ? "systemDefault"
      : `endpoint:${selected.selection.endpoint_id}`
    : "";
  const activeEndpoints = outputs?.endpoints.filter(
    (endpoint) => endpoint.state === "active",
  );
  return (
    <div
      className={`audio-output-picker ${compact ? "is-compact" : ""}`}
      aria-busy={busy}
    >
      <div>
        <b>Audio output</b>
        <small>
          Explicitly persist System default or one stable Windows endpoint. A
          first run has no implicit selection.
        </small>
      </div>
      <label>
        <span>OUTPUT ROUTE</span>
        <select
          aria-label="Audio output route"
          value={selectedValue}
          disabled={!nativeAvailable || busy || !outputs}
          onChange={(event) => {
            const value = event.target.value;
            if (value === "systemDefault") {
              void onSelect({ mode: "systemDefault" });
            } else if (value.startsWith("endpoint:")) {
              void onSelect({
                mode: "endpointId",
                endpoint_id: value.slice("endpoint:".length),
              });
            }
          }}
        >
          <option value="" disabled>
            {nativeAvailable
              ? outputs
                ? "Select an output…"
                : "Loading native outputs…"
              : "Native desktop required"}
          </option>
          <option value="systemDefault">System default (follow Windows)</option>
          {outputs?.endpoints.map((endpoint) => (
            <option
              key={`${endpoint.endpointId}-${endpoint.generation}`}
              value={`endpoint:${endpoint.endpointId}`}
              disabled={endpoint.state !== "active"}
            >
              {endpoint.friendlyName}
              {endpoint.systemDefault ? " · current system default" : ""}
              {endpoint.state !== "active" ? ` · ${endpoint.state}` : ""}
            </option>
          ))}
        </select>
      </label>
      <button
        className="secondary-action"
        disabled={!nativeAvailable || busy}
        onClick={() => void onRefresh()}
      >
        {busy ? "Refreshing outputs…" : "Refresh audio outputs"}
      </button>
      <p className={error ? "audio-output-picker__error" : ""} role="status">
        {error
          ? `Selection not changed: ${error}`
          : selected
            ? `Saved ${selected.selection.mode === "systemDefault" ? "System default" : "exact endpoint"}: ${selected.resolved.friendlyName} · generation ${selected.resolved.generation}`
            : nativeAvailable
              ? activeEndpoints?.length
                ? "Selection required before audible output."
                : outputs
                  ? "No active output endpoint is available."
                  : "Reading native output state…"
              : "Browser preview cannot enumerate or persist Windows audio outputs."}
      </p>
    </div>
  );
}

function AudioInputPicker({
  nativeAvailable,
  inputs,
  selected,
  busy,
  error,
  onRefresh,
  onSelect,
  compact = false,
}: {
  nativeAvailable: boolean;
  inputs: NativeAudioInputSnapshot | null;
  selected: NativeSelectedAudioInput | null;
  busy: boolean;
  error: string | null;
  onRefresh: () => Promise<void>;
  onSelect: (selection: NativeAudioInputSelection) => Promise<void>;
  compact?: boolean;
}) {
  const selectedValue = selected
    ? selected.selection.mode === "systemDefault"
      ? "systemDefault"
      : `endpoint:${selected.selection.endpoint_id}`
    : "";
  const activeEndpoints = inputs?.endpoints.filter(
    (endpoint) => endpoint.state === "active",
  );
  return (
    <div
      className={`audio-output-picker audio-input-picker ${compact ? "is-compact" : ""}`}
      aria-busy={busy}
    >
      <div>
        <b>Audio input</b>
        <small>
          Explicitly persist System default or one stable Windows microphone
          endpoint. This selects routing only.
        </small>
      </div>
      <label>
        <span>INPUT ROUTE</span>
        <select
          aria-label="Audio input route"
          value={selectedValue}
          disabled={!nativeAvailable || busy || !inputs}
          onChange={(event) => {
            const value = event.target.value;
            if (value === "systemDefault") {
              void onSelect({ mode: "systemDefault" });
            } else if (value.startsWith("endpoint:")) {
              void onSelect({
                mode: "endpointId",
                endpoint_id: value.slice("endpoint:".length),
              });
            }
          }}
        >
          <option value="" disabled>
            {nativeAvailable
              ? inputs
                ? "Select an input…"
                : "Loading native inputs…"
              : "Native desktop required"}
          </option>
          <option value="systemDefault">System default (follow Windows)</option>
          {inputs?.endpoints.map((endpoint) => (
            <option
              key={`${endpoint.endpointId}-${endpoint.generation}`}
              value={`endpoint:${endpoint.endpointId}`}
              disabled={endpoint.state !== "active"}
            >
              {endpoint.friendlyName}
              {endpoint.systemDefault ? " · current system default" : ""}
              {endpoint.state !== "active" ? ` · ${endpoint.state}` : ""}
            </option>
          ))}
        </select>
      </label>
      <button
        className="secondary-action"
        disabled={!nativeAvailable || busy}
        onClick={() => void onRefresh()}
      >
        {busy ? "Refreshing inputs…" : "Refresh audio inputs"}
      </button>
      <p className={error ? "audio-output-picker__error" : ""} role="status">
        {error
          ? `Selection not changed: ${error}`
          : selected
            ? `Saved ${selected.selection.mode === "systemDefault" ? "System default" : "exact endpoint"}: ${selected.resolved.friendlyName} · generation ${selected.resolved.generation}`
            : nativeAvailable
              ? activeEndpoints?.length
                ? "Selection required before native push-to-talk can use an endpoint."
                : inputs
                  ? "No active input endpoint is available."
                  : "Reading native input state…"
              : "Browser preview cannot enumerate or persist Windows audio inputs."}
      </p>
      <dl
        className="audio-input-evidence"
        aria-label="Microphone evidence state"
      >
        <div>
          <dt>Signal level</dt>
          <dd>Not measured</dd>
        </div>
        <div>
          <dt>Noise floor</dt>
          <dd>Not measured</dd>
        </div>
        <div>
          <dt>Captured frames</dt>
          <dd>Not measured</dd>
        </div>
        <div>
          <dt>Permission</dt>
          <dd>Not measured</dd>
        </div>
      </dl>
      <small className="audio-input-proof-note">
        Endpoint selection does not prove microphone allocation, physical PTT,
        audio frames, speech recognition, or a final transcript. Session PTT
        becomes turn input only after a broker-authoritative receipt is returned
        and explicitly consumed once by the native turn.
      </small>
    </div>
  );
}

function OnboardingOverlay({
  step,
  bootstrap,
  providerPresent,
  nvidiaPresent,
  loadout,
  credentialStates,
  nativeAvailable,
  audioOutputs,
  selectedAudioOutput,
  audioOutputBusy,
  audioOutputError,
  audioInputs,
  selectedAudioInput,
  audioInputBusy,
  audioInputError,
  captureAvailable,
  captureProof,
  deliveredTurn,
  running,
  preferences,
  feedback,
  setPreferences,
  onBack,
  onNext,
  onCapture,
  onProviderAction,
  onRefreshAudioOutputs,
  onSelectAudioOutput,
  onRefreshAudioInputs,
  onSelectAudioInput,
  selectedSttReadiness,
  selectedSttCapture,
  selectedSttStatus,
  selectedSttTerminal,
  selectedSttReceiptDisposition,
  selectedSttScope,
  selectedSttBusy,
  selectedSttError,
  onStartSelectedStt,
  onCancelSelectedStt,
  onRetrySelectedStt,
  onUseTypedInput,
  onRun,
  onClose,
}: {
  step: number;
  bootstrap: NativeBootstrapHealth;
  providerPresent: boolean;
  nvidiaPresent: boolean;
  loadout: ProviderLoadout;
  credentialStates: NativeProviderCredentialSummary[];
  nativeAvailable: boolean;
  audioOutputs: NativeAudioOutputSnapshot | null;
  selectedAudioOutput: NativeSelectedAudioOutput | null;
  audioOutputBusy: boolean;
  audioOutputError: string | null;
  audioInputs: NativeAudioInputSnapshot | null;
  selectedAudioInput: NativeSelectedAudioInput | null;
  audioInputBusy: boolean;
  audioInputError: string | null;
  captureAvailable: boolean;
  captureProof: SyntheticCaptureProof | null;
  deliveredTurn: (NativeSimulationEvent & { type: "completed" }) | null;
  running: boolean;
  preferences: AppPreferences;
  feedback: string | null;
  setPreferences: (
    update: AppPreferences | ((current: AppPreferences) => AppPreferences),
  ) => void;
  onBack: () => void;
  onNext: () => void;
  onCapture: () => void;
  onProviderAction: (
    providerId: "elevenlabs" | "nvidia-nim",
    action: "save" | "validate",
  ) => void;
  onRefreshAudioOutputs: () => Promise<void>;
  onSelectAudioOutput: (selection: NativeAudioOutputSelection) => Promise<void>;
  onRefreshAudioInputs: () => Promise<void>;
  onSelectAudioInput: (selection: NativeAudioInputSelection) => Promise<void>;
  selectedSttReadiness: SelectedSttReadiness;
  selectedSttCapture: SelectedSttCapturing | null;
  selectedSttStatus: SelectedSttStatus | null;
  selectedSttTerminal: SelectedSttTerminal | null;
  selectedSttReceiptDisposition: "ready" | "submitted" | "rejected" | null;
  selectedSttScope: SelectedSttScope | null;
  selectedSttBusy: boolean;
  selectedSttError: string | null;
  onStartSelectedStt: () => void;
  onCancelSelectedStt: () => void;
  onRetrySelectedStt: (generation: number) => void;
  onUseTypedInput: () => void;
  onRun: () => void;
  onClose?: () => void;
}) {
  const onboardingRoles = ROLE_ORDER.map((role) => {
    const route = loadout.routes[role];
    const routeProvider = providerFor(role, route.providerId);
    const credentialId =
      route.providerId === "nvidia-nim-magpie"
        ? "nvidia-nim"
        : route.providerId;
    const credential = credentialStates.find(
      (item) => item.providerId === credentialId,
    );
    const skipped = routeProvider.execution === "Off";
    const configuredInactive =
      role === "stt" || role === "embeddings" || role === "vision";
    const ready =
      skipped ||
      routeProvider.execution === "Local" ||
      credential?.status === "present";
    return {
      role,
      route,
      routeProvider,
      credentialId,
      credential,
      skipped,
      configuredInactive,
      ready,
    };
  });
  const preferredCredentialProvider = onboardingRoles.some(
    (item) => item.credentialId === "nvidia-nim",
  )
    ? ("nvidia-nim" as const)
    : ("elevenlabs" as const);
  const preferredCredentialPresent =
    preferredCredentialProvider === "nvidia-nim"
      ? nvidiaPresent
      : providerPresent;
  return (
    <div className="setup-scrim" role="presentation">
      <section
        className="setup-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="setup-title"
      >
        <header className="setup-header">
          <div>
            <span className="brand-mark">N2</span>
            <span>
              <b>Guided setup</b>
              <small>Real state only</small>
            </span>
          </div>
          {onClose && (
            <button onClick={onClose} aria-label="Close setup">
              ×
            </button>
          )}
        </header>
        <ol className="setup-progress">
          {ONBOARDING_STEPS.map((item, index) => (
            <li
              key={item.id}
              className={
                index === step ? "current" : index < step ? "complete" : ""
              }
            >
              <span>{index < step ? "✓" : index + 1}</span>
              <b>{item.label}</b>
            </li>
          ))}
        </ol>
        <div className="setup-content">
          {step === 0 && (
            <>
              <span className="eyebrow">01 / system</span>
              <h1 id="setup-title">Prove the native boundary</h1>
              <p>
                NPC 2.0 needs its authenticated runtime and media broker before
                it can select a window or deliver audio.
              </p>
              <div className="setup-check">
                <span
                  className={
                    bootstrap.kind === "snapshot"
                      ? "check-status passed"
                      : "check-status warning"
                  }
                />
                <div>
                  <b>{runtimeSummary(bootstrap).label}</b>
                  <small>
                    {bootstrap.kind === "snapshot"
                      ? bootstrap.snapshot.runtime.detail
                      : "Open the native desktop shell for a real check."}
                  </small>
                </div>
              </div>
            </>
          )}
          {step === 1 && (
            <>
              <span className="eyebrow">02 / world</span>
              <h1 id="setup-title">Select the safe test world</h1>
              <p>
                The first run uses only the task-owned Eclipse Harbor window.
                Online games and detected anti-cheat remain blocked.
              </p>
              <div className="setup-selection selected">
                <span>EH</span>
                <div>
                  <b>Eclipse Harbor</b>
                  <small>
                    Synthetic GUI target · single-player · Mara Venn
                  </small>
                </div>
                <i>{captureProof ? `PID ${captureProof.pid}` : "Configured"}</i>
              </div>
              <button
                className="secondary-action setup-inline-action"
                disabled={!captureAvailable}
                onClick={onCapture}
              >
                {captureProof
                  ? "Reselect running synthetic target"
                  : captureAvailable
                    ? "Select running synthetic target"
                    : "Native debug capture unavailable"}
              </button>
            </>
          )}
          {step === 2 && (
            <>
              <span className="eyebrow">03 / voice</span>
              <h1 id="setup-title">Start API-first</h1>
              <p>
                Hosted conversation models leave the game GPU free for play and
                optional local mouth motion. Advanced mixed-local profiles are
                available after setup, once every model has a measured fit.
              </p>
              <div className="setup-selection selected">
                <span>API</span>
                <div>
                  <b>{loadout.name}</b>
                  <small>
                    Selected six-role preference · runtime consumption is proven
                    only after a completed turn
                  </small>
                </div>
                <i>{loadout.scope}</i>
              </div>
              {preferences.execution !== "cloud" && (
                <button
                  className="secondary-action setup-inline-action"
                  onClick={() =>
                    setPreferences((current) => ({
                      ...current,
                      execution: "cloud",
                      localOnly: false,
                    }))
                  }
                >
                  Use cloud execution preference
                </button>
              )}
              <div
                className="onboarding-role-grid"
                aria-label="Selected loadout readiness"
              >
                {onboardingRoles.map((item) => (
                  <div className="setup-check" key={item.role}>
                    <span
                      className={
                        item.ready
                          ? "check-status passed"
                          : "check-status warning"
                      }
                    />
                    <div>
                      <b>
                        {ROLE_META[item.role].short} · {item.routeProvider.name}
                      </b>
                      <small>
                        {item.skipped
                          ? "Skipped by this selected route."
                          : item.configuredInactive
                            ? `${item.ready ? "Configured" : "Blocked"}, but inactive in the typed-turn vertical slice.`
                            : item.ready
                              ? item.routeProvider.execution === "Local"
                                ? "Configured locally; runtime evidence is still required."
                                : "Required native vault reference is present."
                              : `Blocked: ${item.credential?.detail ?? `credential state for ${item.credentialId} is unavailable`}`}
                      </small>
                    </div>
                  </div>
                ))}
              </div>
              <button
                className="secondary-action setup-inline-action"
                disabled={!nativeAvailable}
                onClick={() =>
                  onProviderAction(
                    preferredCredentialProvider,
                    preferredCredentialPresent ? "validate" : "save",
                  )
                }
              >
                {preferredCredentialPresent
                  ? `Validate ${preferredCredentialProvider} vault binding`
                  : `Add ${preferredCredentialProvider} credential securely`}
              </button>
              <AudioOutputPicker
                compact
                nativeAvailable={nativeAvailable}
                outputs={audioOutputs}
                selected={selectedAudioOutput}
                busy={audioOutputBusy}
                error={audioOutputError}
                onRefresh={onRefreshAudioOutputs}
                onSelect={onSelectAudioOutput}
              />
              {preferences.ptt && (
                <AudioInputPicker
                  compact
                  nativeAvailable={nativeAvailable}
                  inputs={audioInputs}
                  selected={selectedAudioInput}
                  busy={audioInputBusy}
                  error={audioInputError}
                  onRefresh={onRefreshAudioInputs}
                  onSelect={onSelectAudioInput}
                />
              )}
              {!selectedAudioOutput && (
                <p className="setup-blocked-reason" role="status">
                  Select and persist an audio output before continuing. The app
                  does not silently assume the Windows default endpoint.
                </p>
              )}
              {preferences.ptt && !selectedAudioInput && (
                <p className="setup-blocked-reason" role="status">
                  Push-to-talk is selected, so persist an audio input before
                  continuing. Input selection does not prove captured frames or
                  a transcript.
                </p>
              )}
            </>
          )}
          {step === 3 && (
            <>
              <span className="eyebrow">04 / test</span>
              <h1 id="setup-title">Prove one delivered turn</h1>
              <p>
                A live provider transcript stays inside the native runtime. The
                explicit next action sends only its opaque receipt ID and
                generation for one-time consumption, or you can use a typed
                setup turn.
              </p>
              {preferences.ptt && (
                <>
                  <SelectedSttControl
                    compact
                    readiness={selectedSttReadiness}
                    capture={selectedSttCapture}
                    status={selectedSttStatus}
                    terminal={selectedSttTerminal}
                    receiptDisposition={selectedSttReceiptDisposition}
                    scope={selectedSttScope}
                    busy={selectedSttBusy}
                    error={selectedSttError}
                    onStart={onStartSelectedStt}
                    onCancel={onCancelSelectedStt}
                    onRetry={onRetrySelectedStt}
                  />
                  <button
                    className="quiet-button setup-inline-action"
                    onClick={onUseTypedInput}
                  >
                    Use a typed setup turn instead
                  </button>
                  <small className="control-reason">
                    Captured STT text never enters the WebView. Expired,
                    consumed, replayed, cancelled, or failed receipts require a
                    new explicit capture.
                  </small>
                </>
              )}
              <div className="rehearsal-card">
                <span
                  className={
                    deliveredTurn ? "pulse-ring complete" : "pulse-ring"
                  }
                >
                  {deliveredTurn ? "✓" : preferences.ptt ? "PTT" : "TXT"}
                </span>
                <div>
                  <b>
                    {deliveredTurn
                      ? "Runtime completion received"
                      : running
                        ? "Turn in progress"
                        : "Bounded lighthouse rehearsal"}
                  </b>
                  <small>
                    {deliveredTurn
                      ? hasReceiptBackedAudio(deliveredTurn)
                        ? "Live TTS, nonzero submission, and endpoint-drain receipts were reported."
                        : deliveredTurn.runtimeFixtureOnly
                          ? "Deterministic delivery completed; live providers and audible speech were not claimed."
                          : "Runtime text completed; this event did not prove audio submission and drain."
                      : "Run the selected route and inspect its delivered-only result."}
                  </small>
                </div>
              </div>
              <button
                className="secondary-action setup-inline-action"
                disabled={
                  !nativeAvailable ||
                  running ||
                  (preferences.ptt &&
                    (selectedSttTerminal?.status !== "transcriptReady" ||
                      selectedSttReceiptDisposition !== "ready"))
                }
                onClick={onRun}
              >
                {running
                  ? "Turn in progress…"
                  : deliveredTurn
                    ? "Run the turn again"
                    : preferences.ptt
                      ? selectedSttTerminal?.status === "transcriptReady" &&
                        selectedSttReceiptDisposition === "ready"
                        ? "Send receipt-backed PTT turn"
                        : selectedSttReceiptDisposition === "submitted" ||
                            selectedSttReceiptDisposition === "rejected"
                          ? "Receipt spent — capture again"
                          : "Live PTT receipt required"
                      : "Run bounded typed turn"}
              </button>
            </>
          )}
          {feedback && (
            <p className="setup-feedback" role="status" aria-live="polite">
              {feedback}
            </p>
          )}
        </div>
        <footer className="setup-footer">
          <div>
            <b>{ONBOARDING_STEPS[step].label}</b>
            <small>
              Step {step + 1} of {ONBOARDING_STEPS.length}
            </small>
          </div>
          <div>
            {step > 0 && (
              <button className="quiet-button" onClick={onBack}>
                Back
              </button>
            )}
            <button
              className="primary-action small"
              onClick={onNext}
              disabled={
                (step === 2 &&
                  nativeAvailable &&
                  (!selectedAudioOutput ||
                    (preferences.ptt && !selectedAudioInput))) ||
                (step === ONBOARDING_STEPS.length - 1 &&
                  nativeAvailable &&
                  !deliveredTurn)
              }
            >
              {step === ONBOARDING_STEPS.length - 1
                ? "Finish setup"
                : "Continue"}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
