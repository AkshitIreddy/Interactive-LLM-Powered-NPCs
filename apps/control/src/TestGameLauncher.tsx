import { useEffect, useRef, useState } from "react";
import {
  readSyntheticReviewTargetStatus,
  type SyntheticReplayCaptureAvailability,
  type SyntheticReviewTargetStatus,
} from "./tauriBridge";
import "./TestGameLauncher.css";

export interface TestGameLauncherProps {
  availability: SyntheticReplayCaptureAvailability;
  connected: boolean;
  busy: boolean;
  onLaunchAndConnect: () => void | Promise<void>;
  onConnectionLost?: () => void;
}

type StatusState =
  | { kind: "loading" }
  | { kind: "loaded"; target: SyntheticReviewTargetStatus }
  | { kind: "failed"; detail: string }
  | { kind: "nativeUnavailable"; detail: string };

function unavailableDetail(availability: SyntheticReplayCaptureAvailability) {
  if (availability.available) return "";
  return availability.reason === "browserPreview"
    ? "Open the native review app to launch the included practice game."
    : "The included practice game is available in review builds.";
}

export function TestGameLauncher({
  availability,
  connected,
  busy,
  onLaunchAndConnect,
  onConnectionLost,
}: TestGameLauncherProps) {
  const [revision, setRevision] = useState(0);
  const [status, setStatus] = useState<StatusState>(() =>
    availability.available
      ? { kind: "loading" }
      : {
          kind: "nativeUnavailable",
          detail: unavailableDetail(availability),
        },
  );
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const nativeWasConnected = useRef(false);
  const connectionLostHandler = useRef(onConnectionLost);

  useEffect(() => {
    connectionLostHandler.current = onConnectionLost;
  }, [onConnectionLost]);

  useEffect(() => {
    if (!availability.available) {
      setStatus({
        kind: "nativeUnavailable",
        detail: unavailableDetail(availability),
      });
      return;
    }
    let current = true;
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    setStatus({ kind: "loading" });
    const readStatus = () => {
      void readSyntheticReviewTargetStatus(availability)
        .then((target) => {
          if (!current) return;
          if (target) {
            const nowConnected = target.state === "connected";
            if (nativeWasConnected.current && !nowConnected)
              connectionLostHandler.current?.();
            nativeWasConnected.current = nowConnected;
            setStatus({ kind: "loaded", target });
            if (nowConnected) refreshTimer = setTimeout(readStatus, 2_000);
          } else {
            setStatus({
              kind: "failed",
              detail: "The native app did not return a practice-game status.",
            });
          }
        })
        .catch((error: unknown) => {
          if (!current) return;
          setStatus({
            kind: "failed",
            detail:
              error instanceof Error
                ? error.message
                : "The practice-game status could not be read.",
          });
        });
    };
    readStatus();
    return () => {
      current = false;
      if (refreshTimer) clearTimeout(refreshTimer);
    };
  }, [
    availability.available,
    availability.available ? availability.commandName : availability.reason,
    revision,
  ]);

  const target = status.kind === "loaded" ? status.target : null;
  const ready = target?.state === "readyToLaunch";
  const nativeConnected = target?.state === "connected";
  const disabled = busy || actionBusy || !ready || nativeConnected;
  const stateLabel = nativeConnected
    ? "Connected"
    : status.kind === "loading"
      ? "Checking included game"
      : ready
        ? "Ready to launch"
        : target?.state === "missing"
          ? "Game files missing"
          : target?.state === "invalid"
            ? "Files need repair"
            : "Native app required";

  const launch = async () => {
    setActionBusy(true);
    setActionError(null);
    try {
      await onLaunchAndConnect();
      setRevision((value) => value + 1);
    } catch (error) {
      setActionError(
        error instanceof Error
          ? error.message
          : "The practice game could not be launched.",
      );
    } finally {
      setActionBusy(false);
    }
  };

  return (
    <section className="test-game-launcher" aria-labelledby="test-game-title">
      <div className="test-game-beacon" aria-hidden="true">
        <span>EH</span>
        <i />
      </div>
      <div className="test-game-copy">
        <span className="test-game-eyebrow">Included practice game</span>
        <div className="test-game-heading-row">
          <h2 id="test-game-title">Eclipse Harbor</h2>
          <span
            className={`test-game-state ${nativeConnected || ready ? "is-ready" : ""}`}
            role="status"
          >
            {stateLabel}
          </span>
        </div>
        <p>
          Test game capture and lip-sync without finding or opening another
          file. NPC / LINK launches the included game for you.
        </p>
        {target && (
          <div className="test-game-location">
            <span>{target.executableName}</span>
            <code title={target.executablePath ?? target.expectedRelativePath}>
              {target.executablePath ?? target.expectedRelativePath}
            </code>
          </div>
        )}
        {status.kind === "failed" && (
          <p className="test-game-message is-error">{status.detail}</p>
        )}
        {status.kind === "nativeUnavailable" && (
          <p className="test-game-message">{status.detail}</p>
        )}
        {target &&
          target.state !== "readyToLaunch" &&
          target.state !== "connected" && (
            <p className="test-game-message is-error">{target.detail}</p>
          )}
        {actionError && (
          <p className="test-game-message is-error">{actionError}</p>
        )}
      </div>
      <div className="test-game-actions">
        <button
          className="primary-action test-game-launch-action"
          type="button"
          disabled={disabled}
          onClick={() => void launch()}
        >
          {nativeConnected
            ? "Game connected"
            : busy || actionBusy
              ? "Launching…"
              : "Launch & connect"}
        </button>
        {status.kind === "failed" && (
          <button
            className="secondary-action"
            type="button"
            onClick={() => setRevision((value) => value + 1)}
          >
            Check again
          </button>
        )}
        <small>
          {nativeConnected
            ? `The exact window is connected${target?.targetProcessId ? ` · PID ${target.targetProcessId}` : ""}.`
            : connected && ready
              ? "Checking the native connection after launch."
              : ready
                ? "One click opens the game and connects its window."
                : "Launch becomes available after the included files pass their integrity check."}
        </small>
      </div>
    </section>
  );
}
