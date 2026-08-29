import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";

describe("Response Console", () => {
  it("renders the ready command deck and all primary destinations", () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: /your worlds are authored/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("complementary", { name: "Primary" }),
    ).toBeInTheDocument();
    for (const label of [
      "Home",
      "Games",
      "Characters",
      "Conversation",
      "Presence",
      "Performance",
      "Models",
      "Diagnostics",
      "Settings",
      "Help",
    ]) {
      expect(
        screen.getByRole("button", { name: new RegExp(`^${label}`) }),
      ).toBeInTheDocument();
    }
  });

  it("navigates to the profile library without a reload", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: /^Games/ }));
    expect(
      screen.getByRole("heading", { name: "Choose a world." }),
    ).toBeInTheDocument();
    expect(screen.getByText("Skyrim Special Edition")).toBeInTheDocument();
    expect(window.location.search).toContain("page=games");
  });

  it("supports deep-linked page, degraded, contrast, and reduced-motion states", () => {
    window.history.replaceState(
      null,
      "",
      "/?page=presence&state=degraded&contrast=high&motion=reduce",
    );
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Seen only when useful." }),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Simulated audio-and-subtitle fallback",
    );
    expect(document.documentElement.dataset.contrast).toBe("high");
    expect(document.documentElement).toHaveClass("force-reduced-motion");
  });

  it("supports deterministic optical-light and large-text acceptance routes", () => {
    window.history.replaceState(null, "", "/?theme=light&largeText=1");
    render(<App />);
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.largeText).toBe("true");
  });

  it("renders the requested onboarding step and preserves privacy language", () => {
    window.history.replaceState(null, "", "/?onboarding=1&step=2");
    render(<App />);
    const onboarding = screen.getByTestId("onboarding");
    expect(
      within(onboarding).getByRole("heading", {
        name: "Where should conversations happen?",
      }),
    ).toBeInTheDocument();
    expect(
      within(onboarding).getByText("Cloud use is never automatic"),
    ).toBeInTheDocument();
    expect(
      within(onboarding).getByRole("button", { name: /API first/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      within(onboarding).getByRole("button", { name: /Hybrid/ }),
    ).toBeDisabled();
    expect(
      within(onboarding).getByRole("button", { name: /Fully local/ }),
    ).toBeDisabled();
  });

  it("keeps local conversation packs unavailable in API-first setup", () => {
    window.history.replaceState(null, "", "/?onboarding=1&step=2");
    render(<App />);
    expect(screen.getAllByText("No selectable local pack")).toHaveLength(2);
    expect(document.body).not.toHaveTextContent(
      "Depends on selected local packs",
    );
    expect(document.body).not.toHaveTextContent("6.2 GB local storage");
    expect(document.body).not.toHaveTextContent("4.8 GB local storage");
  });

  it("opens command search with Ctrl+K and navigates to a result", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.keyboard("{Control>}k{/Control}");
    const dialog = screen.getByRole("dialog", { name: "Find anything" });
    await user.type(
      within(dialog).getByPlaceholderText(/go to a page/i),
      "diagnostics",
    );
    await user.click(
      within(dialog).getByRole("button", { name: /Open Diagnostics/ }),
    );
    expect(
      screen.getByRole("heading", { name: "Illustrative diagnostic fixture." }),
    ).toBeInTheDocument();
  });

  it("runs the deterministic Response Spine sequence and stops cleanly", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);
    await user.click(
      screen.getByRole("button", { name: "Run a private simulation" }),
    );
    expect(screen.getByLabelText("Response pipeline")).toHaveClass("is-live");
    expect(screen.getByText("Simulation fixture active")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "End simulation" }));
    expect(screen.getByLabelText("Response pipeline")).not.toHaveClass(
      "is-live",
    );
    expect(screen.getByRole("status")).toHaveTextContent(/simulation ended/i);
    vi.useRealTimers();
  });
});
