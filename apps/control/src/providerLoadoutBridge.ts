import {
  ROLE_ORDER,
  providerFor,
  type ManualFallback,
  type ProviderLoadout,
  type ProviderRole,
  type RouteChoice,
} from "./providerLoadouts";

type NativeRole =
  | "llm"
  | "stt"
  | "tts"
  | "embeddings"
  | "vision"
  | "lipsync"
  | "retrieval";
type NativeScope =
  | { kind: "global" }
  | { kind: "game"; game_id: string }
  | { kind: "character"; game_id: string; character_id: string };

interface NativeDisclosure {
  catalog_revision: number;
  execution: "local" | "hosted" | "external_local";
  egress: "none" | "provider_cloud" | "user_configured_endpoint";
  privacy_summary: string;
  cost_summary: string;
  transmitted_data: Array<
    | "transcript"
    | "microphone_audio"
    | "response_text"
    | "game_context"
    | "memory_context"
    | "image"
  >;
}

interface NativeModelRoute {
  provider_id: string;
  model_id: string;
  voice_id?: string | null;
  credential: { provider_id: string; reference_id: string } | null;
  disclosure: NativeDisclosure;
  explicit_user_selection: boolean;
}

interface NativeRoleRoute {
  primary: NativeModelRoute;
  fallbacks: Array<{
    route: NativeModelRoute;
    activation: "manual_only";
    user_authorized: boolean;
  }>;
}

type NativeRoleOverride =
  | { mode: "inherit" }
  | { mode: "disabled" }
  | { mode: "route"; route: NativeRoleRoute };

export interface NativeProviderLoadout {
  id: string;
  name: string;
  scope: NativeScope;
  parent: string | null;
  roles: Partial<Record<NativeRole, NativeRoleOverride>>;
}

export interface NativeLoadoutDocument {
  format: "npc-provider-loadouts";
  schema_version: 1;
  loadouts: Record<string, NativeProviderLoadout>;
  activation: {
    global: string;
    games: Record<string, string>;
    characters: Record<string, Record<string, string>>;
  };
}

export interface NativeLoadoutSnapshot {
  document: NativeLoadoutDocument;
  catalogRevision: number;
  persistenceHealth:
    | "healthy"
    | "firstRun"
    | "recoveredLastGood"
    | "recoveredSeed";
  detail: string;
  credentialsChecked: false;
  networkRequestPerformed: false;
}

export interface NativeProviderPrivateEvaluationAcknowledgement {
  schemaVersion: 1;
  providerId: "nvidia-nim";
  mode: "privateEvaluationOnly";
  termsRevision: string;
  catalogRevision: number;
  applicationNamespace:
    | "io.github.akshitireddy.interactive-npcs.debug"
    | "io.github.akshitireddy.interactive-npcs.review";
  acknowledgedAtEpochMs: number;
  promotionSupported: false;
  publicationSupported: false;
}

export interface NativeProviderPrivateEvaluationPolicy {
  schemaVersion: 1;
  providerId: "nvidia-nim";
  mode: "privateEvaluationOnly";
  termsRevision: string;
  termsUrl: string;
  catalogRevision: number;
  applicationNamespace: string;
  namespaceEligible: boolean;
  trialPurposesOnly: true;
  productionUseSupported: false;
  promotionSupported: false;
  publicationSupported: false;
  confidentialSensitiveOrPersonalDataSupported: false;
  limitsApply: true;
  separateSubscriptionRequiredForProduction: true;
  accessScope: string;
  rateLimitNote: string;
  prohibitedData: Array<
    "confidential" | "controlled_or_sensitive" | "personal" | "game_secrets"
  >;
  securityAbuseLogging: boolean;
  productImprovementCollectionDisclosed: boolean;
  serviceSpecificDisclosuresApply: boolean;
  exactModelTermsApply: boolean;
  acknowledgement: NativeProviderPrivateEvaluationAcknowledgement | null;
}

export interface NativeResolvedProviderLoadout {
  schema_version: number;
  leaf_loadout_id: string;
  inheritance_chain: string[];
  roles: Partial<Record<Exclude<NativeRole, "retrieval">, NativeRoleRoute>>;
}

export interface NativeProviderLoadoutReview {
  resolved: NativeResolvedProviderLoadout;
  offline: boolean;
  credentialsChecked: false;
  networkRequestPerformed: false;
  detail: string;
}

export interface NativeTtsStockVoiceDiscovery {
  schemaVersion: number;
  providerId: "nvidia-nim-magpie";
  modelId: "magpie-tts-multilingual";
  status: "available" | "unavailable";
  voices: Array<{
    voiceId: string;
    displayName: string;
    language: string;
    styles: string[];
    provenance: "providerStockDiscovery";
  }>;
  provenance: "providerStockDiscovery";
  refresh: {
    requested: boolean;
    performed: boolean;
    cacheHit: boolean;
    refreshedAtEpochMs?: number;
    expiresAtEpochMs?: number;
  };
  error?: { code: string; detail: string; retryable: boolean };
}

export type LoadoutSource =
  | { kind: "browser"; detail: string }
  | { kind: "native"; snapshot: NativeLoadoutSnapshot };

const hasTauri = () => "__TAURI_INTERNALS__" in window;
export const BROWSER_CATALOG_REVISION = 9;
const nativeRole = (role: ProviderRole): NativeRole =>
  role === "lipSync" ? "lipsync" : role;

function routeDisclosure(
  role: ProviderRole,
  route: RouteChoice,
  catalogRevision: number,
): NativeDisclosure {
  const provider = providerFor(role, route.providerId);
  const transmitted: NativeDisclosure["transmitted_data"] =
    provider.execution !== "Cloud"
      ? []
      : role === "stt"
        ? ["microphone_audio"]
        : role === "tts"
          ? ["response_text"]
          : role === "embeddings"
            ? ["game_context", "memory_context"]
            : role === "vision"
              ? ["image", "game_context"]
              : role === "lipSync"
                ? []
                : ["transcript", "game_context", "memory_context"];
  return {
    catalog_revision: catalogRevision,
    execution: provider.execution === "Cloud" ? "hosted" : "local",
    egress: provider.execution === "Cloud" ? "provider_cloud" : "none",
    privacy_summary: provider.privacy,
    cost_summary: provider.cost,
    transmitted_data: transmitted,
  };
}

function toNativeRoute(
  role: ProviderRole,
  route: RouteChoice,
  existing?: NativeModelRoute,
  catalogRevision = BROWSER_CATALOG_REVISION,
): NativeModelRoute {
  const provider = providerFor(role, route.providerId);
  const credentialProviderId = route.providerId.startsWith("nvidia-nim")
    ? "nvidia-nim"
    : route.providerId;
  return {
    provider_id: route.providerId,
    model_id: route.modelId,
    voice_id:
      role === "tts" ? (route.voiceId ?? existing?.voice_id ?? null) : null,
    credential:
      provider.execution === "Cloud"
        ? (existing?.credential ?? {
            provider_id: credentialProviderId,
            reference_id: "personal",
          })
        : null,
    disclosure: routeDisclosure(role, route, catalogRevision),
    explicit_user_selection: true,
  };
}

function toNativeScope(loadout: ProviderLoadout): NativeScope {
  if (loadout.scope === "global") return { kind: "global" };
  if (loadout.scope === "game")
    return { kind: "game", game_id: loadout.targetId ?? "cyberpunk-2077" };
  const [gameId = "eclipse-harbor", characterId = "mara-venn"] = (
    loadout.targetId ?? "eclipse-harbor/mara-venn"
  ).split("/");
  return { kind: "character", game_id: gameId, character_id: characterId };
}

function contextFor(loadout: ProviderLoadout) {
  const scope = toNativeScope(loadout);
  if (scope.kind === "global") {
    return { game_id: null, character_id: null };
  }
  if (scope.kind === "game") {
    return { game_id: scope.game_id, character_id: null };
  }
  return {
    game_id: scope.game_id,
    character_id: scope.character_id,
  };
}

function parentFor(loadout: ProviderLoadout, document: NativeLoadoutDocument) {
  if (loadout.scope === "global") return null;
  if (loadout.scope === "game") return document.activation.global;
  const [gameId = "eclipse-harbor"] = (
    loadout.targetId ?? "eclipse-harbor/mara-venn"
  ).split("/");
  return document.activation.games[gameId] ?? document.activation.global;
}

export function toNativeLoadout(
  loadout: ProviderLoadout,
  document: NativeLoadoutDocument,
  existing?: NativeProviderLoadout,
  catalogRevision = BROWSER_CATALOG_REVISION,
): NativeProviderLoadout {
  const roles: NativeProviderLoadout["roles"] = {};
  for (const role of ROLE_ORDER) {
    const route = loadout.routes[role];
    if (
      (role === "lipSync" || role === "vision") &&
      route.providerId === "disabled"
    ) {
      roles[nativeRole(role)] = { mode: "disabled" };
      continue;
    }
    const fallback = loadout.fallbacks[role];
    const existingOverride =
      existing?.roles[nativeRole(role)] ??
      (role === "embeddings" ? existing?.roles.retrieval : undefined);
    const existingRoute =
      existingOverride?.mode === "route" ? existingOverride.route : undefined;
    const existingFallback = existingRoute?.fallbacks.find(
      (candidate) =>
        candidate.route.provider_id === fallback?.providerId &&
        candidate.route.model_id === fallback?.modelId,
    );
    roles[nativeRole(role)] = {
      mode: "route",
      route: {
        primary: toNativeRoute(
          role,
          route,
          existingRoute?.primary,
          catalogRevision,
        ),
        fallbacks: fallback?.authorized
          ? [
              {
                route: toNativeRoute(
                  role,
                  fallback,
                  existingFallback?.route,
                  catalogRevision,
                ),
                activation: "manual_only",
                user_authorized: true,
              },
            ]
          : [],
      },
    };
  }
  return {
    id: loadout.id,
    name: loadout.name,
    scope: existing?.scope ?? toNativeScope(loadout),
    parent: existing?.parent ?? parentFor(loadout, document),
    roles,
  };
}

function isActive(
  loadout: NativeProviderLoadout,
  document: NativeLoadoutDocument,
) {
  if (loadout.scope.kind === "global")
    return document.activation.global === loadout.id;
  if (loadout.scope.kind === "game")
    return document.activation.games[loadout.scope.game_id] === loadout.id;
  return (
    document.activation.characters[loadout.scope.game_id]?.[
      loadout.scope.character_id
    ] === loadout.id
  );
}

function target(native: NativeProviderLoadout) {
  if (native.scope.kind === "global")
    return { scope: "global" as const, targetLabel: "Every game" };
  if (native.scope.kind === "game") {
    return {
      scope: "game" as const,
      targetId: native.scope.game_id,
      targetLabel:
        native.scope.game_id === "cyberpunk-2077"
          ? "Cyberpunk 2077"
          : native.scope.game_id,
    };
  }
  return {
    scope: "character" as const,
    targetId: `${native.scope.game_id}/${native.scope.character_id}`,
    targetLabel:
      native.scope.game_id === "eclipse-harbor" &&
      native.scope.character_id === "mara-venn"
        ? "Eclipse Harbor · Mara Venn"
        : `${native.scope.game_id} · ${native.scope.character_id}`,
  };
}

function inheritedRole(
  id: string,
  role: ProviderRole,
  document: NativeLoadoutDocument,
  seen = new Set<string>(),
): NativeRoleOverride | undefined {
  if (seen.has(id)) return undefined;
  seen.add(id);
  const loadout = document.loadouts[id];
  const override =
    loadout?.roles[nativeRole(role)] ??
    (role === "embeddings" ? loadout?.roles.retrieval : undefined);
  if (override && override.mode !== "inherit") return override;
  return loadout?.parent
    ? inheritedRole(loadout.parent, role, document, seen)
    : undefined;
}

const safeDefaults: Record<ProviderRole, RouteChoice> = {
  llm: { providerId: "groq", modelId: "qwen/qwen3.6-27b" },
  stt: { providerId: "assemblyai", modelId: "u3-rt-pro" },
  tts: {
    providerId: "cartesia",
    modelId: "sonic-3.6",
    voiceId: "a0e99841-438c-4a64-b679-ae501e7d6091",
  },
  embeddings: { providerId: "fts-only", modelId: "sqlite-fts5" },
  vision: { providerId: "disabled", modelId: "disabled" },
  lipSync: { providerId: "disabled", modelId: "disabled" },
};

function toBrowserFallback(
  role: ProviderRole,
  fallback: NativeRoleRoute["fallbacks"][number] | undefined,
): ManualFallback | undefined {
  if (!fallback) return undefined;
  return {
    providerId: fallback.route.provider_id,
    modelId: fallback.route.model_id,
    authorized:
      fallback.user_authorized && fallback.activation === "manual_only",
  };
}

export function fromNativeSnapshot(
  snapshot: NativeLoadoutSnapshot,
): ProviderLoadout[] {
  return Object.values(snapshot.document.loadouts).map((native, index) => {
    const routes = structuredClone(safeDefaults);
    const fallbacks: ProviderLoadout["fallbacks"] = {};
    for (const role of ROLE_ORDER) {
      const override = inheritedRole(native.id, role, snapshot.document);
      if (override?.mode === "route") {
        routes[role] = {
          providerId: override.route.primary.provider_id,
          modelId: override.route.primary.model_id,
          ...(role === "tts" && override.route.primary.voice_id
            ? { voiceId: override.route.primary.voice_id }
            : {}),
        };
        const fallback = toBrowserFallback(role, override.route.fallbacks[0]);
        if (fallback) fallbacks[role] = fallback;
      } else if (
        (role === "lipSync" || role === "vision") &&
        override?.mode === "disabled"
      ) {
        routes[role] = { providerId: "disabled", modelId: "disabled" };
      }
    }
    return {
      id: native.id,
      name: native.name,
      ...target(native),
      active: isActive(native, snapshot.document),
      routes,
      fallbacks,
      revision: index + 1,
    };
  });
}

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function nativeSnapshot(): Promise<NativeLoadoutSnapshot | null> {
  if (!hasTauri()) return null;
  return invoke<NativeLoadoutSnapshot>("provider_loadout_snapshot");
}

export async function nativeCreate(
  loadout: ProviderLoadout,
  document: NativeLoadoutDocument,
  catalogRevision: number,
) {
  return invoke<NativeLoadoutSnapshot>("create_provider_loadout", {
    loadout: toNativeLoadout(loadout, document, undefined, catalogRevision),
  });
}

export async function nativeClone(
  sourceId: string,
  newId: string,
  newName: string,
) {
  return invoke<NativeLoadoutSnapshot>("clone_provider_loadout", {
    sourceId,
    newId,
    newName,
  });
}

export async function nativeRename(id: string, newName: string) {
  return invoke<NativeLoadoutSnapshot>("rename_provider_loadout", {
    id,
    newName,
  });
}

export async function nativeDelete(id: string) {
  return invoke<NativeLoadoutSnapshot>("delete_provider_loadout", { id });
}

export async function nativeActivate(id: string) {
  return invoke<NativeLoadoutSnapshot>("activate_provider_loadout", { id });
}

export async function nativeDeactivateScope(loadout: ProviderLoadout) {
  if (loadout.scope === "global") {
    throw new Error("The global provider route cannot be deactivated");
  }
  return invoke<NativeLoadoutSnapshot>("deactivate_provider_loadout_scope", {
    scope: toNativeScope(loadout),
  });
}

export async function nativeReview(loadout: ProviderLoadout, offline: boolean) {
  return invoke<NativeProviderLoadoutReview>("review_provider_loadout", {
    context: contextFor(loadout),
    offline,
  });
}

export async function nativeDiscoverTtsStockVoices(forceRefresh: boolean) {
  return invoke<NativeTtsStockVoiceDiscovery>("discover_tts_stock_voices", {
    forceRefresh,
  });
}

export async function nativePrivateEvaluationAcknowledgement() {
  if (!hasTauri()) return null;
  return invoke<NativeProviderPrivateEvaluationAcknowledgement | null>(
    "provider_private_evaluation_acknowledgement",
  );
}

export async function nativePrivateEvaluationPolicy() {
  return invoke<NativeProviderPrivateEvaluationPolicy>(
    "provider_private_evaluation_policy",
  );
}

export async function nativeAcknowledgePrivateEvaluation(
  termsRevision: string,
) {
  return invoke<NativeProviderPrivateEvaluationAcknowledgement>(
    "acknowledge_provider_private_evaluation",
    {
      request: {
        providerId: "nvidia-nim",
        termsRevision,
        explicitUserConfirmation: true,
      },
    },
  );
}

export async function nativeUpdate(
  loadout: ProviderLoadout,
  document: NativeLoadoutDocument,
  catalogRevision: number,
) {
  const existing = document.loadouts[loadout.id];
  return invoke<NativeLoadoutSnapshot>("update_provider_loadout", {
    id: loadout.id,
    loadout: toNativeLoadout(loadout, document, existing, catalogRevision),
  });
}
