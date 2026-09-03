import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  cancelSelectedSttPushToTalk,
  normalizeSelectedSttCapturing,
  normalizeSelectedSttStatus,
  normalizeSelectedSttTerminal,
  readSelectedSttPushToTalkStatus,
  startSelectedSttPushToTalk,
  type SelectedSttCapturing,
} from "./selectedSttBridge";

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  channel: null as { onmessage?: (message: unknown) => void } | null,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: native.invoke,
  Channel: class {
    onmessage?: (message: unknown) => void;
    constructor() {
      native.channel = this;
    }
  },
}));

const capturing: SelectedSttCapturing = {
  schemaVersion: 1,
  status: "capturing",
  sessionId: "stt-session-1",
  turnId: "stt-turn-1",
  generation: 14,
  inputEndpointId: "windows-microphone-4",
  inputEndpointGeneration: 27,
  route: {
    providerId: "assemblyai",
    modelId: "u3-rt-pro",
    egress: "microphone_audio_and_optional_non_secret_context",
    automaticFallback: false,
  },
};

const transcriptReady = {
  schemaVersion: 1,
  status: "transcriptReady",
  sessionId: "stt-session-1",
  turnId: "stt-turn-1",
  generation: 14,
  receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d6953f",
  receiptSha256: "a".repeat(64),
  route: {
    providerId: "assemblyai",
    modelId: "u3-rt-pro",
    credentialReference: "providers/assemblyai",
    egress: "microphone_audio_and_optional_non_secret_context",
    generation: 14,
    inputEndpointId: "windows-microphone-4",
    inputEndpointGeneration: 27,
    manualRetry: false,
    automaticFallback: false,
    capturedFrames: 24_000,
    pttVirtualKey: 119,
    pttPressTransitionSequence: 41,
    pttPressedQpc: 1_000,
    pttReleaseTransitionSequence: 42,
    pttReleasedQpc: 2_000,
  },
  chunksSent: 8,
  pcmBytesSent: 2560,
  partialEvents: 2,
  errorCode: null,
  retryable: false,
};

describe("selected hosted STT bridge", () => {
  beforeEach(() => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    native.channel = null;
    native.invoke.mockReset();
  });

  afterEach(() => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__;
  });

  it("sends only the bounded public request and accepts an exact receipt", async () => {
    native.invoke.mockResolvedValueOnce(capturing);
    const terminal = vi.fn();
    await expect(
      startSelectedSttPushToTalk(
        {
          schemaVersion: 1,
          gameProfileId: "skyrim-special-edition",
          characterId: "lydia",
          contextHint: null,
          attempt: { kind: "initial" },
          oneTimeToken: "forged",
          pcm: [1, 2, 3],
          sessionId: "forged-session",
        } as never,
        terminal,
      ),
    ).resolves.toEqual(capturing);
    expect(native.invoke).toHaveBeenCalledWith(
      "start_selected_stt_push_to_talk",
      {
        request: {
          schemaVersion: 1,
          gameProfileId: "skyrim-special-edition",
          characterId: "lydia",
          contextHint: null,
          attempt: { kind: "initial" },
        },
        events: expect.anything(),
      },
    );

    native.channel?.onmessage?.(transcriptReady);
    expect(terminal).toHaveBeenCalledWith(transcriptReady);
    native.channel?.onmessage?.(transcriptReady);
    expect(terminal).toHaveBeenCalledTimes(1);
  });

  it("ignores stale, mismatched, and secret-bearing terminal events", async () => {
    native.invoke.mockResolvedValueOnce(capturing);
    const terminal = vi.fn();
    await startSelectedSttPushToTalk(
      { schemaVersion: 1, attempt: { kind: "initial" } },
      terminal,
    );

    native.channel?.onmessage?.({ ...transcriptReady, generation: 13 });
    native.channel?.onmessage?.({
      ...transcriptReady,
      route: { ...transcriptReady.route, inputEndpointId: "forged-endpoint" },
    });
    native.channel?.onmessage?.({ ...transcriptReady, oneTimeToken: "secret" });
    native.channel?.onmessage?.({ ...transcriptReady, pcm: [1, 2, 3] });
    native.channel?.onmessage?.({
      ...transcriptReady,
      transcript: "The WebView must reject transcript text.",
    });
    native.channel?.onmessage?.({
      ...transcriptReady,
      receiptId: "not-a-uuid",
    });
    native.channel?.onmessage?.({
      ...transcriptReady,
      receiptSha256: "A".repeat(64),
    });
    native.channel?.onmessage?.({
      ...transcriptReady,
      route: { ...transcriptReady.route, automaticFallback: true },
    });
    native.channel?.onmessage?.({
      ...transcriptReady,
      route: { ...transcriptReady.route, pttVirtualKey: 118 },
    });
    native.channel?.onmessage?.({
      ...transcriptReady,
      route: {
        ...transcriptReady.route,
        pttReleaseTransitionSequence:
          transcriptReady.route.pttPressTransitionSequence,
      },
    });
    native.channel?.onmessage?.({
      ...transcriptReady,
      route: { ...transcriptReady.route, capturedFrames: 0 },
    });
    expect(terminal).not.toHaveBeenCalled();
  });

  it("binds manual retry to the prior terminal generation", async () => {
    native.invoke.mockResolvedValueOnce({ ...capturing, generation: 15 });
    await startSelectedSttPushToTalk(
      {
        schemaVersion: 1,
        attempt: {
          kind: "manualRetry",
          priorGeneration: 14,
          userAuthorized: true,
        },
      },
      vi.fn(),
    );
    expect(native.invoke).toHaveBeenCalledWith(
      "start_selected_stt_push_to_talk",
      expect.objectContaining({
        request: expect.objectContaining({
          attempt: {
            kind: "manualRetry",
            priorGeneration: 14,
            userAuthorized: true,
          },
        }),
      }),
    );
  });

  it("retains only the first terminal event received before the start receipt", async () => {
    let resolveStart!: (value: SelectedSttCapturing) => void;
    native.invoke.mockImplementationOnce(
      async () =>
        await new Promise<SelectedSttCapturing>((resolve) => {
          resolveStart = resolve;
        }),
    );
    const terminal = vi.fn();
    const start = startSelectedSttPushToTalk(
      { schemaVersion: 1, attempt: { kind: "initial" } },
      terminal,
    );
    await vi.waitFor(() => expect(native.channel).not.toBeNull());
    native.channel?.onmessage?.(transcriptReady);
    native.channel?.onmessage?.({
      ...transcriptReady,
      receiptId: "018f47c2-7202-7c8d-8e2a-9e2791d69540",
      receiptSha256: "b".repeat(64),
    });
    resolveStart(capturing);
    await start;
    expect(terminal).toHaveBeenCalledTimes(1);
    expect(terminal).toHaveBeenCalledWith(transcriptReady);
  });

  it("strictly normalizes start, status, and cancellation receipts", async () => {
    expect(normalizeSelectedSttCapturing({ ...capturing, pcm: [] })).toBeNull();
    expect(
      normalizeSelectedSttTerminal(transcriptReady, {
        ...capturing,
        manualRetry: false,
      }),
    ).toEqual(transcriptReady);
    expect(
      normalizeSelectedSttStatus({
        schemaVersion: 1,
        status: "arming",
        generation: 14,
      }),
    ).toBeNull();
    expect(
      normalizeSelectedSttStatus({
        schemaVersion: 1,
        status: "arming",
        generation: 0,
        sessionId: null,
        turnId: null,
        inputEndpointId: null,
        inputEndpointGeneration: null,
      }),
    ).toBeNull();

    native.invoke
      .mockResolvedValueOnce({
        schemaVersion: 1,
        status: "arming",
        generation: 14,
        sessionId: null,
        turnId: null,
        inputEndpointId: null,
        inputEndpointGeneration: null,
      })
      .mockResolvedValueOnce({
        schemaVersion: 1,
        status: "capturing",
        generation: 14,
        sessionId: "stt-session-1",
        turnId: "stt-turn-1",
        inputEndpointId: "windows-microphone-4",
        inputEndpointGeneration: 27,
      })
      .mockResolvedValueOnce({
        schemaVersion: 1,
        outcome: "cancellationRequested",
        generation: 14,
      });
    await expect(readSelectedSttPushToTalkStatus()).resolves.toEqual({
      schemaVersion: 1,
      status: "arming",
      generation: 14,
      sessionId: null,
      turnId: null,
      inputEndpointId: null,
      inputEndpointGeneration: null,
    });
    await expect(readSelectedSttPushToTalkStatus()).resolves.toMatchObject({
      status: "capturing",
      generation: 14,
    });
    await expect(cancelSelectedSttPushToTalk()).resolves.toEqual({
      schemaVersion: 1,
      outcome: "cancellationRequested",
      generation: 14,
    });
    expect(native.invoke.mock.calls).toEqual([
      ["selected_stt_push_to_talk_status", {}],
      ["selected_stt_push_to_talk_status", {}],
      ["cancel_selected_stt_push_to_talk", {}],
    ]);
  });
});
