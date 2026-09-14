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
  deleteProviderCredential,
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
  saveProductPreferences,
  saveOnboarding,
  saveDiagnosticsSettings,
  selectAudioInput,
  selectAudioOutput,
  startNativeSimulation,
  syntheticReplayCaptureAvailability,
  syntheticReviewTargetAvailability,
  prepareSyntheticReviewTarget,
  verifySelectedGameCapture,
  type SyntheticReplayCaptureAvailability,
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
  type NativeProductPreferenceScope,
  type NativeScopedProductPreferences,
  type NativeSimulationEvent,
  type NativeTurnExecutionEvidence,
  type OnboardingSnapshot,
} from "./tauriBridge";
import { ProviderLoadoutEditor } from "./ProviderLoadoutEditor";
import { ProviderAccounts, type AccountAction } from "./ProviderAccounts";
import { WorkspaceSections } from "./WorkspaceSections";
import { Icon, type IconName } from "./icons";
import {
  nativeSnapshot as readNativeLoadouts,
  fromNativeSnapshot,
} from "./providerLoadoutBridge";
import { SetupSystemCheck } from "./SetupSystemCheck";
import { TestGameLauncher } from "./TestGameLauncher";
import { ContentPackWorkspace } from "./ContentPackWorkspace";
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

const NAV: Array<{ id: ProductPage; label: string; icon: IconName }> = [
  { id: "session", label: "Channel", icon: "conversation" },
  { id: "world", label: "Games", icon: "games" },
  { id: "voice", label: "Loadout", icon: "headphones" },
  { id: "settings", label: "Settings", icon: "settings" },
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
  if (requested === "diagnostics") return "diagnostics";
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

function hasSpokenSetupTurn(
  completed: (NativeSimulationEvent & { type: "completed" }) | null,
  subtitles: boolean,
) {
  return Boolean(
    completed &&
      !completed.runtimeFixtureOnly &&
      completed.turnExecution?.success.llmProviderLive &&
      hasReceiptBackedAudio(completed) &&
      (!subtitles || hasReceiptBackedSubtitle(completed)),
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

function productPreferenceScopeKey(scope: NativeProductPreferenceScope) {
  if (scope.kind === "global") return "global";
  if (scope.kind === "game") return `game:${scope.gameProfileId}`;
  return `character:${scope.gameProfileId}/${scope.characterId}`;
}

function sameProductPreferenceScope(
  left: NativeProductPreferenceScope,
  right: NativeProductPreferenceScope,
) {
  return productPreferenceScopeKey(left) === productPreferenceScopeKey(right);
}

function materialProductPreferenceKey(
  snapshot: NativeProductPreferenceSnapshot,
) {
  const effective = snapshot.effective;
  return JSON.stringify({
    scope: productPreferenceScopeKey(effective.scope),
    executionPreset: effective.executionPreset.value,
    performancePreset: effective.performancePreset.value,
    verbosity: effective.verbosity.value,
    creativity: effective.creativity.value,
    responseLength: effective.responseLength.value,
    interruptionMode: effective.interruptionMode.value,
    inputMode: effective.inputMode.value,
    subtitles: effective.subtitles.value,
    overlay: effective.overlay.value,
    memory: effective.memory.value,
    emotion: effective.emotion.value,
    vision: effective.vision.value,
    webcamPresence: effective.webcamPresence.value,
    egress: {
      transcript: effective.egress.transcript.value,
      microphoneAudio: effective.egress.microphoneAudio.value,
      capturedGameImage: effective.egress.capturedGameImage.value,
      localMemoryContext: effective.egress.localMemoryContext.value,
    },
    automaticProviderFallback: effective.automaticProviderFallback,
  });
}

export function ProductConsole() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const [page, setPage] = useState<ProductPage>(initialPage);
  const [voiceInitialSection, setVoiceInitialSection] = useState("loadout");
  const [bootstrap, setBootstrap] = useState<NativeBootstrapHealth>(
    LOADING_NATIVE_BOOTSTRAP,
  );
  const [preferences, setPreferences] =
    useState<AppPreferences>(DEFAULT_PREFERENCES);
  const [nativeLoadouts, setNativeLoadouts] = useState<
    ProviderLoadout[] | null
  >(null);
  const configurationChangedAt = useRef(0);
  const [configurationRevision, setConfigurationRevision] = useState(0);
  const configurationRevisionRef = useRef(0);
  const markConfigurationChanged = useCallback(() => {
    configurationRevisionRef.current += 1;
    setConfigurationRevision(configurationRevisionRef.current);
    configurationChangedAt.current = Math.max(
      Date.now() + 1,
      configurationChangedAt.current + 1,
    );
  }, []);
  const nativeLoadoutsRef = useRef<ProviderLoadout[] | null>(null);
  const acceptConfiguredLoadouts = useCallback(
    (value: ProviderLoadout[]) => {
      if (JSON.stringify(nativeLoadoutsRef.current) !== JSON.stringify(value)) {
        nativeLoadoutsRef.current = value;
        setNativeLoadouts(value);
        markConfigurationChanged();
      }
    },
    [markConfigurationChanged],
  );
  const [setupOpen, setSetupOpen] = useState(query.get("onboarding") === "1");
  const [setupStep, setSetupStep] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [turnEvents, setTurnEvents] = useState<NativeSimulationEvent[]>([]);
  const [deliveredTurn, setDeliveredTurn] = useState<
    (NativeSimulationEvent & { type: "completed" }) | null
  >(null);
  const turnRequestEpochRef = useRef(0);
  const [selectedGameProfileId, setSelectedGameProfileId] =
    useState("cyberpunk-2077");
  const [selectedCharacter, setSelectedCharacter] =
    useState<NativeCharacterInspection | null>(null);
  const [selectedTarget, setSelectedTarget] =
    useState<NativeGameTargetSelection | null>(null);
  const [turnWorldMode, setTurnWorldMode] = useState<
    "syntheticReview" | "selectedWorld"
  >("syntheticReview");
  const explicitWorldModeChoice = useRef(false);
  const [turnInputMode, setTurnInputMode] = useState<"typed" | "ptt">(
    DEFAULT_PREFERENCES.ptt ? "ptt" : "typed",
  );
  const [turnPrompt, setTurnPrompt] = useState(
    "What has this city been like for you lately?",
  );
  const [turnError, setTurnError] = useState<string | null>(null);
  const [turnRoute, setTurnRoute] = useState<{
    loadout: ProviderLoadout;
    capturedAtEpochMs: number;
    configurationChangedAt: number;
    configurationRevision: number;
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
  const [captureBusy, setCaptureBusy] = useState(false);
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

  useEffect(
    () => () => {
      turnRequestEpochRef.current += 1;
    },
    [],
  );

  const currentSttScope = useMemo<SelectedSttScope>(
    () =>
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
          },
    [selectedCharacter, turnWorldMode],
  );
  const currentProductPreferenceScope = useMemo<NativeProductPreferenceScope>(
    () => ({
      kind: "character",
      gameProfileId: currentSttScope.gameProfileId,
      characterId: currentSttScope.characterId,
    }),
    [currentSttScope.characterId, currentSttScope.gameProfileId],
  );
  const currentProductPreferenceScopeKey = productPreferenceScopeKey(
    currentProductPreferenceScope,
  );
  const sessionConfigurationIdentityKey = useMemo(
    () =>
      JSON.stringify({
        worldMode: turnWorldMode,
        gameProfileId: selectedGameProfileId,
        characterScope: currentProductPreferenceScopeKey,
        target: selectedTarget
          ? {
              gameProfileId: selectedTarget.gameProfileId,
              processId: selectedTarget.target.processId,
              nativeWindow: selectedTarget.target.nativeWindow,
              executablePathSha256: selectedTarget.target.executablePathSha256,
              captureAuthorized: selectedTarget.captureAuthorized,
              safetyState: selectedTarget.safetyState,
            }
          : null,
        inputMode: turnInputMode,
        preferences: {
          execution: preferences.execution,
          performance: preferences.performance,
          subtitles: preferences.subtitles,
          ptt: preferences.ptt,
          localOnly: preferences.localOnly,
          screenPresence: preferences.screenPresence,
        },
      }),
    [
      currentProductPreferenceScopeKey,
      preferences.execution,
      preferences.localOnly,
      preferences.performance,
      preferences.ptt,
      preferences.screenPresence,
      preferences.subtitles,
      selectedGameProfileId,
      selectedTarget,
      turnInputMode,
      turnWorldMode,
    ],
  );
  const observedSessionConfigurationRef = useRef<string | null>(null);
  const currentProductPreferenceScopeRef = useRef(
    currentProductPreferenceScope,
  );
  currentProductPreferenceScopeRef.current = currentProductPreferenceScope;
  const [sessionPreferenceSnapshot, setSessionPreferenceSnapshot] =
    useState<NativeProductPreferenceSnapshot | null>(null);
  const sessionPreferenceSnapshotRef =
    useRef<NativeProductPreferenceSnapshot | null>(null);
  const [sessionPreferenceBusy, setSessionPreferenceBusy] = useState<
    "load" | "save" | null
  >(null);
  const [sessionPreferenceError, setSessionPreferenceError] = useState<
    string | null
  >(null);
  const sessionPreferenceSequence = useRef(0);
  const appliedSessionPreferenceRef = useRef<{
    materialKey: string;
    scopeKey: string;
    inputMode: "ptt" | "vad";
  } | null>(null);

  const applyNativeProductPreferences = useCallback(
    (productPreferences: NativeProductPreferenceSnapshot) => {
      const requestedScope = currentProductPreferenceScopeRef.current;
      if (
        !sameProductPreferenceScope(
          productPreferences.effective.scope,
          requestedScope,
        )
      ) {
        return false;
      }
      const effective = productPreferences.effective;
      const scopeKey = productPreferenceScopeKey(effective.scope);
      const materialKey = materialProductPreferenceKey(productPreferences);
      const previouslyApplied = appliedSessionPreferenceRef.current;
      const materialChanged = previouslyApplied?.materialKey !== materialKey;
      const inputPreferenceChanged =
        !previouslyApplied ||
        previouslyApplied.scopeKey !== scopeKey ||
        previouslyApplied.inputMode !== effective.inputMode.value;

      sessionPreferenceSnapshotRef.current = productPreferences;
      setSessionPreferenceSnapshot(productPreferences);
      if (materialChanged) {
        setPreferences((current) => {
          const next: AppPreferences = {
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
          };
          return JSON.stringify(current) === JSON.stringify(next)
            ? current
            : next;
        });
        markConfigurationChanged();
      }
      if (inputPreferenceChanged) {
        setTurnInputMode(effective.inputMode.value === "ptt" ? "ptt" : "typed");
      }
      appliedSessionPreferenceRef.current = {
        materialKey,
        scopeKey,
        inputMode: effective.inputMode.value,
      };
      return true;
    },
    [markConfigurationChanged],
  );

  const refreshCurrentProductPreferences = useCallback(async () => {
    const sequence = ++sessionPreferenceSequence.current;
    const scope = currentProductPreferenceScopeRef.current;
    setSessionPreferenceBusy("load");
    setSessionPreferenceError(null);
    try {
      const next = await readProductPreferences(scope);
      if (sequence !== sessionPreferenceSequence.current) return null;
      if (!next) {
        throw new Error("Native product preferences returned no snapshot.");
      }
      if (!applyNativeProductPreferences(next)) {
        throw new Error(
          "Native product preferences returned a different character scope.",
        );
      }
      return next;
    } catch (error) {
      if (sequence !== sessionPreferenceSequence.current) return null;
      const message =
        error instanceof Error
          ? error.message
          : "Native product preferences could not be loaded.";
      setSessionPreferenceError(message);
      return null;
    } finally {
      if (sequence === sessionPreferenceSequence.current) {
        setSessionPreferenceBusy(null);
      }
    }
  }, [applyNativeProductPreferences]);

  const acceptPreferenceWorkspaceSnapshot = useCallback(
    (next: NativeProductPreferenceSnapshot) => {
      if (!applyNativeProductPreferences(next)) {
        void refreshCurrentProductPreferences();
      }
    },
    [applyNativeProductPreferences, refreshCurrentProductPreferences],
  );

  const saveSessionSubtitles = useCallback(
    async (enabled: boolean) => {
      if (
        bootstrap.kind !== "snapshot" ||
        !bootstrap.snapshot.runtime.connected
      ) {
        setSessionPreferenceError(
          "Open the native desktop app to change subtitles.",
        );
        return;
      }
      const sequence = ++sessionPreferenceSequence.current;
      const scope = currentProductPreferenceScopeRef.current;
      setSessionPreferenceBusy("save");
      setSessionPreferenceError(null);
      try {
        let base = sessionPreferenceSnapshotRef.current;
        if (!base || !sameProductPreferenceScope(base.effective.scope, scope)) {
          base = await readProductPreferences(scope);
        }
        if (sequence !== sessionPreferenceSequence.current) return;
        if (!base) {
          throw new Error("Native product preferences returned no snapshot.");
        }
        if (!sameProductPreferenceScope(base.effective.scope, scope)) {
          throw new Error(
            "Native product preferences returned a different character scope.",
          );
        }
        const currentEntry = base.entries.find((candidate) =>
          sameProductPreferenceScope(candidate.scope, scope),
        );
        const entry: NativeScopedProductPreferences = currentEntry
          ? {
              ...currentEntry,
              scope,
              overrides: { ...currentEntry.overrides, subtitles: enabled },
            }
          : { scope, overrides: { subtitles: enabled } };
        const next = await saveProductPreferences(base.revision, entry);
        if (sequence !== sessionPreferenceSequence.current) return;
        if (!next) {
          throw new Error(
            "Native product preferences returned no saved subtitle state.",
          );
        }
        if (!applyNativeProductPreferences(next)) {
          throw new Error(
            "Saved subtitle preferences did not resolve the current character.",
          );
        }
        setNotice(
          `Subtitles ${enabled ? "enabled" : "hidden"} for ${currentSttScope.characterLabel}.`,
        );
      } catch (error) {
        if (sequence !== sessionPreferenceSequence.current) return;
        setSessionPreferenceError(
          error instanceof Error
            ? error.message
            : "Subtitles could not be saved for this character.",
        );
      } finally {
        if (sequence === sessionPreferenceSequence.current) {
          setSessionPreferenceBusy(null);
        }
      }
    },
    [applyNativeProductPreferences, bootstrap, currentSttScope.characterLabel],
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

  useEffect(() => {
    if (bootstrap.kind !== "snapshot") return;
    let current = true;
    void readNativeLoadouts()
      .then((value) => {
        if (current && value)
          acceptConfiguredLoadouts(fromNativeSnapshot(value));
      })
      .catch(() => {
        if (current) setNativeLoadouts(null);
      });
    return () => {
      current = false;
    };
  }, [bootstrap.kind, acceptConfiguredLoadouts]);

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
        markConfigurationChanged();
        setNotice(
          `Saved exact endpoint: ${selected.resolved.friendlyName}. Future receipts must identify the drained endpoint.`,
        );
        const catalog = await enumerateAudioOutputs();
        if (catalog) setAudioOutputs(catalog);
      } catch (error) {
        setAudioOutputError(
          error instanceof Error
            ? `Selection not changed: ${error.message}`
            : "The selected audio output could not be persisted.",
        );
      } finally {
        setAudioOutputBusy(false);
      }
    },
    [markConfigurationChanged],
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
        markConfigurationChanged();
        setNotice(
          `Saved exact endpoint: ${selected.resolved.friendlyName}. Endpoint routing is configured; microphone frames, signal quality, and a transcript are not yet proven.`,
        );
        const catalog = await enumerateAudioInputs();
        if (catalog) setAudioInputs(catalog);
      } catch (error) {
        setAudioInputError(
          error instanceof Error
            ? `Selection not changed: ${error.message}`
            : "The selected audio input could not be persisted.",
        );
      } finally {
        setAudioInputBusy(false);
      }
    },
    [markConfigurationChanged],
  );

  useEffect(() => {
    const controller = new AbortController();
    void loadNativeBootstrapHealth({ signal: controller.signal })
      .then((health) => {
        setBootstrap(health);
        if (health.kind !== "snapshot") return;
        const onboarding = health.snapshot.onboarding;
        const requestedGameId = onboarding?.selectedGameId;
        const nativeGameId =
          health.snapshot.gameProfiles?.find(
            (profile) => profile.id === "cyberpunk-2077",
          )?.id ??
          (health.snapshot.gameProfiles?.some(
            (profile) => profile.id === requestedGameId,
          )
            ? requestedGameId
            : health.snapshot.gameProfiles?.[0]?.id);
        if (nativeGameId) setSelectedGameProfileId(nativeGameId);
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
        } else {
          setSetupOpen(true);
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
    if (bootstrap.kind !== "snapshot") {
      sessionPreferenceSequence.current += 1;
      sessionPreferenceSnapshotRef.current = null;
      setSessionPreferenceSnapshot(null);
      setSessionPreferenceBusy(null);
      return;
    }
    void refreshCurrentProductPreferences();
    return () => {
      sessionPreferenceSequence.current += 1;
    };
  }, [
    bootstrap.kind,
    currentProductPreferenceScopeKey,
    refreshCurrentProductPreferences,
  ]);

  useEffect(() => {
    if (observedSessionConfigurationRef.current === null) {
      observedSessionConfigurationRef.current = sessionConfigurationIdentityKey;
      return;
    }
    if (
      observedSessionConfigurationRef.current !==
      sessionConfigurationIdentityKey
    ) {
      observedSessionConfigurationRef.current = sessionConfigurationIdentityKey;
      markConfigurationChanged();
    }
  }, [markConfigurationChanged, sessionConfigurationIdentityKey]);

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
        if (
          active &&
          inspection &&
          inspection.gameProfileId === selectedGameProfileId
        ) {
          setSelectedCharacter(inspection);
          if (
            inspection.gameProfileId === "cyberpunk-2077" &&
            !explicitWorldModeChoice.current
          ) {
            setTurnWorldMode("selectedWorld");
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
  const configuredLoadout =
    nativeLoadouts?.find(
      (item) =>
        item.active &&
        item.scope === "character" &&
        item.targetId ===
          `${currentSttScope.gameProfileId}/${currentSttScope.characterId}`,
    ) ??
    nativeLoadouts?.find(
      (item) =>
        item.active &&
        item.scope === "game" &&
        item.targetId === currentSttScope.gameProfileId,
    ) ??
    nativeLoadouts?.find((item) => item.active && item.scope === "global") ??
    activeBrowserLoadoutFor(
      currentSttScope.gameProfileId,
      currentSttScope.characterId,
    );
  const configuredSttRoute = configuredLoadout.routes.stt;
  const configuredTtsCredentialId =
    configuredLoadout.routes.tts.providerId === "nvidia-nim-magpie"
      ? "nvidia-nim"
      : configuredLoadout.routes.tts.providerId;
  const configuredTtsPresent = Boolean(
    nativeLoadouts &&
      snapshot?.providers?.some(
        (item) =>
          item.providerId === configuredTtsCredentialId &&
          item.status === "present",
      ),
  );
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
  const reviewTargetAvailable = syntheticReviewTargetAvailability(
    Boolean(snapshot?.capabilities?.debugSyntheticReviewTargetLaunch),
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

  const openMouthMotion = () => {
    setSetupOpen(false);
    navigate("world");
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
    if (setupStep === 1 && !captureProof?.receipt?.verified) {
      setNotice(
        "Select the test game and verify its capture before continuing.",
      );
      return;
    }
    if (setupStep < ONBOARDING_STEPS.length - 1) {
      const next = setupStep + 1;
      const saved = await persistSetup(
        false,
        ONBOARDING_STEPS[next].id,
        preferences,
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
    if (
      !hasSpokenSetupTurn(deliveredTurn, preferences.subtitles) ||
      !turnRoute ||
      turnRoute.configurationChangedAt !== configurationChangedAt.current ||
      turnRoute.configurationRevision !== configurationRevision
    ) {
      setNotice(
        "Run a spoken test turn first. Setup finishes when the selected voice returns audio and playback completes.",
      );
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
    const requestEpoch = ++turnRequestEpochRef.current;
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
      loadout: structuredClone(configuredLoadout),
      capturedAtEpochMs: Date.now(),
      configurationChangedAt: configurationChangedAt.current,
      configurationRevision: configurationRevisionRef.current,
    });
    try {
      const started = await startNativeSimulation(
        preferences.execution,
        (event) => {
          if (turnRequestEpochRef.current !== requestEpoch) return;
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
      if (turnRequestEpochRef.current !== requestEpoch) return;
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
      if (turnRequestEpochRef.current !== requestEpoch) return;
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
    const stopEpoch = ++turnRequestEpochRef.current;
    try {
      await cancelNativeSimulation();
    } catch (error) {
      if (turnRequestEpochRef.current !== stopEpoch) return;
      const detail =
        error instanceof Error
          ? error.message
          : "The native turn could not be cancelled.";
      setTurnError(detail);
      setNotice(detail);
    } finally {
      if (turnRequestEpochRef.current === stopEpoch) setRunning(false);
    }
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

  const verifySyntheticCaptureFor = async (
    expected: SyntheticCaptureProof | null,
  ) => {
    if (!expected) {
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
        result.target.processId === expected.pid &&
        result.target.nativeWindow === expected.hwnd &&
        result.target.executableName.toLocaleLowerCase() ===
          expected.executable.toLocaleLowerCase() &&
        evidence.selectedProcessId === expected.pid &&
        evidence.selectedWindowHandle === expected.hwnd &&
        evidence.selectedExecutableName.toLocaleLowerCase() ===
          expected.executable.toLocaleLowerCase();
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

  const verifySyntheticCapture = () => verifySyntheticCaptureFor(captureProof);
  const connectReviewGame = async (reportError = false) => {
    markConfigurationChanged();
    setCaptureBusy(true);
    setNotice(null);
    try {
      const target = await prepareSyntheticReviewTarget(reviewTargetAvailable);
      const expected: SyntheticCaptureProof = {
        pid: target.targetProcessId,
        hwnd: target.targetWindowHandle,
        executable: target.targetExecutableBasename,
        frames: 0,
        receipt: null,
      };
      setCaptureProof(expected);
      await verifySyntheticCaptureFor(expected);
    } catch (error) {
      setCaptureProof(null);
      setNotice(
        error instanceof Error
          ? error.message
          : "Could not start the test game. Open Games and try again.",
      );
      if (reportError) throw error;
    } finally {
      setCaptureBusy(false);
    }
  };

  const runProviderAction = async (
    providerId: string,
    action: AccountAction,
  ) => {
    setProviderBusy(providerId);
    try {
      const result =
        action === "save"
          ? await promptAndSaveProviderCredential(providerId)
          : action === "delete"
            ? await deleteProviderCredential(providerId)
            : await testProviderCredential(providerId);
      setNotice(result.detail);
      if (action !== "validate" && result.outcome !== "cancelled") {
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
    <div className={`product-shell cyberpunk-shell page-${page}`}>
      <aside
        className="product-rail"
        aria-label="Primary navigation"
        inert={setupOpen}
      >
        <button
          className="brand-lockup"
          onClick={() => navigate("session")}
          aria-label="Open session deck"
        >
          <span className="brand-mark">N2</span>
          <span>
            <b>NPC / LINK</b>
            <small>Interactive worlds</small>
          </span>
        </button>
        <nav>
          {NAV.map((item) => (
            <button
              key={item.id}
              className={page === item.id ? "nav-item selected" : "nav-item"}
              onClick={() => navigate(item.id)}
              aria-current={page === item.id ? "page" : undefined}
            >
              <Icon name={item.icon} size={19} />
              <b>{item.label}</b>
            </button>
          ))}
        </nav>
        <div className="rail-safety">
          <span className="status-dot" />
          <b>Cyberpunk 2077</b>
          <small>Personal build</small>
        </div>
      </aside>

      <main className="product-main" inert={setupOpen}>
        <header className="product-topbar">
          <div>
            <span className="eyebrow">
              {page === "diagnostics"
                ? "Settings / Troubleshooting"
                : `Your workspace / ${NAV.find((item) => item.id === page)?.label}`}
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

        {page === "session" && (
          <SignalRail
            activeStage={activeStage}
            running={running}
            delivered={Boolean(deliveredTurn)}
            providerPresent={providerPresent}
            captureProof={captureProof}
            selectedCharacter={selectedCharacter}
            selectedTarget={selectedTarget}
            turnWorldMode={turnWorldMode}
            loadout={configuredLoadout}
            accounts={snapshot?.providers ?? []}
            onNavigate={navigate}
          />
        )}

        {page === "session" && (
          <SessionPage
            running={running}
            deliveredTurn={deliveredTurn}
            events={turnEvents}
            providerPresent={configuredTtsPresent}
            captureProof={captureProof}
            selectedCharacter={selectedCharacter}
            selectedTarget={selectedTarget}
            turnWorldMode={turnWorldMode}
            setTurnWorldMode={(value) => {
              explicitWorldModeChoice.current = true;
              setTurnWorldMode(value);
            }}
            inputMode={turnInputMode}
            setInputMode={setTurnInputMode}
            prompt={turnPrompt}
            setPrompt={setTurnPrompt}
            turnError={turnError}
            turnRoute={turnRoute}
            subtitles={preferences.subtitles}
            setSubtitles={(subtitles) => void saveSessionSubtitles(subtitles)}
            subtitlesBusy={sessionPreferenceBusy}
            subtitlesError={sessionPreferenceError}
            subtitlesReady={Boolean(
              sessionPreferenceSnapshot &&
                sameProductPreferenceScope(
                  sessionPreferenceSnapshot.effective.scope,
                  currentProductPreferenceScope,
                ),
            )}
            subtitleScopeLabel={currentSttScope.characterLabel}
            nativeAvailable={
              bootstrap.kind === "snapshot" &&
              bootstrap.snapshot.runtime.connected
            }
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
            onNavigate={navigate}
            onSetupMouthTracking={() => {
              setVoiceInitialSection("local");
              navigate("voice");
            }}
            onPreferencesSnapshot={acceptPreferenceWorkspaceSnapshot}
            onConnectReviewGame={() => connectReviewGame(true)}
            reviewTargetAvailable={reviewTargetAvailable}
            onReviewConnectionLost={() => setCaptureProof(null)}
            captureBusy={captureBusy}
            captureProof={captureProof}
            captureAvailable={captureDebugAvailable.available}
            onCapture={selectSyntheticTarget}
            onVerifyCapture={verifySyntheticCapture}
            nativeAvailable={bootstrap.kind === "snapshot"}
            gameProfiles={snapshot?.gameProfiles ?? []}
            gameProfileId={selectedGameProfileId}
            onGameProfileChange={(gameProfileId) => {
              explicitWorldModeChoice.current = true;
              setSelectedGameProfileId(gameProfileId);
              setSelectedCharacter(null);
            }}
            onTargetChange={setSelectedTarget}
            onCharacterChange={(inspection) => {
              explicitWorldModeChoice.current = true;
              setSelectedCharacter(inspection);
              setTurnWorldMode("selectedWorld");
            }}
            selectedCharacter={selectedCharacter}
            identityEnrollmentStatus={identityEnrollmentStatus}
            identityEnrollmentError={identityEnrollmentError}
          />
        )}
        {page === "voice" && !setupOpen && (
          <VoicePage
            initialSection={voiceInitialSection}
            onManageMouthMotion={openMouthMotion}
            onNativeLoadoutsChange={acceptConfiguredLoadouts}
            accounts={snapshot?.providers ?? []}
            gameProfileId={currentSttScope.gameProfileId}
            characterId={currentSttScope.characterId}
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
              const saved = await saveOnboarding({
                ...nowSnapshot(
                  snapshot?.onboarding?.completed ?? false,
                  snapshot?.onboarding?.currentStep ?? "scan",
                  next,
                ),
                selectedGameId: selectedGameProfileId,
              });
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
            onProductPreferenceSnapshot={acceptPreferenceWorkspaceSnapshot}
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
          onConnectReviewGame={connectReviewGame}
          reviewLaunchAvailable={reviewTargetAvailable.available}
          captureBusy={captureBusy}
          step={setupStep}
          bootstrap={bootstrap}
          providerPresent={providerPresent}
          nvidiaPresent={nvidiaPresent}
          loadout={configuredLoadout}
          onManageMouthMotion={openMouthMotion}
          onNativeLoadoutsChange={acceptConfiguredLoadouts}
          setupProofCurrent={Boolean(
            turnRoute &&
              turnRoute.configurationChangedAt ===
                configurationChangedAt.current &&
              turnRoute.configurationRevision === configurationRevision,
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
          onVerifyCapture={verifySyntheticCapture}
          accounts={snapshot?.providers ?? []}
          providerBusy={providerBusy}
          gameProfileId={currentSttScope.gameProfileId}
          characterId={currentSttScope.characterId}
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
          onClose={() => {
            setSetupOpen(false);
            setNotice(
              snapshot?.onboarding?.completed
                ? null
                : "Setup saved for later. Open Setup to continue.",
            );
          }}
        />
      )}
    </div>
  );
}

function SignalRail({
  activeStage,
  running,
  delivered,
  captureProof,
  selectedCharacter,
  selectedTarget,
  turnWorldMode,
  loadout,
  accounts,
  onNavigate,
}: {
  activeStage: string | null;
  running: boolean;
  delivered: boolean;
  providerPresent: boolean;
  captureProof: SyntheticCaptureProof | null;
  selectedCharacter: NativeCharacterInspection | null;
  selectedTarget: NativeGameTargetSelection | null;
  turnWorldMode: "syntheticReview" | "selectedWorld";
  loadout: ProviderLoadout;
  accounts: NativeProviderCredentialSummary[];
  onNavigate: (page: ProductPage) => void;
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
        : captureProof
          ? "Mara Venn · test character"
          : "Mara Venn · preview",
      ready: selectedWorld || Boolean(captureProof),
    },
    {
      label: "Voice service",
      value: providerFor("tts", loadout.routes.tts.providerId).name,
      ready: accounts.some(
        (account) =>
          account.providerId ===
            (loadout.routes.tts.providerId === "nvidia-nim-magpie"
              ? "nvidia-nim"
              : loadout.routes.tts.providerId) && account.status === "present",
      ),
    },
    {
      label: "Voice",
      value: loadout.routes.tts.voiceId
        ? loadout.routes.tts.voiceId.startsWith("Magpie-")
          ? (loadout.routes.tts.voiceId.split(".").at(-1) ?? "Configured voice")
          : "Stock voice configured"
        : "Choose a stock voice",
      ready: false,
    },
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
        <button
          className="signal-node"
          key={node.label}
          onClick={() =>
            onNavigate(
              index < 2 ? "world" : index < 4 ? "voice" : "diagnostics",
            )
          }
        >
          <span className={node.ready ? "signal-index ready" : "signal-index"}>
            {String(index + 1).padStart(2, "0")}
          </span>
          <span>
            <small>{node.label}</small>
            <b>{node.value}</b>
          </span>
        </button>
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
          <small>Hold F8 to speak, release to send for transcription</small>
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
      <details className="evidence-disclosure stt-connection-details">
        <summary>Microphone connection details</summary>
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
      </details>
      <p className="selected-stt-physical-note">
        Enable push-to-talk, then hold F8 within 8 seconds. Speak for up to 10
        seconds and release.
      </p>
      <p className="stt-service-label">
        Speech recognition by AssemblyAI · microphone starts when you hold F8.
      </p>
      <div className="selected-stt-actions">
        <button
          className="secondary-action"
          disabled={!readiness.ready || active || busy}
          aria-describedby={startReasonId}
          onClick={onStart}
        >
          {busy ? "Preparing microphone…" : "Enable push-to-talk"}
        </button>
        {active && (
          <button
            className="stop-action"
            disabled={busy && !arming}
            onClick={onCancel}
          >
            Stop listening
          </button>
        )}
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
        <details
          open={terminal.status !== "transcriptReady"}
          className="evidence-disclosure"
        >
          <summary>
            {terminal.status === "transcriptReady"
              ? "Transcription ready · view details"
              : "Capture result"}
          </summary>
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
                  {terminal.chunksSent} chunks · {terminal.pcmBytesSent} PCM
                  bytes · {terminal.route.capturedFrames} captured frames ·
                  physical F8 press sequence{" "}
                  {terminal.route.pttPressTransitionSequence} → release{" "}
                  {terminal.route.pttReleaseTransitionSequence} ·{" "}
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
        </details>
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
  subtitlesBusy,
  subtitlesError,
  subtitlesReady,
  subtitleScopeLabel,
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
  turnRoute: {
    loadout: ProviderLoadout;
    capturedAtEpochMs: number;
    configurationChangedAt: number;
    configurationRevision: number;
  } | null;
  subtitles: boolean;
  setSubtitles: (value: boolean) => void;
  subtitlesBusy: "load" | "save" | null;
  subtitlesError: string | null;
  subtitlesReady: boolean;
  subtitleScopeLabel: string;
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
    ? "Reply sent to your audio device"
    : subtitleDelivered
      ? "Reply shown in subtitles"
      : deliveredTurn?.runtimeFixtureOnly
        ? "Practice response · simulated dialogue"
        : deliveredTurn
          ? "Text reply ready · audio not confirmed"
          : "Your conversation starts here";
  return (
    <div className="page-grid session-grid">
      <section className="instrument-panel primary-instrument">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">
              {turnWorldMode === "selectedWorld" && selectedCharacter
                ? "Conversation channel"
                : captureProof
                  ? "Local test character"
                  : "Preview · local test character"}
            </span>
            <h1>
              {turnWorldMode === "selectedWorld" && selectedCharacter
                ? selectedCharacter.character.displayName
                : "Mara Venn"}
            </h1>
            <p>
              {turnWorldMode === "selectedWorld" && selectedCharacter
                ? `${selectedCharacter.gameDisplayName} · ${selectedCharacter.character.promptRole} · selected by you`
                : "Eclipse Harbor · local test game"}
            </p>
          </div>
          <span className="identity-seal" aria-hidden="true">
            {(turnWorldMode === "selectedWorld" && selectedCharacter
              ? selectedCharacter.character.displayName
              : "Mara Venn"
            )
              .split(/\s+/)
              .slice(0, 2)
              .map((word) => word[0])
              .join("")}
          </span>
        </div>
        <div className="turn-composer">
          <details className="conversation-source">
            <summary>
              <span>
                {turnWorldMode === "selectedWorld" && selectedCharacter
                  ? selectedCharacter.gameDisplayName
                  : "Eclipse Harbor"}
              </span>
              <b>Change conversation</b>
            </summary>
            <div
              className="segmented-control turn-world-mode"
              aria-label="Turn world source"
            >
              <button
                className={turnWorldMode === "syntheticReview" ? "active" : ""}
                aria-pressed={turnWorldMode === "syntheticReview"}
                onClick={() => setTurnWorldMode("syntheticReview")}
              >
                Practice game
              </button>
              <button
                className={turnWorldMode === "selectedWorld" ? "active" : ""}
                aria-pressed={turnWorldMode === "selectedWorld"}
                disabled={!selectedCharacter}
                onClick={() => setTurnWorldMode("selectedWorld")}
              >
                Selected character
              </button>
            </div>
            <button className="text-action" onClick={() => onNavigate("world")}>
              Choose game & character →
            </button>
          </details>
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
              Push-to-talk · F8
            </button>
          </div>
          {inputMode === "typed" && (
            <label className="message-field">
              <span>Your message</span>
              <textarea
                aria-label="Turn transcript"
                value={prompt}
                placeholder={
                  turnWorldMode === "selectedWorld" && selectedCharacter
                    ? `Talk to ${selectedCharacter.character.displayName}…`
                    : "Ask the practice character something…"
                }
                maxLength={500}
                rows={4}
                disabled={running}
                onChange={(event) => setPrompt(event.target.value)}
              />
              <small>
                {prompt.length}/500 · Sent to your selected reply provider
              </small>
            </label>
          )}
          {inputMode === "ptt" && (
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
          )}
        </div>
        <div className="session-controls">
          <button
            className="primary-action"
            onClick={onRun}
            disabled={
              !nativeAvailable ||
              running ||
              (inputMode === "ptt" &&
                (selectedSttTerminal?.status !== "transcriptReady" ||
                  selectedSttReceiptDisposition !== "ready"))
            }
          >
            {running
              ? "Turn in progress"
              : inputMode === "typed"
                ? "Send message"
                : selectedSttTerminal?.status === "transcriptReady" &&
                    selectedSttReceiptDisposition === "ready"
                  ? "Send voice message"
                  : selectedSttReceiptDisposition === "submitted" ||
                      selectedSttReceiptDisposition === "rejected"
                    ? "Record another message"
                    : "Record a message first"}
            <span>
              {nativeAvailable ? "Native runtime" : "Desktop runtime required"}
            </span>
          </button>
          {running && (
            <button className="stop-action" onClick={onStop}>
              Cancel generation
            </button>
          )}
          <label className="subtitle-control">
            <input
              type="checkbox"
              checked={subtitles}
              disabled={
                !nativeAvailable || subtitlesBusy !== null || !subtitlesReady
              }
              onChange={(event) => setSubtitles(event.target.checked)}
            />
            <span>
              <b>Subtitles</b>
              <small>
                {!nativeAvailable
                  ? "Installed app required"
                  : subtitlesBusy === "load"
                    ? "Loading current character…"
                    : subtitlesBusy === "save"
                      ? "Saving current character…"
                      : !subtitlesReady
                        ? "Current character preference unavailable"
                        : `For ${subtitleScopeLabel}`}
              </small>
            </span>
          </label>
        </div>
        {subtitlesError && (
          <p className="selected-stt-error" role="alert">
            {subtitlesError}
          </p>
        )}
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
            <small>Conversation</small>
            <b>{completionLabel}</b>
            {deliveredTurn && subtitles ? (
              <p>{deliveredTurn.deliveredText}</p>
            ) : deliveredTurn ? (
              <p className="subtitle-hidden">
                Subtitle text hidden by session preference.
              </p>
            ) : null}
            {executionEvidence && (
              <details className="evidence-disclosure">
                <summary>Delivery & identity details</summary>
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
                        AssemblyAI u3-rt-pro receipt{" "}
                        {acceptedSttReceipt.receiptId}
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
                          {acceptedSttReceipt.route.pttPressTransitionSequence}{" "}
                          → release{" "}
                          {
                            acceptedSttReceipt.route
                              .pttReleaseTransitionSequence
                          }{" "}
                          · {acceptedSttReceipt.route.capturedFrames} captured
                          frames
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
                      This completed native event proves the opaque capture
                      receipt was accepted for this turn. Transcript, microphone
                      audio, and provider credentials are not exposed here.
                    </small>
                  </section>
                )}
                {executionEvidence &&
                  executionEvidence.degradations.length > 0 && (
                    <ul
                      className="turn-degradations"
                      aria-label="Turn degradations"
                    >
                      {executionEvidence.degradations.map(
                        (degradation, index) => (
                          <li key={`${degradation.type}-${index}`}>
                            <b>{degradation.type}</b>
                            <span>{degradation.reason}</span>
                          </li>
                        ),
                      )}
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
              </details>
            )}
          </div>
        </div>
      </section>

      <aside className="session-side">
        <section className="instrument-panel compact-panel">
          <div className="panel-title">
            <h2>Your connection</h2>
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
              <dd>
                {inputMode === "typed" ? "Keyboard" : "Push-to-talk · F8"}
              </dd>
            </div>
            <div>
              <dt>Subtitles</dt>
              <dd>{subtitles ? "Visible" : "Hidden"}</dd>
            </div>
          </dl>
          <button className="text-action" onClick={() => onNavigate("world")}>
            Select game & character →
          </button>
          <button className="text-action" onClick={() => onNavigate("voice")}>
            Configure voice & models →
          </button>
          <div className="visual-mode-note">
            <Icon name="presence" size={20} />
            <span>
              <b>Mouth motion</b>
              <small>
                Select an NPC in Games for basic motion. Prepared packs add
                detail.
              </small>
            </span>
          </div>
        </section>
        <details className="instrument-panel trace-panel evidence-disclosure">
          <summary>
            Turn activity <span>{events.length} events</span>
          </summary>
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
        </details>
        <details className="instrument-panel route-receipt-panel evidence-disclosure">
          <summary>Provider route evidence</summary>
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
        </details>
      </aside>
    </div>
  );
}

function WorldPage({
  onNavigate,
  onSetupMouthTracking,
  onPreferencesSnapshot,
  onConnectReviewGame,
  reviewTargetAvailable,
  onReviewConnectionLost,
  captureBusy,
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
  onNavigate: (page: ProductPage) => void;
  onSetupMouthTracking: () => void;
  onPreferencesSnapshot: (snapshot: NativeProductPreferenceSnapshot) => void;
  onConnectReviewGame: () => void | Promise<void>;
  reviewTargetAvailable: SyntheticReplayCaptureAvailability;
  onReviewConnectionLost: () => void;
  captureBusy: boolean;
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
  const [contentPackRevision, setContentPackRevision] = useState(0);
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">Choose who answers</span>
        <h1>Games</h1>
        <p>
          {nativeAvailable
            ? "Connect Cyberpunk 2077. Choose who you want to talk to."
            : "Your Cyberpunk characters, voices and memories. Connect a game in the desktop app."}
        </p>
      </header>
      <TestGameLauncher
        availability={reviewTargetAvailable}
        connected={Boolean(captureProof)}
        busy={captureBusy}
        onLaunchAndConnect={onConnectReviewGame}
        onConnectionLost={onReviewConnectionLost}
      />
      <GameTargetWorkspace
        onSetupMouthTracking={onSetupMouthTracking}
        nativeAvailable={nativeAvailable}
        onPreferencesSnapshot={onPreferencesSnapshot}
        gameProfiles={gameProfiles}
        gameProfileId={gameProfileId}
        onGameProfileChange={onGameProfileChange}
        onSelectionChange={onTargetChange}
      />
      <details className="practice-lab evidence-disclosure">
        <summary>
          Practice environment details{" "}
          <span>Try a conversation in the included test game</span>
        </summary>
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
              <p>An original test world for your first conversation</p>
            </div>
          </section>
          <section className="instrument-panel">
            <div className="panel-title">
              <h2>Connect the test game</h2>
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
              <dl
                className="capture-receipt"
                aria-label="Synthetic WGC receipt"
              >
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
      </details>
      <CharacterDatabase
        key={`${gameProfileId}:${contentPackRevision}`}
        nativeAvailable={nativeAvailable}
        gameProfileId={gameProfileId}
        onSelectionChange={onCharacterChange}
      />
      <ContentPackWorkspace
        nativeAvailable={nativeAvailable}
        gameProfileId={gameProfileId}
        onApplied={() => setContentPackRevision((current) => current + 1)}
        onOpenProviders={() => onNavigate("voice")}
      />
      <details className="instrument-panel evidence-disclosure identity-enrollment-panel">
        <summary>
          Character recognition <span className="badge wait">Unavailable</span>
        </summary>
        <p className="body-copy">
          Automatic recognition is not qualified. Choose the character manually;
          that selection controls its prompt, voice and memory.
        </p>
        <dl className="facts">
          <div>
            <dt>Selected character</dt>
            <dd>
              {selectedCharacter?.character.displayName ??
                "Choose a character above"}
            </dd>
          </div>
          <div>
            <dt>Reference status</dt>
            <dd>
              {identityEnrollmentError ??
                identityEnrollmentStatus?.detail ??
                "No qualified reference pack"}
            </dd>
          </div>
        </dl>
      </details>
    </div>
  );
}

function VoicePage({
  initialSection = "loadout",
  onManageMouthMotion,
  onNativeLoadoutsChange,
  accounts,
  gameProfileId,
  characterId,
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
  initialSection?: string;
  onManageMouthMotion: () => void;
  onNativeLoadoutsChange: (loadouts: ProviderLoadout[]) => void;
  accounts: NativeProviderCredentialSummary[];
  gameProfileId: string;
  characterId: string;
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
  onProviderAction: (providerId: string, action: AccountAction) => void;
  onSave: () => void;
}) {
  const [section, setSection] = useState(initialSection);
  const [accountId, setAccountId] = useState("nvidia-nim");
  return (
    <div className="page-stack voice-workspace">
      <header className="page-heading">
        <span className="eyebrow">Configure your connection</span>
        <h1>Neural loadout</h1>
        <p>Select a component. Make it yours.</p>
      </header>
      <WorkspaceSections
        label="Voice workspace"
        selectedId={section}
        onSelectionChange={setSection}
        sections={[
          {
            id: "loadout",
            label: "Model loadout",
            description: "Choose models & stock voice",
            content: (
              <ProviderLoadoutEditor
                onManageMouthMotion={onManageMouthMotion}
                onNativeLoadoutsChange={onNativeLoadoutsChange}
                gameProfileId={gameProfileId}
                characterId={characterId}
                gameProfileLabel={
                  gameProfileId === "eclipse-harbor"
                    ? "Eclipse Harbor"
                    : undefined
                }
                characterLabel={
                  characterId === "mara-venn" ? "Mara Venn" : undefined
                }
                onManageProvider={(id) => {
                  setAccountId(id === "nvidia-nim-magpie" ? "nvidia-nim" : id);
                  setSection("accounts");
                }}
              />
            ),
          },
          {
            id: "accounts",
            label: "Accounts",
            description: "Connect your providers",
            content: (
              <ProviderAccounts
                selectedProviderId={accountId}
                onProviderChange={setAccountId}
                accounts={accounts}
                nativeAvailable={nativeAvailable}
                busy={providerBusy}
                onAction={onProviderAction}
              />
            ),
          },
          {
            id: "microphone",
            label: "Microphone",
            description: "Input & push-to-talk",
            content: (
              <section className="instrument-panel settings-output-panel">
                <div className="panel-title">
                  <h2>Microphone & push-to-talk</h2>
                  <span className="badge">F8</span>
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
            ),
          },
          {
            id: "local",
            label: "Local models",
            description: "Inventory & PC budget",
            content: (
              <>
                <section className="instrument-panel">
                  <div className="panel-title">
                    <h2>Execution preference</h2>
                    <span className="badge">API first</span>
                  </div>
                  <p className="body-copy">
                    Hosted models leave your GPU available for the game. Each
                    account has its own charges, limits and data policy.
                  </p>
                  <button
                    className="secondary-action"
                    disabled={!nativeAvailable}
                    onClick={onSave}
                  >
                    Save cloud execution preference
                  </button>
                  <details className="evidence-disclosure">
                    <summary>Connected account status</summary>
                    <p>
                      ElevenLabs:{" "}
                      {providerPresent ? "key present" : "key needed"}.{" "}
                      {providerDetail}
                    </p>
                    <p>
                      NVIDIA NIM: {nvidiaPresent ? "key present" : "key needed"}
                      . {nvidiaDetail}
                    </p>
                    <p>
                      NVIDIA trial routes require an eligible private evaluation
                      and are not unlimited production services.
                    </p>
                  </details>
                </section>
                <LocalResourcePlanner
                  models={models}
                  nativeAvailable={nativeAvailable}
                />
              </>
            ),
          },
        ]}
      />
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
          <span className="eyebrow">System health</span>
          <h1>Diagnostics</h1>
          <p>
            Check connections, investigate a failed turn, or export a local
            report.
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
            {diagnosticsMatrix
              ? `${diagnosticsMatrix.checks.filter((check) => check.status === "ok").length} / ${diagnosticsMatrix.checks.length}`
              : "Not measured"}
          </b>
          <small>
            {diagnostics?.generatedAtEpochMs
              ? new Date(diagnostics.generatedAtEpochMs).toLocaleString()
              : "Run checks in the desktop application"}
          </small>
        </article>
      </div>
      <section className="instrument-panel diagnostic-matrix-panel">
        <div className="panel-title">
          <div>
            <span className="eyebrow">Connections & capabilities</span>
            <h2>System checks</h2>
          </div>
          <span className="badge">
            {diagnosticsMatrix
              ? `${diagnosticsMatrix.checks.length} checks`
              : "Not loaded"}
          </span>
        </div>
        <p className="source-disclosure">
          Run checks after changing a game, device or provider. Expand a result
          for its source and timestamp.
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
            <p>Open the Windows app and refresh checks to inspect this PC.</p>
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
            <div
              className="diagnostic-matrix-list"
              role="list"
              aria-label="System check results"
            >
              {diagnosticsMatrix.checks.map((check) => (
                <article key={check.checkId} role="listitem">
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
      <details className="instrument-panel diagnostics-v2-panel evidence-disclosure">
        <summary>Local event history & recovery</summary>
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
      </details>
      <div className="diagnostic-detail-grid">
        <details className="instrument-panel check-table evidence-disclosure">
          <summary>Service checks & exact results</summary>
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
            <div role="list" aria-label="Service check results">
              {checks.map((check) => (
                <div className="check-row" role="listitem" key={check.id}>
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
        </details>
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
          ).sort(
            (left, right) =>
              Number(right.searchTerms.includes(normalizedGuideQuery)) -
              Number(left.searchTerms.includes(normalizedGuideQuery)),
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
        <span className="eyebrow">Make it yours</span>
        <h1>Settings & help</h1>
        <p>Adjust your defaults, audio devices and subtitle style.</p>
      </header>
      <WorkspaceSections
        label="Settings workspace"
        sections={[
          {
            id: "preferences",
            label: "Preferences",
            description: "Defaults & subtitle style",
            content: (
              <ProductPreferencesWorkspace
                nativeAvailable={nativeAvailable}
                gameProfileId={gameProfileId}
                characterId={characterId}
                onSnapshot={onProductPreferenceSnapshot}
              />
            ),
          },
          {
            id: "devices",
            label: "Audio devices",
            description: "Microphone & playback",
            content: (
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
            ),
          },
          {
            id: "help",
            label: "Help",
            description: "Search the local guide",
            content: (
              <section className="instrument-panel guide-list">
                <div className="panel-title">
                  <h2>Reviewed in-app guide</h2>
                  <span className="badge good">
                    {SUPPORT_GUIDES.length} topics
                  </span>
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
                <div
                  className="guide-results"
                  aria-label="Guide search results"
                >
                  {filteredGuides.length > 0 ? (
                    filteredGuides.map((guide, index) => (
                      <button
                        key={guide.id}
                        aria-pressed={activeGuide.id === guide.id}
                        onClick={() => setActiveGuideId(guide.id)}
                      >
                        <span aria-hidden="true">
                          {String(index + 1).padStart(2, "0")}
                        </span>
                        <div>
                          <b>{guide.title}</b>
                          <small>{guide.summary}</small>
                        </div>
                        <i>→</i>
                      </button>
                    ))
                  ) : (
                    <p className="guide-empty" role="status">
                      No reviewed guide topic matches “{guideQuery}”. Try
                      privacy, voice, game, overlay, setup, or diagnostics.
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
            ),
          },
        ]}
      />
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
          Choose where you hear the character. System default follows your
          Windows sound settings.
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
          Choose your microphone. Recording starts when you use push-to-talk.
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
      <details className="technical-disclosure">
        <summary>Microphone measurement details</summary>
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
          Choosing a microphone does not start recording or measure signal
          levels. Capture results appear with your conversation.
        </small>
      </details>
    </div>
  );
}

function OnboardingOverlay({
  onManageMouthMotion,
  setupProofCurrent,
  onConnectReviewGame,
  reviewLaunchAvailable,
  captureBusy,
  onNativeLoadoutsChange,
  onVerifyCapture,
  accounts,
  providerBusy,
  gameProfileId,
  characterId,
  step,
  bootstrap,
  loadout,
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
  onManageMouthMotion: () => void;
  setupProofCurrent: boolean;
  onConnectReviewGame: () => void;
  reviewLaunchAvailable: boolean;
  captureBusy: boolean;
  onNativeLoadoutsChange: (loadouts: ProviderLoadout[]) => void;
  onVerifyCapture: () => void;
  accounts: NativeProviderCredentialSummary[];
  providerBusy: string | null;
  gameProfileId: string;
  characterId: string;
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
  onProviderAction: (providerId: string, action: AccountAction) => void;
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
  const [voiceSection, setVoiceSection] = useState("accounts");
  const [accountId, setAccountId] = useState("nvidia-nim");
  const dialogRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    dialogRef.current?.focus();
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, []);
  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("h1")?.focus();
  }, [step]);
  return (
    <div className="setup-scrim" role="presentation">
      <section
        className="setup-dialog"
        ref={dialogRef}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === "Escape" && onClose) {
            event.preventDefault();
            onClose();
          }
          if (event.key !== "Tab") return;
          const focusable = Array.from(
            dialogRef.current?.querySelectorAll<HTMLElement>(
              'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, [tabindex="0"]',
            ) ?? [],
          ).filter(
            (element) =>
              !element.closest("[hidden]") &&
              element.getClientRects().length > 0,
          );
          const first = focusable[0];
          const last = focusable.at(-1);
          if (!first) {
            event.preventDefault();
            dialogRef.current?.focus();
            return;
          }
          if (
            event.shiftKey &&
            (document.activeElement === first ||
              document.activeElement === dialogRef.current)
          ) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="setup-title"
      >
        <header className="setup-header">
          <div>
            <span className="brand-mark">N2</span>
            <span>
              <b>Guided setup</b>
              <small>Your first conversation</small>
            </span>
          </div>
          {onClose && (
            <button onClick={onClose} aria-label="Close setup">
              Finish later
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
              <h1 id="setup-title" tabIndex={-1}>
                Let’s get you connected
              </h1>
              <p>
                Start with the local test game, connect a voice, then try your
                first conversation.
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
              <SetupSystemCheck nativeAvailable={nativeAvailable} />
            </>
          )}
          {step === 1 && (
            <>
              <span className="eyebrow">02 / world</span>
              <h1 id="setup-title" tabIndex={-1}>
                Meet Mara at Eclipse Harbor
              </h1>
              <p>
                Use the included test game to check capture and audio before
                choosing another game.
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
                className="primary-action setup-inline-action"
                disabled={!reviewLaunchAvailable || captureBusy}
                onClick={onConnectReviewGame}
              >
                {captureBusy
                  ? "Connecting…"
                  : captureProof?.receipt?.verified
                    ? "Reconnect test game"
                    : "Start & connect test game"}
              </button>
              <button
                className="secondary-action setup-inline-action"
                disabled={!captureAvailable || captureBusy}
                onClick={onCapture}
              >
                {captureProof
                  ? "Reselect running synthetic target"
                  : captureAvailable
                    ? "Select running synthetic target"
                    : "Native debug capture unavailable"}
              </button>
              <button
                className="quiet-button setup-inline-action"
                disabled={!captureProof}
                onClick={onVerifyCapture}
              >
                Verify live capture
              </button>
              <p className="control-reason">
                {captureProof?.receipt?.verified
                  ? `Connected · frame ${captureProof.frames} · capture is advancing`
                  : "Select the test game, then verify that its captured frames advance."}
              </p>
            </>
          )}
          {step === 2 && (
            <>
              <span className="eyebrow">03 / voice</span>
              <h1 id="setup-title" tabIndex={-1}>
                Choose your voice & models
              </h1>
              <p>
                Connect the accounts your loadout uses. Select a reply model and
                a stock voice, then choose where you’ll hear the character.
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
              <WorkspaceSections
                label="Setup voice configuration"
                selectedId={voiceSection}
                onSelectionChange={setVoiceSection}
                sections={[
                  {
                    id: "accounts",
                    label: "Connect accounts",
                    content: (
                      <ProviderAccounts
                        selectedProviderId={accountId}
                        onProviderChange={setAccountId}
                        accounts={accounts}
                        nativeAvailable={nativeAvailable}
                        busy={providerBusy}
                        onAction={onProviderAction}
                      />
                    ),
                  },
                  {
                    id: "models",
                    label: "Choose models",
                    content: (
                      <ProviderLoadoutEditor
                        onManageMouthMotion={onManageMouthMotion}
                        onNativeLoadoutsChange={onNativeLoadoutsChange}
                        gameProfileId={gameProfileId}
                        characterId={characterId}
                        gameProfileLabel={
                          gameProfileId === "eclipse-harbor"
                            ? "Eclipse Harbor"
                            : undefined
                        }
                        characterLabel={
                          characterId === "mara-venn" ? "Mara Venn" : undefined
                        }
                        mode="onboarding"
                        onManageProvider={(id) => {
                          setAccountId(
                            id === "nvidia-nim-magpie" ? "nvidia-nim" : id,
                          );
                          setVoiceSection("accounts");
                        }}
                      />
                    ),
                  },
                ]}
              />
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
              <h1 id="setup-title" tabIndex={-1}>
                Try your first conversation
              </h1>
              <p>
                Ask Mara about the old lighthouse. Use push-to-talk or send the
                sample question. Setup completes after the selected voice
                returns audio and playback finishes.
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
                (step === 1 &&
                  nativeAvailable &&
                  !captureProof?.receipt?.verified) ||
                (step === ONBOARDING_STEPS.length - 1 &&
                  nativeAvailable &&
                  (!hasSpokenSetupTurn(deliveredTurn, preferences.subtitles) ||
                    !setupProofCurrent))
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
