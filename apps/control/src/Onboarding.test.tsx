import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Onboarding } from "./Onboarding";
import type { AppPreferences } from "./types";

const preferences: AppPreferences = {
  execution: "cloud",
  performance: "balanced",
  subtitles: true,
  ptt: true,
  localOnly: false,
  // A stale saved preference must not turn unavailable capture or animation on.
  screenPresence: true,
  diagnostics: true,
};

function renderStep(initialStep: number) {
  return render(
    <Onboarding
      initialStep={initialStep}
      preferences={preferences}
      updatePreferences={vi.fn()}
      onClose={vi.fn()}
    />,
  );
}

describe("legacy onboarding evidence boundaries", () => {
  it("shows hardware as unmeasured instead of naming a fixture PC or fit", () => {
    renderStep(1);

    expect(
      screen.getByRole("heading", {
        name: "No hardware fit has been measured.",
      }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Unmeasured").length).toBeGreaterThan(0);
    expect(document.body).not.toHaveTextContent("RTX 4080");
    expect(document.body).not.toHaveTextContent("i9-13980HX");
    expect(document.body).not.toHaveTextContent("Great fit");
  });

  it("does not turn a boolean rehearsal into microphone or device proof", () => {
    renderStep(5);

    expect(
      screen.getByRole("button", {
        name: "Native microphone rehearsal unavailable",
      }),
    ).toBeDisabled();
    expect(screen.getByText("No native device evidence")).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("HEARD CLEARLY");
    expect(document.body).not.toHaveTextContent("Realtek");
    expect(document.body).not.toHaveTextContent("look good");
  });

  it("keeps capture and visual animation separately disabled with exact reasons", () => {
    renderStep(6);

    const capture = screen.getByRole("switch", {
      name: /See the game window/i,
    });
    const animation = screen.getByRole("switch", {
      name: /Animate visible speech/i,
    });
    expect(capture).toBeDisabled();
    expect(animation).toBeDisabled();
    expect(capture).not.toHaveAttribute("data-selected");
    expect(animation).not.toHaveAttribute("data-selected");
    expect(
      screen.getByText(/ordinary game capture is fail-closed/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/no signed complete lip-sync pack/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("5.2 GB");
  });

  it("does not turn fixture presets or a timer into performance and turn proof", () => {
    const { unmount } = renderStep(7);
    expect(
      screen.getAllByRole("button", {
        name: /Competitive|Fast|Balanced|Immersive|Maximum quality|Custom/,
      }),
    ).toEqual(
      expect.arrayContaining([expect.objectContaining({ disabled: true })]),
    );
    expect(
      screen.getByText(/No FPS or VRAM target is inferred/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(/≤\s*(2|3|5|10|15)%/);
    expect(screen.getAllByText("Unmeasured").length).toBeGreaterThan(0);
    expect(document.body).not.toHaveTextContent("12 GB");
    unmount();

    renderStep(8);
    expect(
      screen.getByRole("heading", {
        name: "No conversation test ran in this view.",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Run private test" }),
    ).toBeDisabled();
    expect(document.body).not.toHaveTextContent("1.31 seconds");
    expect(document.body).not.toHaveTextContent("All checks passed");
  });

  it("does not advertise VAD, fully-local readiness, or measured frame protection", () => {
    renderStep(0);
    expect(document.body).toHaveTextContent(
      /Voice activity detection is unavailable/i,
    );
    expect(document.body).toHaveTextContent(/API-first by default/i);
    expect(document.body).toHaveTextContent(
      /fully local intent remain experimental or unavailable/i,
    );
    expect(document.body).toHaveTextContent(
      /No frame-rate promise is inferred/i,
    );
    expect(document.body).not.toHaveTextContent(/Talk naturally/i);
    expect(document.body).not.toHaveTextContent(/Protect frame rate/i);
  });
});
