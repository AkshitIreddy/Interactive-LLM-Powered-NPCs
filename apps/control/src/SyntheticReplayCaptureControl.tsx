import { useState } from "react";
import { ActionButton, StatusPill } from "./components";
import { Icon } from "./icons";
import {
  clearSyntheticReplayCaptureTarget,
  readSyntheticReplayCaptureDiagnostics,
  runSyntheticReplayCapture,
  syntheticReplayCaptureAvailability,
  type SyntheticReplayCaptureAvailability,
  type SyntheticReplayCaptureResult,
} from "./tauriBridge";

type ProbeState = "idle" | "running" | "invoked" | "failed";

export function SyntheticReplayCaptureControl({
  availability = syntheticReplayCaptureAvailability(),
  run = runSyntheticReplayCapture,
  readDiagnostics = readSyntheticReplayCaptureDiagnostics,
  clearTarget = clearSyntheticReplayCaptureTarget,
}: {
  availability?: SyntheticReplayCaptureAvailability;
  run?: (
    availability: SyntheticReplayCaptureAvailability,
  ) => Promise<SyntheticReplayCaptureResult>;
  readDiagnostics?: () => Promise<SyntheticReplayCaptureResult>;
  clearTarget?: () => Promise<SyntheticReplayCaptureResult>;
}) {
  const [state, setState] = useState<ProbeState>("idle");
  const [failure, setFailure] = useState<string | null>(null);
  const [result, setResult] = useState<SyntheticReplayCaptureResult | null>(
    null,
  );
  const disabled = !availability.available || state === "running";

  const invokeProbe = async () => {
    setState("running");
    setFailure(null);
    try {
      setResult(await run(availability));
      setState("invoked");
    } catch (error) {
      setState("failed");
      setFailure(error instanceof Error ? error.message : "Unknown failure");
    }
  };

  const refreshDiagnostics = async () => {
    setState("running");
    setFailure(null);
    try {
      setResult(await readDiagnostics());
      setState("invoked");
    } catch (error) {
      setState("failed");
      setFailure(error instanceof Error ? error.message : "Unknown failure");
    }
  };

  const clearSelection = async () => {
    setState("running");
    setFailure(null);
    try {
      await clearTarget();
      setResult(null);
      setState("idle");
    } catch (error) {
      setState("failed");
      setFailure(error instanceof Error ? error.message : "Unknown failure");
    }
  };

  const availabilityText = availability.available
    ? "Native debug command is bound for this build."
    : availability.reason === "browserPreview"
      ? "Browser preview only · open the configured desktop debug build."
      : "Release build · debug capture commands are intentionally absent.";

  return (
    <section
      className="synthetic-replay-probe"
      aria-labelledby="synthetic-replay-probe-title"
      data-testid="synthetic-replay-capture-control"
    >
      <div className="synthetic-replay-probe__heading">
        <div>
          <span className="panel-label">QA TEST HARNESS · SYNTHETIC ONLY</span>
          <h2 id="synthetic-replay-probe-title">
            Replay a still as a game window
          </h2>
        </div>
        <StatusPill tone={availability.available ? "teal" : "warn"}>
          {availability.available ? "Native hook bound" : "Hook unavailable"}
        </StatusPill>
      </div>
      <p>
        Loops a synthetic still-image source so capture, target selection, and
        subtitle placement can be recorded without opening a game.
      </p>
      <div className="synthetic-replay-probe__limits" role="note">
        <Icon name="shield" />
        <span>
          <strong>Not a gameplay, audio, or lip-sync test.</strong>
          No microphone, voice model, face animation, or performance claim is
          implied by this probe.
        </span>
      </div>
      <div className="synthetic-replay-probe__actions">
        <ActionButton
          icon="play"
          variant="outline"
          isDisabled={disabled}
          onPress={invokeProbe}
        >
          {state === "running"
            ? "Invoking native capture probe…"
            : "Run synthetic capture probe"}
        </ActionButton>
        <span>{availabilityText}</span>
      </div>
      {state === "invoked" && result && (
        <div className="synthetic-replay-probe__result" role="status">
          <strong>Native synthetic target selected</strong>
          <p>
            This is target and broker diagnostics evidence. Audio and lip-sync
            remain outside this probe.
          </p>
          <dl>
            <div>
              <dt>Executable</dt>
              <dd>{result.targetExecutableBasename}</dd>
            </div>
            <div>
              <dt>Process / window</dt>
              <dd>
                PID {result.targetProcessId} · HWND {result.targetWindowHandle}
              </dd>
            </div>
            <div>
              <dt>Capture backend</dt>
              <dd>
                {result.diagnostics.captureBackend} ·{" "}
                {result.diagnostics.targetState}
              </dd>
            </div>
            <div>
              <dt>Frame evidence</dt>
              <dd>
                {result.diagnostics.framesReceived} received ·{" "}
                {result.diagnostics.framesPresented} presented ·{" "}
                {result.diagnostics.framesDropped} dropped
              </dd>
            </div>
          </dl>
          <div className="synthetic-replay-probe__result-actions">
            <ActionButton
              variant="quiet"
              icon="refresh"
              onPress={refreshDiagnostics}
            >
              Refresh native diagnostics
            </ActionButton>
            <ActionButton variant="quiet" icon="close" onPress={clearSelection}>
              Clear synthetic target
            </ActionButton>
          </div>
        </div>
      )}
      {state === "failed" && (
        <p className="synthetic-replay-probe__result is-failed" role="alert">
          Native capture probe failed: {failure}
        </p>
      )}
    </section>
  );
}
