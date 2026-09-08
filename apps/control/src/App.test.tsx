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

  it("renders four game-menu destinations with troubleshooting out of primary navigation", async () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Mara Venn" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Session signal rail")).toHaveTextContent(
      "Eclipse Harbor",
    );
    for (const label of ["Channel", "Night City", "Loadout", "Settings"]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(
      screen.queryByRole("button", { name: "Diagnostics" }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByText("Browser preview — native actions unavailable"),
    ).toBeInTheDocument();
  });

  it("maps a saved legacy Characters route to World", () => {
    window.history.replaceState(null, "", "/?page=characters");
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Night City" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Your Cyberpunk characters, voices and memories/i),
    ).toBeInTheDocument();
  });

  it("navigates without a reload and updates the owned route", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "Loadout" }));
    expect(
      screen.getByRole("heading", { name: "Neural loadout" }),
    ).toBeInTheDocument();
    expect(window.location.search).toBe("?page=voice");
  });

  it("keeps accounts and scoped model profiles reachable through progressive sections", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/?page=voice");
    render(<App />);

    expect(screen.getByLabelText("Voice workspace")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Accounts/i }));
    expect(
      screen.getByRole("heading", { name: "Provider accounts" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connect account" }),
    ).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Model loadout/i }));
    expect(screen.getAllByText("Balanced API").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("heading", {
        name: /Choose the route for the next conversation/i,
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
      screen.getByRole("dialog", { name: /get you connected/i }),
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
    const dialog = screen.getByRole("dialog", { name: /get you connected/i });

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
        name: /Record a message first/i,
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: /Enable push-to-talk/i }),
    ).toBeDisabled();
    expect(
      screen.getByText(/Browser preview cannot allocate microphone/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Your conversation starts here"),
    ).toBeInTheDocument();
  });

  it.each([
    {
      query: "onboarding",
      action: "Rerun guided setup",
      destination: /get you connected/i,
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
      destination: "Neural loadout",
      role: "heading" as const,
    },
    {
      query: "loadout",
      action: "Open loadout",
      destination: "Neural loadout",
      role: "heading" as const,
    },
    {
      query: "process",
      action: "Open Night City",
      destination: "Night City",
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

      await user.click(screen.getByRole("button", { name: /^Help/i }));

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
