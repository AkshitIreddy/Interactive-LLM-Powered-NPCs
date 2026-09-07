import { useEffect, useState } from "react";
import {
  readLocalResourceTelemetry,
  type NativeResourceTelemetryResult,
  type NativeTelemetryObservation,
} from "./tauriBridge";

function memoryValue(value: NativeTelemetryObservation<number> | undefined) {
  return value?.availability === "available"
    ? `${(value.value / 1024 ** 3).toFixed(1)} GB`
    : "Not measured";
}

export function SetupSystemCheck({
  nativeAvailable,
}: {
  nativeAvailable: boolean;
}) {
  const [result, setResult] = useState<NativeResourceTelemetryResult | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!nativeAvailable) return;
    let current = true;
    setBusy(true);
    void readLocalResourceTelemetry()
      .then((value) => {
        if (current) setResult(value);
      })
      .catch(() => {
        if (current)
          setError(
            "Hardware details could not be read. You can still configure hosted models.",
          );
      })
      .finally(() => {
        if (current) setBusy(false);
      });
    return () => {
      current = false;
    };
  }, [nativeAvailable]);
  const refresh = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await readLocalResourceTelemetry());
    } catch {
      setError(
        "Hardware details could not be read. Try again or continue with hosted models.",
      );
    } finally {
      setBusy(false);
    }
  };
  const hardware = result?.snapshot;
  return (
    <section
      className="setup-hardware"
      aria-label="This PC hardware"
      aria-busy={busy}
    >
      <div className="panel-title">
        <h2>This PC</h2>
        <button
          className="quiet-button"
          disabled={!nativeAvailable || busy}
          onClick={() => void refresh()}
        >
          {busy ? "Checking…" : "Refresh hardware"}
        </button>
      </div>
      <dl>
        <div>
          <dt>Graphics</dt>
          <dd>
            {hardware?.adapter.availability === "available"
              ? hardware.adapter.value.description
              : "Not measured"}
          </dd>
        </div>
        <div>
          <dt>Graphics memory</dt>
          <dd>{memoryValue(hardware?.dedicated_vram_bytes)}</dd>
        </div>
        <div>
          <dt>Available system memory</dt>
          <dd>{memoryValue(hardware?.available_ram_bytes)}</dd>
        </div>
      </dl>
      {error && <p role="status">{error}</p>}
      <p>
        Hosted conversation models need no local model download. Local models
        are checked against your game’s resource budget before use.
      </p>
      <details className="evidence-disclosure">
        <summary>Privacy & measurement details</summary>
        <p>
          This check reads local hardware only. The app does not capture your
          webcam or send a game frame during setup. Your selected provider
          receives only the inputs needed by its configured route.
        </p>
        <p>
          {hardware
            ? `Hardware observed ${new Date(hardware.captured_unix_millis).toLocaleString()}. This is a hardware snapshot, not a game-performance benchmark.`
            : "Hardware measurements require the native Windows application."}
        </p>
      </details>
    </section>
  );
}
