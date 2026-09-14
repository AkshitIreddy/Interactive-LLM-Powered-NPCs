import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App";

describe("P0 honesty boundaries", () => {
  beforeEach(() => window.history.replaceState(null, "", "/"));

  it("keeps delivery empty until a runtime completion event", () => {
    render(<App />);
    expect(
      screen.getByText("Your conversation starts here"),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("ready to answer");
    expect(document.body).not.toHaveTextContent(
      "Live provider audio delivered",
    );
  });

  it("labels the synthetic target and does not claim a detected game", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?page=world");
    render(<App />);
    expect(
      screen.getByRole("button", { name: "Launch & connect" }),
    ).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: /Practice environment details/i }),
    );
    expect(screen.getByText("Debug fixture only")).toBeInTheDocument();
    expect(
      screen.getByText("interactive-npcs-synthetic-target.exe"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Native debug target unavailable" }),
    ).toBeDisabled();
    expect(document.body).not.toHaveTextContent("Detected installation");
  });

  it("keeps local packs blocked until qualification", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?page=voice");
    render(<App />);
    await user.click(screen.getByRole("button", { name: /Local models/i }));
    await user.click(screen.getByRole("button", { name: "Downloads" }));
    expect(
      screen.getByText("Native signed catalog unavailable in browser preview"),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Advanced packs" }));
    expect(
      screen.getByText("Native pack lifecycle unavailable in browser preview"),
    ).toBeVisible();
    expect(document.body).toHaveTextContent(
      /not a complete lip-sync model and is unavailable to normal product sessions/i,
    );
    expect(document.body).not.toHaveTextContent("Downloading");
  });

  it("renders no illustrative measurements in browser diagnostics", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?page=diagnostics");
    render(<App />);
    expect(screen.getByText("Not measured")).toBeInTheDocument();
    expect(screen.getByText("No native matrix loaded")).toBeInTheDocument();
    await user.click(screen.getByText("Local event history & recovery"));
    expect(document.body).toHaveTextContent(
      /Browser preview does not invent event, privacy, or recovery state/i,
    );
    expect(document.body).not.toHaveTextContent("84 ms");
  });

  it("states the external safety boundary and manual identity authority", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?page=world");
    render(<App />);
    await user.click(
      screen.getByRole("button", { name: /Practice environment details/i }),
    );
    expect(screen.getByText("Single-player only")).toBeInTheDocument();
    expect(document.body).toHaveTextContent(
      /Ordinary commercial-game capture remains fail-closed/i,
    );
    await user.click(
      screen.getByRole("button", { name: "Close Practice environment" }),
    );
    await user.click(
      screen.getByRole("button", { name: /Character recognition/i }),
    );
    expect(screen.getByText("Automatic match unavailable")).toBeVisible();
    expect(
      screen.getByText(
        /Choose the character manually; that selection controls its prompt, voice and memory/i,
      ),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /Recognize/i }),
    ).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Native game rig");
  });
});
