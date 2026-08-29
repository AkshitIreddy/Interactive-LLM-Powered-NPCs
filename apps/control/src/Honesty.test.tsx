import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App";

describe("P0 honesty boundaries", () => {
  beforeEach(() => window.history.replaceState(null, "", "/"));

  it("keeps delivery empty until a runtime completion event", () => {
    render(<App />);
    expect(
      screen.getByText("No delivered turn in this session"),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("ready to answer");
    expect(document.body).not.toHaveTextContent(
      "Live provider audio delivered",
    );
  });

  it("labels the synthetic target and does not claim a detected game", () => {
    window.history.replaceState(null, "", "/?page=world");
    render(<App />);
    expect(
      screen.getByText(/Synthetic validation target/i),
    ).toBeInTheDocument();
    expect(screen.getByText(/task-owned GUI executable/i)).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Detected installation");
  });

  it("keeps local packs blocked until qualification", () => {
    window.history.replaceState(null, "", "/?page=voice");
    render(<App />);
    expect(
      screen.getByText("Local visual packs not qualified"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No download or enable control is shown yet."),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Downloading");
  });

  it("renders no illustrative measurements in browser diagnostics", () => {
    window.history.replaceState(null, "", "/?page=diagnostics");
    render(<App />);
    expect(
      screen.getByText(/No illustrative CPU, GPU, latency/i),
    ).toBeInTheDocument();
    expect(screen.getByText("No native results yet")).toBeInTheDocument();
    expect(
      screen.getByText(/Browser preview deliberately invents nothing/i),
    ).toBeInTheDocument();
  });

  it("states the external safety boundary and identity authority", () => {
    window.history.replaceState(null, "", "/?page=world");
    render(<App />);
    expect(screen.getAllByText("Single-player only")).toHaveLength(2);
    expect(
      screen.getByText(/no face or demographic inference/i),
    ).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Native game rig");
  });
});
