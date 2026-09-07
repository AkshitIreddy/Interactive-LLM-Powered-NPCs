import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  readCharacterContentOverride,
  resetCharacterContentOverride,
  saveCharacterContentOverride,
} from "./characterContentOverrides";

describe("character content override bridge", () => {
  beforeEach(() => {
    invoke.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  it("uses stable game and character scope for read, save, and reset", async () => {
    invoke.mockResolvedValue({ persisted: true });
    await readCharacterContentOverride("cyberpunk-2077", "misty-olzewski");
    expect(invoke).toHaveBeenNthCalledWith(1, "character_content_override", {
      request: {
        gameProfileId: "cyberpunk-2077",
        characterId: "misty-olzewski",
      },
    });

    const draft = {
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      displayName: "Misty",
      biography: "A player-authored backstory.",
      promptContext: "The player is a returning customer.",
    };
    await saveCharacterContentOverride(draft);
    expect(invoke).toHaveBeenNthCalledWith(
      2,
      "save_character_content_override",
      { request: draft },
    );

    await resetCharacterContentOverride("cyberpunk-2077", "misty-olzewski");
    expect(invoke).toHaveBeenNthCalledWith(
      3,
      "reset_character_content_override",
      {
        request: {
          gameProfileId: "cyberpunk-2077",
          characterId: "misty-olzewski",
        },
      },
    );
  });
});
