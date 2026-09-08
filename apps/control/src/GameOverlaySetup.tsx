import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  readProductPreferences,
  saveProductPreferences,
  type NativeProductPreferenceScope,
  type NativeProductPreferenceSnapshot,
  type NativeScopedProductPreferences,
} from "./tauriBridge";

function sameGameScope(
  left: NativeProductPreferenceScope,
  right: NativeProductPreferenceScope,
) {
  return (
    left.kind === "game" &&
    right.kind === "game" &&
    left.gameProfileId === right.gameProfileId
  );
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export function GameOverlaySetup({
  nativeAvailable,
  gameProfileId,
  onSnapshot,
}: {
  nativeAvailable: boolean;
  gameProfileId: string;
  onSnapshot?: (snapshot: NativeProductPreferenceSnapshot) => void;
}) {
  const scope = useMemo<NativeProductPreferenceScope>(
    () => ({ kind: "game", gameProfileId }),
    [gameProfileId],
  );
  const currentScopeRef = useRef(scope);
  currentScopeRef.current = scope;
  const onSnapshotRef = useRef(onSnapshot);
  onSnapshotRef.current = onSnapshot;
  const operationSequence = useRef(0);
  const [snapshot, setSnapshot] =
    useState<NativeProductPreferenceSnapshot | null>(null);
  const [busy, setBusy] = useState<"load" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const acceptSnapshot = useCallback(
    (
      next: NativeProductPreferenceSnapshot,
      expectedScope: NativeProductPreferenceScope,
    ) => {
      if (!sameGameScope(next.effective.scope, expectedScope)) {
        throw new Error(
          "Native product preferences returned a different game scope.",
        );
      }
      setSnapshot(next);
      onSnapshotRef.current?.(next);
    },
    [],
  );

  const load = useCallback(async () => {
    const operation = ++operationSequence.current;
    const expectedScope = scope;
    setSnapshot(null);
    setError(null);
    if (!nativeAvailable) {
      setBusy(null);
      return;
    }
    setBusy("load");
    try {
      const next = await readProductPreferences(expectedScope);
      if (
        operation !== operationSequence.current ||
        !sameGameScope(currentScopeRef.current, expectedScope)
      )
        return;
      if (!next)
        throw new Error("Native product preferences returned no snapshot.");
      acceptSnapshot(next, expectedScope);
    } catch (cause) {
      if (
        operation !== operationSequence.current ||
        !sameGameScope(currentScopeRef.current, expectedScope)
      )
        return;
      setSnapshot(null);
      setError(errorText(cause, "Game overlay preference could not load."));
    } finally {
      if (
        operation === operationSequence.current &&
        sameGameScope(currentScopeRef.current, expectedScope)
      )
        setBusy(null);
    }
  }, [acceptSnapshot, nativeAvailable, scope]);

  useEffect(() => {
    void load();
    return () => {
      operationSequence.current += 1;
    };
  }, [load]);

  const setOverlay = async (enabled: boolean) => {
    if (!nativeAvailable || !snapshot || busy) return;
    const operation = ++operationSequence.current;
    const expectedScope = scope;
    const existingEntry = snapshot.entries.find((entry) =>
      sameGameScope(entry.scope, expectedScope),
    );
    const entry: NativeScopedProductPreferences = existingEntry
      ? {
          ...existingEntry,
          scope: expectedScope,
          overrides: { ...existingEntry.overrides, overlay: enabled },
        }
      : { scope: expectedScope, overrides: { overlay: enabled } };
    setBusy("save");
    setError(null);
    try {
      const next = await saveProductPreferences(snapshot.revision, entry);
      if (
        operation !== operationSequence.current ||
        !sameGameScope(currentScopeRef.current, expectedScope)
      )
        return;
      if (!next)
        throw new Error(
          "Native product preferences returned no saved snapshot.",
        );
      acceptSnapshot(next, expectedScope);
    } catch (cause) {
      if (
        operation !== operationSequence.current ||
        !sameGameScope(currentScopeRef.current, expectedScope)
      )
        return;
      setError(errorText(cause, "Game overlay preference could not be saved."));
    } finally {
      if (
        operation === operationSequence.current &&
        sameGameScope(currentScopeRef.current, expectedScope)
      )
        setBusy(null);
    }
  };

  const enabled = snapshot?.effective.overlay.value ?? false;
  const stateLabel = !nativeAvailable
    ? "Windows app required"
    : busy
      ? "Pending"
      : error
        ? "Unavailable"
        : snapshot
          ? enabled
            ? "On"
            : "Off"
          : "Pending";

  return (
    <section
      className="game-overlay-setup"
      aria-labelledby="game-overlay-setup-title"
    >
      <div className="game-overlay-setup__heading">
        <div>
          <span className="eyebrow">In-game display</span>
          <h3 id="game-overlay-setup-title">Game overlay</h3>
        </div>
        <span
          className={enabled && !busy && !error ? "badge good" : "badge wait"}
          aria-live="polite"
        >
          {stateLabel}
        </span>
      </div>
      <p>
        Show subtitles and mouth motion over the game. Select an NPC to track
        its mouth.
      </p>
      {!nativeAvailable ? (
        <button className="secondary-action" disabled>
          Open Windows app to change
        </button>
      ) : snapshot ? (
        <button
          className="secondary-action"
          disabled={busy !== null}
          onClick={() => void setOverlay(!enabled)}
        >
          {busy === "save"
            ? "Saving…"
            : enabled
              ? "Turn off game overlay"
              : "Enable game overlay"}
        </button>
      ) : (
        <button
          className="secondary-action"
          disabled={busy !== null}
          onClick={() => void load()}
        >
          {busy === "load" ? "Checking…" : "Try again"}
        </button>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
