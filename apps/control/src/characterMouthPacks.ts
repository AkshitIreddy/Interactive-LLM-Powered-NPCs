export const MOUTH_PACK_MANIFEST_MAX_BYTES = 64 * 1024;
export const MOUTH_PACK_TEXTURE_MAX_BYTES = 16 * 1024 * 1024;

export interface CharacterMouthPackFiles {
  gameProfileId: string;
  atlasJsonText: string;
  textureFileName: string;
  textureBase64: string;
}

export interface CharacterMouthPackPreview {
  schemaVersion: 1;
  gameProfileId: string;
  characterId: string;
  atlasSchemaVersion: number;
  identityRevision: number;
  manifestSha256: string;
  textureFileName: string;
  textureSizeBytes: number;
  contentSha256: string;
  enrollmentBindingSha256: string;
  privateReviewBindingValidated: boolean;
  networkRequestPerformed: false;
}

export interface InstalledCharacterMouthPack {
  gameProfileId: string;
  characterId: string;
  atlasSchemaVersion: number;
  identityRevision: number;
  manifestSha256: string;
  textureFileName: string;
  textureSizeBytes: number;
  contentSha256: string;
  enrollmentBindingSha256: string;
  importedAtUnixMs: number;
}

export interface EnabledCharacterMouthPack {
  gameProfileId: string;
  characterId: string;
  contentSha256: string;
  enrollmentBindingSha256: string;
  enabledAtUnixMs: number;
}

export interface DisabledCharacterMouthPack {
  gameProfileId: string;
  characterId: string;
  contentSha256: string;
  disabled: true;
  retainedInstalledRevision: true;
}

export interface CharacterMouthPackState {
  schemaVersion: 1;
  installed: InstalledCharacterMouthPack[];
  enabled: EnabledCharacterMouthPack[];
  detail: string;
}

const hasTauri = () => "__TAURI_INTERNALS__" in window;

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function inspectCharacterMouthPack(
  files: CharacterMouthPackFiles,
) {
  if (!hasTauri())
    throw new Error("Mouth packs require the installed Windows app.");
  return invoke<CharacterMouthPackPreview>("inspect_character_mouth_pack", {
    files,
  });
}

export async function importCharacterMouthPack(
  files: CharacterMouthPackFiles,
  expectedContentSha256: string,
) {
  if (!hasTauri())
    throw new Error("Mouth packs require the installed Windows app.");
  return invoke<InstalledCharacterMouthPack>("import_character_mouth_pack", {
    request: { files, expectedContentSha256 },
  });
}

export async function enableCharacterMouthPack(
  gameProfileId: string,
  expectedContentSha256: string,
) {
  if (!hasTauri())
    throw new Error("Mouth packs require the installed Windows app.");
  return invoke<EnabledCharacterMouthPack>("enable_character_mouth_pack", {
    request: { gameProfileId, expectedContentSha256 },
  });
}

export async function disableCharacterMouthPack(
  gameProfileId: string,
  expectedContentSha256: string,
) {
  if (!hasTauri())
    throw new Error("Mouth packs require the installed Windows app.");
  return invoke<DisabledCharacterMouthPack>("disable_character_mouth_pack", {
    request: { gameProfileId, expectedContentSha256 },
  });
}

export async function readCharacterMouthPackState(): Promise<CharacterMouthPackState | null> {
  if (!hasTauri()) return null;
  return invoke<CharacterMouthPackState>("character_mouth_pack_state");
}

export async function readMouthPackFiles(
  gameProfileId: string,
  atlasFile: File,
  textureFile: File,
): Promise<CharacterMouthPackFiles> {
  if (atlasFile.size === 0 || atlasFile.size > MOUTH_PACK_MANIFEST_MAX_BYTES)
    throw new Error("atlas.json must contain 1–64 KiB.");
  if (!atlasFile.name.toLowerCase().endsWith(".json"))
    throw new Error("Choose the pack's atlas.json manifest.");
  if (textureFile.size === 0 || textureFile.size > MOUTH_PACK_TEXTURE_MAX_BYTES)
    throw new Error("The .bin texture must contain 1 byte–16 MiB.");
  if (!textureFile.name.toLowerCase().endsWith(".bin"))
    throw new Error("Choose the .bin texture named by atlas.json.");
  const [atlasJsonText, texture] = await Promise.all([
    readFile(atlasFile, "text"),
    readFile(textureFile, "arrayBuffer"),
  ]);
  const bytes = new Uint8Array(texture);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return {
    gameProfileId,
    atlasJsonText,
    textureFileName: textureFile.name,
    textureBase64: btoa(binary),
  };
}

function readFile(file: File, mode: "text"): Promise<string>;
function readFile(file: File, mode: "arrayBuffer"): Promise<ArrayBuffer>;
function readFile(
  file: File,
  mode: "text" | "arrayBuffer",
): Promise<string | ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`Could not read ${file.name}.`));
    reader.onload = () => {
      if (mode === "text" && typeof reader.result === "string") {
        resolve(reader.result);
      } else if (
        mode === "arrayBuffer" &&
        reader.result instanceof ArrayBuffer
      ) {
        resolve(reader.result);
      } else {
        reject(new Error(`Could not decode ${file.name}.`));
      }
    };
    if (mode === "text") reader.readAsText(file);
    else reader.readAsArrayBuffer(file);
  });
}
