import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  readEffectiveConfiguration,
  readProductPreferences,
  readSubtitlePreferences,
  resetProductPreferences,
  resetSubtitlePreferences,
  saveProductPreferences,
  saveSubtitlePreferences,
  type NativeEffectiveConfigurationSnapshot,
  type NativeEffectivePreference,
  type NativeExecutionPreset,
  type NativeInterruptionMode,
  type NativePerformancePreset,
  type NativePreferenceInputMode,
  type NativeProductPreferenceOverrides,
  type NativeProductPreferenceScope,
  type NativeProductPreferenceSnapshot,
  type NativeResponseLength,
  type NativeScopedProductPreferences,
  type NativeScopedSubtitlePreferences,
  type NativeSubtitleEffectiveSource,
  type NativeSubtitlePreferenceOverrides,
  type NativeSubtitlePreferenceSnapshot,
  type NativeVerbosity,
} from "./tauriBridge";

type ScopeKind = NativeProductPreferenceScope["kind"];

const EXECUTION_PRESETS: Array<[NativeExecutionPreset, string]> = [
  ["cloud", "Cloud"],
  ["hybrid", "Hybrid"],
  ["fullyLocal", "Fully local"],
];

const PERFORMANCE_PRESETS: Array<[NativePerformancePreset, string]> = [
  ["competitive", "Competitive"],
  ["fast", "Fast"],
  ["balanced", "Balanced"],
  ["immersive", "Immersive"],
  ["maximum", "Maximum"],
  ["custom", "Custom"],
];

function sameScope(
  left: NativeProductPreferenceScope,
  right: NativeProductPreferenceScope,
) {
  return (
    left.kind === right.kind &&
    (left.kind === "global" ||
      (right.kind !== "global" &&
        left.gameProfileId === right.gameProfileId &&
        (left.kind === "game" ||
          (right.kind === "character" &&
            left.characterId === right.characterId))))
  );
}

function scopeLabel(scope: NativeProductPreferenceScope) {
  if (scope.kind === "global") return "Global";
  if (scope.kind === "game") return `Game · ${scope.gameProfileId}`;
  return `Character · ${scope.characterId}`;
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

function inheritedLabel<T>(value: NativeEffectivePreference<T>) {
  return `${value.sourceKind} from ${scopeLabel(value.sourceScope)}`;
}

function readableIdentifier(value: string) {
  return value
    .replaceAll("_", " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function ProductPreferencesWorkspace({
  nativeAvailable,
  gameProfileId,
  characterId,
  onSnapshot,
}: {
  nativeAvailable: boolean;
  gameProfileId: string;
  characterId: string | null;
  onSnapshot?: (snapshot: NativeProductPreferenceSnapshot) => void;
}) {
  const [scopeKind, setScopeKind] = useState<ScopeKind>("global");
  const [preferenceSection, setPreferenceSection] = useState<
    "conversation" | "subtitles" | "routing"
  >("conversation");
  const [conversationGroup, setConversationGroup] = useState<
    "response" | "interaction" | "presence"
  >("response");
  const [snapshot, setSnapshot] =
    useState<NativeProductPreferenceSnapshot | null>(null);
  const [draft, setDraft] = useState<NativeScopedProductPreferences | null>(
    null,
  );
  const [busy, setBusy] = useState<"load" | "save" | "reset" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [resetArmed, setResetArmed] = useState(false);
  const loadSequence = useRef(0);

  const scope = useMemo<NativeProductPreferenceScope>(() => {
    if (scopeKind === "global") return { kind: "global" };
    if (scopeKind === "game" || !characterId)
      return { kind: "game", gameProfileId };
    return { kind: "character", gameProfileId, characterId };
  }, [characterId, gameProfileId, scopeKind]);

  const applySnapshot = useCallback(
    (next: NativeProductPreferenceSnapshot) => {
      setSnapshot(next);
      const entry = next.entries.find((candidate) =>
        sameScope(candidate.scope, next.effective.scope),
      );
      setDraft(
        entry
          ? structuredClone(entry)
          : {
              scope: next.effective.scope,
              overrides: {},
            },
      );
      onSnapshot?.(next);
    },
    [onSnapshot],
  );

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    if (!nativeAvailable) {
      setSnapshot(null);
      setDraft(null);
      return;
    }
    setBusy("load");
    setError(null);
    setNotice(null);
    setResetArmed(false);
    try {
      const next = await readProductPreferences(scope);
      if (!next)
        throw new Error("Native product preferences returned no snapshot.");
      if (sequence !== loadSequence.current) return;
      applySnapshot(next);
    } catch (cause) {
      if (sequence !== loadSequence.current) return;
      setSnapshot(null);
      setDraft(null);
      setError(errorText(cause, "Native product preferences could not load."));
    } finally {
      if (sequence === loadSequence.current) setBusy(null);
    }
  }, [applySnapshot, nativeAvailable, scope]);

  useEffect(() => {
    void load();
  }, [load]);

  const setOverride = <K extends keyof NativeProductPreferenceOverrides>(
    key: K,
    value: NativeProductPreferenceOverrides[K] | undefined,
  ) =>
    setDraft((current) => {
      if (!current) return current;
      const overrides = { ...current.overrides };
      if (value === undefined) delete overrides[key];
      else overrides[key] = value;
      return { ...current, overrides };
    });

  const save = async () => {
    if (!snapshot || !draft) return;
    setBusy("save");
    setError(null);
    setNotice(null);
    try {
      const next = await saveProductPreferences(snapshot.revision, draft);
      if (!next)
        throw new Error("Native product preferences returned no saved state.");
      applySnapshot(next);
      setNotice(
        "Intent saved atomically. No provider route, local pack, egress fallback, or in-flight turn was activated.",
      );
    } catch (cause) {
      setError(
        errorText(cause, "Native product preferences could not be saved."),
      );
    } finally {
      setBusy(null);
    }
  };

  const reset = async () => {
    if (!snapshot) return;
    if (!resetArmed) {
      setResetArmed(true);
      setNotice(
        `Select Confirm reset to remove ${scopeLabel(scope)} intent. Effective inherited values will remain visible.`,
      );
      return;
    }
    setBusy("reset");
    setError(null);
    try {
      const next = await resetProductPreferences(snapshot.revision, scope);
      if (!next)
        throw new Error("Native product preferences returned no reset state.");
      applySnapshot(next);
      setResetArmed(false);
      setNotice(
        "Scope reset persisted. No provider route or local pack was activated.",
      );
    } catch (cause) {
      setError(
        errorText(cause, "Native product preferences could not be reset."),
      );
    } finally {
      setBusy(null);
    }
  };

  const effective = snapshot?.effective;
  const scopeCanSave =
    draft &&
    (scope.kind !== "global" ||
      (draft.executionPreset !== undefined &&
        draft.performancePreset !== undefined));

  return (
    <div className="product-preferences-stack preference-editor-shell">
      <nav className="preference-editor-nav" aria-label="Preference category">
        <button
          aria-current={
            preferenceSection === "conversation" ? "page" : undefined
          }
          onClick={() => setPreferenceSection("conversation")}
        >
          <b>Conversation</b>
          <small>Behavior, performance and presence</small>
        </button>
        <button
          aria-current={preferenceSection === "subtitles" ? "page" : undefined}
          onClick={() => setPreferenceSection("subtitles")}
        >
          <b>Subtitles</b>
          <small>Typography, placement and backplate</small>
        </button>
        <button
          aria-current={preferenceSection === "routing" ? "page" : undefined}
          onClick={() => setPreferenceSection("routing")}
        >
          <b>Effective setup</b>
          <small>Resolved providers and local resources</small>
        </button>
      </nav>
      <div
        className="preference-editor-pane"
        hidden={preferenceSection !== "conversation"}
      >
        <section className="instrument-panel product-preferences-workspace">
          <div className="panel-title workspace-heading">
            <div>
              <span className="eyebrow">Apply defaults by scope</span>
              <h2>Conversation defaults</h2>
            </div>
            <span className={nativeAvailable ? "badge good" : "badge wait"}>
              {nativeAvailable ? "Native store" : "Preview only"}
            </span>
          </div>
          <div className="preference-scope-command">
            <div
              className="preference-scope-tabs"
              aria-label="Preference scope"
            >
              {(["global", "game", "character"] as const).map((kind) => (
                <button
                  key={kind}
                  className={scopeKind === kind ? "selected" : ""}
                  aria-pressed={scopeKind === kind}
                  disabled={
                    busy !== null || (kind === "character" && !characterId)
                  }
                  title={
                    kind === "character" && !characterId
                      ? "Select an authored character in World first."
                      : undefined
                  }
                  onClick={() => setScopeKind(kind)}
                >
                  {kind === "global"
                    ? "Global"
                    : kind === "game"
                      ? "Selected game"
                      : "Selected character"}
                </button>
              ))}
            </div>
            <p className="preference-scope-context" role="status">
              Editing <strong>{scopeLabel(scope)}</strong>
            </p>
          </div>

          {snapshot && draft && effective && (
            <div className="preference-actions preference-actions--persistent">
              <button
                className="primary-action small"
                disabled={!scopeCanSave || busy !== null}
                onClick={() => void save()}
              >
                {busy === "save"
                  ? "Saving…"
                  : `Save ${scopeLabel(scope)} intent`}
              </button>
              <button
                className={resetArmed ? "quiet-button danger" : "quiet-button"}
                disabled={busy !== null}
                onClick={() => void reset()}
              >
                {busy === "reset"
                  ? "Resetting…"
                  : resetArmed
                    ? "Confirm reset"
                    : "Reset this scope"}
              </button>
              <button
                className="quiet-button"
                disabled={busy !== null}
                onClick={() => void load()}
              >
                Reload saved values
              </button>
            </div>
          )}

          {!nativeAvailable && (
            <div className="empty-state">
              <b>Your preferences live in the desktop app</b>
              <p>
                Open it to adjust conversation style, performance and controls.
              </p>
            </div>
          )}
          {nativeAvailable && busy === "load" && !snapshot && (
            <div className="empty-state" aria-live="polite">
              <b>Loading native preferences…</b>
            </div>
          )}
          {nativeAvailable && !snapshot && !error && busy !== "load" && (
            <div className="empty-state">
              <b>No native preference snapshot</b>
              <button className="quiet-button" onClick={() => void load()}>
                Retry native load
              </button>
            </div>
          )}

          {snapshot && draft && effective && (
            <>
              <nav
                className="conversation-group-nav"
                aria-label="Conversation setting group"
              >
                {(
                  [
                    ["response", "Response", "Speed, quality and style"],
                    ["interaction", "Interaction", "Input and display"],
                    ["presence", "Presence", "Memory, vision and privacy"],
                  ] as const
                ).map(([id, label, description]) => (
                  <button
                    key={id}
                    aria-current={conversationGroup === id ? "page" : undefined}
                    onClick={() => setConversationGroup(id)}
                  >
                    <b>{label}</b>
                    <small>{description}</small>
                  </button>
                ))}
              </nav>

              <div
                className="preference-preset-grid preference-preset-grid--primary"
                hidden={conversationGroup !== "response"}
              >
                <PreferenceSelect
                  label="Execution preset"
                  value={draft.executionPreset ?? ""}
                  inherited={effective.executionPreset}
                  allowInherit={scope.kind !== "global"}
                  onChange={(value) =>
                    setDraft((current) =>
                      current
                        ? {
                            ...current,
                            executionPreset:
                              value === ""
                                ? undefined
                                : (value as NativeExecutionPreset),
                          }
                        : current,
                    )
                  }
                  options={EXECUTION_PRESETS}
                />
                <PreferenceSelect
                  label="Performance preset"
                  value={draft.performancePreset ?? ""}
                  inherited={effective.performancePreset}
                  allowInherit={scope.kind !== "global"}
                  onChange={(value) =>
                    setDraft((current) =>
                      current
                        ? {
                            ...current,
                            performancePreset:
                              value === ""
                                ? undefined
                                : (value as NativePerformancePreset),
                          }
                        : current,
                    )
                  }
                  options={PERFORMANCE_PRESETS}
                />
              </div>

              <div
                className="preference-overrides"
                data-conversation-group={conversationGroup}
              >
                <h3>
                  {conversationGroup === "response"
                    ? "Response style"
                    : conversationGroup === "interaction"
                      ? "Input and display"
                      : "Presence and privacy"}
                </h3>
                <p>
                  {conversationGroup === "response"
                    ? "Choose how quickly and fully a character answers."
                    : conversationGroup === "interaction"
                      ? "Choose how conversations start, pause and appear."
                      : "Choose which optional context the character may use."}
                </p>
                <div className="preference-field-grid">
                  <PreferenceSelect
                    label="Verbosity"
                    value={draft.overrides.verbosity ?? ""}
                    inherited={effective.verbosity}
                    allowInherit
                    onChange={(value) =>
                      setOverride(
                        "verbosity",
                        value === "" ? undefined : (value as NativeVerbosity),
                      )
                    }
                    options={[
                      ["concise", "Concise"],
                      ["standard", "Standard"],
                      ["detailed", "Detailed"],
                    ]}
                  />
                  <PreferenceSelect
                    label="Response length"
                    value={draft.overrides.responseLength ?? ""}
                    inherited={effective.responseLength}
                    allowInherit
                    onChange={(value) =>
                      setOverride(
                        "responseLength",
                        value === ""
                          ? undefined
                          : (value as NativeResponseLength),
                      )
                    }
                    options={[
                      ["short", "Short"],
                      ["medium", "Medium"],
                      ["long", "Long"],
                    ]}
                  />
                  <PreferenceSelect
                    label="Interruption"
                    value={draft.overrides.interruptionMode ?? ""}
                    inherited={effective.interruptionMode}
                    allowInherit
                    onChange={(value) =>
                      setOverride(
                        "interruptionMode",
                        value === ""
                          ? undefined
                          : (value as NativeInterruptionMode),
                      )
                    }
                    options={[
                      ["immediate", "Immediate"],
                      ["finishSentence", "Finish sentence"],
                      ["disabled", "Disabled"],
                    ]}
                  />
                  <PreferenceSelect
                    label="Input"
                    value={draft.overrides.inputMode ?? ""}
                    inherited={effective.inputMode}
                    allowInherit
                    onChange={(value) =>
                      setOverride(
                        "inputMode",
                        value === ""
                          ? undefined
                          : (value as NativePreferenceInputMode),
                      )
                    }
                    options={[
                      ["ptt", "Push to talk"],
                      ["vad", "Voice activity detection"],
                    ]}
                  />
                  <label className="preference-control creativity-control">
                    <span>Creativity</span>
                    <select
                      aria-label="Creativity source"
                      value={
                        draft.overrides.creativity === undefined
                          ? "inherit"
                          : "override"
                      }
                      onChange={(event) =>
                        setOverride(
                          "creativity",
                          event.target.value === "inherit"
                            ? undefined
                            : effective.creativity.value,
                        )
                      }
                    >
                      <option value="inherit">Use default</option>
                      <option value="override">Custom</option>
                    </select>
                    <input
                      aria-label="Creativity value"
                      type="range"
                      min={0}
                      max={100}
                      value={
                        draft.overrides.creativity ?? effective.creativity.value
                      }
                      disabled={draft.overrides.creativity === undefined}
                      onChange={(event) =>
                        setOverride("creativity", Number(event.target.value))
                      }
                    />
                    <small>
                      Current: {effective.creativity.value}/100 ·{" "}
                      {scopeLabel(effective.creativity.sourceScope)}
                    </small>
                  </label>
                  <BooleanPreference
                    label="Subtitles"
                    value={draft.overrides.subtitles}
                    inherited={effective.subtitles}
                    onChange={(next) => setOverride("subtitles", next)}
                  />
                  <BooleanPreference
                    label="Overlay request"
                    value={draft.overrides.overlay}
                    inherited={effective.overlay}
                    onChange={(next) => setOverride("overlay", next)}
                  />
                </div>
                {conversationGroup === "interaction" && (
                  <p className="preference-control-note">
                    The overlay appears after a game window is connected and
                    ready.
                  </p>
                )}
                <details
                  className="technical-disclosure optional-capability-disclosure"
                  hidden={conversationGroup !== "presence"}
                  open={conversationGroup === "presence"}
                >
                  <summary>Optional memory, vision, and presence</summary>
                  <div className="preference-field-grid">
                    {(
                      [
                        ["memory", "Memory", effective.memory],
                        ["emotion", "Emotion", effective.emotion],
                        ["vision", "Vision", effective.vision],
                        [
                          "webcamPresence",
                          "Webcam presence intent",
                          effective.webcamPresence,
                        ],
                      ] as const
                    ).map(([key, label, value]) => (
                      <BooleanPreference
                        key={key}
                        label={label}
                        value={draft.overrides[key]}
                        inherited={value}
                        onChange={(next) => setOverride(key, next)}
                      />
                    ))}
                  </div>
                  <p className="preference-control-note">
                    Webcam presence is not available yet; this stores your
                    preference.
                  </p>
                </details>
              </div>

              <details
                className="technical-disclosure preference-technical-disclosure"
                hidden={conversationGroup !== "presence"}
              >
                <summary>
                  <span>Privacy and routing details</span>
                  {" · "}
                  <span className="preference-technical-summary">
                    <span>
                      {snapshot.routeSnapshot?.sourceLoadoutId ??
                        "No active route"}
                    </span>
                    {" · "}
                    <span>
                      {snapshot.resourceSnapshot.admissionReceiptPresent
                        ? (snapshot.resourceSnapshot.admissionStatus ??
                          "Admission receipt present")
                        : "No admission receipt"}
                    </span>
                    {" · "}
                    <span>
                      <span>Captured game image</span>{" "}
                      <b>
                        {effective.egress.capturedGameImage.value === "denied"
                          ? "Denied"
                          : "Selected provider route only"}
                      </b>
                    </span>
                  </span>
                </summary>

                <div className="preference-proof-grid">
                  <article>
                    <span>Route authority snapshot</span>
                    <b>
                      Loadout{" "}
                      {snapshot.routeSnapshot?.sourceLoadoutId ?? "none"}
                    </b>
                    <small>
                      {snapshot.routeSnapshot
                        ? `sha256 ${snapshot.routeSnapshot.sha256.slice(0, 12)}… · generation ${snapshot.routeSnapshot.generation ?? "unreported"}`
                        : "No route receipt was available for this scope."}
                    </small>
                  </article>
                  <article>
                    <span>Local resource authority</span>
                    <b>
                      Admission:{" "}
                      {snapshot.resourceSnapshot.admissionReceiptPresent
                        ? (snapshot.resourceSnapshot.admissionStatus ??
                          "receipt present")
                        : "no receipt"}
                    </b>
                    <small>
                      {snapshot.resourceSnapshot.selectionId ??
                        "No selected local loadout"}
                    </small>
                  </article>
                  <article>
                    <span>Mutation safety</span>
                    <b>Intent only</b>
                    <small>
                      Automatic fallback off · route/pack activation false
                    </small>
                  </article>
                </div>

                <div
                  className="preference-egress"
                  aria-label="Effective egress"
                >
                  <h3>Data allowed for the selected route</h3>
                  {(
                    [
                      ["Transcript", effective.egress.transcript],
                      ["Microphone audio", effective.egress.microphoneAudio],
                      [
                        "Game image capture",
                        effective.egress.capturedGameImage,
                      ],
                      [
                        "Local memory context",
                        effective.egress.localMemoryContext,
                      ],
                    ] as const
                  ).map(([label, value]) => (
                    <div key={label}>
                      <span>{label}</span>
                      <b>
                        {value.value === "denied"
                          ? "Denied"
                          : "Selected provider route only"}
                      </b>
                      <small>{inheritedLabel(value)}</small>
                    </div>
                  ))}
                </div>
              </details>

              <div
                className="preference-migration"
                hidden={conversationGroup !== "presence"}
              >
                <b>
                  Schema v{snapshot.schemaVersion} · revision{" "}
                  {snapshot.revision}
                </b>
                <span>
                  {snapshot.migration.state.replace(/([A-Z])/g, " $1")}
                </span>
                <small>{snapshot.migration.detail}</small>
              </div>
            </>
          )}

          {notice && (
            <p className="inline-status" role="status">
              {notice}
            </p>
          )}
          {error && (
            <p className="inline-status error" role="alert">
              {error}
            </p>
          )}
        </section>
      </div>
      <div
        className="preference-editor-pane"
        hidden={preferenceSection !== "subtitles"}
      >
        <SubtitlePreferencesPanel
          nativeAvailable={nativeAvailable}
          scope={scope}
        />
      </div>
      <div
        className="preference-editor-pane"
        hidden={preferenceSection !== "routing"}
      >
        {snapshot && effective && (
          <section className="instrument-panel preference-routing-overview">
            <div className="panel-title workspace-heading">
              <div>
                <span className="eyebrow">Route and data boundary</span>
                <h2>Current authority</h2>
              </div>
              <span className="badge">Read only</span>
            </div>
            <dl className="preference-routing-summary">
              <div>
                <dt>Provider loadout</dt>
                <dd>
                  {snapshot.routeSnapshot?.sourceLoadoutId ?? "No active route"}
                </dd>
              </div>
              <div>
                <dt>Local admission</dt>
                <dd>
                  {snapshot.resourceSnapshot.admissionReceiptPresent
                    ? (snapshot.resourceSnapshot.admissionStatus ??
                      "Admission receipt present")
                    : "No admission receipt"}
                </dd>
              </div>
              <div>
                <dt>Captured game image</dt>
                <dd>
                  {effective.egress.capturedGameImage.value === "denied"
                    ? "Denied"
                    : "Selected provider route only"}
                </dd>
              </div>
              <div>
                <dt>Fallback policy</dt>
                <dd>Automatic fallback off</dd>
              </div>
            </dl>
            <p className="preference-routing-migration">
              {snapshot.migration.detail}
            </p>
          </section>
        )}
        <EffectiveConfigurationInspector
          nativeAvailable={nativeAvailable}
          scope={scope}
        />
      </div>
    </div>
  );
}

function subtitleSourceLabel(source: NativeSubtitleEffectiveSource) {
  switch (source.kind) {
    case "bundledManifestDefault":
      return "bundled manifest default";
    case "bundledStyle":
      return `bundled style ${source.styleId ?? "unreported"}`;
    case "rendererDefault":
      return "native renderer default";
    case "persistedScope":
      return source.scope
        ? `persisted ${scopeLabel(source.scope)}`
        : "persisted scope";
  }
}

function SubtitlePreferencesPanel({
  nativeAvailable,
  scope,
}: {
  nativeAvailable: boolean;
  scope: NativeProductPreferenceScope;
}) {
  const [snapshot, setSnapshot] =
    useState<NativeSubtitlePreferenceSnapshot | null>(null);
  const [draft, setDraft] = useState<NativeScopedSubtitlePreferences | null>(
    null,
  );
  const [busy, setBusy] = useState<"load" | "save" | "reset" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [resetArmed, setResetArmed] = useState(false);
  const loadSequence = useRef(0);

  const applySnapshot = useCallback(
    (next: NativeSubtitlePreferenceSnapshot) => {
      setSnapshot(next);
      const entry = next.entries.find((candidate) =>
        sameScope(candidate.scope, scope),
      );
      setDraft(
        entry
          ? structuredClone(entry)
          : { scope, selectedStyleId: undefined, overrides: {} },
      );
    },
    [scope],
  );

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    if (!nativeAvailable) {
      setSnapshot(null);
      setDraft(null);
      return;
    }
    setBusy("load");
    setError(null);
    setNotice(null);
    setResetArmed(false);
    try {
      const next = await readSubtitlePreferences(scope);
      if (!next)
        throw new Error("Native subtitle preferences returned no snapshot.");
      if (sequence !== loadSequence.current) return;
      applySnapshot(next);
    } catch (cause) {
      if (sequence !== loadSequence.current) return;
      setSnapshot(null);
      setDraft(null);
      setError(errorText(cause, "Native subtitle preferences could not load."));
    } finally {
      if (sequence === loadSequence.current) setBusy(null);
    }
  }, [applySnapshot, nativeAvailable, scope]);

  useEffect(() => {
    void load();
  }, [load]);

  const setOverride = <K extends keyof NativeSubtitlePreferenceOverrides>(
    key: K,
    value: NativeSubtitlePreferenceOverrides[K] | undefined,
  ) =>
    setDraft((current) => {
      if (!current) return current;
      const overrides = { ...current.overrides };
      if (value === undefined) delete overrides[key];
      else overrides[key] = value;
      return { ...current, overrides };
    });

  const save = async () => {
    if (!snapshot || !draft) return;
    setBusy("save");
    setError(null);
    setNotice(null);
    try {
      const next = await saveSubtitlePreferences(snapshot.revision, draft);
      if (!next)
        throw new Error("Native subtitle preferences returned no saved state.");
      applySnapshot(next);
      setNotice(
        "Subtitle intent saved atomically. The validated native renderer parameters below are authoritative; no font or style asset was downloaded.",
      );
    } catch (cause) {
      setError(errorText(cause, "Subtitle preferences could not be saved."));
    } finally {
      setBusy(null);
    }
  };

  const reset = async () => {
    if (!snapshot) return;
    if (!resetArmed) {
      setResetArmed(true);
      setNotice(
        `Select Confirm subtitle reset to remove ${scopeLabel(scope)} overrides.`,
      );
      return;
    }
    setBusy("reset");
    setError(null);
    try {
      const next = await resetSubtitlePreferences(snapshot.revision, scope);
      if (!next)
        throw new Error("Native subtitle preferences returned no reset state.");
      applySnapshot(next);
      setResetArmed(false);
      setNotice("Subtitle scope reset persisted after explicit confirmation.");
    } catch (cause) {
      setError(errorText(cause, "Subtitle preferences could not be reset."));
    } finally {
      setBusy(null);
    }
  };

  const effective = snapshot?.effective;
  const effectiveStyle = snapshot?.availableStyles.find(
    (style) => style.styleId === effective?.selectedStyleId.value,
  );
  const hasDraftMutation = Boolean(
    draft?.selectedStyleId ||
      (draft && Object.keys(draft.overrides).length > 0),
  );

  return (
    <section className="instrument-panel subtitle-preferences-workspace">
      <div className="panel-title workspace-heading">
        <div>
          <span className="eyebrow">In-game presentation</span>
          <h2>Subtitle look</h2>
        </div>
        <span className={nativeAvailable ? "badge good" : "badge wait"}>
          {nativeAvailable ? "Validated assets" : "Preview only"}
        </span>
      </div>
      <p className="source-disclosure">
        Choose a style, then tune the size, position and background.
      </p>

      {!nativeAvailable && (
        <div className="empty-state compact">
          <b>Customize subtitles in the desktop app</b>
          <p>Your selected style applies to the next conversation.</p>
        </div>
      )}
      {nativeAvailable && busy === "load" && !snapshot && (
        <div className="empty-state compact" aria-live="polite">
          <b>Loading native subtitle preferences…</b>
        </div>
      )}

      {snapshot && draft && effective && (
        <>
          <div className="subtitle-preference-grid">
            <label className="preference-control">
              <span>Subtitle look and typography</span>
              <select
                aria-label="Bundled subtitle style"
                value={draft.selectedStyleId ?? ""}
                onChange={(event) =>
                  setDraft((current) =>
                    current
                      ? {
                          ...current,
                          selectedStyleId: event.target.value || undefined,
                        }
                      : current,
                  )
                }
              >
                <option value="">Inherit validated default</option>
                {snapshot.availableStyles.map((style) => (
                  <option key={style.styleId} value={style.styleId}>
                    {readableIdentifier(style.styleId)}
                  </option>
                ))}
              </select>
              <small>
                Effective {readableIdentifier(effective.selectedStyleId.value)}{" "}
                · {subtitleSourceLabel(effective.selectedStyleId.source)}
              </small>
              {effectiveStyle && (
                <small>
                  Body font role {effectiveStyle.bodyFontRole} · speaker font
                  role {effectiveStyle.speakerFontRole}
                </small>
              )}
            </label>
            <SubtitleNumberPreference
              label="Safe area (dp)"
              value={draft.overrides.safeAreaDp}
              effective={effective.safeAreaDp}
              min={0}
              max={256}
              step={1}
              onChange={(value) => setOverride("safeAreaDp", value)}
            />
            <SubtitleNumberPreference
              label="Text scale"
              value={draft.overrides.textScale}
              effective={effective.textScale}
              min={0.75}
              max={2}
              step={0.05}
              onChange={(value) => setOverride("textScale", value)}
            />
            <SubtitleNumberPreference
              label="Global opacity"
              value={draft.overrides.opacity}
              effective={effective.opacity}
              min={0.25}
              max={1}
              step={0.05}
              onChange={(value) => setOverride("opacity", value)}
            />
            <label className="preference-control">
              <span>Native backplate</span>
              <select
                aria-label="Native backplate"
                value={
                  draft.overrides.backplateEnabled === undefined
                    ? "inherit"
                    : draft.overrides.backplateEnabled
                      ? "on"
                      : "off"
                }
                onChange={(event) =>
                  setOverride(
                    "backplateEnabled",
                    event.target.value === "inherit"
                      ? undefined
                      : event.target.value === "on",
                  )
                }
              >
                <option value="inherit">Inherit</option>
                <option value="on">On</option>
                <option value="off">Off</option>
              </select>
              <small>
                Effective {effective.backplateEnabled.value ? "on" : "off"} ·{" "}
                {subtitleSourceLabel(effective.backplateEnabled.source)}
              </small>
            </label>
          </div>

          <div
            className="subtitle-renderer-proof"
            aria-label="Validated renderer parameters"
          >
            <span>Validated renderer parameters</span>
            <b>{effective.rendererParameters.styleId}</b>
            <small>
              Next turn · safe area {effective.rendererParameters.safeAreaDp} dp
              · body {effective.rendererParameters.bodySizeDp} dp · speaker{" "}
              {effective.rendererParameters.speakerSizeDp} dp · backplate{" "}
              {effective.rendererParameters.backplateEnabled ? "on" : "off"} ·
              opacity {effective.rendererParameters.globalOpacity}
            </small>
          </div>

          <details className="subtitle-asset-disclosure technical-disclosure">
            <summary>Font fallback and licenses</summary>
            <p>{snapshot.assets.generatedAssetPolicy}</p>
            <p>
              Font binaries bundled:{" "}
              {snapshot.assets.noFontBinariesBundled ? "no" : "yes"}. System
              lookup is a fallback candidate, not proof a font is installed.
            </p>
            {snapshot.assets.fontRoles.map((role) => (
              <div key={role.roleId}>
                <b>{role.roleId}</b>
                <small>
                  {role.fallbackChain
                    .map(
                      (font) =>
                        `${font.family} · ${font.licenseId} · ${font.availability}`,
                    )
                    .join(" → ")}{" "}
                  · generic {role.genericFallback}
                </small>
              </div>
            ))}
          </details>

          <details className="technical-disclosure subtitle-technical-disclosure">
            <summary>Renderer revision and supported fields</summary>
            <div className="preference-migration">
              <b>
                Subtitle schema v{snapshot.schemaVersion} · revision{" "}
                {snapshot.revision}
              </b>
              <span>{snapshot.migration.state.replace(/([A-Z])/g, " $1")}</span>
              <small>
                {snapshot.migration.detail} · supported fields{" "}
                {snapshot.supportedOverrideFields.join(", ")}
              </small>
            </div>
          </details>
          <div className="preference-actions">
            <button
              className="primary-action small"
              disabled={!hasDraftMutation || busy !== null}
              onClick={() => void save()}
            >
              {busy === "save" ? "Saving…" : "Save subtitle intent"}
            </button>
            <button
              className={resetArmed ? "quiet-button danger" : "quiet-button"}
              disabled={busy !== null}
              onClick={() => void reset()}
            >
              {busy === "reset"
                ? "Resetting…"
                : resetArmed
                  ? "Confirm subtitle reset"
                  : "Reset subtitle scope"}
            </button>
            <button
              className="quiet-button"
              disabled={busy !== null}
              onClick={() => void load()}
            >
              Reload subtitle values
            </button>
          </div>
        </>
      )}
      {notice && (
        <p className="inline-status" role="status">
          {notice}
        </p>
      )}
      {error && (
        <p className="inline-status error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function SubtitleNumberPreference({
  label,
  value,
  effective,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  value: number | undefined;
  effective: NativeSubtitlePreferenceSnapshot["effective"]["safeAreaDp"];
  min: number;
  max: number;
  step: number;
  onChange: (value: number | undefined) => void;
}) {
  return (
    <label className="preference-control subtitle-number-preference">
      <span>{label}</span>
      <select
        aria-label={`${label} source`}
        value={value === undefined ? "inherit" : "override"}
        onChange={(event) =>
          onChange(
            event.target.value === "inherit" ? undefined : effective.value,
          )
        }
      >
        <option value="inherit">Inherit</option>
        <option value="override">Override</option>
      </select>
      <input
        aria-label={label}
        type="number"
        min={min}
        max={max}
        step={step}
        value={value ?? effective.value}
        disabled={value === undefined}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <small>
        Effective {effective.value} · {subtitleSourceLabel(effective.source)}
      </small>
    </label>
  );
}

function effectiveValueLabel(
  value: NativeEffectiveConfigurationSnapshot["entries"][number]["value"],
) {
  switch (value.kind) {
    case "boolean":
      return value.value ? "On" : "Off";
    case "choice":
      return value.value.replace(/([A-Z])/g, " $1");
    case "integer":
      return `${value.value.toLocaleString()} ${value.unit}`;
    case "route":
      return `${value.providerId} / ${value.modelId}${value.voiceId ? ` / ${value.voiceId}` : ""}`;
    case "disabled":
      return "Disabled";
  }
}

function EffectiveConfigurationInspector({
  nativeAvailable,
  scope,
}: {
  nativeAvailable: boolean;
  scope: NativeProductPreferenceScope;
}) {
  const [snapshot, setSnapshot] =
    useState<NativeEffectiveConfigurationSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loadSequence = useRef(0);

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    if (!nativeAvailable) {
      setSnapshot(null);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const next = await readEffectiveConfiguration(scope);
      if (!next)
        throw new Error("Native effective configuration returned no snapshot.");
      if (sequence !== loadSequence.current) return;
      setSnapshot(next);
    } catch (cause) {
      if (sequence !== loadSequence.current) return;
      setSnapshot(null);
      setError(errorText(cause, "Effective configuration could not load."));
    } finally {
      if (sequence === loadSequence.current) setBusy(false);
    }
  }, [nativeAvailable, scope]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <section className="instrument-panel effective-configuration-inspector">
      <div className="panel-title workspace-heading">
        <div>
          <span className="eyebrow">Resolved configuration</span>
          <h2>What the next turn will use</h2>
        </div>
        <span className="badge">{snapshot?.entries.length ?? 0} entries</span>
      </div>
      <p className="source-disclosure">
        Open a row to see where its value comes from. Change conversation and
        subtitle defaults above; provider routes and local models have their own
        workspaces.
      </p>
      {!nativeAvailable && (
        <div className="empty-state compact">
          <b>Open the desktop app to see your active settings</b>
        </div>
      )}
      {busy && !snapshot && (
        <div className="empty-state compact" aria-live="polite">
          <b>Reading native owners…</b>
        </div>
      )}
      {snapshot && (
        <>
          <div className="effective-configuration-proof">
            <b>Read only · preference revision {snapshot.preferenceRevision}</b>
            <small>
              Mutation authority added:{" "}
              {snapshot.mutationAuthorityAdded ? "yes" : "no"} · automatic
              provider fallback:{" "}
              {snapshot.automaticProviderFallback ? "yes" : "no"}
            </small>
          </div>
          <div className="effective-configuration-list" role="list">
            {snapshot.entries.map((entry) => (
              <details key={entry.key} role="listitem">
                <summary>
                  <span>{entry.category.replace(/([A-Z])/g, " $1")}</span>
                  <b>{entry.label}</b>
                  <code>{effectiveValueLabel(entry.value)}</code>
                </summary>
                <dl>
                  <div>
                    <dt>Owner</dt>
                    <dd>{entry.owner}</dd>
                  </div>
                  <div>
                    <dt>Winning scope</dt>
                    <dd>{scopeLabel(entry.winningScope)}</dd>
                  </div>
                  <div>
                    <dt>Source</dt>
                    <dd>
                      {entry.sourceKind} · {entry.persistence.source}
                    </dd>
                  </div>
                  <div>
                    <dt>Default</dt>
                    <dd>{effectiveValueLabel(entry.defaultValue)}</dd>
                  </div>
                </dl>
                <p>{entry.explanation}</p>
                <small>{entry.runtimeConsequence}</small>
                {entry.value.kind === "route" && (
                  <small>
                    {entry.value.execution} · {entry.value.egress} · transmitted{" "}
                    {entry.value.transmittedData.join(", ") || "none"} · manual
                    fallbacks {entry.value.manualFallbackCount} · automatic
                    fallback {entry.value.automaticFallback ? "on" : "off"}
                  </small>
                )}
                {entry.unavailableReason && (
                  <p className="control-reason">
                    Unavailable: {entry.unavailableReason}
                  </p>
                )}
              </details>
            ))}
          </div>
          <button
            className="quiet-button"
            disabled={busy}
            onClick={() => void load()}
          >
            {busy ? "Refreshing…" : "Refresh effective configuration"}
          </button>
        </>
      )}
      {error && (
        <p className="inline-status error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function PreferenceSelect<T extends string>({
  label,
  value,
  inherited,
  allowInherit,
  onChange,
  options,
}: {
  label: string;
  value: T | "";
  inherited: NativeEffectivePreference<T>;
  allowInherit: boolean;
  onChange: (value: string) => void;
  options: Array<readonly [T, string]>;
}) {
  return (
    <label className="preference-control">
      <span>{label}</span>
      <select
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        {allowInherit && <option value="">Use default</option>}
        {options.map(([option, optionLabel]) => (
          <option key={option} value={option}>
            {optionLabel}
          </option>
        ))}
      </select>
      <small>
        Current:{" "}
        {options.find(([option]) => option === inherited.value)?.[1] ??
          readableIdentifier(inherited.value)}{" "}
        · {scopeLabel(inherited.sourceScope)}
      </small>
    </label>
  );
}

function BooleanPreference({
  label,
  value,
  inherited,
  onChange,
}: {
  label: string;
  value: boolean | undefined;
  inherited: NativeEffectivePreference<boolean>;
  onChange: (value: boolean | undefined) => void;
}) {
  return (
    <label className="preference-control">
      <span>{label}</span>
      <select
        aria-label={label}
        value={value === undefined ? "inherit" : value ? "on" : "off"}
        onChange={(event) =>
          onChange(
            event.target.value === "inherit"
              ? undefined
              : event.target.value === "on",
          )
        }
      >
        <option value="inherit">Use default</option>
        <option value="on">On</option>
        <option value="off">Off</option>
      </select>
      <small>
        Current: {inherited.value ? "On" : "Off"} ·{" "}
        {scopeLabel(inherited.sourceScope)}
      </small>
    </label>
  );
}
