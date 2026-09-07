import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  inspect: vi.fn(),
  activate: vi.fn(),
  state: vi.fn(),
}));

vi.mock("./contentPacks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./contentPacks")>()),
  inspectContentPackJson: bridge.inspect,
  activateContentPack: bridge.activate,
  readContentPackState: bridge.state,
}));

import { ContentPackWorkspace } from "./ContentPackWorkspace";

const preview = {
  schemaVersion: 1 as const,
  namespace: "org.example",
  packId: "night-city-authored",
  version: "1.0.0",
  gameProfileId: "cyberpunk-2077",
  title: "Night City authored context",
  summary: "Original lore and character context.",
  contentSha256: "a".repeat(64),
  displayName: "Cyberpunk 2077",
  characterCount: 7,
  knowledgeCount: 18,
  providerRecommendations: [
    {
      role: "llm" as const,
      providerId: "groq",
      modelId: "qwen/qwen3.6-27b",
      characterId: null,
      voiceId: null,
      rationale: "Fast structured dialogue candidate.",
      catalogAvailable: true,
      detail: "Selectable but not applied automatically.",
    },
  ],
  rights: {
    license_name: "CC-BY-4.0",
    source_summary: "Original text.",
    redistributable: true,
    commercial_use: true,
    derivative_use: true,
    review_status: "approved" as const,
  },
  applyDetail: "Saved player choices remain unchanged.",
  networkRequestPerformed: false as const,
  providerRoutesChanged: false as const,
};

describe("ContentPackWorkspace", () => {
  beforeEach(() => {
    bridge.inspect.mockReset();
    bridge.activate.mockReset();
    bridge.state.mockReset();
    bridge.state.mockResolvedValue({
      schemaVersion: 1,
      active: [],
      detail: "None",
    });
    bridge.inspect.mockResolvedValue(preview);
    bridge.activate.mockResolvedValue({
      namespace: preview.namespace,
      packId: preview.packId,
      version: preview.version,
      gameProfileId: preview.gameProfileId,
      title: preview.title,
      contentSha256: preview.contentSha256,
      activatedAtUnixMillis: 1,
      characterCount: 7,
      knowledgeCount: 18,
      providerRecommendationCount: 1,
    });
  });

  it("previews exact file bytes, exposes provider editing, and applies the reviewed digest", async () => {
    const user = userEvent.setup();
    const onApplied = vi.fn();
    const onOpenProviders = vi.fn();
    bridge.state
      .mockResolvedValueOnce({ schemaVersion: 1, active: [], detail: "None" })
      .mockResolvedValueOnce({
        schemaVersion: 1,
        active: [
          {
            namespace: preview.namespace,
            packId: preview.packId,
            version: preview.version,
            gameProfileId: preview.gameProfileId,
            title: preview.title,
            contentSha256: preview.contentSha256,
            activatedAtUnixMillis: 1,
            characterCount: 7,
            knowledgeCount: 18,
            providerRecommendationCount: 1,
          },
        ],
        detail: "Active",
      });
    render(
      <ContentPackWorkspace
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        onApplied={onApplied}
        onOpenProviders={onOpenProviders}
      />,
    );
    const fileText = '{"format":"npc.content-pack"}';
    const file = new File([fileText], "night-city.json", {
      type: "application/json",
    });
    await user.upload(screen.getByLabelText("Choose content-pack JSON"), file);
    await user.click(screen.getByRole("button", { name: "Preview pack" }));
    await screen.findByText("Night City authored context");
    expect(bridge.inspect).toHaveBeenCalledWith(fileText);
    await user.click(
      screen.getByRole("button", { name: "Review in Voice & models" }),
    );
    expect(onOpenProviders).toHaveBeenCalledOnce();
    await user.click(screen.getByRole("button", { name: "Use pack content" }));
    await waitFor(() => expect(onApplied).toHaveBeenCalledOnce());
    expect(bridge.activate).toHaveBeenCalledWith(fileText, "a".repeat(64));
    expect(await screen.findByText("Pack active")).toBeInTheDocument();
  });

  it("rejects archives before any native inspection", async () => {
    render(
      <ContentPackWorkspace
        nativeAvailable
        gameProfileId="cyberpunk-2077"
        onApplied={() => undefined}
        onOpenProviders={() => undefined}
      />,
    );
    fireEvent.change(screen.getByLabelText("Choose content-pack JSON"), {
      target: {
        files: [new File(["archive"], "pack.zip", { type: "application/zip" })],
      },
    });
    expect(
      await screen.findByText(/Choose a \.json content-pack file/),
    ).toBeInTheDocument();
    expect(bridge.inspect).not.toHaveBeenCalled();
  });

  it("shows that appearance references remain a separate qualified workflow", async () => {
    render(
      <ContentPackWorkspace
        nativeAvailable={false}
        gameProfileId="cyberpunk-2077"
        onApplied={() => undefined}
        onOpenProviders={() => undefined}
      />,
    );
    expect(screen.getByText(/does not enroll face images/)).toBeInTheDocument();
    expect(screen.getByLabelText("Choose content-pack JSON")).toBeDisabled();
  });
});
