import type { ExecutionMode, StageId } from "./types";

export interface NativeTurnExecutionSuccess {
  llmProviderLive: boolean;
  ttsProviderLive: boolean;
  sttSkipped: boolean;
  subtitleDelivered: boolean;
  subtitleReceiptCount: number;
  audioSubmitted: boolean;
  audioDrained: boolean;
  audioReceiptCount: number;
}

export interface NativeManualActorPickerPresentation {
  schemaVersion: 1;
  state: "waiting" | "selected" | "cancelled" | "unavailable";
  unavailableReason?:
    | "noAdmittedNativeCandidateSet"
    | "staleNativeFrame"
    | "nativeOverlayUnavailable"
    | "noActiveSelection";
  detail: string;
}

export interface NativeTurnExecutionEvidence {
  consumedRoute: {
    schemaVersion: number;
    sourceLoadoutId: string;
    generation: number;
    sha256: string;
    llm: { providerId: string; modelId: string; voiceId: string | null } | null;
    tts: { providerId: string; modelId: string; voiceId: string | null } | null;
    privateEvaluationAcknowledgement?: {
      providerId: string;
      termsRevision: string;
      catalogRevision: number;
      applicationNamespace:
        | "io.github.akshitireddy.interactive-npcs.debug"
        | "io.github.akshitireddy.interactive-npcs.review";
      acknowledgementSha256: string;
      modalities: Array<"llm" | "embeddings" | "tts">;
      promotionSupported: false;
      publicationSupported: false;
    };
  };
  input?: {
    mode: "typed" | "pushToTalk";
    pushToTalkState: "notRequested" | "transcriptReady" | "captureUnavailable";
    selectedSttReceipt?: {
      schemaVersion: 1;
      receiptId: string;
      receiptSha256: string;
      captureSessionId: string;
      captureTurnId: string;
      captureGeneration: number;
      gameId: string;
      characterId: string | null;
      sourceLoadoutId: string;
      route: {
        providerId: "assemblyai";
        modelId: "u3-rt-pro";
        credentialReference: string;
        egress: "microphone_audio_and_optional_non_secret_context";
        generation: number;
        inputEndpointId: string;
        inputEndpointGeneration: number;
        manualRetry: boolean;
        automaticFallback: false;
        capturedFrames: number;
        pttVirtualKey: 119;
        pttPressTransitionSequence: number;
        pttPressedQpc: number;
        pttReleaseTransitionSequence: number;
        pttReleasedQpc: number;
      };
      chunksSent: number;
      pcmBytesSent: number;
      partialEvents: number;
    };
  };
  deliveryState?: "delivered" | "cancelled" | "manualRetryRequired";
  commitState?: "committed" | "notCommitted" | "commitDeferred";
  degradations: NativeTurnExecutionDegradation[];
  success: NativeTurnExecutionSuccess;
  subtitlePresentationReceipts: NativeSubtitlePresentationReceipt[];
  audioReceipts: Array<{
    receiptId: string;
    submitted: boolean;
    drained: boolean;
    outputSelectionMode?: "systemDefault" | "endpointId";
    outputEndpointId?: string;
    outputEndpointGeneration?: number;
  }>;
}

export interface NativeSubtitlePresentationReceipt {
  receiptId: string;
  sentenceId: number;
  provenance:
    | "trustedNativeCapture"
    | "consoleBottomCenterUnavailable"
    | "deterministicFixture";
  presentationId: number;
  targetGeometryEpoch: number;
  captureSequence: number;
  graphicsGeneration: number;
  layerHashHex: string;
  presentedQpcTicks: string;
  desktopXPx: number;
  desktopYPx: number;
  widthPx: number;
  heightPx: number;
  dpiX: number;
  dpiY: number;
  direction: "leftToRight" | "rightToLeft";
  bidiShapingApplied: boolean;
  graphemeClustersPreserved: boolean;
  usedBottomCenterFallback: boolean;
  colorTreatment:
    | "sdrPremultipliedSourceOver"
    | "scrgbLinearSourceOver"
    | "hdr10ToneMappedSourceOver"
    | "windowsCompositorSdrWhiteMapping";
  committed: boolean;
}

export type NativeCharacterSelectionEvidence =
  | {
      status: "known";
      character_id: string;
      reason:
        | "explicit"
        | "addressed_alias"
        | "sticky_current"
        | "visual_confidence";
    }
  | { status: "background" }
  | { status: "ambiguous"; candidate_ids: string[] };

export interface NativeCharacterEncounterEvidence {
  schema_version: string;
  encounter_id: string;
  game_profile_id: string;
  archetype_character_id: string;
  continuity_key_sha256: string;
  selected_voice: {
    binding_id: string;
    adapter_id: string;
    provider_voice_id: string;
    locale: string;
    traits: string[];
    catalog_version?: string;
    license?: string;
  };
  created_at_ms: number;
  last_seen_at_ms: number;
  expires_at_ms: number;
  status: "active" | "expired";
}

export interface NativeCharacterPromptEvidence {
  schemaVersion: string;
  profileId: string;
  characterId: string;
  authorities: Array<
    | "core_canon"
    | "character_profile"
    | "game_public"
    | "character_authored"
    | "session_summary"
    | "recent_delivered_turns"
    | "retrieved_memory"
  >;
  recordCount: number;
  retrievalProvenance: {
    schema_version: string;
    policy_fingerprint_sha256: string;
    query_sha256: string;
    selected_profile_knowledge_ids: string[];
    selected_memory_item_ids: string[];
    embedding_metadata_ids: string[];
  };
  scopedMemoryItemIds: string[];
  scopedMemoryClasses: string[];
}

export interface NativeCharacterContextEvidence {
  profileId: string;
  characterId: string;
  selection?: NativeCharacterSelectionEvidence;
  identitySource: string;
  explicitSelection: boolean;
  encounter?: NativeCharacterEncounterEvidence;
  prompt?: NativeCharacterPromptEvidence;
}

export type NativeEncounterLifecycleEvent =
  | {
      kind: "corrected_to_authored_character";
      encounter_id: string;
      character_id: string;
      source: "manual_explicit";
      occurred_at_ms: number;
      memory_migration_required: true;
    }
  | {
      kind: "merged_into_encounter";
      source_encounter_id: string;
      destination_encounter_id: string;
      source: "manual_explicit";
      occurred_at_ms: number;
      memory_migration_required: true;
    };

export interface NativeEncounterMutationResult {
  schemaVersion: 1;
  gameProfileId: string;
  event: NativeEncounterLifecycleEvent;
  memoryMigrationRequired: true;
  memoryMigrationPerformed: false;
  detail: string;
}

export type NativeTurnExecutionDegradation =
  | { type: "typedInput"; reason: string }
  | { type: "audioOnly"; reason: string }
  | { type: "subtitleOnly"; reason: string }
  | {
      type: "manualRetryRequired";
      failedRole: string;
      providerId: string | null;
      reason: string;
      retryable: boolean;
    };

export type NativeVisualPresentationEvidence = {
  schemaVersion: number;
  sourceFrameSequence: number;
  residualProposed: boolean;
  presented: boolean;
  degraded: boolean;
  pixelSource: string | null;
  pixelScope: string | null;
  detail: string;
};

export type NativeSimulationEvent =
  | {
      type: "started";
      simulationId: string;
      generation: number;
      sequence: number;
      measurementBasis:
        | "deterministicFixture"
        | "trustedRuntimeFixture"
        | "controlledBenchmark"
        | "pendingProviderEvidence";
    }
  | {
      type: "stageStarted";
      simulationId: string;
      generation: number;
      sequence: number;
      stage: StageId;
      estimatedDurationMs: number;
    }
  | {
      type: "stageCompleted";
      simulationId: string;
      generation: number;
      sequence: number;
      stage: StageId;
      fixtureElapsedMs: number;
    }
  | {
      type: "sentenceReady";
      simulationId: string;
      generation: number;
      sequence: number;
      text: string;
    }
  | {
      type: "completed";
      simulationId: string;
      generation: number;
      sequence: number;
      fixtureFirstAudioMs: number | null;
      runtimeFixtureOnly: boolean;
      deliveredText: string;
      turnExecution?: NativeTurnExecutionEvidence;
      characterContext?: NativeCharacterContextEvidence;
      visualPresentation?: NativeVisualPresentationEvidence;
    }
  | {
      type: "cancelled";
      simulationId: string;
      generation: number;
      sequence: number;
      reason: string;
      turnExecution?: NativeTurnExecutionEvidence;
      characterContext?: NativeCharacterContextEvidence;
    }
  | {
      type: "failed";
      simulationId: string;
      generation: number;
      sequence: number;
      reason: string;
      turnExecution?: NativeTurnExecutionEvidence;
      characterContext?: NativeCharacterContextEvidence;
    };

const hasTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type NativeConnectionState =
  | "cold"
  | "starting"
  | "ready"
  | "restartBackoff"
  | "quarantined"
  | "developmentFixture"
  | "unavailable"
  | "shuttingDown"
  | "stopped";

export type NativeOnboardingStep =
  | "welcome"
  | "scan"
  | "execution"
  | "game"
  | "providers"
  | "microphone"
  | "presence"
  | "performance"
  | "simulation"
  | "ready";

export interface NativePreferenceSnapshot {
  execution: ExecutionMode;
  performance:
    | "competitive"
    | "fast"
    | "balanced"
    | "immersive"
    | "maximum"
    | "custom";
  subtitles: boolean;
  ptt: boolean;
  localOnly: boolean;
  screenPresence: boolean;
  diagnostics: boolean;
}

export interface OnboardingSnapshot {
  schemaVersion: number;
  completed: boolean;
  currentStep: NativeOnboardingStep;
  selectedGameId: string | null;
  preferences: NativePreferenceSnapshot;
  updatedAtEpochMs: number;
}

export interface OnboardingPersistence {
  health: "healthy" | "firstRun" | "recoveredFromInvalidFile" | "unavailable";
  detail: string;
}

export interface SaveOnboardingResult {
  onboarding: OnboardingSnapshot;
  persistence: OnboardingPersistence;
}

export interface NativeProviderCredentialSummary {
  providerId: string;
  displayName: string;
  credentialReference: string | null;
  status: "present" | "missing" | "notRequired" | "unavailable";
  detail: string;
}

export interface NativeGameProfileSummary {
  id: string;
  displayName: string;
  wave: string;
  safety: "singlePlayerOnly" | "offlineOnly";
  catalogState: "bundled" | "missingFromBundle";
  defaultFallback: string;
}

export interface NativeModelSummary {
  id: string;
  displayName: string;
  purpose: string;
  execution: string;
  lifecycle: string;
  installation: "notInspected" | "userImportRequired" | "catalogOnly";
  qualificationNote: string | null;
}

export type NativeAudioOutputSelection =
  | { mode: "systemDefault" }
  | { mode: "endpointId"; endpoint_id: string };

export interface NativeAudioOutputEndpoint {
  endpointId: string;
  friendlyName: string;
  state: "active" | "disabled" | "notPresent" | "unplugged";
  systemDefault: boolean;
  generation: number;
}

export interface NativeAudioOutputSnapshot {
  schemaVersion: number;
  catalogGeneration: number;
  endpoints: NativeAudioOutputEndpoint[];
}

export interface NativeSelectedAudioOutput {
  schemaVersion: number;
  selection: NativeAudioOutputSelection;
  resolved: NativeAudioOutputEndpoint;
}

export type NativeAudioInputSelection =
  | { mode: "systemDefault" }
  | { mode: "endpointId"; endpoint_id: string };

export interface NativeAudioInputEndpoint {
  endpointId: string;
  friendlyName: string;
  state: "active" | "disabled" | "notPresent" | "unplugged";
  systemDefault: boolean;
  generation: number;
}

export interface NativeAudioInputSnapshot {
  schemaVersion: number;
  catalogGeneration: number;
  endpoints: NativeAudioInputEndpoint[];
}

export interface NativeSelectedAudioInput {
  schemaVersion: number;
  selection: NativeAudioInputSelection;
  resolved: NativeAudioInputEndpoint;
}

export interface NativeIdentityReferenceEnrollmentStatus {
  schemaVersion: 1;
  status: "unavailable";
  reasonCode: "identity_pack_not_admitted";
  detail: string;
  signedIdentityPackAdmitted: false;
  nativePickerAvailableAfterAdmission: true;
  rawPixelsExposedToWebview: false;
  workerCapabilityExposedToWebview: false;
}

export interface NativeIdentityReferenceEnrollmentRequest {
  gameProfileId: string;
  characterId: string;
  referenceId: string;
  subjectDisplayName: string;
  sourceClass: "user_private" | "original_synthetic";
  ownerUserId?: string | null;
  originalWorkLicense?: string | null;
  explicitUserConsent: boolean;
}

export interface NativeIdentityReferenceEnrollmentReceipt {
  gallerySchemaVersion: number;
  galleryGeneration: number;
  gameProfileId: string;
  characterId: string;
  referenceId: string;
  sourceAssetSha256: string;
  normalizedPixelSha256: string;
  referenceCount: number;
}

export type NativeProductPreferenceScope =
  | { kind: "global" }
  | { kind: "game"; gameProfileId: string }
  | {
      kind: "character";
      gameProfileId: string;
      characterId: string;
    };

export type NativeExecutionPreset = "cloud" | "hybrid" | "fullyLocal";
export type NativePerformancePreset =
  | "competitive"
  | "fast"
  | "balanced"
  | "immersive"
  | "maximum"
  | "custom";
export type NativeVerbosity = "concise" | "standard" | "detailed";
export type NativeResponseLength = "short" | "medium" | "long";
export type NativeInterruptionMode =
  | "immediate"
  | "finishSentence"
  | "disabled";
export type NativePreferenceInputMode = "ptt" | "vad";

export interface NativeProductPreferenceOverrides {
  verbosity?: NativeVerbosity;
  creativity?: number;
  responseLength?: NativeResponseLength;
  interruptionMode?: NativeInterruptionMode;
  inputMode?: NativePreferenceInputMode;
  subtitles?: boolean;
  overlay?: boolean;
  memory?: boolean;
  emotion?: boolean;
  vision?: boolean;
  webcamPresence?: boolean;
}

export interface NativeScopedProductPreferences {
  scope: NativeProductPreferenceScope;
  executionPreset?: NativeExecutionPreset;
  performancePreset?: NativePerformancePreset;
  overrides: NativeProductPreferenceOverrides;
}

export interface NativeEffectivePreference<T> {
  value: T;
  sourceScope: NativeProductPreferenceScope;
  sourceKind: "preset" | "override" | "inherited";
}

export interface NativeProductPreferenceSnapshot {
  schemaVersion: number;
  revision: number;
  entries: NativeScopedProductPreferences[];
  effective: {
    scope: NativeProductPreferenceScope;
    executionPreset: NativeEffectivePreference<NativeExecutionPreset>;
    performancePreset: NativeEffectivePreference<NativePerformancePreset>;
    verbosity: NativeEffectivePreference<NativeVerbosity>;
    creativity: NativeEffectivePreference<number>;
    responseLength: NativeEffectivePreference<NativeResponseLength>;
    interruptionMode: NativeEffectivePreference<NativeInterruptionMode>;
    inputMode: NativeEffectivePreference<NativePreferenceInputMode>;
    subtitles: NativeEffectivePreference<boolean>;
    overlay: NativeEffectivePreference<boolean>;
    memory: NativeEffectivePreference<boolean>;
    emotion: NativeEffectivePreference<boolean>;
    vision: NativeEffectivePreference<boolean>;
    webcamPresence: NativeEffectivePreference<boolean>;
    egress: {
      transcript: NativeEffectivePreference<NativeEgressDisposition>;
      microphoneAudio: NativeEffectivePreference<NativeEgressDisposition>;
      capturedGameImage: NativeEffectivePreference<NativeEgressDisposition>;
      localMemoryContext: NativeEffectivePreference<NativeEgressDisposition>;
    };
    automaticProviderFallback: false;
  };
  migration: {
    state: "current" | "migratedLegacyOnboarding" | "recoveredDefaults";
    fromSchemaVersion: number | null;
    detail: string;
  };
  routeSnapshot: {
    sourceLoadoutId: string;
    generation: number | null;
    sha256: string;
    activationPerformed: false;
  } | null;
  resourceSnapshot: {
    selectionId: string | null;
    admissionStatus: string | null;
    admissionReceiptPresent: boolean;
    exactTargetPid: number | null;
    activationPerformed: false;
  };
  automaticProviderFallback: false;
  mutationActivatedRoutesOrPacks: false;
}

export type NativeSubtitlePreferenceScope = NativeProductPreferenceScope;

export interface NativeSubtitlePreferenceOverrides {
  safeAreaDp?: number;
  textScale?: number;
  backplateEnabled?: boolean;
  opacity?: number;
}

export interface NativeScopedSubtitlePreferences {
  scope: NativeSubtitlePreferenceScope;
  selectedStyleId?: string;
  overrides: NativeSubtitlePreferenceOverrides;
}

export type NativeSubtitleEffectiveSourceKind =
  | "bundledManifestDefault"
  | "bundledStyle"
  | "rendererDefault"
  | "persistedScope";

export interface NativeSubtitleEffectiveSource {
  kind: NativeSubtitleEffectiveSourceKind;
  scope?: NativeSubtitlePreferenceScope;
  styleId?: string;
}

export interface NativeSubtitleEffectiveValue<T> {
  value: T;
  source: NativeSubtitleEffectiveSource;
}

export interface NativeSubtitlePreferenceSnapshot {
  schemaVersion: number;
  revision: number;
  entries: NativeScopedSubtitlePreferences[];
  effective: {
    requestedScope: NativeSubtitlePreferenceScope;
    selectedStyleId: NativeSubtitleEffectiveValue<string>;
    safeAreaDp: NativeSubtitleEffectiveValue<number>;
    textScale: NativeSubtitleEffectiveValue<number>;
    backplateEnabled: NativeSubtitleEffectiveValue<boolean>;
    opacity: NativeSubtitleEffectiveValue<number>;
    rendererParameters: {
      styleId: string;
      safeAreaDp: number;
      bodySizeDp: number;
      speakerSizeDp: number;
      backplateEnabled: boolean;
      globalOpacity: number;
    };
  };
  availableStyles: Array<{
    styleId: string;
    bodyFontRole: string;
    speakerFontRole: string;
    safeAreaDp: number;
    bodySizeDp: number;
    speakerSizeDp: number;
    backplateEnabled: boolean;
  }>;
  assets: {
    noFontBinariesBundled: boolean;
    generatedAssetPolicy: string;
    fontRoles: Array<{
      roleId: string;
      fallbackChain: Array<{
        family: string;
        platforms: string[];
        scripts: string[];
        availability: "systemLookupRequired";
        licenseId: string;
        binaryBundled: boolean;
        redistribution: string;
        usageBasis: string;
        reference: string | null;
      }>;
      genericFallback: string;
      shapingFeatures: string[];
    }>;
  };
  supportedOverrideFields: string[];
  migration: {
    state: "current" | "initializedDefaults" | "migratedLegacyV0";
    fromSchemaVersion: number | null;
    detail: string;
  };
}

export type NativeEffectiveConfigurationValue =
  | { kind: "boolean"; value: boolean }
  | { kind: "choice"; value: string }
  | { kind: "integer"; value: number; unit: string }
  | {
      kind: "route";
      providerId: string;
      modelId: string;
      voiceId: string | null;
      execution: string;
      egress: string;
      transmittedData: string[];
      manualFallbackCount: number;
      automaticFallback: false;
    }
  | { kind: "disabled" };

export interface NativeEffectiveConfigurationEntry {
  key: string;
  category:
    | "conversation"
    | "providerRoute"
    | "memory"
    | "presentation"
    | "privacy"
    | "performance"
    | "optionalVisual";
  label: string;
  owner: "productPreferences" | "providerLoadouts" | "localResources";
  value: NativeEffectiveConfigurationValue;
  defaultValue: NativeEffectiveConfigurationValue;
  winningScope: NativeProductPreferenceScope;
  sourceKind: string;
  persistence: {
    source:
      | "nativePersisted"
      | "migratedOnboarding"
      | "recoveredDefault"
      | "builtInDefault"
      | "firstRunSeed"
      | "recoveredLastGood"
      | "recoveredSeed";
    detail: string;
  };
  explanation: string;
  runtimeConsequence: string;
  unavailableReason: string | null;
}

export interface NativeEffectiveConfigurationSnapshot {
  schemaVersion: number;
  scope: NativeProductPreferenceScope;
  preferenceRevision: number;
  readOnly: true;
  mutationAuthorityAdded: false;
  automaticProviderFallback: false;
  entries: NativeEffectiveConfigurationEntry[];
}

export type NativeEgressDisposition = "denied" | "selectedProviderRoute";

export interface NativeDiagnosticCheck {
  id: string;
  status: "passed" | "informational" | "warning";
  title: string;
  detail: string;
  remediation: string | null;
}

export interface NativeDiagnosticSummary {
  overall: "readyForSimulation" | "degraded";
  generatedAtEpochMs: number;
  measurements: {
    state: string;
    reason: string;
    currentResultsAreReleaseEvidence: boolean;
  };
  checks: NativeDiagnosticCheck[];
}

export interface NativeSafetyBoundary {
  singlePlayerOnly: boolean;
  blocksOnlineModes: boolean;
  blocksDetectedAntiCheat: boolean;
  silentEgressChangesAllowed: boolean;
  credentialValuesExposedToWebview: boolean;
}

export interface NativeMediaBrokerDiagnostics {
  state: string;
  captureBackend: string;
  overlayBackend: string;
  captureAudio: string;
  renderAudio: string;
  targetState: string;
  deviceGeneration: number;
  audioDeviceGeneration: number;
  cancellationGeneration: number;
  framesReceived: number;
  framesPresented: number;
  framesDropped: number;
  overlaysSuppressed: number;
}

export interface NativeBootstrapSnapshot {
  contractVersion: number;
  appVersion: string;
  onboarding?: OnboardingSnapshot;
  onboardingPersistence?: OnboardingPersistence;
  runtime: {
    state: NativeConnectionState;
    connected: boolean;
    backend: "deterministicFixture" | "nativeRuntime";
    processId: number | null;
    restartCount: number;
    recentFailureCount: number;
    protocolVersion: string | null;
    fixtureOnly: boolean;
    detail: string;
  };
  mediaBroker: {
    state: NativeConnectionState;
    connected: boolean;
    processId: number | null;
    restartCount: number;
    recentFailureCount: number;
    protocolVersion: number | null;
    fixtureOnly: boolean;
    brokerState: string | null;
    captureAvailable: boolean;
    overlayAvailable: boolean;
    captureAudioAvailable: boolean;
    renderAudioAvailable: boolean;
    detail: string;
  };
  providers?: NativeProviderCredentialSummary[];
  gameProfiles?: NativeGameProfileSummary[];
  models?: NativeModelSummary[];
  diagnostics?: NativeDiagnosticSummary;
  safety?: NativeSafetyBoundary;
  capabilities?: Record<string, boolean>;
}

export type NativeBootstrapHealth =
  | { kind: "loading"; attempts: 0 }
  | { kind: "browserPreview"; attempts: 0 }
  | {
      kind: "snapshot";
      attempts: number;
      snapshot: NativeBootstrapSnapshot;
    }
  | { kind: "unavailable"; attempts: number; detail: string };

export const LOADING_NATIVE_BOOTSTRAP: NativeBootstrapHealth = {
  kind: "loading",
  attempts: 0,
};

const asRecord = (value: unknown): Record<string, unknown> | null =>
  typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;

const hasOnlyKeys = (
  record: Record<string, unknown>,
  allowedKeys: readonly string[],
) => Object.keys(record).every((key) => allowedKeys.includes(key));

const isPositiveSafeInteger = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value > 0;

const isNonNegativeSafeInteger = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

const isCanonicalNonNilUuid = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
    value,
  ) &&
  value !== "00000000-0000-0000-0000-000000000000";

const isLowercaseSha256 = (value: unknown): value is string =>
  typeof value === "string" && /^[0-9a-f]{64}$/.test(value);

const isRouteValue = (value: unknown, maximumLength: number): value is string =>
  typeof value === "string" &&
  value.trim().length > 0 &&
  value.length <= maximumLength &&
  !/[\u0000-\u001f\u007f-\u009f]/.test(value);

const isNativeIdentifier = (value: unknown): value is string =>
  typeof value === "string" &&
  value.length > 0 &&
  value.length <= 128 &&
  /^[a-z0-9._:/-]+$/.test(value) &&
  !value.includes("..");

const field = (
  record: Record<string, unknown>,
  camelCase: string,
  snakeCase: string,
) => record[camelCase] ?? record[snakeCase];

const finiteNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : null;

const finiteSignedNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const stringArray = (value: unknown): string[] | null =>
  Array.isArray(value) && value.every((item) => typeof item === "string")
    ? value
    : null;

function normalizeCharacterContext(
  value: unknown,
): NativeCharacterContextEvidence | null {
  const context = asRecord(value);
  if (
    !context ||
    typeof context.profileId !== "string" ||
    typeof context.characterId !== "string" ||
    typeof context.identitySource !== "string" ||
    typeof context.explicitSelection !== "boolean"
  )
    return null;

  let selection: NativeCharacterSelectionEvidence | undefined;
  if (context.selection !== undefined && context.selection !== null) {
    const raw = asRecord(context.selection);
    if (!raw || typeof raw.status !== "string") return null;
    if (
      raw.status === "known" &&
      typeof raw.character_id === "string" &&
      (raw.reason === "explicit" ||
        raw.reason === "addressed_alias" ||
        raw.reason === "sticky_current" ||
        raw.reason === "visual_confidence")
    ) {
      selection = {
        status: "known",
        character_id: raw.character_id,
        reason: raw.reason,
      };
    } else if (raw.status === "background") {
      selection = { status: "background" };
    } else if (raw.status === "ambiguous") {
      const candidateIds = stringArray(raw.candidate_ids);
      if (!candidateIds) return null;
      selection = { status: "ambiguous", candidate_ids: candidateIds };
    } else {
      return null;
    }
  }

  let encounter: NativeCharacterEncounterEvidence | undefined;
  if (context.encounter !== undefined && context.encounter !== null) {
    const raw = asRecord(context.encounter);
    const voice = raw ? asRecord(raw.selected_voice) : null;
    const traits = voice ? stringArray(voice.traits) : null;
    const created = raw ? finiteNumber(raw.created_at_ms) : null;
    const seen = raw ? finiteNumber(raw.last_seen_at_ms) : null;
    const expires = raw ? finiteNumber(raw.expires_at_ms) : null;
    if (
      !raw ||
      !voice ||
      !traits ||
      typeof raw.schema_version !== "string" ||
      typeof raw.encounter_id !== "string" ||
      typeof raw.game_profile_id !== "string" ||
      typeof raw.archetype_character_id !== "string" ||
      typeof raw.continuity_key_sha256 !== "string" ||
      created === null ||
      seen === null ||
      expires === null ||
      (raw.status !== "active" && raw.status !== "expired") ||
      typeof voice.binding_id !== "string" ||
      typeof voice.adapter_id !== "string" ||
      typeof voice.provider_voice_id !== "string" ||
      typeof voice.locale !== "string"
    )
      return null;
    encounter = {
      schema_version: raw.schema_version,
      encounter_id: raw.encounter_id,
      game_profile_id: raw.game_profile_id,
      archetype_character_id: raw.archetype_character_id,
      continuity_key_sha256: raw.continuity_key_sha256,
      selected_voice: {
        binding_id: voice.binding_id,
        adapter_id: voice.adapter_id,
        provider_voice_id: voice.provider_voice_id,
        locale: voice.locale,
        traits,
        ...(typeof voice.catalog_version === "string"
          ? { catalog_version: voice.catalog_version }
          : {}),
        ...(typeof voice.license === "string"
          ? { license: voice.license }
          : {}),
      },
      created_at_ms: created,
      last_seen_at_ms: seen,
      expires_at_ms: expires,
      status: raw.status,
    };
  }

  let prompt: NativeCharacterPromptEvidence | undefined;
  if (context.prompt !== undefined && context.prompt !== null) {
    const raw = asRecord(context.prompt);
    const provenance = raw ? asRecord(raw.retrievalProvenance) : null;
    const authorities = raw ? stringArray(raw.authorities) : null;
    const scopedMemoryItemIds = raw
      ? stringArray(raw.scopedMemoryItemIds)
      : null;
    const scopedMemoryClasses = raw
      ? stringArray(raw.scopedMemoryClasses)
      : null;
    const selectedProfileKnowledgeIds = provenance
      ? stringArray(provenance.selected_profile_knowledge_ids)
      : null;
    const selectedMemoryItemIds = provenance
      ? stringArray(provenance.selected_memory_item_ids)
      : null;
    const embeddingMetadataIds = provenance
      ? stringArray(provenance.embedding_metadata_ids)
      : null;
    const recordCount = raw ? finiteNumber(raw.recordCount) : null;
    const allowedAuthorities = new Set([
      "core_canon",
      "character_profile",
      "game_public",
      "character_authored",
      "session_summary",
      "recent_delivered_turns",
      "retrieved_memory",
    ]);
    if (
      !raw ||
      !provenance ||
      !authorities ||
      authorities.some((item) => !allowedAuthorities.has(item)) ||
      !scopedMemoryItemIds ||
      !scopedMemoryClasses ||
      !selectedProfileKnowledgeIds ||
      !selectedMemoryItemIds ||
      !embeddingMetadataIds ||
      recordCount === null ||
      typeof raw.schemaVersion !== "string" ||
      typeof raw.profileId !== "string" ||
      typeof raw.characterId !== "string" ||
      typeof provenance.schema_version !== "string" ||
      typeof provenance.policy_fingerprint_sha256 !== "string" ||
      typeof provenance.query_sha256 !== "string"
    )
      return null;
    prompt = {
      schemaVersion: raw.schemaVersion,
      profileId: raw.profileId,
      characterId: raw.characterId,
      authorities: authorities as NativeCharacterPromptEvidence["authorities"],
      recordCount,
      retrievalProvenance: {
        schema_version: provenance.schema_version,
        policy_fingerprint_sha256: provenance.policy_fingerprint_sha256,
        query_sha256: provenance.query_sha256,
        selected_profile_knowledge_ids: selectedProfileKnowledgeIds,
        selected_memory_item_ids: selectedMemoryItemIds,
        embedding_metadata_ids: embeddingMetadataIds,
      },
      scopedMemoryItemIds,
      scopedMemoryClasses,
    };
  }

  return {
    profileId: context.profileId,
    characterId: context.characterId,
    identitySource: context.identitySource,
    explicitSelection: context.explicitSelection,
    ...(selection ? { selection } : {}),
    ...(encounter ? { encounter } : {}),
    ...(prompt ? { prompt } : {}),
  };
}

function normalizeTurnExecution(
  value: unknown,
): NativeTurnExecutionEvidence | null {
  const execution = asRecord(value);
  const success = execution ? asRecord(execution.success) : null;
  const consumedRoute = execution ? asRecord(execution.consumedRoute) : null;
  const receipts = execution?.audioReceipts;
  const subtitleReceipts = execution?.subtitlePresentationReceipts;
  if (
    !execution ||
    !success ||
    !consumedRoute ||
    !Array.isArray(receipts) ||
    !Array.isArray(subtitleReceipts)
  )
    return null;

  let input: NativeTurnExecutionEvidence["input"] | undefined;
  if (execution.input !== undefined) {
    const rawInput = asRecord(execution.input);
    if (
      !rawInput ||
      !hasOnlyKeys(rawInput, [
        "mode",
        "pushToTalkState",
        "selectedSttReceipt",
      ]) ||
      (rawInput.mode !== "typed" && rawInput.mode !== "pushToTalk") ||
      (rawInput.pushToTalkState !== "notRequested" &&
        rawInput.pushToTalkState !== "transcriptReady" &&
        rawInput.pushToTalkState !== "captureUnavailable")
    ) {
      return null;
    }

    if (rawInput.mode === "typed") {
      if (
        rawInput.pushToTalkState !== "notRequested" ||
        rawInput.selectedSttReceipt !== undefined
      ) {
        return null;
      }
      input = { mode: "typed", pushToTalkState: "notRequested" };
    } else if (rawInput.pushToTalkState === "transcriptReady") {
      const receipt = asRecord(rawInput.selectedSttReceipt);
      const route = receipt ? asRecord(receipt.route) : null;
      if (
        !receipt ||
        !route ||
        !hasOnlyKeys(receipt, [
          "schemaVersion",
          "receiptId",
          "receiptSha256",
          "captureSessionId",
          "captureTurnId",
          "captureGeneration",
          "gameId",
          "characterId",
          "sourceLoadoutId",
          "route",
          "chunksSent",
          "pcmBytesSent",
          "partialEvents",
        ]) ||
        !hasOnlyKeys(route, [
          "providerId",
          "modelId",
          "credentialReference",
          "egress",
          "generation",
          "inputEndpointId",
          "inputEndpointGeneration",
          "manualRetry",
          "automaticFallback",
          "capturedFrames",
          "pttVirtualKey",
          "pttPressTransitionSequence",
          "pttPressedQpc",
          "pttReleaseTransitionSequence",
          "pttReleasedQpc",
        ]) ||
        receipt.schemaVersion !== 1 ||
        !isCanonicalNonNilUuid(receipt.receiptId) ||
        !isLowercaseSha256(receipt.receiptSha256) ||
        !isCanonicalNonNilUuid(receipt.captureSessionId) ||
        !isCanonicalNonNilUuid(receipt.captureTurnId) ||
        !isPositiveSafeInteger(receipt.captureGeneration) ||
        !isRouteValue(receipt.gameId, 128) ||
        (receipt.characterId !== null &&
          !isRouteValue(receipt.characterId, 128)) ||
        !isNativeIdentifier(receipt.sourceLoadoutId) ||
        !isPositiveSafeInteger(receipt.chunksSent) ||
        !isPositiveSafeInteger(receipt.pcmBytesSent) ||
        !isNonNegativeSafeInteger(receipt.partialEvents) ||
        route.providerId !== "assemblyai" ||
        route.modelId !== "u3-rt-pro" ||
        route.credentialReference !== "providers/assemblyai" ||
        route.egress !== "microphone_audio_and_optional_non_secret_context" ||
        !isPositiveSafeInteger(route.generation) ||
        route.generation !== receipt.captureGeneration ||
        !isRouteValue(route.inputEndpointId, 1024) ||
        !isPositiveSafeInteger(route.inputEndpointGeneration) ||
        typeof route.manualRetry !== "boolean" ||
        route.automaticFallback !== false ||
        !isPositiveSafeInteger(route.capturedFrames) ||
        route.pttVirtualKey !== 119 ||
        !isPositiveSafeInteger(route.pttPressTransitionSequence) ||
        !isPositiveSafeInteger(route.pttPressedQpc) ||
        !isPositiveSafeInteger(route.pttReleaseTransitionSequence) ||
        route.pttReleaseTransitionSequence <=
          route.pttPressTransitionSequence ||
        !isPositiveSafeInteger(route.pttReleasedQpc) ||
        route.pttReleasedQpc < route.pttPressedQpc
      ) {
        return null;
      }

      input = {
        mode: "pushToTalk",
        pushToTalkState: "transcriptReady",
        selectedSttReceipt: {
          schemaVersion: 1,
          receiptId: receipt.receiptId,
          receiptSha256: receipt.receiptSha256,
          captureSessionId: receipt.captureSessionId,
          captureTurnId: receipt.captureTurnId,
          captureGeneration: receipt.captureGeneration,
          gameId: receipt.gameId,
          characterId: receipt.characterId,
          sourceLoadoutId: receipt.sourceLoadoutId,
          route: {
            providerId: "assemblyai",
            modelId: "u3-rt-pro",
            credentialReference: "providers/assemblyai",
            egress: "microphone_audio_and_optional_non_secret_context",
            generation: route.generation,
            inputEndpointId: route.inputEndpointId,
            inputEndpointGeneration: route.inputEndpointGeneration,
            manualRetry: route.manualRetry,
            automaticFallback: false,
            capturedFrames: route.capturedFrames,
            pttVirtualKey: 119,
            pttPressTransitionSequence: route.pttPressTransitionSequence,
            pttPressedQpc: route.pttPressedQpc,
            pttReleaseTransitionSequence: route.pttReleaseTransitionSequence,
            pttReleasedQpc: route.pttReleasedQpc,
          },
          chunksSent: receipt.chunksSent,
          pcmBytesSent: receipt.pcmBytesSent,
          partialEvents: receipt.partialEvents,
        },
      };
    } else {
      if (
        rawInput.pushToTalkState !== "captureUnavailable" ||
        rawInput.selectedSttReceipt !== undefined
      ) {
        return null;
      }
      input = {
        mode: "pushToTalk",
        pushToTalkState: "captureUnavailable",
      };
    }
  }
  const normalizeProviderRoute = (value: unknown) => {
    if (value === null) return null;
    const route = asRecord(value);
    return route &&
      typeof route.providerId === "string" &&
      typeof route.modelId === "string" &&
      (typeof route.voiceId === "string" || route.voiceId === null)
      ? {
          providerId: route.providerId,
          modelId: route.modelId,
          voiceId: route.voiceId,
        }
      : undefined;
  };
  const llm = normalizeProviderRoute(consumedRoute.llm);
  const tts = normalizeProviderRoute(consumedRoute.tts);
  const privateEvaluation = asRecord(
    consumedRoute.privateEvaluationAcknowledgement,
  );
  const consumedGeneration = finiteNumber(consumedRoute.generation);
  const consumedSchemaVersion = finiteNumber(consumedRoute.schemaVersion);
  if (
    llm === undefined ||
    tts === undefined ||
    consumedGeneration === null ||
    consumedSchemaVersion === null ||
    typeof consumedRoute.sourceLoadoutId !== "string" ||
    typeof consumedRoute.sha256 !== "string"
  )
    return null;
  let privateEvaluationAcknowledgement:
    | NativeTurnExecutionEvidence["consumedRoute"]["privateEvaluationAcknowledgement"]
    | undefined;
  if (consumedRoute.privateEvaluationAcknowledgement !== undefined) {
    const privateEvaluationCatalogRevision = finiteNumber(
      privateEvaluation?.catalogRevision,
    );
    if (
      !privateEvaluation ||
      typeof privateEvaluation.providerId !== "string" ||
      typeof privateEvaluation.termsRevision !== "string" ||
      privateEvaluationCatalogRevision === null ||
      (privateEvaluation.applicationNamespace !==
        "io.github.akshitireddy.interactive-npcs.debug" &&
        privateEvaluation.applicationNamespace !==
          "io.github.akshitireddy.interactive-npcs.review") ||
      typeof privateEvaluation.acknowledgementSha256 !== "string" ||
      privateEvaluation.acknowledgementSha256.length === 0 ||
      !Array.isArray(privateEvaluation.modalities) ||
      privateEvaluation.modalities.length === 0 ||
      privateEvaluation.modalities.some(
        (modality) =>
          modality !== "llm" && modality !== "embeddings" && modality !== "tts",
      ) ||
      privateEvaluation.promotionSupported !== false ||
      privateEvaluation.publicationSupported !== false
    ) {
      return null;
    }
    privateEvaluationAcknowledgement = {
      providerId: privateEvaluation.providerId,
      termsRevision: privateEvaluation.termsRevision,
      catalogRevision: privateEvaluationCatalogRevision,
      applicationNamespace: privateEvaluation.applicationNamespace,
      acknowledgementSha256: privateEvaluation.acknowledgementSha256,
      modalities: privateEvaluation.modalities as Array<
        "llm" | "embeddings" | "tts"
      >,
      promotionSupported: false,
      publicationSupported: false,
    };
  }
  const booleanFields = [
    "llmProviderLive",
    "ttsProviderLive",
    "sttSkipped",
    "subtitleDelivered",
    "audioSubmitted",
    "audioDrained",
  ] as const;
  if (booleanFields.some((key) => typeof success[key] !== "boolean")) {
    return null;
  }
  const audioReceiptCount = finiteNumber(success.audioReceiptCount);
  const subtitleReceiptCount = finiteNumber(success.subtitleReceiptCount);
  if (audioReceiptCount === null || subtitleReceiptCount === null) return null;
  const audioReceipts = receipts.flatMap((value) => {
    const receipt = asRecord(value);
    if (
      !receipt ||
      typeof receipt.receiptId !== "string" ||
      receipt.receiptId.length === 0 ||
      typeof receipt.submitted !== "boolean" ||
      typeof receipt.drained !== "boolean"
    )
      return [];
    const hasEndpointEvidence =
      receipt.outputSelectionMode !== undefined ||
      receipt.outputEndpointId !== undefined ||
      receipt.outputEndpointGeneration !== undefined;
    const endpointGeneration = finiteNumber(receipt.outputEndpointGeneration);
    if (
      hasEndpointEvidence &&
      ((receipt.outputSelectionMode !== "systemDefault" &&
        receipt.outputSelectionMode !== "endpointId") ||
        typeof receipt.outputEndpointId !== "string" ||
        receipt.outputEndpointId.length === 0 ||
        endpointGeneration === null)
    )
      return [];
    return [
      {
        receiptId: receipt.receiptId,
        submitted: receipt.submitted,
        drained: receipt.drained,
        ...(hasEndpointEvidence
          ? {
              outputSelectionMode: receipt.outputSelectionMode as
                | "systemDefault"
                | "endpointId",
              outputEndpointId: receipt.outputEndpointId as string,
              outputEndpointGeneration: endpointGeneration as number,
            }
          : {}),
      },
    ];
  });
  if (audioReceipts.length !== receipts.length) return null;
  const subtitlePresentationReceipts = subtitleReceipts.flatMap((value) => {
    const receipt = asRecord(value);
    if (!receipt) return [];
    const numericFields = [
      "sentenceId",
      "presentationId",
      "targetGeometryEpoch",
      "captureSequence",
      "graphicsGeneration",
      "widthPx",
      "heightPx",
      "dpiX",
      "dpiY",
    ] as const;
    const numbers = Object.fromEntries(
      numericFields.map((key) => [key, finiteNumber(receipt[key])]),
    ) as Record<(typeof numericFields)[number], number | null>;
    const desktopXPx = finiteSignedNumber(receipt.desktopXPx);
    const desktopYPx = finiteSignedNumber(receipt.desktopYPx);
    const direction = receipt.direction;
    const colorTreatment = receipt.colorTreatment;
    if (
      numericFields.some((key) => numbers[key] === null) ||
      desktopXPx === null ||
      desktopYPx === null ||
      typeof receipt.receiptId !== "string" ||
      receipt.receiptId.length === 0 ||
      ![
        "trustedNativeCapture",
        "consoleBottomCenterUnavailable",
        "deterministicFixture",
      ].includes(receipt.provenance as string) ||
      typeof receipt.layerHashHex !== "string" ||
      receipt.layerHashHex.length === 0 ||
      typeof receipt.presentedQpcTicks !== "string" ||
      !/^[1-9][0-9]*$/.test(receipt.presentedQpcTicks) ||
      (direction !== "leftToRight" && direction !== "rightToLeft") ||
      ![
        "sdrPremultipliedSourceOver",
        "scrgbLinearSourceOver",
        "hdr10ToneMappedSourceOver",
        "windowsCompositorSdrWhiteMapping",
      ].includes(colorTreatment as string) ||
      typeof receipt.bidiShapingApplied !== "boolean" ||
      typeof receipt.graphemeClustersPreserved !== "boolean" ||
      typeof receipt.usedBottomCenterFallback !== "boolean" ||
      receipt.committed !== true ||
      numbers.widthPx === 0 ||
      numbers.heightPx === 0 ||
      numbers.dpiX === 0 ||
      numbers.dpiY === 0 ||
      (direction === "rightToLeft" && !receipt.bidiShapingApplied) ||
      !receipt.graphemeClustersPreserved
    )
      return [];
    return [
      {
        receiptId: receipt.receiptId,
        sentenceId: numbers.sentenceId as number,
        provenance:
          receipt.provenance as NativeSubtitlePresentationReceipt["provenance"],
        presentationId: numbers.presentationId as number,
        targetGeometryEpoch: numbers.targetGeometryEpoch as number,
        captureSequence: numbers.captureSequence as number,
        graphicsGeneration: numbers.graphicsGeneration as number,
        layerHashHex: receipt.layerHashHex,
        presentedQpcTicks: receipt.presentedQpcTicks,
        desktopXPx,
        desktopYPx,
        widthPx: numbers.widthPx as number,
        heightPx: numbers.heightPx as number,
        dpiX: numbers.dpiX as number,
        dpiY: numbers.dpiY as number,
        direction: direction as NativeSubtitlePresentationReceipt["direction"],
        bidiShapingApplied: receipt.bidiShapingApplied,
        graphemeClustersPreserved: receipt.graphemeClustersPreserved,
        usedBottomCenterFallback: receipt.usedBottomCenterFallback,
        colorTreatment:
          colorTreatment as NativeSubtitlePresentationReceipt["colorTreatment"],
        committed: true as const,
      },
    ];
  });
  if (
    subtitlePresentationReceipts.length !== subtitleReceipts.length ||
    subtitlePresentationReceipts.length !== subtitleReceiptCount ||
    (success.subtitleDelivered && subtitleReceiptCount === 0)
  )
    return null;
  const deliveryState = execution.deliveryState;
  const commitState = execution.commitState;
  const rawDegradations = execution.degradations;
  const degradations = Array.isArray(rawDegradations)
    ? rawDegradations.flatMap((value): NativeTurnExecutionDegradation[] => {
        const degradation = asRecord(value);
        if (!degradation || typeof degradation.reason !== "string") return [];
        if (
          degradation.type === "typedInput" ||
          degradation.type === "audioOnly" ||
          degradation.type === "subtitleOnly"
        ) {
          return [
            { type: degradation.type, reason: degradation.reason },
          ] as NativeTurnExecutionDegradation[];
        }
        if (
          degradation.type === "manualRetryRequired" &&
          typeof degradation.failedRole === "string" &&
          (typeof degradation.providerId === "string" ||
            degradation.providerId === null) &&
          typeof degradation.retryable === "boolean"
        ) {
          return [
            {
              type: "manualRetryRequired",
              failedRole: degradation.failedRole,
              providerId: degradation.providerId,
              reason: degradation.reason,
              retryable: degradation.retryable,
            },
          ];
        }
        return [];
      })
    : [];
  return {
    consumedRoute: {
      schemaVersion: consumedSchemaVersion,
      sourceLoadoutId: consumedRoute.sourceLoadoutId,
      generation: consumedGeneration,
      sha256: consumedRoute.sha256,
      llm,
      tts,
      ...(privateEvaluationAcknowledgement
        ? { privateEvaluationAcknowledgement }
        : {}),
    },
    ...(input ? { input } : {}),
    ...(deliveryState === "delivered" ||
    deliveryState === "cancelled" ||
    deliveryState === "manualRetryRequired"
      ? { deliveryState }
      : {}),
    ...(commitState === "committed" ||
    commitState === "notCommitted" ||
    commitState === "commitDeferred"
      ? { commitState }
      : {}),
    degradations,
    success: {
      llmProviderLive: success.llmProviderLive as boolean,
      ttsProviderLive: success.ttsProviderLive as boolean,
      sttSkipped: success.sttSkipped as boolean,
      subtitleDelivered: success.subtitleDelivered as boolean,
      subtitleReceiptCount,
      audioSubmitted: success.audioSubmitted as boolean,
      audioDrained: success.audioDrained as boolean,
      audioReceiptCount,
    },
    audioReceipts,
    subtitlePresentationReceipts,
  };
}

function normalizeVisualPresentation(
  value: unknown,
): NativeVisualPresentationEvidence | null {
  const receipt = asRecord(value);
  if (!receipt) return null;
  const schemaVersion = finiteNumber(
    field(receipt, "schemaVersion", "schema_version"),
  );
  const sourceFrameSequence = finiteNumber(
    field(receipt, "sourceFrameSequence", "source_frame_sequence"),
  );
  const pixelSource = field(receipt, "pixelSource", "pixel_source");
  const pixelScope = field(receipt, "pixelScope", "pixel_scope");
  if (
    schemaVersion === null ||
    sourceFrameSequence === null ||
    typeof receipt.residualProposed !== "boolean" ||
    typeof receipt.presented !== "boolean" ||
    typeof receipt.degraded !== "boolean" ||
    (typeof pixelSource !== "string" && pixelSource !== null) ||
    (typeof pixelScope !== "string" && pixelScope !== null) ||
    typeof receipt.detail !== "string"
  )
    return null;
  return {
    schemaVersion,
    sourceFrameSequence,
    residualProposed: receipt.residualProposed,
    presented: receipt.presented,
    degraded: receipt.degraded,
    pixelSource,
    pixelScope,
    detail: receipt.detail,
  };
}

/** Normalizes both current snake-case enum fields and future camel-case wire fields. */
export function normalizeNativeSimulationEvent(
  value: unknown,
): NativeSimulationEvent | null {
  const record = asRecord(value);
  if (!record || typeof record.type !== "string") return null;
  const simulationId = field(record, "simulationId", "simulation_id");
  const generation = finiteNumber(record.generation);
  const sequence = finiteNumber(record.sequence);
  if (
    typeof simulationId !== "string" ||
    generation === null ||
    sequence === null
  )
    return null;
  const identity = { simulationId, generation, sequence };

  switch (record.type) {
    case "started": {
      const measurementBasis = field(
        record,
        "measurementBasis",
        "measurement_basis",
      );
      if (
        measurementBasis !== "deterministicFixture" &&
        measurementBasis !== "trustedRuntimeFixture" &&
        measurementBasis !== "controlledBenchmark" &&
        measurementBasis !== "pendingProviderEvidence"
      )
        return null;
      return { type: "started", ...identity, measurementBasis };
    }
    case "stageStarted": {
      const stage = record.stage as StageId;
      const estimatedDurationMs = finiteNumber(
        field(record, "estimatedDurationMs", "estimated_duration_ms"),
      );
      if (!isStageId(stage) || estimatedDurationMs === null) return null;
      return {
        type: "stageStarted",
        ...identity,
        stage,
        estimatedDurationMs,
      };
    }
    case "stageCompleted": {
      const stage = record.stage as StageId;
      const fixtureElapsedMs = finiteNumber(
        field(record, "fixtureElapsedMs", "fixture_elapsed_ms"),
      );
      if (!isStageId(stage) || fixtureElapsedMs === null) return null;
      return {
        type: "stageCompleted",
        ...identity,
        stage,
        fixtureElapsedMs,
      };
    }
    case "sentenceReady":
      return typeof record.text === "string"
        ? { type: "sentenceReady", ...identity, text: record.text }
        : null;
    case "completed": {
      const deliveredText = field(record, "deliveredText", "delivered_text");
      if (typeof deliveredText !== "string") return null;
      const runtimeFixtureOnly = field(
        record,
        "runtimeFixtureOnly",
        "runtime_fixture_only",
      );
      const turnExecution = normalizeTurnExecution(
        field(record, "turnExecution", "turn_execution"),
      );
      const characterContext = normalizeCharacterContext(
        field(record, "characterContext", "character_context"),
      );
      const visualPresentation = normalizeVisualPresentation(
        field(record, "visualPresentation", "visual_presentation"),
      );
      return {
        type: "completed",
        ...identity,
        fixtureFirstAudioMs: finiteNumber(
          field(record, "fixtureFirstAudioMs", "fixture_first_audio_ms"),
        ),
        // Legacy runtimes omitted this field. Defaulting to fixture-only is the
        // conservative compatibility behavior: absence must never imply that
        // live audio was delivered.
        runtimeFixtureOnly:
          typeof runtimeFixtureOnly === "boolean" ? runtimeFixtureOnly : true,
        deliveredText,
        ...(turnExecution ? { turnExecution } : {}),
        ...(characterContext ? { characterContext } : {}),
        ...(visualPresentation ? { visualPresentation } : {}),
      };
    }
    case "cancelled":
    case "failed": {
      if (typeof record.reason !== "string") return null;
      const turnExecution = normalizeTurnExecution(
        field(record, "turnExecution", "turn_execution"),
      );
      const characterContext = normalizeCharacterContext(
        field(record, "characterContext", "character_context"),
      );
      return {
        type: record.type,
        ...identity,
        reason: record.reason,
        ...(turnExecution ? { turnExecution } : {}),
        ...(characterContext ? { characterContext } : {}),
      };
    }
    default:
      return null;
  }
}

const isStageId = (value: string): value is StageId =>
  [
    "listening",
    "transcribing",
    "identifying",
    "remembering",
    "responding",
    "voicing",
    "animating",
  ].includes(value);

const abortError = () => {
  const error = new Error("Native bootstrap polling was cancelled.");
  error.name = "AbortError";
  return error;
};

const sleep = (durationMs: number, signal?: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    if (signal?.aborted) {
      reject(abortError());
      return;
    }
    const timer = window.setTimeout(() => {
      signal?.removeEventListener("abort", cancel);
      resolve();
    }, durationMs);
    const cancel = () => {
      window.clearTimeout(timer);
      reject(abortError());
    };
    signal?.addEventListener("abort", cancel, { once: true });
  });

export async function loadNativeBootstrapHealth(
  options: {
    maxAttempts?: number;
    initialDelayMs?: number;
    maxElapsedMs?: number;
    signal?: AbortSignal;
    sleep?: (durationMs: number, signal?: AbortSignal) => Promise<void>;
    now?: () => number;
    onProgress?: (health: NativeBootstrapHealth) => void;
  } = {},
  invokeBootstrap?: () => Promise<NativeBootstrapSnapshot>,
): Promise<NativeBootstrapHealth> {
  if (!invokeBootstrap && !hasTauri())
    return { kind: "browserPreview", attempts: 0 };

  const maxAttempts = Math.max(1, options.maxAttempts ?? 30);
  const initialDelayMs = Math.max(0, options.initialDelayMs ?? 120);
  const maxElapsedMs = Math.max(0, options.maxElapsedMs ?? 20_000);
  const wait = options.sleep ?? sleep;
  const now = options.now ?? Date.now;
  const startedAt = now();
  const invokeSnapshot =
    invokeBootstrap ??
    (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<NativeBootstrapSnapshot>("bootstrap_snapshot");
    });
  let lastSnapshot: NativeBootstrapSnapshot | null = null;
  let lastError = "Native bootstrap did not return a snapshot.";
  let attemptsPerformed = 0;

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    if (options.signal?.aborted) throw abortError();
    attemptsPerformed = attempt;
    try {
      const snapshot = await invokeSnapshot();
      if (options.signal?.aborted) throw abortError();
      lastSnapshot = snapshot;
      const health: NativeBootstrapHealth = {
        kind: "snapshot",
        attempts: attempt,
        snapshot,
      };
      options.onProgress?.(health);
      if (snapshot.runtime.connected && snapshot.mediaBroker.connected)
        return health;
      lastError = [snapshot.runtime.detail, snapshot.mediaBroker.detail]
        .filter(Boolean)
        .join(" ");
    } catch (error) {
      if (error instanceof Error && error.name === "AbortError") throw error;
      lastError =
        error instanceof Error ? error.message : "Unknown bootstrap failure";
    }

    const elapsedMs = now() - startedAt;
    if (attempt >= maxAttempts || elapsedMs >= maxElapsedMs) break;
    const remainingMs = maxElapsedMs - elapsedMs;
    await wait(
      Math.min(initialDelayMs * 2 ** (attempt - 1), 1_000, remainingMs),
      options.signal,
    );
  }

  if (lastSnapshot)
    return {
      kind: "snapshot",
      attempts: attemptsPerformed,
      snapshot: lastSnapshot,
    };
  return {
    kind: "unavailable",
    attempts: attemptsPerformed,
    detail: lastError,
  };
}

/** Persists onboarding in the native app-data store. Browser previews do not write. */
export async function saveOnboarding(
  onboarding: OnboardingSnapshot,
): Promise<SaveOnboardingResult | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SaveOnboardingResult>("save_onboarding", { onboarding });
}

/** Runs the authenticated runtime's bounded doctor report when native. */
/** Rebuilds the control-plane diagnostic summary from current native state. */
export async function readDiagnosticSummary(): Promise<NativeDiagnosticSummary | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<NativeDiagnosticSummary>("diagnostic_summary");
}

export type SyntheticReplayCaptureAvailability =
  | {
      available: true;
      commandName: string;
    }
  | {
      available: false;
      reason: "browserPreview" | "releaseBuild";
    };

export type SyntheticReplayCaptureDiagnostics = NativeMediaBrokerDiagnostics;

export interface NativeCaptureEvidence {
  schemaVersion: 3;
  selectedProcessId: number;
  selectedWindowHandle: number;
  deviceGeneration: number;
  geometryEpoch: number;
  latestFrameSequence: number;
  latestFrameQpc: number;
  initialContentHash: number;
  latestContentHash: number;
  contentHashChanges: number;
  geometryChanges: number;
  nonadvancingFrames: number;
  contentWidth: number;
  contentHeight: number;
  overlayCaptureExcluded: boolean;
  overlayVisualsAllowed: boolean;
  pixelSource: "windowsGraphicsCaptureTexture" | "desktopDuplicationTexture";
  pixelScope: "exactSelectedWindow" | "fullDisplayOutput";
  externalDisplayOverlayPixelsExcluded: boolean;
  desktopLuminanceExcludedFromPixelEvidence: boolean;
  externalDisplayOverlaysMayChangePerceivedBrightness: true;
  selectedExecutableName: string;
}

export interface NativeGameCaptureVerification {
  schemaVersion: 1;
  gameProfileId: string;
  target: NativeGameTargetCandidate;
  capture: NativeCaptureEvidence;
  exactPidHwndExecutableMatch: true;
  frameSequenceAdvanced: true;
  contentChanged: boolean;
  reviewFixtureMotionMode?: string;
  safetyState: "verified_synthetic_fixture" | "verified_exact_window_wgc";
}

export interface SyntheticReplayCaptureResult {
  targetProcessId: number;
  targetWindowHandle: number;
  targetExecutableBasename: string;
  fixtureMotionMode: string;
  diagnostics: SyntheticReplayCaptureDiagnostics;
  captureEvidence?: NativeCaptureEvidence;
}

export interface PreparedSyntheticReviewTarget {
  schemaVersion: 1;
  launched: boolean;
  executablePath: string;
  targetProcessId: number;
  targetWindowHandle: number;
  targetExecutableBasename: string;
  fixtureMotionMode: string;
}

export function syntheticReviewTargetAvailability(
  debugCapabilityEnabled = false,
): SyntheticReplayCaptureAvailability {
  if (!hasTauri()) return { available: false, reason: "browserPreview" };
  if (!debugCapabilityEnabled)
    return { available: false, reason: "releaseBuild" };
  return {
    available: true,
    commandName: "prepare_synthetic_review_target",
  };
}

export async function prepareSyntheticReviewTarget(
  availability = syntheticReviewTargetAvailability(),
): Promise<PreparedSyntheticReviewTarget> {
  if (!availability.available) {
    throw new Error(
      availability.reason === "browserPreview"
        ? "The local test game requires the native desktop shell."
        : "The local test game is unavailable in release builds.",
    );
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<PreparedSyntheticReviewTarget>(availability.commandName);
}

/**
 * The bootstrap capability comes from Rust's `cfg(debug_assertions)` build.
 * Release builds never advertise or render this control surface.
 */
export function syntheticReplayCaptureAvailability(
  debugCapabilityEnabled = false,
): SyntheticReplayCaptureAvailability {
  if (!hasTauri()) return { available: false, reason: "browserPreview" };
  if (!debugCapabilityEnabled)
    return { available: false, reason: "releaseBuild" };
  return {
    available: true,
    commandName: "debug_select_synthetic_replay_capture_target",
  };
}

export async function runSyntheticReplayCapture(
  availability = syntheticReplayCaptureAvailability(),
): Promise<SyntheticReplayCaptureResult> {
  if (!availability.available) {
    throw new Error(
      availability.reason === "browserPreview"
        ? "Synthetic replay capture requires the native desktop shell."
        : "Synthetic replay capture is unavailable in release builds.",
    );
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SyntheticReplayCaptureResult>(availability.commandName);
}

export interface NativeGameTargetCandidate {
  processId: number;
  nativeWindow: number;
  executableName: string;
  executablePathSha256: string;
  title: string;
  foreground: boolean;
  clientWidth: number;
  clientHeight: number;
}

export interface NativeGameTargetSelection {
  schemaVersion: number;
  gameProfileId: string;
  target: NativeGameTargetCandidate;
  processInstanceBound: boolean;
  userConfirmedOfflineSinglePlayer: boolean;
  captureAuthorized: boolean;
  safetyState: string;
  safetyDetail: string;
}

export interface NativeCharacterInspection {
  schemaVersion: number;
  gameProfileId: string;
  gameDisplayName: string;
  selectedCharacterId: string | null;
  character: {
    id: string;
    displayName: string;
    aliases: string[];
    biography: string;
    personality: string;
    dialogueStyle: string;
    styleExamples: Array<{
      id: string;
      speaker: string;
      text: string;
      situationTags: string[];
      toneTags: string[];
      weightMillis: number;
      provenanceId: string;
    }>;
    openingLines: string[];
    backgroundNpc: boolean;
    promptRole: string;
    promptObjectives: string[];
    promptConstraints: string[];
    knowledgeRefs: string[];
    voice: {
      description: string;
      locale: string;
      styleTags: string[];
      providerVoiceId: string | null;
      adapterId: string | null;
      catalogVersion: string | null;
      license: string | null;
      userOverrideAllowed: boolean;
    };
    identity: {
      strategy: string;
      evidence: string[];
      fallback: string;
      automaticFaceRecognitionClaimed: boolean;
    };
  };
  authoredKnowledge: Array<{
    id: string;
    authority: string;
    ownerCharacterId: string | null;
    text: string;
    topicTags: string[];
    spoilerTier: string;
    provenanceId: string;
  }>;
  provenance: Array<{
    id: string;
    title: string;
    kind: string;
    sourceUrl: string | null;
    license: string | null;
    notes: string | null;
    sourceRevision: string | null;
    sourcePath: string | null;
    sourceSha256: string | null;
    reviewStatus: string | null;
    transformVersion: string | null;
  }>;
  deliveredMemory: Array<{
    turnId: string;
    speaker: string;
    deliveredText: string;
    contentSha256: string;
    deliveredAtMs: number;
    sequence: number;
    providerId: string | null;
    deliveryReceiptId: string | null;
    provenanceSourceKind: string;
    provenanceSourceId?: string;
    provenanceSourceUri?: string;
  }>;
  memoryScope: {
    userId: string;
    profileId: string;
    gameId: string;
    characterId: string;
    sessionId: string | null;
    saveId: string | null;
    crossGameWideningAllowed: boolean;
  };
}

export interface NativeSelectedCharacterResult {
  gameProfileId: string;
  characterId: string;
  persisted: boolean;
}

export interface NativeCharacterMemoryStatus {
  schemaVersion: number;
  gameProfileId: string;
  characterId: string;
  deliveredTurns: number;
  structuredMemories: number;
  legacyItems: number;
  hasMemory: boolean;
  storeSchemaVersion: number;
  integrityOk: boolean;
  integrityMessages: string[];
  databaseBytes: number;
  crossGameWideningAllowed: false;
}

export interface NativeLocalMemoryBackup {
  backupId: string;
  createdAtMs?: number;
  bytes: number;
  pagesCopied?: number;
  containsAllLocalMemory: true;
  localOnly: true;
}

export interface NativeCharacterMemoryEraseResult {
  gameProfileId: string;
  characterId: string;
  erasureId: string;
  scopeSha256: string;
  deliveredTurnsDeleted: number;
  structuredMemoriesDeleted: number;
  legacyItemsDeleted: number;
  outboxJobsDeleted: number;
  erasedAtMs: number;
  backup: NativeLocalMemoryBackup | null;
  backupRetainsErasedData: boolean;
  crossGameWideningAllowed: false;
}

export interface NativeCharacterMemoryRestoreResult {
  operationId: string;
  backupId: string;
  restored: boolean;
  priorStoreQuarantined: boolean;
  reopenedIntegrityOk: boolean;
  storeSchemaVersion: number;
  deliveredTurns: number;
  structuredMemories: number;
  backupsPreserved: boolean;
  runtimeRestartsOnNextUse: boolean;
  auditPersisted: boolean;
}

export interface NativeRemoveAllLocalMemoryResult {
  operationId: string;
  removedArtifacts: number;
  removedBytes: number;
  backupsRemoved: boolean;
  backupsPreserved: boolean;
  priorStoreQuarantinesRemoved: boolean;
  emptyStoreReopened: boolean;
  reopenedIntegrityOk: boolean;
  storeSchemaVersion: number;
  runtimeRestartsOnNextUse: boolean;
  auditPersisted: boolean;
}

export interface NativeCharacterCatalogSnapshot {
  schemaVersion: number;
  gameProfileId: string;
  gameDisplayName: string;
  selectedCharacterId: string | null;
  defaultCharacterId: string;
  characters: Array<{
    id: string;
    displayName: string;
    aliases: string[];
    backgroundNpc: boolean;
    voiceDescription: string;
    identityStrategy: string;
  }>;
  editableAuthoredData: boolean;
}

export interface NativeResourceGovernorPolicy {
  schema: string;
  vram_soft_ceiling_basis_points: number;
  ram_soft_ceiling_basis_points: number;
  minimum_vram_safety_bytes: number;
  proportional_vram_safety_basis_points: number;
  minimum_ram_safety_bytes: number;
  maximum_snapshot_age_millis: number;
  keep_warm_millis: number;
  unload_ttl_millis: number;
}

export interface NativeLocalResourceSettings {
  schemaVersion: number;
  governor: NativeResourceGovernorPolicy;
  gameReserveVramBytes: number;
  gameAdditionalReserveRamBytes: number;
  preferredResidency: "cpu_resident" | "gpu_resident" | "cpu_resident_gpu_cold";
}

export type NativeTelemetryObservation<T> =
  | {
      availability: "available";
      value: T;
      provenance: {
        captured_unix_millis: number;
        captured_monotonic_millis: number;
        source: string;
      };
    }
  | {
      availability: "unavailable";
      reason:
        | string
        | { os_error?: { code: number }; driver_error?: { code: number } };
      provenance: {
        captured_unix_millis: number;
        captured_monotonic_millis: number;
        source: string;
      };
    };

export interface NativeResourceTelemetrySnapshot {
  schema: string;
  captured_unix_millis: number;
  captured_monotonic_millis: number;
  selected_game_pid: number | null;
  physical_ram_bytes: NativeTelemetryObservation<number>;
  available_ram_bytes: NativeTelemetryObservation<number>;
  adapter: NativeTelemetryObservation<{
    description: string;
    luid: { low_part: number; high_part: number };
    vendor_id: number;
    device_id: number;
    subsystem_id: number;
    revision: number;
  }>;
  device_fingerprint_sha256: NativeTelemetryObservation<string>;
  dedicated_vram_bytes: NativeTelemetryObservation<number>;
  os_local_vram_budget_bytes: NativeTelemetryObservation<number>;
  current_process_local_vram_bytes: NativeTelemetryObservation<number>;
  total_device_pressure_vram_bytes: NativeTelemetryObservation<number>;
  selected_game_working_set_bytes: NativeTelemetryObservation<number>;
  selected_game_vram_bytes: NativeTelemetryObservation<number>;
}

export interface NativeResourceTelemetryResult {
  settings: NativeLocalResourceSettings;
  snapshot: NativeResourceTelemetrySnapshot;
  admissionReady: boolean;
  admissionDetail: string;
}

export type NativeModelPackRole =
  | "language_model"
  | "speech_recognition"
  | "speech_synthesis"
  | "embedding"
  | "vision"
  | "lip_sync"
  | "animation"
  | "other";

export interface NativeSelectedLoadoutSelection {
  selection_id: string;
  roles: Array<{
    role: NativeModelPackRole;
    identity: { pack_id: string; revision: string };
    preferred_residency:
      | "cpu_resident"
      | "gpu_resident"
      | "cpu_resident_gpu_cold";
  }>;
  expected_idle_millis: number;
}

export interface NativeSelectedLoadoutPlannerResult {
  schemaVersion: number;
  ready: boolean;
  detail: string;
  selected: NativeSelectedLoadoutSelection | null;
  planner: {
    schema: string;
    trusted_measurement_streams: number;
    active_evidence: Array<{ pack_id: string; revision: string }>;
    pending_work: number;
    pending_by_kind: Record<string, number>;
  } | null;
}

export interface NativeSelectedLoadoutAdmissionResult {
  schemaVersion: number;
  ready: boolean;
  detail: string;
  persisted: boolean;
  decision: {
    schema: string;
    selection_id: string;
    status: "admitted" | "blocked";
    reason_code: string | null;
    detail: string;
    exact_target_pid: number | null;
    selected_roles: NativeSelectedLoadoutSelection["roles"];
    live_snapshot: NativeResourceTelemetrySnapshot | null;
    admission_receipt: {
      snapshot_monotonic_millis: number;
      device_fingerprint_sha256: string;
      models: unknown[];
      vram_soft_ceiling_bytes: number;
      protected_desktop_and_game_vram_bytes: number;
      selected_peak_vram_bytes: number;
      vram_safety_bytes: number;
      projected_total_vram_bytes: number;
      ram_soft_ceiling_bytes: number;
      selected_peak_ram_bytes: number;
      ram_safety_bytes: number;
    } | null;
    residency_decisions: Array<{
      role: NativeModelPackRole;
      identity: { pack_id: string; revision: string };
      current: string;
      disposition: "keep_warm" | "cpu_resident_gpu_cold" | "unload";
      expected_idle_millis: number;
      compared_p99_reload_millis: number;
      evidence_placement: NativeSelectedLoadoutSelection["roles"][number]["preferred_residency"];
      measured_envelope_sha256: string;
    }>;
    pressure_cancellations: Array<{ work_id: string; reason: string }>;
  } | null;
}

export interface NativeTrustedLocalPackCatalog {
  schemaVersion: number;
  ready: boolean;
  detail: string;
  trustScope: "automated_local_review_bootstrap" | "production_release" | null;
  productionTrust: boolean;
  rotationRequiredBeforeRelease: boolean;
  promotionSupported: boolean;
  publicationSupported: boolean;
  packs: NativeTrustedLocalPack[];
}

export interface NativeTrustedLocalPack {
  identity: { pack_id: string; revision: string };
  display_name: string;
  description: string;
  recommendation_reason: string | null;
  capability: { kind: NativeModelPackRole; scope: "generic" | "game_specific" };
  runtime: string;
  runtime_revision: string | null;
  abi: string;
  backends: string[];
  exact_artifact_download_bytes: number;
  installed_bytes: number;
  planning_storage_bytes: number;
  planning_peak_install_bytes: number;
  admission_state:
    | "candidate_unqualified"
    | "blocked_pending_measurement"
    | "eligible_after_external_admission";
  admission_reason: string;
  allowed_residencies: NativeSelectedLoadoutSelection["roles"][number]["preferred_residency"][];
  license: {
    spdx_expression: string | null;
    license_name: string;
    license_url: string;
    redistributable: boolean;
    acceptance_required: boolean;
    component_ids: string[];
  };
  lifecycle: {
    explicit_download_required: boolean;
    automatic_download_allowed: boolean;
    install_strategy: "verify_then_atomic_activate";
    repair_strategy: "verify_quarantine_reinstall";
    remove_requires_unreferenced: boolean;
    activation_gates: string[];
  };
  non_qualifying_review_evidence_count: number;
  qualified_envelope_count: number;
  measurement_status: "unavailable" | "qualified";
  measurement_detail: string;
  qualified_measurement: {
    schema: string;
    report_id: string;
    sequence: number;
    measured_unix_seconds: number;
    expires_unix_seconds: number;
    device_fingerprint_sha256: string;
    identity: { pack_id: string; revision: string };
    manifest_sha256: string;
    capability: NativeModelPackRole;
    benchmark_suite_revision: string;
    runtime: string;
    runtime_revision: string;
    backend: string;
    sample_count: number;
    placements: Partial<
      Record<
        NativeSelectedLoadoutSelection["roles"][number]["preferred_residency"],
        {
          resident_ram_bytes: number;
          p99_total_ram_bytes: number;
          resident_vram_bytes: number;
          p99_workspace_vram_bytes: number;
          p99_load_millis: number;
          p99_reload_millis: number;
          p99_operation_millis: number;
        }
      >
    >;
  } | null;
}

export type NativeTrustedOptionalPackPhase =
  | "not_installed"
  | "downloading"
  | "verifying"
  | "staging"
  | "installed_inactive_awaiting_self_test"
  | "active"
  | "repairing"
  | "repair_required"
  | "removing"
  | "rolled_back";

export interface NativeTrustedOptionalPackState {
  identity: { pack_id: string; revision: string };
  phase: NativeTrustedOptionalPackPhase;
  explicitDownloadRequired: boolean;
  automaticDownloadAllowed: boolean;
  licenseAcceptanceRequired: boolean;
  canInstall: boolean;
  canRepair: boolean;
  canRemove: boolean;
  detail: string;
}

export interface NativeTrustedOptionalPackLifecycle {
  schemaVersion: number;
  ready: boolean;
  detail: string;
  packs: NativeTrustedOptionalPackState[];
}

export interface NativeTrustedOptionalPackActivationReceipt {
  schemaVersion: number;
  identity: { pack_id: string; revision: string };
  manifestSha256: string;
  installedContentTreeSha256: string;
  attestationSha256: string;
  providerLoadDurationMillis: number;
  trustDomain: "local_review_dev_only" | "release_threshold";
  detail: string;
}

export interface NativeTrustedOptionalPackActivationResult {
  schemaVersion: number;
  receipt: NativeTrustedOptionalPackActivationReceipt;
  lifecycle: NativeTrustedOptionalPackLifecycle;
}

export type NativeExperimentalPackPhase =
  | "not_installed"
  | "downloading"
  | "installed_inactive"
  | "activation_blocked_missing_measured_envelope"
  | "repair_required";

export interface NativeExperimentalPackState {
  schemaVersion: number;
  packId: string;
  revision: string;
  phase: NativeExperimentalPackPhase;
  installedArtifactSha256: string[];
  trustDomain: string;
  experimental: boolean;
  explicitDownloadRequired: boolean;
  completeLipSyncModel: boolean;
  detail: string;
}

export interface NativeDiagnosticEventV2 {
  monotonicNs: number;
  component: string;
  eventName: string;
  severity: "trace" | "debug" | "info" | "warn" | "error";
  status: "ok" | "degraded" | "failed" | "skipped";
  provenance: "measured" | "unmeasured" | "fixture";
  traceId: string | null;
  spanId: string | null;
  durationMs: number | null;
  timing: unknown | null;
  errorCode: string | null;
  providerId: string | null;
  modelId: string | null;
  numeric: Record<string, number>;
  labels: Record<string, string>;
}

export interface NativeRecoveryMetadata {
  schemaVersion: string;
  currentSessionId: string;
  previousSession: {
    status: "no_marker" | "unclean_exit" | "corrupt_marker";
    sessionId: string | null;
    applicationVersion: string | null;
    startedAtUtc: string | null;
    lastUpdatedAtUtc: string | null;
    lastPhase: string | null;
    lastEventMonotonicNs: number | null;
    priorRecoveryAttempt: number | null;
  };
  suggestedActions: Array<{
    actionId: string;
    kind: string;
    label: string;
    targetId: string | null;
    requiresConfirmation: boolean;
  }>;
}

export interface NativeDiagnosticsV2Snapshot {
  schemaVersion: number;
  maximumDiskBytes: number;
  requestedEventLimit: number;
  events: Array<{
    schemaVersion: string;
    sessionId: string;
    sequence: number;
    event: NativeDiagnosticEventV2;
    redactions: unknown[];
  }>;
  skippedCorruptRecords: number;
  privacy: {
    schemaVersion: string;
    remoteTelemetry: "prohibited";
    automaticUpload: "prohibited";
    exportInitiation: "user_initiated_only";
    locallyRecorded: string[];
    excludedByDesign: string[];
  };
  recovery: NativeRecoveryMetadata;
  verbosity: NativeDiagnosticVerbosity;
}

export type NativeDiagnosticVerbosity = "essential" | "standard" | "verbose";

export interface NativeDiagnosticsSettings {
  schemaVersion: 1;
  verbosity: NativeDiagnosticVerbosity;
}

export interface NativeDiagnosticMatrixCheck {
  checkId: string;
  category: string;
  status: "ok" | "degraded" | "failed" | "skipped";
  provenance: "measured" | "unmeasured" | "fixture";
  observedAtUtc: string | null;
  durationMs: number | null;
  timing: unknown | null;
  summaryCode: string;
  summary: string;
  errorCode: string | null;
  providerId: string | null;
  modelId: string | null;
  metrics: Record<
    string,
    {
      value: number;
      unit: string;
      provenance: "measured" | "unmeasured" | "fixture";
    }
  >;
  suggestedActions: Array<{
    actionId: string;
    kind:
      | "retry_check"
      | "restart_component"
      | "recheck_permissions"
      | "open_settings_section"
      | "open_bundled_help"
      | "select_alternative_provider"
      | "reduce_local_model_load"
      | "disable_optional_feature";
    label: string;
    targetId: string | null;
    requiresConfirmation: boolean;
  }>;
}

export interface NativeDiagnosticsMatrix {
  schemaVersion: "1.0.0";
  correlatedTurnId: string | null;
  credentialPresence: Array<{
    providerId: string;
    state: "present" | "absent" | "unavailable" | "unknown";
    provenance: "measured" | "unmeasured" | "fixture";
    observedAtUtc: string | null;
  }>;
  checks: NativeDiagnosticMatrixCheck[];
}

export interface NativeDiagnosticsExportResult {
  fileName: string;
  preview: {
    schemaVersion: string;
    checkCount: number;
    eventCount: number;
    includesRecoveryMetadata: boolean;
    serializedBytes: number;
    sha256: string;
    redactions: unknown[];
    remoteTelemetry: boolean;
    automaticUpload: boolean;
  };
  uploaded: false;
}

export type NativeBenchmarkRunState =
  | "idle"
  | "running"
  | "cancelling"
  | "completed"
  | "partial"
  | "unavailable"
  | "cancelled"
  | "failed";

export interface NativeBenchmarkStatus {
  state: NativeBenchmarkRunState;
  reportId: string | null;
  requestedIterations: number;
  completedIterations: number;
  startedAtUtc: string | null;
  elapsedMillis: number;
  reportFileName: string | null;
  unavailableComponents: string[];
  actionCodes: string[];
}

export interface NativeBenchmarkComponentReadiness {
  component: string;
  availability: "ready" | "unavailable";
  reason?: string;
  action?: string;
}

export interface NativeBenchmarkMetricSummary {
  metric: string;
  unit: string;
  sampleCount: number;
  minimum: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  maximum: number;
}

export interface NativeBenchmarkReport {
  schemaVersion: string;
  reportId: string;
  generatedAtUtc: string;
  state: Exclude<NativeBenchmarkRunState, "idle" | "running" | "cancelling">;
  classification: {
    executionMode: "live" | "mocked" | "simulated";
    measurementKind: "measured" | "planning_estimate";
    acceptanceEligible: boolean;
    reason: string;
  };
  bounds: {
    requestedIterations: number;
    completedIterations: number;
    timeoutMillis: number;
    baselineWindowMillis: number;
    elapsedMillis: number;
    completedWithinBounds: boolean;
  };
  binding: {
    gameProfileId: string;
    executableSha256: string;
    targetInstanceRecorded: false;
    loadoutRevision: string;
    providerRoutes: Array<{
      role: string;
      providerId: string;
      modelId: string;
      routeRevision: string;
      executionMode: "live" | "mocked" | "simulated";
    }>;
  };
  hardware: {
    operatingSystem: string;
    architecture: string;
    logicalProcessorCount: number;
    adapterDescription: string | null;
    adapterFingerprintSha256: string | null;
    physicalRamBytes: number | null;
    dedicatedVramBytes: number | null;
    hostnameRecorded: false;
    environmentVariablesRecorded: false;
  };
  provenance: {
    runtimeRevision: string;
    brokerRevision: string;
    compositorRevision: string;
    processLoadSamplerRevision: string;
    gameFrameSamplerRevision: string;
    timingClock: string;
    systemTelemetrySchema: string;
    providerPayloadsRecorded: false;
    promptsRecorded: false;
    transcriptsRecorded: false;
    audioRecorded: false;
    screenshotsRecorded: false;
    credentialsRecorded: false;
    filePathsRecorded: false;
  };
  coverage: {
    components: NativeBenchmarkComponentReadiness[];
    unavailableComponents: string[];
    invalidReceiptCount: number;
    failedIterationCount: number;
  };
  metrics: NativeBenchmarkMetricSummary[];
  frameImpact: {
    baselineFrameTimeMs: NativeBenchmarkMetricSummary | null;
    activeFrameTimeMs: NativeBenchmarkMetricSummary | null;
    baselineFps: NativeBenchmarkMetricSummary | null;
    activeFps: NativeBenchmarkMetricSummary | null;
    p50FrameTimeDeltaMs: number | null;
    p95FrameTimeDeltaMs: number | null;
    p50FpsDelta: number | null;
    p50FpsImpactPercent: number | null;
  };
  persistence: {
    state: string;
    reportFileName: string | null;
    atomicWrite: boolean;
    reportDirectoryRecorded: false;
  };
}

async function nativeInvoke<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export const discoverGameTargets = (gameProfileId: string) =>
  nativeInvoke<NativeGameTargetCandidate[]>("discover_game_targets", {
    gameProfileId,
  });

export const readSelectedGameTarget = () =>
  nativeInvoke<NativeGameTargetSelection | null>("selected_game_target");

export const selectGameTarget = (
  gameProfileId: string,
  nativeWindowHint: number,
  explicitUserConfirmedOfflineSinglePlayer: boolean,
) =>
  nativeInvoke<NativeGameTargetSelection>("select_game_target", {
    request: {
      gameProfileId,
      nativeWindowHint,
      explicitUserConfirmedOfflineSinglePlayer,
    },
  });

export const clearGameTarget = () => nativeInvoke<void>("clear_game_target");

export const startManualActorPicker = () =>
  nativeInvoke<NativeManualActorPickerPresentation>(
    "start_manual_actor_picker",
  );

export const readManualActorPickerStatus = () =>
  nativeInvoke<NativeManualActorPickerPresentation>(
    "manual_actor_picker_status",
  );

export const cancelManualActorPicker = () =>
  nativeInvoke<NativeManualActorPickerPresentation>(
    "cancel_manual_actor_picker",
  );

export const verifySelectedGameCapture = () =>
  nativeInvoke<NativeGameCaptureVerification>("verify_selected_game_capture");

export const inspectCharacterDatabase = (
  gameProfileId: string,
  characterId?: string,
) =>
  nativeInvoke<NativeCharacterInspection>("character_database_inspection", {
    request: {
      gameProfileId,
      ...(characterId ? { characterId } : {}),
      recentTurnLimit: 20,
    },
  });

export const readCharacterDatabaseCatalog = (gameProfileId: string) =>
  nativeInvoke<NativeCharacterCatalogSnapshot>("character_database_catalog", {
    gameProfileId,
  });

export const persistSelectedCharacter = (
  gameProfileId: string,
  characterId: string,
) =>
  nativeInvoke<NativeSelectedCharacterResult>("select_game_character", {
    gameProfileId,
    characterId,
  });

export const readCharacterMemoryStatus = (
  gameProfileId: string,
  characterId: string,
) =>
  nativeInvoke<NativeCharacterMemoryStatus>("character_memory_status", {
    request: { gameProfileId, characterId },
  });

export const backupAllLocalMemory = () =>
  nativeInvoke<NativeLocalMemoryBackup>("backup_all_local_memory", {
    request: { explicitUserConfirmation: true },
  });

export const listLocalMemoryBackups = () =>
  nativeInvoke<NativeLocalMemoryBackup[]>("list_local_memory_backups");

export const deleteLocalMemoryBackup = (backupId: string) =>
  nativeInvoke<{ backupId: string; deleted: boolean }>(
    "delete_local_memory_backup",
    {
      request: { backupId, explicitUserConfirmation: true },
    },
  );

export const eraseCharacterMemory = (
  gameProfileId: string,
  characterId: string,
  backupBeforeErasure: boolean,
) =>
  nativeInvoke<NativeCharacterMemoryEraseResult>("erase_character_memory", {
    request: {
      gameProfileId,
      characterId,
      explicitUserConfirmation: true,
      backupBeforeErasure,
    },
  });

export const restoreLocalMemoryBackup = (backupId: string) =>
  nativeInvoke<NativeCharacterMemoryRestoreResult>(
    "restore_local_memory_backup",
    {
      request: { backupId, explicitUserConfirmation: true },
    },
  );

export const removeAllLocalMemory = (includeBackups: boolean) =>
  nativeInvoke<NativeRemoveAllLocalMemoryResult>("remove_all_local_memory", {
    request: { explicitUserConfirmation: true, includeBackups },
  });

export const correctEncounterToAuthoredCharacter = (request: {
  gameProfileId: string;
  encounterId: string;
  characterId: string;
  explicitUserConfirmation: true;
}) =>
  nativeInvoke<NativeEncounterMutationResult>(
    "correct_encounter_to_authored_character",
    { request },
  );

export const mergeUnknownEncounters = (request: {
  gameProfileId: string;
  sourceEncounterId: string;
  destinationEncounterId: string;
  explicitUserConfirmation: true;
}) =>
  nativeInvoke<NativeEncounterMutationResult>("merge_unknown_encounters", {
    request,
  });

export const readLocalResourceSettings = () =>
  nativeInvoke<NativeLocalResourceSettings>("local_resource_settings");

export const enumerateAudioOutputs = () =>
  nativeInvoke<NativeAudioOutputSnapshot>("enumerate_audio_outputs");

export const readSelectedAudioOutput = () =>
  nativeInvoke<NativeSelectedAudioOutput | null>("selected_audio_output");

export const selectAudioOutput = (selection: NativeAudioOutputSelection) =>
  nativeInvoke<NativeSelectedAudioOutput>("select_audio_output", {
    selection,
  });

export const enumerateAudioInputs = () =>
  nativeInvoke<NativeAudioInputSnapshot>("enumerate_audio_inputs");

export const readSelectedAudioInput = () =>
  nativeInvoke<NativeSelectedAudioInput | null>("selected_audio_input");

export const selectAudioInput = (selection: NativeAudioInputSelection) =>
  nativeInvoke<NativeSelectedAudioInput>("select_audio_input", {
    selection,
  });

export const readIdentityReferenceEnrollmentStatus = () =>
  nativeInvoke<NativeIdentityReferenceEnrollmentStatus>(
    "identity_reference_enrollment_status",
  );

export const enrollIdentityReference = (
  request: NativeIdentityReferenceEnrollmentRequest,
) =>
  nativeInvoke<NativeIdentityReferenceEnrollmentReceipt>(
    "enroll_identity_reference",
    { request },
  );

export const readProductPreferences = (scope: NativeProductPreferenceScope) =>
  nativeInvoke<NativeProductPreferenceSnapshot>(
    "product_preferences_snapshot",
    {
      scope,
    },
  );

export const readEffectiveConfiguration = (
  scope: NativeProductPreferenceScope,
) =>
  nativeInvoke<NativeEffectiveConfigurationSnapshot>(
    "effective_configuration_snapshot",
    { request: { scope } },
  );

export const readSubtitlePreferences = (scope: NativeSubtitlePreferenceScope) =>
  nativeInvoke<NativeSubtitlePreferenceSnapshot>("read_subtitle_preferences", {
    request: { scope },
  });

export const saveSubtitlePreferences = (
  expectedRevision: number,
  entry: NativeScopedSubtitlePreferences,
) =>
  nativeInvoke<NativeSubtitlePreferenceSnapshot>("save_subtitle_preferences", {
    request: { expectedRevision, entry },
  });

export const resetSubtitlePreferences = (
  expectedRevision: number,
  scope: NativeSubtitlePreferenceScope,
) =>
  nativeInvoke<NativeSubtitlePreferenceSnapshot>("reset_subtitle_preferences", {
    request: {
      expectedRevision,
      scope,
      explicitUserConfirmation: true,
    },
  });

export const saveProductPreferences = (
  expectedRevision: number,
  entry: NativeScopedProductPreferences,
) =>
  nativeInvoke<NativeProductPreferenceSnapshot>("save_product_preferences", {
    request: { expectedRevision, entry },
  });

export const resetProductPreferences = (
  expectedRevision: number,
  scope: NativeProductPreferenceScope,
) =>
  nativeInvoke<NativeProductPreferenceSnapshot>("reset_product_preferences", {
    request: {
      expectedRevision,
      scope,
      explicitUserConfirmation: true,
    },
  });

export const saveLocalResourceSettings = (
  settings: NativeLocalResourceSettings,
) =>
  nativeInvoke<NativeLocalResourceSettings>("save_local_resource_settings", {
    settings,
  });

export const readLocalResourceTelemetry = () =>
  nativeInvoke<NativeResourceTelemetryResult>("local_resource_telemetry");

export const prepareSupportedVisualLoadout = () =>
  nativeInvoke<NativeSelectedLoadoutPlannerResult>(
    "prepare_supported_visual_loadout",
    {
      request: { explicitUserConfirmation: true },
    },
  );

export const readSelectedLocalLoadoutPlanner = () =>
  nativeInvoke<NativeSelectedLoadoutPlannerResult>(
    "selected_local_loadout_planner",
  );

export const readTrustedLocalPackCatalog = () =>
  nativeInvoke<NativeTrustedLocalPackCatalog>("trusted_local_pack_catalog");

export const readTrustedOptionalPackLifecycle = () =>
  nativeInvoke<NativeTrustedOptionalPackLifecycle>(
    "trusted_optional_pack_lifecycle",
  );

export const mutateTrustedOptionalPack = (
  action: "install" | "repair" | "remove",
  identity: { pack_id: string; revision: string },
  licenseAccepted: boolean,
) =>
  nativeInvoke<NativeTrustedOptionalPackLifecycle>(
    {
      install: "install_trusted_optional_pack",
      repair: "repair_trusted_optional_pack",
      remove: "remove_trusted_optional_pack",
    }[action],
    {
      request: {
        packId: identity.pack_id,
        revision: identity.revision,
        explicitUserConfirmation: true,
        licenseAccepted,
      },
    },
  );

export const cancelTrustedOptionalPackDownload = (identity: {
  pack_id: string;
  revision: string;
}) =>
  nativeInvoke<boolean>("cancel_trusted_optional_pack_download", {
    request: {
      packId: identity.pack_id,
      revision: identity.revision,
      explicitUserConfirmation: true,
      licenseAccepted: false,
    },
  });

export const activateTrustedOptionalPack = (
  identity: {
    pack_id: string;
    revision: string;
  },
  selection: NativeSelectedLoadoutSelection,
) =>
  nativeInvoke<NativeTrustedOptionalPackActivationResult>(
    "activate_trusted_optional_pack",
    {
      request: {
        packId: identity.pack_id,
        revision: identity.revision,
        explicitUserConfirmation: true,
        selection,
      },
    },
  );

export const admitSelectedLocalLoadout = (
  selection: NativeSelectedLoadoutSelection,
) =>
  nativeInvoke<NativeSelectedLoadoutAdmissionResult>(
    "admit_selected_local_loadout",
    { selection },
  );

export const readExperimentalVisualPackStatus = () =>
  nativeInvoke<NativeExperimentalPackState>("experimental_visual_pack_status");

export const mutateExperimentalVisualPack = (
  action: "install" | "activate" | "repair" | "remove",
  state: Pick<NativeExperimentalPackState, "packId" | "revision">,
) =>
  nativeInvoke<NativeExperimentalPackState>(
    {
      install: "install_experimental_model_pack",
      activate: "activate_experimental_model_pack",
      repair: "repair_model_pack",
      remove: "remove_model_pack",
    }[action],
    {
      request: {
        packId: state.packId,
        revision: state.revision,
        explicitUserConfirmation: true,
      },
    },
  );

export const readDiagnosticsV2 = (maxEvents = 100) =>
  nativeInvoke<NativeDiagnosticsV2Snapshot>("diagnostics_v2_snapshot", {
    maxEvents,
  });

export const readDiagnosticsMatrix = () =>
  nativeInvoke<NativeDiagnosticsMatrix>("diagnostics_v2_matrix");

export const readDiagnosticsSettings = () =>
  nativeInvoke<NativeDiagnosticsSettings>("diagnostics_v2_settings");

export const saveDiagnosticsSettings = (settings: NativeDiagnosticsSettings) =>
  nativeInvoke<NativeDiagnosticsSettings>("save_diagnostics_v2_settings", {
    settings,
  });

export async function exportDiagnosticsV2(maxEvents = 100) {
  const result = await nativeInvoke<NativeDiagnosticsExportResult>(
    "export_diagnostics_v2",
    {
      request: { maxEvents, explicitUserConfirmation: true },
    },
  );
  if (result === null) return null;
  if (
    typeof result.fileName !== "string" ||
    result.fileName.length === 0 ||
    result.fileName.length > 128 ||
    /[\\/\u0000-\u001f\u007f-\u009f]/.test(result.fileName)
  ) {
    throw new Error("Native diagnostics export returned an invalid file name.");
  }
  return result;
}

export const startThisPcBenchmark = (request: {
  requestedIterations: number;
  timeoutMillis: number;
  baselineWindowMillis: number;
}) =>
  nativeInvoke<NativeBenchmarkStatus>("start_this_pc_benchmark", { request });

export const cancelThisPcBenchmark = () =>
  nativeInvoke<NativeBenchmarkStatus>("cancel_this_pc_benchmark");

export const readThisPcBenchmarkStatus = () =>
  nativeInvoke<NativeBenchmarkStatus>("this_pc_benchmark_status");

export const readThisPcBenchmarkReport = (reportId: string) =>
  nativeInvoke<NativeBenchmarkReport>("this_pc_benchmark_report", {
    reportId,
  });

export interface DevLiveTtsSelection {
  providerId: string;
  modelId: string;
  voiceId: string;
  explicitUserAuthorization: true;
}

export interface StartNativeSimulationOptions {
  gameProfileId: string;
  characterName?: string;
  characterId: string;
  transcript?: string;
  selectedSttReceipt?: {
    receiptId: string;
    generation: number;
  };
  enabledSpoilerTiers?: string[];
  devLiveTts?: DevLiveTtsSelection;
}

export async function startNativeSimulation(
  execution: ExecutionMode,
  onEvent: (event: NativeSimulationEvent) => void,
  options: StartNativeSimulationOptions,
): Promise<boolean> {
  if (!hasTauri()) return false;
  const [{ invoke, Channel }] = await Promise.all([
    import("@tauri-apps/api/core"),
  ]);
  const events = new Channel<unknown>();
  events.onmessage = (wireEvent) => {
    const event = normalizeNativeSimulationEvent(wireEvent);
    if (!event) return;
    onEvent(event);
  };
  await invoke("start_simulation", {
    request: {
      gameProfileId: options.gameProfileId,
      ...(options.characterName === undefined
        ? {}
        : { characterName: options.characterName }),
      characterId: options.characterId,
      ...(options.selectedSttReceipt
        ? { selectedSttReceipt: options.selectedSttReceipt }
        : {
            transcript:
              options.transcript ??
              "Did you ever make it to the old lighthouse?",
          }),
      enabledSpoilerTiers: options.enabledSpoilerTiers ?? [],
      executionMode: execution,
      ...(options.devLiveTts === undefined
        ? {}
        : { devLiveTts: options.devLiveTts }),
    },
    events,
  });
  return true;
}

export async function cancelNativeSimulation(): Promise<boolean> {
  if (!hasTauri()) return false;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("cancel_simulation");
  return true;
}
