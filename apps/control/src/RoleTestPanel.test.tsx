import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NativeLoadoutRoleTestResult } from "./providerLoadoutBridge";

const bridge = vi.hoisted(() => ({ testRole: vi.fn() }));

vi.mock("./providerLoadoutBridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./providerLoadoutBridge")>()),
  nativeTestRole: bridge.testRole,
}));

import { playTestToneAudition, RoleTestPanel } from "./RoleTestPanel";
import { ProviderLoadoutEditor } from "./ProviderLoadoutEditor";
import { resetBrowserLoadoutsForTests } from "./providerLoadouts";

const result = (
  overrides: Partial<NativeLoadoutRoleTestResult> = {},
): NativeLoadoutRoleTestResult => ({
  role: "llm",
  providerId: "groq",
  modelId: "qwen/qwen3.8-27b",
  outcome: "ok",
  latencyMs: 42.4,
  credentialStatus: "present",
  runtimeChecked: true,
  networkRequestPerformed: false,
  responseBodyReturned: false,
  detail: "Route is ready.",
  ...overrides,
});

describe("per-role route test", () => {
  beforeEach(() => {
    bridge.testRole.mockReset();
    window.localStorage.clear();
    resetBrowserLoadoutsForTests();
  });

  it("runs the native check for the selected route and shows ok with latency", async () => {
    const user = userEvent.setup();
    bridge.testRole.mockResolvedValue(result());
    render(
      <RoleTestPanel
        role="llm"
        roleLabel="Reply model"
        route={{ providerId: "groq", modelId: "qwen/qwen3.8-27b" }}
        providerName="Groq"
        testable
      />,
    );
    await user.click(screen.getByRole("button", { name: "Test Reply model" }));
    expect(bridge.testRole).toHaveBeenCalledWith("llm", {
      providerId: "groq",
      modelId: "qwen/qwen3.8-27b",
    });
    expect(await screen.findByText("Ready · 42 ms")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Route is ready. Check took 42 ms.",
    );
  });

  it("reports a failure reason instead of a latency-free pass", async () => {
    const user = userEvent.setup();
    bridge.testRole.mockResolvedValue(
      result({
        outcome: "needsCredential",
        credentialStatus: "missing",
        runtimeChecked: false,
        latencyMs: 0.3,
        detail: "No saved key for this provider. Connect the account first.",
      }),
    );
    render(
      <RoleTestPanel
        role="llm"
        roleLabel="Reply model"
        route={{ providerId: "groq", modelId: "qwen/qwen3.8-27b" }}
        providerName="Groq"
        testable
      />,
    );
    await user.click(screen.getByRole("button", { name: "Test Reply model" }));
    expect(await screen.findByText("Needs a key · 0.3 ms")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("No saved key");
  });

  it("shows a rejected command as an error and clears a result when the route changes", async () => {
    const user = userEvent.setup();
    bridge.testRole.mockRejectedValueOnce("runtime operation failed: boom");
    const { rerender } = render(
      <RoleTestPanel
        role="llm"
        roleLabel="Reply model"
        route={{ providerId: "groq", modelId: "qwen/qwen3.8-27b" }}
        providerName="Groq"
        testable
      />,
    );
    await user.click(screen.getByRole("button", { name: "Test Reply model" }));
    expect(
      await screen.findByText("Test failed: runtime operation failed: boom"),
    ).toBeVisible();

    bridge.testRole.mockResolvedValueOnce(result());
    await user.click(screen.getByRole("button", { name: "Test Reply model" }));
    await screen.findByText("Ready · 42 ms");
    rerender(
      <RoleTestPanel
        role="llm"
        roleLabel="Reply model"
        route={{ providerId: "groq", modelId: "openai/gpt-oss-20b" }}
        providerName="Groq"
        testable
      />,
    );
    await waitFor(() =>
      expect(screen.queryByText("Ready · 42 ms")).not.toBeInTheDocument(),
    );
  });

  it("says plainly when the check is unavailable outside the installed app", async () => {
    const user = userEvent.setup();
    bridge.testRole.mockResolvedValue(null);
    render(
      <RoleTestPanel
        role="stt"
        roleLabel="Speech recognition"
        route={{ providerId: "assemblyai", modelId: "universal-3-6-pro" }}
        providerName="AssemblyAI"
        testable
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "Test Speech recognition" }),
    );
    expect(
      await screen.findByText(/Route tests run in the installed Windows app/),
    ).toBeVisible();
  });

  it("disables the test for a route that cannot run", () => {
    render(
      <RoleTestPanel
        role="stt"
        roleLabel="Speech recognition"
        route={{ providerId: "openai", modelId: "gpt-4o-mini-transcribe" }}
        providerName="OpenAI"
        testable={false}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Test Speech recognition" }),
    ).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("nothing to test");
  });

  it("offers a voice audition only for the local test voice", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <RoleTestPanel
        role="tts"
        roleLabel="Character voice"
        route={{ providerId: "elevenlabs", modelId: "eleven_flash_v2_5" }}
        providerName="ElevenLabs"
        testable
      />,
    );
    expect(
      screen.getByRole("button", { name: "Audition voice" }),
    ).toBeDisabled();
    expect(document.body).toHaveTextContent(
      /Hosted voices are heard on a real conversation turn/,
    );

    rerender(
      <RoleTestPanel
        role="tts"
        roleLabel="Character voice"
        route={{ providerId: "testing", modelId: "testing-synth-v1" }}
        providerName="Testing"
        testable
      />,
    );
    const audition = screen.getByRole("button", { name: "Audition voice" });
    expect(audition).toBeEnabled();
    // jsdom has no Web Audio, so the panel reports that honestly.
    await user.click(audition);
    expect(
      await screen.findByText("This window cannot play audio."),
    ).toBeVisible();
  });
});

describe("test-tone audition", () => {
  it("schedules one tone per word on a fresh audio context and releases it", async () => {
    const oscillators: Array<{
      start: ReturnType<typeof vi.fn>;
      stop: ReturnType<typeof vi.fn>;
      frequency: { value: number };
      connect: ReturnType<typeof vi.fn>;
      type: string;
    }> = [];
    const close = vi.fn().mockResolvedValue(undefined);
    const context = {
      state: "running",
      currentTime: 1,
      destination: {},
      resume: vi.fn(),
      close,
      createGain: () => ({ gain: { value: 0 }, connect: vi.fn() }),
      createOscillator: () => {
        const oscillator = {
          start: vi.fn(),
          stop: vi.fn(),
          connect: vi.fn(),
          frequency: { value: 0 },
          type: "sine",
        };
        oscillators.push(oscillator);
        return oscillator;
      },
    } as unknown as AudioContext;
    vi.useFakeTimers();
    try {
      await expect(
        playTestToneAudition("one two three", () => context),
      ).resolves.toBe(true);
      expect(oscillators).toHaveLength(3);
      expect(oscillators.every((tone) => tone.type === "square")).toBe(true);
      expect(oscillators[0].start).toHaveBeenCalledWith(expect.any(Number));
      vi.advanceTimersByTime(2_000);
      expect(close).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
    await expect(playTestToneAudition("hi", () => null)).resolves.toBe(false);
  });
});

describe("loadout editor integration", () => {
  it("puts a Test button on the active role and explains the browser preview", async () => {
    const user = userEvent.setup();
    bridge.testRole.mockResolvedValue(null);
    render(<ProviderLoadoutEditor />);
    await user.click(screen.getByRole("button", { name: "Test Reply model" }));
    expect(bridge.testRole).toHaveBeenCalledWith(
      "llm",
      expect.objectContaining({ providerId: "groq" }),
    );
    expect(
      await screen.findByText(/Route tests run in the installed Windows app/),
    ).toBeVisible();
  });
});
