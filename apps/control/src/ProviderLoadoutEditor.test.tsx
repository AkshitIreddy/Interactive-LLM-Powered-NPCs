import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { ProviderLoadoutEditor } from "./ProviderLoadoutEditor";
import { resetBrowserLoadoutsForTests } from "./providerLoadouts";

describe("provider and model loadouts", () => {
  beforeEach(() => {
    window.localStorage.clear();
    resetBrowserLoadoutsForTests();
  });

  it("shows explicit role routes and a next-turn boundary", () => {
    render(<ProviderLoadoutEditor />);

    expect(
      screen.getByRole("heading", { name: /Build a route/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("Swaps begin next turn")).toBeInTheDocument();
    expect(screen.getByLabelText("Reply model provider")).toHaveValue("openai");
    expect(screen.getByLabelText("Speech recognition provider")).toHaveValue(
      "openai",
    );
    expect(screen.getByLabelText("Speech recognition model")).toHaveAttribute(
      "title",
      "GPT-4o mini Transcribe",
    );
    expect(screen.getByLabelText("Character voice provider")).toHaveValue(
      "elevenlabs",
    );
    expect(screen.getByLabelText("Memory retrieval provider")).toHaveValue(
      "fts-only",
    );
    expect(screen.getByLabelText("Optional lip-sync provider")).toHaveValue(
      "disabled",
    );
    expect(document.body).not.toHaveTextContent("API key value");
  });

  it("creates, renames, clones, and activates an initially inactive scoped loadout", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    await user.click(
      screen.getByRole("button", { name: /GAME OVERRIDE Game/ }),
    );
    await user.click(
      screen.getByRole("button", { name: "Create game loadout" }),
    );
    const name = screen.getByRole("textbox", { name: "Loadout name" });
    await user.clear(name);
    await user.type(name, "Quiet night route");
    expect(name).toHaveValue("Quiet night route");

    await user.click(screen.getByRole("button", { name: "Clone" }));
    expect(screen.getByRole("textbox", { name: "Loadout name" })).toHaveValue(
      "Quiet night route copy",
    );
    await user.click(
      screen.getByRole("button", { name: "Activate for next turn" }),
    );
    expect(
      screen.getByRole("button", { name: "Active for next turn" }),
    ).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      /turn already listening.*keeps its original route/i,
    );
  });

  it("requires explicit authorization for manual recovery and never calls it automatic", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    const fallback = screen.getByLabelText(
      "Reply model manual fallback provider",
    );
    expect(fallback).toBeDisabled();
    await user.click(
      screen.getByRole("checkbox", { name: /LLM manual retry/ }),
    );
    expect(fallback).toBeEnabled();
    expect(screen.getByText("Automatic fallback disabled")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      /still require your action/i,
    );
  });

  it("can save a desired lip-sync engine without claiming it is downloadable", async () => {
    const user = userEvent.setup();
    render(<ProviderLoadoutEditor />);

    await user.selectOptions(
      screen.getByLabelText("Optional lip-sync provider"),
      "local-visual-worker",
    );
    await user.selectOptions(
      screen.getByLabelText("Optional lip-sync model"),
      "musetalk",
    );
    expect(screen.getByLabelText("Optional lip-sync model")).toHaveValue(
      "musetalk",
    );
    expect(screen.getByText(/No pack is downloadable yet/)).toBeInTheDocument();
  });
});
