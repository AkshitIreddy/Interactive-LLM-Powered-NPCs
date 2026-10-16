import { useEffect, useRef, useState } from "react";
import { ActionButton, StatusPill } from "./components";
import {
  nativeTestRole,
  type NativeLoadoutRoleTestOutcome,
  type NativeLoadoutRoleTestResult,
} from "./providerLoadoutBridge";
import type { ProviderRole, RouteChoice } from "./providerLoadouts";

export const AUDITION_LINE = "This is how the selected voice sounds.";

const OUTCOME_LABEL: Record<NativeLoadoutRoleTestOutcome, string> = {
  ok: "Ready",
  unknownRoute: "Unknown route",
  notSelectable: "Not selectable",
  needsCredential: "Needs a key",
  credentialStoreUnavailable: "Key store unavailable",
  runtimeUnavailable: "Runtime unavailable",
  providerContractUnavailable: "Provider not advertised",
};

type TestState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "done"; result: NativeLoadoutRoleTestResult }
  | { kind: "unavailable"; detail: string }
  | { kind: "error"; detail: string };

const formatLatency = (milliseconds: number) =>
  milliseconds < 10
    ? `${milliseconds.toFixed(1)} ms`
    : `${Math.round(milliseconds)} ms`;

/**
 * Clearly synthetic test tones whose length follows the line, matching what
 * the zero-spend `testing` voice does. Returns false when this window cannot
 * play audio (for example a headless test environment).
 */
export async function playTestToneAudition(
  text: string,
  audioContextFactory: () => AudioContext | null = defaultAudioContext,
): Promise<boolean> {
  const context = audioContextFactory();
  if (!context) return false;
  if (context.state === "suspended") await context.resume();
  const words = text.split(/\s+/).filter(Boolean).slice(0, 12);
  const start = context.currentTime + 0.05;
  const gain = context.createGain();
  gain.gain.value = 0.08;
  gain.connect(context.destination);
  words.forEach((word, index) => {
    const oscillator = context.createOscillator();
    oscillator.type = "square";
    oscillator.frequency.value = 220 + (word.length % 6) * 40;
    oscillator.connect(gain);
    const at = start + index * 0.14;
    oscillator.start(at);
    oscillator.stop(at + 0.1);
  });
  const total = (words.length * 0.14 + 0.2) * 1000;
  window.setTimeout(() => void context.close().catch(() => undefined), total);
  return true;
}

function defaultAudioContext(): AudioContext | null {
  const Constructor =
    window.AudioContext ??
    (window as Window & { webkitAudioContext?: typeof AudioContext })
      .webkitAudioContext;
  return Constructor ? new Constructor() : null;
}

/**
 * Per-role Test button, plus a voice audition for the TTS role. The check is
 * a native readiness check (catalog, saved key presence, runtime contract); it
 * never sends content to a provider and says so.
 */
export function RoleTestPanel({
  role,
  roleLabel,
  route,
  providerName,
  testable,
}: {
  role: Exclude<ProviderRole, "lipSync">;
  roleLabel: string;
  route: RouteChoice;
  providerName: string;
  /** False when the current route cannot be executed (nothing to test). */
  testable: boolean;
}) {
  const routeKey = `${role}|${route.providerId}|${route.modelId}`;
  const [state, setState] = useState<TestState>({ kind: "idle" });
  const [audition, setAudition] = useState<string | null>(null);
  const activeRouteKey = useRef(routeKey);

  // A result describes one route; changing provider or model clears it.
  useEffect(() => {
    activeRouteKey.current = routeKey;
    setState({ kind: "idle" });
    setAudition(null);
  }, [routeKey]);

  const run = async () => {
    const startedFor = routeKey;
    setState({ kind: "running" });
    try {
      const result = await nativeTestRole(role, route);
      if (activeRouteKey.current !== startedFor) return;
      setState(
        result
          ? { kind: "done", result }
          : {
              kind: "unavailable",
              detail:
                "Route tests run in the installed Windows app. Browser preview makes no checks.",
            },
      );
    } catch (error) {
      if (activeRouteKey.current !== startedFor) return;
      setState({
        kind: "error",
        detail:
          error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : "The route test failed.",
      });
    }
  };

  const canAudition = role === "tts" && route.providerId === "testing";
  const runAudition = async () => {
    try {
      const played = await playTestToneAudition(AUDITION_LINE);
      setAudition(
        played
          ? "Played synthetic test tones in this window. This is not a voice and does not use the native speaker selection."
          : "This window cannot play audio.",
      );
    } catch {
      setAudition("Audio playback was blocked by this window.");
    }
  };

  const result = state.kind === "done" ? state.result : null;
  return (
    <section
      className="loadout-role-test"
      aria-label={`${roleLabel} route test`}
    >
      <div className="loadout-role-test__actions">
        <ActionButton
          variant="outline"
          icon="check"
          onPress={() => void run()}
          isDisabled={!testable || state.kind === "running"}
        >
          {state.kind === "running" ? "Testing…" : `Test ${roleLabel}`}
        </ActionButton>
        {role === "tts" && (
          <ActionButton
            variant="outline"
            icon="headphones"
            onPress={() => void runAudition()}
            isDisabled={!canAudition}
          >
            Audition voice
          </ActionButton>
        )}
        {result && (
          <StatusPill tone={result.outcome === "ok" ? "ok" : "warn"}>
            {OUTCOME_LABEL[result.outcome]} · {formatLatency(result.latencyMs)}
          </StatusPill>
        )}
      </div>
      <p
        className={`loadout-role__note${
          state.kind === "error" || (result && result.outcome !== "ok")
            ? " is-error"
            : ""
        }`}
        role="status"
      >
        {state.kind === "idle" &&
          (testable
            ? `Checks ${providerName} for this role: catalog entry, saved key and runtime support. Nothing is sent to the provider.`
            : "This route is not executable, so there is nothing to test.")}
        {state.kind === "running" && "Checking this route…"}
        {state.kind === "unavailable" && state.detail}
        {state.kind === "error" && `Test failed: ${state.detail}`}
        {result &&
          `${result.detail} Check took ${formatLatency(result.latencyMs)}.`}
      </p>
      {role === "tts" && (
        <p className="loadout-role__note" role="status">
          {audition ??
            (canAudition
              ? `Plays a short synthetic tone sequence for "${AUDITION_LINE}".`
              : "Voice audition is available for the local test voice. Hosted voices are heard on a real conversation turn; no standalone voice request is made from this page.")}
        </p>
      )}
    </section>
  );
}
