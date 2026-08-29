import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App";

describe("NPC 2.0 product console", () => {
  beforeEach(() => window.history.replaceState(null, "", "/"));

  it("renders the compact session deck and five owned destinations", async () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Mara Venn" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Session signal rail")).toHaveTextContent(
      "Eclipse Harbor",
    );
    for (const label of [
      "01 Session deck",
      "02 World",
      "03 Voice & models",
      "04 Diagnostics",
      "05 Settings & guide",
    ]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(await screen.findByText(/Browser preview/i)).toBeInTheDocument();
  });

  it("maps a saved legacy Characters route to World", () => {
    window.history.replaceState(null, "", "/?page=characters");
    render(<App />);
    expect(screen.getByRole("heading", { name: "World" })).toBeInTheDocument();
    expect(
      screen.getByText(/explicit selection in the Eclipse Harbor profile/i),
    ).toBeInTheDocument();
  });

  it("navigates without a reload and updates the owned route", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: /Voice & models/i }));
    expect(
      screen.getByRole("heading", { name: "Voice & models" }),
    ).toBeInTheDocument();
    expect(window.location.search).toBe("?page=voice");
  });

  it("renders the four-step setup flow on explicit preview", () => {
    window.history.replaceState(null, "", "/?onboarding=1");
    render(<App />);
    expect(
      screen.getByRole("dialog", { name: "Prove the native boundary" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Step 1 of 4")).toBeInTheDocument();
    expect(
      screen.getByText(/Open the native desktop shell for a real check/i),
    ).toBeInTheDocument();
  });

  it("does not claim a browser fixture was delivered", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: /Run spoken turn/i }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      /Native desktop runtime is required/i,
    );
    expect(
      screen.getByText("No delivered turn in this session"),
    ).toBeInTheDocument();
  });
});
