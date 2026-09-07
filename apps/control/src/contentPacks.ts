export const CONTENT_PACK_MAX_BYTES = 2 * 1024 * 1024;

export type ContentPackProviderRole = "llm" | "stt" | "tts" | "embeddings";

export interface ContentPackRecommendationPreview {
  role: ContentPackProviderRole;
  providerId: string;
  modelId: string;
  characterId: string | null;
  voiceId: string | null;
  rationale: string;
  catalogAvailable: boolean;
  detail: string;
}

export interface ContentPackRights {
  license_name: string;
  source_summary: string;
  redistributable: boolean;
  commercial_use: boolean;
  derivative_use: boolean;
  review_status: "approved";
}

export interface ContentPackPreview {
  schemaVersion: 1;
  namespace: string;
  packId: string;
  version: string;
  gameProfileId: string;
  title: string;
  summary: string;
  contentSha256: string;
  displayName: string;
  characterCount: number;
  knowledgeCount: number;
  providerRecommendations: ContentPackRecommendationPreview[];
  rights: ContentPackRights;
  applyDetail: string;
  networkRequestPerformed: false;
  providerRoutesChanged: false;
}

export interface ActiveContentPack {
  namespace: string;
  packId: string;
  version: string;
  gameProfileId: string;
  title: string;
  contentSha256: string;
  activatedAtUnixMillis: number;
  characterCount: number;
  knowledgeCount: number;
  providerRecommendationCount: number;
}

export interface ContentPackState {
  schemaVersion: 1;
  active: ActiveContentPack[];
  detail: string;
}

const hasTauri = () => "__TAURI_INTERNALS__" in window;

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function inspectContentPackJson(jsonText: string) {
  if (!hasTauri()) {
    throw new Error(
      "Content packs can be reviewed in the installed Windows app.",
    );
  }
  return invoke<ContentPackPreview>("inspect_content_pack", {
    request: { jsonText },
  });
}

export async function activateContentPack(
  jsonText: string,
  expectedContentSha256: string,
) {
  if (!hasTauri()) {
    throw new Error(
      "Content packs can be activated in the installed Windows app.",
    );
  }
  return invoke<ActiveContentPack>("activate_content_pack", {
    request: { jsonText, expectedContentSha256 },
  });
}

export async function readContentPackState(): Promise<ContentPackState | null> {
  if (!hasTauri()) return null;
  return invoke<ContentPackState>("content_pack_state");
}
