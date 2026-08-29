import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./App";

describe("P0 honesty boundaries", () => {
  it("does not present authored catalog data as a live runtime snapshot", () => {
    render(<App />);
    expect(screen.getByText("No live game reported")).toBeInTheDocument();
    expect(screen.getByLabelText("Response pipeline")).toHaveTextContent(
      "NO LIVE TURN",
    );
    expect(document.body).not.toHaveTextContent("Runtime ready");
    expect(document.body).not.toHaveTextContent("Replay verified");
    expect(document.body).not.toHaveTextContent("ready to answer");
  });

  it("labels every static game profile as authored and not live-certified", () => {
    window.history.replaceState(null, "", "/?page=games");
    render(<App />);
    expect(screen.getAllByText("Authored · not live-certified")).toHaveLength(
      17,
    );
    expect(document.body).not.toHaveTextContent("Detected");
    expect(document.body).not.toHaveTextContent("Native rig + subtitles");
    expect(document.body.textContent).not.toMatch(/\d+ authored/);
  });

  it("keeps the baseline API-first and local downloads optional", () => {
    window.history.replaceState(null, "", "/?page=models");
    render(<App />);
    expect(screen.getByText(/API first/)).toBeInTheDocument();
    expect(screen.getByText(/no local model required/)).toBeInTheDocument();
    expect(screen.getByText(/Current frame wins/)).toBeInTheDocument();
    expect(screen.getByText("Audio2Face-3D regression")).toBeInTheDocument();
    expect(screen.getByText("NVIDIA AR SDK LipSync")).toBeInTheDocument();
    expect(screen.getByText("MuseTalk 1.5")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "No qualified visual packs" }),
    ).toBeDisabled();
    expect(
      screen.getByText("No visual route is selectable or installed."),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Mark desired");
    expect(document.body).not.toHaveTextContent("Pack unavailable");
    expect(document.body).not.toHaveTextContent("Moonshine Medium");
    expect(document.body).not.toHaveTextContent("llama.cpp");
    expect(document.body).not.toHaveTextContent("signatures current");
    expect(document.body).not.toHaveTextContent("Downloading");
  });

  it("marks hardcoded response timing as simulation fixture data", () => {
    window.history.replaceState(null, "", "/?page=home&state=active");
    render(<App />);
    expect(screen.getByLabelText("Response pipeline")).toHaveTextContent(
      "SIMULATED TURN · BROWSER FIXTURE",
    );
    expect(screen.getByLabelText("Response pipeline")).toHaveTextContent(
      "SIM 412 ms",
    );
    expect(screen.getByLabelText("Response pipeline")).toHaveTextContent(
      "SIM FIXTURE",
    );
    expect(screen.getByText("Responding · 5/7")).toBeInTheDocument();
  });

  it("keeps onboarding completion explicitly unverified", () => {
    window.history.replaceState(null, "", "/?onboarding=1&step=9");
    render(<App />);
    expect(screen.getByText("SETUP PREVIEW COMPLETE")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /Runtime verification remains/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connect and verify runtime" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Run private simulation" }),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("RESPONSE CONSOLE READY");
  });

  it("shows authored onboarding examples without claiming game detection", () => {
    window.history.replaceState(null, "", "/?onboarding=1&step=3");
    render(<App />);

    expect(
      screen.getByText(
        "6 authored profile examples · runtime detection pending",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No detected installations in this fixture."),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Authored · unverified")).toHaveLength(6);
    expect(
      screen.getByRole("button", { name: "Detection pending" }),
    ).toBeDisabled();
    expect(document.body).not.toHaveTextContent("installations found");
  });

  it("keeps game presentation external and injection-free", () => {
    window.history.replaceState(null, "", "/?page=presence");
    render(<App />);

    expect(
      screen.getByText("External game-window capture"),
    ).toBeInTheDocument();
    expect(screen.getByText(/no injection into the game/i)).toBeInTheDocument();
    expect(screen.getByText("Screen-space mouth motion")).toBeInTheDocument();
    expect(screen.getByText(/Optional experiment/)).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "The game frame stays in charge." }),
    ).toBeInTheDocument();
    expect(screen.getByText("Mouth residual only")).toBeInTheDocument();
    expect(screen.getByText("FAIL OPEN")).toBeInTheDocument();
    expect(
      screen.getByText(/show the untouched game on the next displayed frame/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: /Keep presentation external/ }),
    ).toBeDisabled();
    expect(document.body).not.toHaveTextContent("Native game rig");
    expect(document.body).not.toHaveTextContent("exact-build adapter");
  });

  it("explains local resource admission without claiming a live fit", () => {
    window.history.replaceState(null, "", "/?page=performance");
    render(<App />);

    expect(
      screen.getByRole("heading", { name: "Measure before a model loads." }),
    ).toBeInTheDocument();
    for (const state of ["Fits", "CPU-only", "Conflicts", "Unverified"]) {
      expect(screen.getByText(state)).toBeInTheDocument();
    }
    expect(
      screen.getByText(/never silently loaded or moved to cloud/i),
    ).toBeInTheDocument();
  });

  it("does not offer an unprovisioned local model during simulated recovery", () => {
    window.history.replaceState(null, "", "/?page=diagnostics&state=error");
    render(<App />);
    expect(
      screen.getByRole("button", {
        name: "Advanced local fallback unavailable",
      }),
    ).toBeDisabled();
    expect(document.body).not.toHaveTextContent("Use local model this turn");
  });

  it("uses an actionable simulation CTA instead of Return home for generic empty views", () => {
    window.history.replaceState(null, "", "/?page=home&state=empty");
    render(<App />);
    expect(
      screen.getByRole("button", { name: "Run private simulation" }),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Return home");
  });
});
