import type { ExecutionMode, StageId } from "./types";

export type NativeSimulationEvent =
  | {
      type: "started";
      simulationId: string;
      generation: number;
      sequence: number;
      measurementBasis:
        | "deterministicFixture"
        | "trustedRuntimeFixture"
        | "controlledBenchmark";
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
    }
  | {
      type: "cancelled";
      simulationId: string;
      generation: number;
      sequence: number;
      reason: string;
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

export interface NativeDoctorReport {
  schemaVersion: string;
  status: string;
  profileCount: number;
  providerCount: number;
  modelCount: number;
  discoveredInstallationCount: number;
  discoveryErrorCount: number;
  vectorBackend: string;
  hostedProviderContracts: string[];
  modelManifestExample: string;
  performanceMeasurementsCaptured: boolean;
  powerProfileChanged: boolean;
}

export interface NativeRuntimeProfileSummary {
  id: string;
  displayName: string;
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

const field = (
  record: Record<string, unknown>,
  camelCase: string,
  snakeCase: string,
) => record[camelCase] ?? record[snakeCase];

const finiteNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : null;

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
        measurementBasis !== "controlledBenchmark"
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
      };
    }
    case "cancelled":
      return typeof record.reason === "string"
        ? { type: "cancelled", ...identity, reason: record.reason }
        : null;
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
export async function readRuntimeDoctor(): Promise<NativeDoctorReport | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<NativeDoctorReport>("runtime_doctor");
}

/** Reads the runtime-owned profile catalog; browser previews have no profiles. */
export async function readRuntimeProfileSummaries(): Promise<
  NativeRuntimeProfileSummary[]
> {
  if (!hasTauri()) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<NativeRuntimeProfileSummary[]>("runtime_profile_summaries");
}

/** Reads live broker counters when the authenticated native broker is available. */
export async function readMediaBrokerDiagnostics(): Promise<NativeMediaBrokerDiagnostics | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<NativeMediaBrokerDiagnostics>("media_broker_diagnostics");
}

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

export interface SyntheticReplayCaptureResult {
  targetProcessId: number;
  targetWindowHandle: number;
  targetExecutableBasename: string;
  diagnostics: SyntheticReplayCaptureDiagnostics;
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

export async function readSyntheticReplayCaptureDiagnostics(): Promise<SyntheticReplayCaptureResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SyntheticReplayCaptureResult>(
    "debug_synthetic_replay_capture_diagnostics",
  );
}

export async function clearSyntheticReplayCaptureTarget(): Promise<SyntheticReplayCaptureResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<SyntheticReplayCaptureResult>(
    "debug_clear_synthetic_replay_capture_target",
  );
}

export interface DevLiveTtsSelection {
  providerId: string;
  modelId: string;
  voiceId: string;
  explicitUserAuthorization: true;
}

export interface StartNativeSimulationOptions {
  gameProfileId?: string;
  characterName?: string;
  characterId?: string;
  transcript?: string;
  devLiveTts?: DevLiveTtsSelection;
}

export async function startNativeSimulation(
  execution: ExecutionMode,
  onEvent: (event: NativeSimulationEvent) => void,
  options?: StartNativeSimulationOptions,
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
      gameProfileId: options?.gameProfileId ?? "eclipse-harbor",
      characterName: options?.characterName ?? "Mara Venn",
      ...(options?.characterId === undefined
        ? {}
        : { characterId: options.characterId }),
      transcript:
        options?.transcript ?? "Did you ever make it to the old lighthouse?",
      executionMode: execution,
      ...(options?.devLiveTts === undefined
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
