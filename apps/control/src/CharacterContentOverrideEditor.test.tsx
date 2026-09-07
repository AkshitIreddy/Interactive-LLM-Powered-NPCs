import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const overrides = vi.hoisted(() => ({
  read: vi.fn(),
  save: vi.fn(),
  reset: vi.fn(),
}));

vi.mock("./characterContentOverrides", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./characterContentOverrides")>()),
  readCharacterContentOverride: overrides.read,
  saveCharacterContentOverride: overrides.save,
  resetCharacterContentOverride: overrides.reset,
}));

import { CharacterContentOverrideEditor } from "./ProductWorkspaces";
import type { NativeCharacterInspection } from "./tauriBridge";

const inspection: NativeCharacterInspection = {
  schemaVersion: 1,
  gameProfileId: "cyberpunk-2077",
  gameDisplayName: "Cyberpunk 2077",
  selectedCharacterId: "misty-olzewski",
  character: {
    id: "misty-olzewski",
    displayName: "Misty Olszewski",
    aliases: ["Misty"],
    biography: "Pack biography for Misty.",
    personality: "Perceptive.",
    dialogueStyle: "Warm and concise.",
    styleExamples: [],
    openingLines: [],
    backgroundNpc: false,
    promptRole: "Esoterica owner",
    promptObjectives: ["Respond in character."],
    promptConstraints: ["Do not invent canon."],
    knowledgeRefs: [],
    voice: {
      description: "Warm",
      locale: "en-US",
      styleTags: [],
      providerVoiceId: null,
      adapterId: null,
      catalogVersion: null,
      license: null,
      userOverrideAllowed: true,
    },
    identity: {
      strategy: "explicit_selection",
      evidence: ["explicit_selection"],
      fallback: "explicit_selection",
      automaticFaceRecognitionClaimed: false,
    },
  },
  authoredKnowledge: [],
  provenance: [],
  deliveredMemory: [],
  memoryScope: {
    userId: "local-user",
    profileId: "cyberpunk-2077",
    gameId: "cyberpunk-2077",
    characterId: "misty-olzewski",
    sessionId: null,
    saveId: null,
    crossGameWideningAllowed: false,
  },
};

describe("CharacterContentOverrideEditor", () => {
  beforeEach(() => {
    overrides.read.mockReset();
    overrides.save.mockReset();
    overrides.reset.mockReset();
    overrides.read.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      saved: null,
      detail: "Pack values are active.",
    });
  });

  it("validates and persists the editable player layer by stable IDs", async () => {
    const user = userEvent.setup();
    const onChanged = vi.fn().mockResolvedValue(undefined);
    overrides.save.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      saved: {
        gameProfileId: "cyberpunk-2077",
        characterId: "misty-olzewski",
        displayName: "Misty, After Hours",
        biography: "Player-authored biography.",
        promptContext: "Treat V as a returning client.",
        updatedAtUnixMillis: 42,
      },
      persisted: true,
      detail: "Player layer saved.",
    });
    render(
      <CharacterContentOverrideEditor
        nativeAvailable
        editable
        inspection={inspection}
        onChanged={onChanged}
      />,
    );
    await user.click(screen.getByText("Customize this character"));
    const name = await screen.findByLabelText(/Character name/);
    const biography = screen.getByLabelText(/Backstory \/ biography/);
    const promptContext = screen.getByLabelText(/Extra prompt context/);
    await user.clear(name);
    expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();
    expect(screen.getByText("Enter a character name.")).toBeVisible();
    await user.type(name, "Misty, After Hours");
    await user.clear(biography);
    await user.type(biography, "Player-authored biography.");
    await user.type(promptContext, "Treat V as a returning client.");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(overrides.save).toHaveBeenCalledWith({
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      displayName: "Misty, After Hours",
      biography: "Player-authored biography.",
      promptContext: "Treat V as a returning client.",
    });
    await waitFor(() => expect(onChanged).toHaveBeenCalledOnce());
    expect(screen.getByText("Player layer active")).toBeVisible();
  });

  it("resets only the saved layer so refreshed pack values can return", async () => {
    const user = userEvent.setup();
    const onChanged = vi.fn().mockResolvedValue(undefined);
    const saved = {
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      displayName: "My Misty",
      biography: "My backstory.",
      promptContext: "Known customer.",
      updatedAtUnixMillis: 42,
    };
    overrides.read.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      saved,
      detail: "Player layer active.",
    });
    overrides.reset.mockResolvedValue({
      schemaVersion: 1,
      gameProfileId: "cyberpunk-2077",
      characterId: "misty-olzewski",
      saved: null,
      persisted: true,
      detail: "Current pack values restored.",
    });
    render(
      <CharacterContentOverrideEditor
        nativeAvailable
        editable
        inspection={inspection}
        onChanged={onChanged}
      />,
    );
    await user.click(screen.getByText("Customize this character"));
    expect(await screen.findByDisplayValue("My Misty")).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Reset to pack values" }),
    );
    expect(overrides.reset).toHaveBeenCalledWith(
      "cyberpunk-2077",
      "misty-olzewski",
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledOnce());
    expect(screen.getByText(/Current pack values restored/i)).toBeVisible();
  });

  it("explains pack-reset behavior but stays read-only in browser preview", async () => {
    const user = userEvent.setup();
    render(
      <CharacterContentOverrideEditor
        nativeAvailable={false}
        editable
        inspection={inspection}
        onChanged={vi.fn()}
      />,
    );
    await user.click(screen.getByText("Customize this character"));
    expect(screen.getByLabelText(/Character name/)).toBeDisabled();
    expect(screen.getByText(/latest pack defaults/i)).toBeVisible();
    expect(screen.getByText(/Browser preview is read-only/i)).toBeVisible();
  });
});
