import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App";
import { resetBrowserLoadoutsForTests } from "./providerLoadouts";

describe("NPC 2.0 product console", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/");
    resetBrowserLoadoutsForTests();
  });

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
    expect(
      await screen.findByText("Browser preview — native actions unavailable"),
    ).toBeInTheDocument();
  });

  it("maps a saved legacy Characters route to World", () => {
    window.history.replaceState(null, "", "/?page=characters");
    render(<App />);
    expect(screen.getByRole("heading", { name: "World" })).toBeInTheDocument();
    expect(
      screen.getByText(
        /Process discovery and current-session binding require the native Windows shell/i,
      ),
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

  it("makes the API-first account and advanced profile controls reachable", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?page=voice");
    render(<App />);

    expect(
      screen.getByRole("heading", { name: "NVIDIA NIM" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Save cloud execution preference" }),
    ).toBeDisabled();

    await user.click(screen.getByText("Advanced profiles"));
    expect(screen.getAllByText("Balanced API").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("heading", {
        name: "Build a route for every kind of turn.",
      }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: /CHARACTER OVERRIDE/i }),
    );
    expect(screen.getByText("Mara · cinematic voice")).toBeInTheDocument();
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

  it("shows browser-only onboarding refusal inside the modal", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?onboarding=1");
    render(<App />);
    const dialog = screen.getByRole("dialog", {
      name: "Prove the native boundary",
    });

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(dialog).toHaveTextContent(
      "Open the native desktop app to save and continue guided setup.",
    );
    expect(screen.getByText("Step 1 of 4")).toBeInTheDocument();
  });

  it("does not claim a browser fixture was delivered", async () => {
    render(<App />);
    expect(
      screen.getByRole("button", {
        name: /Capture a PTT receipt first/i,
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: /Arm live PTT capture/i }),
    ).toBeDisabled();
    expect(
      screen.getByText(/Browser preview cannot allocate microphone/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No delivered turn in this session"),
    ).toBeInTheDocument();
  });

  it.each([
    {
      query: "onboarding",
      action: "Rerun guided setup",
      destination: "Prove the native boundary",
      role: "dialog" as const,
    },
    {
      query: "recovery",
      action: "Run native diagnostics",
      destination: "Diagnostics",
      role: "heading" as const,
    },
    {
      query: "retention",
      action: "Review privacy controls",
      destination: "Voice & models",
      role: "heading" as const,
    },
    {
      query: "loadout",
      action: "Open Voice & models",
      destination: "Voice & models",
      role: "heading" as const,
    },
    {
      query: "process",
      action: "Open World",
      destination: "World",
      role: "heading" as const,
    },
    {
      query: "brightness",
      action: "Open overlay diagnostics",
      destination: "Diagnostics",
      role: "heading" as const,
    },
  ])(
    "searches bundled help by keyboard and runs $action",
    async ({ query, action, destination, role }) => {
      const user = userEvent.setup();
      window.history.replaceState(null, "", "/?page=settings");
      render(<App />);

      const search = screen.getByRole("searchbox", {
        name: "Search the bundled guide",
      });
      await user.type(search, query);
      await user.keyboard("{Enter}");
      expect(screen.getByLabelText("Selected guide topic")).toHaveTextContent(
        "support-2026-08-30",
      );

      await user.click(screen.getByRole("button", { name: action }));
      expect(screen.getByRole(role, { name: destination })).toBeInTheDocument();
    },
  );
});
