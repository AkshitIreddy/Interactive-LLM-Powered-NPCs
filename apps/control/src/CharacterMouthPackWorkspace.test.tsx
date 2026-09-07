import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mouthPacks = vi.hoisted(() => ({
  inspect: vi.fn(),
  importPack: vi.fn(),
  enable: vi.fn(),
  disable: vi.fn(),
  state: vi.fn(),
}));

vi.mock("./characterMouthPacks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./characterMouthPacks")>()),
  inspectCharacterMouthPack: mouthPacks.inspect,
  importCharacterMouthPack: mouthPacks.importPack,
  enableCharacterMouthPack: mouthPacks.enable,
  disableCharacterMouthPack: mouthPacks.disable,
  readCharacterMouthPackState: mouthPacks.state,
}));

import { CharacterMouthPackWorkspace } from "./ProductWorkspaces";

const digest = "a".repeat(64);
const preview = {
  schemaVersion: 1 as const,
  gameProfileId: "cyberpunk-2077",
  characterId: "misty-olzewski",
  atlasSchemaVersion: 2,
  identityRevision: 3,
  manifestSha256: "b".repeat(64),
  textureFileName: "misty-mouth.bin",
  textureSizeBytes: 3,
  contentSha256: digest,
  enrollmentBindingSha256: "c".repeat(64),
  privateReviewBindingValidated: true,
  networkRequestPerformed: false as const,
};
const installed = {
  ...preview,
  importedAtUnixMs: 42,
};

describe("CharacterMouthPackWorkspace", () => {
  beforeEach(() => {
    for (const mock of Object.values(mouthPacks)) mock.mockReset();
    mouthPacks.state.mockResolvedValue({
      schemaVersion: 1,
      installed: [],
      enabled: [],
      detail: "No character mouth packs are installed.",
    });
    mouthPacks.inspect.mockResolvedValue(preview);
    mouthPacks.importPack.mockResolvedValue(installed);
    mouthPacks.enable.mockResolvedValue({
      gameProfileId: preview.gameProfileId,
      characterId: preview.characterId,
      contentSha256: digest,
      enrollmentBindingSha256: preview.enrollmentBindingSha256,
      enabledAtUnixMs: 43,
    });
  });

  it("reviews exact files, imports the reviewed digest, and enables only by an explicit action", async () => {
    const user = userEvent.setup();
    render(
      <CharacterMouthPackWorkspace
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        characterId="misty-olzewski"
      />,
    );
    await user.click(screen.getByText("Character mouth pack"));
    await user.upload(
      screen.getByLabelText("Choose mouth atlas JSON"),
      new File(['{"schemaVersion":2}'], "atlas.json", {
        type: "application/json",
      }),
    );
    await user.upload(
      screen.getByLabelText("Choose mouth atlas texture"),
      new File([new Uint8Array([1, 2, 3])], "misty-mouth.bin", {
        type: "application/octet-stream",
      }),
    );
    await user.click(screen.getByRole("button", { name: "Review mouth pack" }));

    await waitFor(() => expect(mouthPacks.inspect).toHaveBeenCalledOnce());
    expect(mouthPacks.inspect).toHaveBeenCalledWith({
      gameProfileId: "cyberpunk-2077",
      atlasJsonText: '{"schemaVersion":2}',
      textureFileName: "misty-mouth.bin",
      textureBase64: "AQID",
    });
    expect(mouthPacks.importPack).not.toHaveBeenCalled();
    expect(mouthPacks.enable).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "Import reviewed pack" }),
    );
    expect(mouthPacks.importPack).toHaveBeenCalledWith(
      expect.objectContaining({ gameProfileId: "cyberpunk-2077" }),
      digest,
    );
    expect(mouthPacks.enable).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "Enable for selected actor" }),
    );
    expect(mouthPacks.enable).toHaveBeenCalledWith("cyberpunk-2077", digest);
  });

  it("shows installed state and disables the binding while retaining its revision", async () => {
    const user = userEvent.setup();
    mouthPacks.state
      .mockResolvedValueOnce({
        schemaVersion: 1,
        installed: [installed],
        enabled: [
          {
            gameProfileId: preview.gameProfileId,
            characterId: preview.characterId,
            contentSha256: digest,
            enrollmentBindingSha256: preview.enrollmentBindingSha256,
            enabledAtUnixMs: 43,
          },
        ],
        detail: "One binding enabled.",
      })
      .mockResolvedValue({
        schemaVersion: 1,
        installed: [installed],
        enabled: [],
        detail: "Binding disabled; revision retained.",
      });
    mouthPacks.disable.mockResolvedValue({
      gameProfileId: preview.gameProfileId,
      characterId: preview.characterId,
      contentSha256: digest,
      disabled: true,
      retainedInstalledRevision: true,
    });
    render(
      <CharacterMouthPackWorkspace
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        characterId="misty-olzewski"
      />,
    );
    await user.click(screen.getByText("Character mouth pack"));
    expect((await screen.findAllByText("Enabled")).length).toBeGreaterThan(0);
    await user.click(
      screen.getByRole("button", { name: "Disable mouth pack" }),
    );
    expect(mouthPacks.disable).toHaveBeenCalledWith("cyberpunk-2077", digest);
    expect(await screen.findByText(/remains installed/i)).toBeVisible();
  });

  it("states the enrollment boundary and stays read-only in browser preview", async () => {
    const user = userEvent.setup();
    render(
      <CharacterMouthPackWorkspace
        nativeAvailable={false}
        gameProfileId="cyberpunk-2077"
        characterId="misty-olzewski"
      />,
    );
    await user.click(screen.getByText("Character mouth pack"));
    expect(
      screen.getByText(/prepared mouth pack for this character/i),
    ).toBeVisible();
    expect(screen.getByLabelText("Choose mouth atlas JSON")).toBeDisabled();
    expect(screen.getByText(/Browser preview is read-only/i)).toBeVisible();
  });
});
