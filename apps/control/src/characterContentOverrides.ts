export interface CharacterContentOverride {
  gameProfileId: string;
  characterId: string;
  displayName: string;
  biography: string;
  promptContext: string;
  updatedAtUnixMillis: number;
}

export interface CharacterContentOverrideSnapshot {
  schemaVersion: 1;
  gameProfileId: string;
  characterId: string;
  saved: CharacterContentOverride | null;
  detail: string;
}

export interface CharacterContentOverrideMutation {
  schemaVersion: 1;
  gameProfileId: string;
  characterId: string;
  saved: CharacterContentOverride | null;
  persisted: boolean;
  detail: string;
}

export interface CharacterContentOverrideDraft {
  gameProfileId: string;
  characterId: string;
  displayName: string;
  biography: string;
  promptContext: string;
}

const hasTauri = () => "__TAURI_INTERNALS__" in window;

async function invoke<T>(command: string, args: Record<string, unknown>) {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function readCharacterContentOverride(
  gameProfileId: string,
  characterId: string,
): Promise<CharacterContentOverrideSnapshot | null> {
  if (!hasTauri()) return null;
  return invoke<CharacterContentOverrideSnapshot>(
    "character_content_override",
    {
      request: { gameProfileId, characterId },
    },
  );
}

export async function saveCharacterContentOverride(
  draft: CharacterContentOverrideDraft,
) {
  if (!hasTauri()) {
    throw new Error(
      "Character customization requires the installed Windows app.",
    );
  }
  return invoke<CharacterContentOverrideMutation>(
    "save_character_content_override",
    { request: draft },
  );
}

export async function resetCharacterContentOverride(
  gameProfileId: string,
  characterId: string,
) {
  if (!hasTauri()) {
    throw new Error(
      "Character customization requires the installed Windows app.",
    );
  }
  return invoke<CharacterContentOverrideMutation>(
    "reset_character_content_override",
    { request: { gameProfileId, characterId } },
  );
}
