export type SelectedSttAttempt =
  | { kind: "initial" }
  | { kind: "manualRetry"; priorGeneration: number; userAuthorized: true };

export interface StartSelectedSttRequest {
  schemaVersion: 1;
  gameProfileId?: string | null;
  characterId?: string | null;
  contextHint?: string | null;
  attempt: SelectedSttAttempt;
}

export interface SelectedSttCapturing {
  schemaVersion: 1;
  status: "capturing";
  sessionId: string;
  turnId: string;
  generation: number;
  inputEndpointId: string;
  inputEndpointGeneration: number;
  route: {
    providerId: "assemblyai";
    modelId: "u3-rt-pro";
    egress: "microphone_audio_and_optional_non_secret_context";
    automaticFallback: false;
  };
}

export interface SelectedSttRouteReceipt {
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
}

export type SelectedSttTerminal = {
  schemaVersion: 1;
  status: "transcriptReady" | "cancelled" | "failed";
  sessionId: string;
  turnId: string;
  generation: number;
  receiptId: string | null;
  receiptSha256: string | null;
  route: SelectedSttRouteReceipt | null;
  chunksSent: number;
  pcmBytesSent: number;
  partialEvents: number;
  errorCode: string | null;
  retryable: boolean;
};

export interface SelectedSttStatus {
  schemaVersion: 1;
  status: "arming" | "capturing" | "idle";
  generation: number;
  sessionId: string | null;
  turnId: string | null;
  inputEndpointId: string | null;
  inputEndpointGeneration: number | null;
}

export interface SelectedSttCancelResult {
  schemaVersion: 1;
  outcome: "cancellationRequested" | "alreadyIdle";
  generation: number;
}

type ExpectedReceipt = SelectedSttCapturing & { manualRetry: boolean };

const hasTauri = () => "__TAURI_INTERNALS__" in window;
const asRecord = (value: unknown): Record<string, unknown> | null =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
const isNonemptyString = (value: unknown): value is string =>
  typeof value === "string" && value.length > 0;
const isCount = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const isCanonicalUuid = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
    value,
  );
const isLowerSha256 = (value: unknown): value is string =>
  typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const isCredentialReference = (value: unknown): value is string =>
  typeof value === "string" &&
  value.length > 0 &&
  value.length <= 256 &&
  /^[A-Za-z0-9._:/-]+$/.test(value);
const hasOnlyKeys = (record: Record<string, unknown>, allowed: string[]) =>
  Object.keys(record).every((key) => allowed.includes(key));

const CAPTURING_KEYS = [
  "schemaVersion",
  "status",
  "sessionId",
  "turnId",
  "generation",
  "inputEndpointId",
  "inputEndpointGeneration",
  "route",
];
const SUMMARY_ROUTE_KEYS = [
  "providerId",
  "modelId",
  "egress",
  "automaticFallback",
];
const TERMINAL_KEYS = [
  "schemaVersion",
  "status",
  "sessionId",
  "turnId",
  "generation",
  "receiptId",
  "receiptSha256",
  "route",
  "chunksSent",
  "pcmBytesSent",
  "partialEvents",
  "errorCode",
  "retryable",
];
const RECEIPT_ROUTE_KEYS = [
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
];

export function normalizeSelectedSttCapturing(
  value: unknown,
): SelectedSttCapturing | null {
  const record = asRecord(value);
  const route = asRecord(record?.route);
  if (
    !record ||
    !hasOnlyKeys(record, CAPTURING_KEYS) ||
    !route ||
    !hasOnlyKeys(route, SUMMARY_ROUTE_KEYS) ||
    record.schemaVersion !== 1 ||
    record.status !== "capturing" ||
    !isNonemptyString(record.sessionId) ||
    !isNonemptyString(record.turnId) ||
    !isCount(record.generation) ||
    record.generation === 0 ||
    !isNonemptyString(record.inputEndpointId) ||
    !isCount(record.inputEndpointGeneration) ||
    record.inputEndpointGeneration === 0 ||
    route.providerId !== "assemblyai" ||
    route.modelId !== "u3-rt-pro" ||
    route.egress !== "microphone_audio_and_optional_non_secret_context" ||
    route.automaticFallback !== false
  )
    return null;
  return record as unknown as SelectedSttCapturing;
}

export function normalizeSelectedSttTerminal(
  value: unknown,
  expected: ExpectedReceipt,
): SelectedSttTerminal | null {
  const record = asRecord(value);
  if (
    !record ||
    !hasOnlyKeys(record, TERMINAL_KEYS) ||
    record.schemaVersion !== 1 ||
    (record.status !== "transcriptReady" &&
      record.status !== "cancelled" &&
      record.status !== "failed") ||
    record.sessionId !== expected.sessionId ||
    record.turnId !== expected.turnId ||
    record.generation !== expected.generation ||
    !isCount(record.chunksSent) ||
    !isCount(record.pcmBytesSent) ||
    !isCount(record.partialEvents) ||
    (record.errorCode !== null &&
      record.errorCode !== undefined &&
      typeof record.errorCode !== "string") ||
    typeof record.retryable !== "boolean"
  )
    return null;

  if (record.status !== "transcriptReady") {
    if (record.receiptId !== null && record.receiptId !== undefined)
      return null;
    if (record.receiptSha256 !== null && record.receiptSha256 !== undefined)
      return null;
    if (record.route !== null && record.route !== undefined) return null;
    return {
      ...(record as unknown as SelectedSttTerminal),
      receiptId: null,
      receiptSha256: null,
      route: null,
      errorCode: typeof record.errorCode === "string" ? record.errorCode : null,
    };
  }

  const route = asRecord(record.route);
  if (
    !isCanonicalUuid(record.receiptId) ||
    !isLowerSha256(record.receiptSha256) ||
    !route ||
    !hasOnlyKeys(route, RECEIPT_ROUTE_KEYS) ||
    route.providerId !== "assemblyai" ||
    route.modelId !== "u3-rt-pro" ||
    !isCredentialReference(route.credentialReference) ||
    route.egress !== "microphone_audio_and_optional_non_secret_context" ||
    route.generation !== expected.generation ||
    route.generation === 0 ||
    route.inputEndpointId !== expected.inputEndpointId ||
    route.inputEndpointGeneration !== expected.inputEndpointGeneration ||
    route.inputEndpointGeneration === 0 ||
    route.manualRetry !== expected.manualRetry ||
    route.automaticFallback !== false ||
    !isCount(route.capturedFrames) ||
    route.capturedFrames === 0 ||
    route.pttVirtualKey !== 119 ||
    !isCount(route.pttPressTransitionSequence) ||
    route.pttPressTransitionSequence === 0 ||
    !isCount(route.pttPressedQpc) ||
    route.pttPressedQpc === 0 ||
    !isCount(route.pttReleaseTransitionSequence) ||
    route.pttReleaseTransitionSequence <= route.pttPressTransitionSequence ||
    !isCount(route.pttReleasedQpc) ||
    route.pttReleasedQpc < route.pttPressedQpc ||
    record.chunksSent === 0 ||
    record.pcmBytesSent === 0 ||
    (record.errorCode !== null && record.errorCode !== undefined) ||
    record.retryable !== false
  )
    return null;
  return {
    ...(record as unknown as SelectedSttTerminal),
    errorCode: null,
  };
}

export function normalizeSelectedSttStatus(
  value: unknown,
): SelectedSttStatus | null {
  const record = asRecord(value);
  if (
    !record ||
    ![
      "schemaVersion",
      "status",
      "generation",
      "sessionId",
      "turnId",
      "inputEndpointId",
      "inputEndpointGeneration",
    ].every((key) => key in record) ||
    !hasOnlyKeys(record, [
      "schemaVersion",
      "status",
      "generation",
      "sessionId",
      "turnId",
      "inputEndpointId",
      "inputEndpointGeneration",
    ]) ||
    record.schemaVersion !== 1 ||
    (record.status !== "arming" &&
      record.status !== "capturing" &&
      record.status !== "idle") ||
    !isCount(record.generation) ||
    (record.status === "arming" && record.generation === 0)
  )
    return null;
  const identityFields = [
    record.sessionId,
    record.turnId,
    record.inputEndpointId,
  ];
  if (record.status === "capturing") {
    if (
      identityFields.some((field) => !isNonemptyString(field)) ||
      !isCount(record.inputEndpointGeneration) ||
      record.inputEndpointGeneration === 0 ||
      record.generation === 0
    )
      return null;
  } else if (
    identityFields.some((field) => field !== null) ||
    record.inputEndpointGeneration !== null
  ) {
    return null;
  }
  return record as unknown as SelectedSttStatus;
}

export function normalizeSelectedSttCancelResult(
  value: unknown,
): SelectedSttCancelResult | null {
  const record = asRecord(value);
  if (
    !record ||
    !hasOnlyKeys(record, ["schemaVersion", "outcome", "generation"]) ||
    record.schemaVersion !== 1 ||
    (record.outcome !== "cancellationRequested" &&
      record.outcome !== "alreadyIdle") ||
    !isCount(record.generation)
  )
    return null;
  return record as unknown as SelectedSttCancelResult;
}

function safeRequest(
  request: StartSelectedSttRequest,
): StartSelectedSttRequest {
  const gameProfileId = request.gameProfileId ?? null;
  const characterId = request.characterId ?? null;
  const contextHint = request.contextHint ?? null;
  if (
    request.schemaVersion !== 1 ||
    (characterId !== null && gameProfileId === null) ||
    (gameProfileId !== null &&
      (!isNonemptyString(gameProfileId) || gameProfileId.length > 128)) ||
    (characterId !== null &&
      (!isNonemptyString(characterId) || characterId.length > 128)) ||
    (contextHint !== null &&
      (typeof contextHint !== "string" ||
        contextHint.length > 4096 ||
        contextHint.includes("\0"))) ||
    (request.attempt.kind !== "initial" &&
      (request.attempt.kind !== "manualRetry" ||
        !isCount(request.attempt.priorGeneration) ||
        request.attempt.userAuthorized !== true))
  ) {
    throw new Error("Selected STT request is invalid.");
  }
  return {
    schemaVersion: 1,
    gameProfileId,
    characterId,
    contextHint,
    attempt:
      request.attempt.kind === "initial"
        ? { kind: "initial" }
        : {
            kind: "manualRetry",
            priorGeneration: request.attempt.priorGeneration,
            userAuthorized: true,
          },
  };
}

export async function startSelectedSttPushToTalk(
  request: StartSelectedSttRequest,
  onTerminal: (event: SelectedSttTerminal) => void,
): Promise<SelectedSttCapturing | null> {
  if (!hasTauri()) return null;
  const { invoke, Channel } = await import("@tauri-apps/api/core");
  const events = new Channel<unknown>();
  let expected: ExpectedReceipt | null = null;
  let terminalDelivered = false;
  const pending: unknown[] = [];
  const process = (wireEvent: unknown) => {
    if (terminalDelivered) return;
    if (!expected) {
      if (pending.length === 0) pending.push(wireEvent);
      return;
    }
    const event = normalizeSelectedSttTerminal(wireEvent, expected);
    if (event) {
      terminalDelivered = true;
      onTerminal(event);
    }
  };
  events.onmessage = process;
  const safe = safeRequest(request);
  const raw = await invoke<unknown>("start_selected_stt_push_to_talk", {
    request: safe,
    events,
  });
  const capturing = normalizeSelectedSttCapturing(raw);
  if (!capturing)
    throw new Error("Native selected STT returned an invalid capture receipt.");
  expected = {
    ...capturing,
    manualRetry: safe.attempt.kind === "manualRetry",
  };
  pending.splice(0).forEach(process);
  return capturing;
}

export async function readSelectedSttPushToTalkStatus(): Promise<SelectedSttStatus | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  const raw = await invoke<unknown>("selected_stt_push_to_talk_status", {});
  const status = normalizeSelectedSttStatus(raw);
  if (!status)
    throw new Error("Native selected STT returned an invalid status receipt.");
  return status;
}

export async function cancelSelectedSttPushToTalk(): Promise<SelectedSttCancelResult | null> {
  if (!hasTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  const raw = await invoke<unknown>("cancel_selected_stt_push_to_talk", {});
  const result = normalizeSelectedSttCancelResult(raw);
  if (!result)
    throw new Error("Native selected STT returned an invalid cancel receipt.");
  return result;
}
