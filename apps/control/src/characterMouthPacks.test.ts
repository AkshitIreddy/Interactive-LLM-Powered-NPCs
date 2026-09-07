import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  disableCharacterMouthPack,
  enableCharacterMouthPack,
  importCharacterMouthPack,
  inspectCharacterMouthPack,
  readCharacterMouthPackState,
  readMouthPackFiles,
} from "./characterMouthPacks";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("character mouth-pack bridge", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("keeps exact reviewed bytes and scope across review, import, enable, and disable", async () => {
    invokeMock.mockResolvedValue({});
    const files = {
      gameProfileId: "cyberpunk-2077",
      atlasJsonText: '{"schemaVersion":2}',
      textureFileName: "misty-mouth.bin",
      textureBase64: "AQID",
    };
    const digest = "a".repeat(64);
    await inspectCharacterMouthPack(files);
    await importCharacterMouthPack(files, digest);
    await enableCharacterMouthPack("cyberpunk-2077", digest);
    await disableCharacterMouthPack("cyberpunk-2077", digest);

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "inspect_character_mouth_pack",
      {
        files,
      },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "import_character_mouth_pack",
      {
        request: { files, expectedContentSha256: digest },
      },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      3,
      "enable_character_mouth_pack",
      {
        request: {
          gameProfileId: "cyberpunk-2077",
          expectedContentSha256: digest,
        },
      },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      4,
      "disable_character_mouth_pack",
      {
        request: {
          gameProfileId: "cyberpunk-2077",
          expectedContentSha256: digest,
        },
      },
    );
  });

  it("reads the manifest as text and texture as bounded base64", async () => {
    const files = await readMouthPackFiles(
      "cyberpunk-2077",
      new File(['{"schemaVersion":2}'], "atlas.json", {
        type: "application/json",
      }),
      new File([new Uint8Array([1, 2, 3])], "misty-mouth.bin", {
        type: "application/octet-stream",
      }),
    );
    expect(files).toEqual({
      gameProfileId: "cyberpunk-2077",
      atlasJsonText: '{"schemaVersion":2}',
      textureFileName: "misty-mouth.bin",
      textureBase64: "AQID",
    });
  });

  it("rejects unsupported files before invoking native code and keeps browser state read-only", async () => {
    await expect(
      readMouthPackFiles(
        "cyberpunk-2077",
        new File(["{}"], "atlas.txt"),
        new File([new Uint8Array([1])], "texture.bin"),
      ),
    ).rejects.toThrow(/atlas\.json manifest/i);
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    await expect(readCharacterMouthPackState()).resolves.toBeNull();
    await expect(
      inspectCharacterMouthPack({
        gameProfileId: "cyberpunk-2077",
        atlasJsonText: "{}",
        textureFileName: "texture.bin",
        textureBase64: "AQ==",
      }),
    ).rejects.toThrow(/installed Windows app/i);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
