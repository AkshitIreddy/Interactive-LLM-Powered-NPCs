import { useCallback, useEffect, useState } from "react";
import type {
  NativeGameProfileSummary,
  NativeModelSummary,
} from "./tauriBridge";
import {
  admitSelectedLocalLoadout,
  backupAllLocalMemory,
  cancelManualActorPicker,
  cancelTrustedOptionalPackDownload,
  clearGameTarget,
  cancelThisPcBenchmark,
  correctEncounterToAuthoredCharacter,
  discoverGameTargets,
  deleteLocalMemoryBackup,
  eraseCharacterMemory,
  inspectCharacterDatabase,
  mergeUnknownEncounters,
  mutateTrustedOptionalPack,
  mutateExperimentalVisualPack,
  persistSelectedCharacter,
  readCharacterDatabaseCatalog,
  readCharacterMemoryStatus,
  readExperimentalVisualPackStatus,
  readLocalResourceSettings,
  readLocalResourceTelemetry,
  readManualActorPickerStatus,
  listLocalMemoryBackups,
  readSelectedGameTarget,
  readSelectedLocalLoadoutPlanner,
  readTrustedLocalPackCatalog,
  readTrustedOptionalPackLifecycle,
  readThisPcBenchmarkReport,
  readThisPcBenchmarkStatus,
  removeAllLocalMemory,
  restoreLocalMemoryBackup,
  saveLocalResourceSettings,
  selectGameTarget,
  startThisPcBenchmark,
  startManualActorPicker,
  type NativeBenchmarkMetricSummary,
  type NativeBenchmarkReport,
  type NativeBenchmarkStatus,
  type NativeCharacterCatalogSnapshot,
  type NativeCharacterInspection,
  type NativeCharacterMemoryEraseResult,
  type NativeCharacterMemoryStatus,
  type NativeCharacterMemoryRestoreResult,
  type NativeCharacterEncounterEvidence,
  type NativeEncounterMutationResult,
  type NativeExperimentalPackState,
  type NativeGameTargetCandidate,
  type NativeGameTargetSelection,
  type NativeManualActorPickerPresentation,
  type NativeLocalResourceSettings,
  type NativeLocalMemoryBackup,
  type NativeResourceTelemetryResult,
  type NativeRemoveAllLocalMemoryResult,
  type NativeSelectedLoadoutAdmissionResult,
  type NativeSelectedLoadoutPlannerResult,
  type NativeTelemetryObservation,
  type NativeTrustedLocalPackCatalog,
  type NativeTrustedOptionalPackLifecycle,
} from "./tauriBridge";

const BROWSER_CHARACTER: NativeCharacterInspection = {
  schemaVersion: 1,
  gameProfileId: "eclipse-harbor",
  gameDisplayName: "Eclipse Harbor",
  selectedCharacterId: "mara-venn",
  character: {
    id: "mara-venn",
    displayName: "Mara Venn",
    aliases: ["Mara"],
    biography:
      "Synthetic browser-preview character. Native character authority is not loaded here.",
    personality: "Measured and direct.",
    dialogueStyle: "Browser preview only.",
    styleExamples: [],
    openingLines: [],
    backgroundNpc: false,
    promptRole: "Synthetic lighthouse keeper",
    promptObjectives: [],
    promptConstraints: ["Never present browser data as native authority."],
    knowledgeRefs: [],
    voice: {
      description: "Preview voice description",
      locale: "en-US",
      styleTags: [],
      providerVoiceId: null,
      adapterId: null,
      catalogVersion: null,
      license: null,
      userOverrideAllowed: false,
    },
    identity: {
      strategy: "explicit_preview_fixture",
      evidence: ["Browser preview fixture"],
      fallback: "No native identity decision",
      automaticFaceRecognitionClaimed: false,
    },
  },
  authoredKnowledge: [],
  provenance: [],
  deliveredMemory: [],
  memoryScope: {
    userId: "browser-preview",
    profileId: "eclipse-harbor",
    gameId: "eclipse-harbor",
    characterId: "mara-venn",
    sessionId: null,
    saveId: null,
    crossGameWideningAllowed: false,
  },
};

type AsyncState<T> =
  | { kind: "loading" }
  | { kind: "empty" }
  | { kind: "ready"; value: T }
  | { kind: "error"; detail: string };
const errorText = (error: unknown, fallback: string) =>
  error instanceof Error ? error.message : fallback;

export function GameTargetWorkspace({
  nativeAvailable,
  gameProfiles,
  gameProfileId,
  onGameProfileChange,
  onSelectionChange,
}: {
  nativeAvailable: boolean;
  gameProfiles: NativeGameProfileSummary[];
  gameProfileId: string;
  onGameProfileChange: (gameProfileId: string) => void;
  onSelectionChange: (selection: NativeGameTargetSelection | null) => void;
}) {
  const [candidates, setCandidates] = useState<
    AsyncState<NativeGameTargetCandidate[]>
  >(nativeAvailable ? { kind: "loading" } : { kind: "empty" });
  const [selected, setSelected] = useState<NativeGameTargetSelection | null>(
    null,
  );
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(
    nativeAvailable
      ? "Loading the current native-session process binding."
      : "Browser preview cannot enumerate or bind Windows game processes.",
  );
  const [actorPicker, setActorPicker] =
    useState<NativeManualActorPickerPresentation>({
      schemaVersion: 1,
      state: "unavailable",
      detail: nativeAvailable
        ? "Bind a game target before selecting a character in the native overlay."
        : "Native actor selection requires the Windows desktop shell.",
    });

  useEffect(() => {
    let active = true;
    if (!nativeAvailable) return;
    void readSelectedGameTarget()
      .then((value) => {
        if (!active) return;
        setSelected(value);
        onSelectionChange(value);
        setCandidates({ kind: "empty" });
        setNotice(
          value
            ? "Current-session process-instance binding loaded. It is intentionally cleared by app restart; visual capture remains governed separately."
            : "No game process is selected in this native session. Discover eligible running windows.",
        );
      })
      .catch((error) => {
        if (!active) return;
        const detail = errorText(error, "Selected target could not be read.");
        setCandidates({ kind: "error", detail });
        setNotice(detail);
      });
    return () => {
      active = false;
    };
  }, [nativeAvailable, onSelectionChange]);

  useEffect(() => {
    if (!nativeAvailable || actorPicker.state !== "waiting") return;
    let active = true;
    const timer = window.setInterval(() => {
      void readManualActorPickerStatus()
        .then((value) => {
          if (active && value) setActorPicker(value);
        })
        .catch(() => {
          if (active)
            setActorPicker({
              schemaVersion: 1,
              state: "unavailable",
              detail: "Native actor selection status became unavailable.",
            });
        });
    }, 250);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [actorPicker.state, nativeAvailable]);

  const discover = async () => {
    setBusy(true);
    setCandidates({ kind: "loading" });
    try {
      const value = await discoverGameTargets(gameProfileId);
      if (!value?.length) {
        setCandidates({ kind: "empty" });
        setNotice(
          "No eligible running window matched this bundled game profile.",
        );
      } else {
        setCandidates({ kind: "ready", value });
        setNotice(
          `${value.length} eligible process-instance candidate${value.length === 1 ? "" : "s"} found.`,
        );
      }
    } catch (error) {
      const detail = errorText(error, "Game target discovery failed.");
      setCandidates({ kind: "error", detail });
      setNotice(detail);
    } finally {
      setBusy(false);
    }
  };
  const choose = async (candidate: NativeGameTargetCandidate) => {
    setBusy(true);
    try {
      const value = await selectGameTarget(
        gameProfileId,
        candidate.nativeWindow,
        confirmed,
      );
      if (!value) throw new Error("Native selection returned no state.");
      setSelected(value);
      onSelectionChange(value);
      setNotice(value.safetyDetail);
    } catch (error) {
      setNotice(errorText(error, "Game target selection failed."));
    } finally {
      setBusy(false);
    }
  };
  const clear = async () => {
    setBusy(true);
    try {
      await clearGameTarget();
      setSelected(null);
      onSelectionChange(null);
      setNotice(
        "Native process binding cleared. Audio/subtitle-only conversation remains available.",
      );
    } catch (error) {
      setNotice(errorText(error, "Game target could not be cleared."));
    } finally {
      setBusy(false);
    }
  };
  const startActorPicker = async () => {
    setBusy(true);
    try {
      const value = await startManualActorPicker();
      setActorPicker(
        value ?? {
          schemaVersion: 1,
          state: "unavailable",
          detail: "Native actor selection requires the Windows desktop shell.",
        },
      );
    } catch (error) {
      setActorPicker({
        schemaVersion: 1,
        state: "unavailable",
        detail: errorText(error, "Native actor selection could not start."),
      });
    } finally {
      setBusy(false);
    }
  };
  const cancelActorPicker = async () => {
    setBusy(true);
    try {
      const value = await cancelManualActorPicker();
      if (value) setActorPicker(value);
    } catch (error) {
      setActorPicker({
        schemaVersion: 1,
        state: "unavailable",
        detail: errorText(
          error,
          "Native actor selection could not be cancelled.",
        ),
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className="instrument-panel native-target-workspace"
      aria-labelledby="target-workspace-title"
    >
      <div className="panel-title">
        <div>
          <span className="eyebrow">Native process-instance boundary</span>
          <h2 id="target-workspace-title">Game target</h2>
        </div>
        <span className={selected ? "badge wait" : "badge"}>
          {selected ? "Bound · capture blocked" : "Not selected"}
        </span>
      </div>
      <p className="source-disclosure">
        {nativeAvailable
          ? "Source: live Windows process/window enumeration and current-session selection. PID/HWND bindings never survive app restart. A checkbox is not trusted offline or anti-cheat proof, so commercial visual capture remains blocked."
          : "Source: browser preview. Process discovery and current-session binding require the native Windows shell."}
      </p>
      <div className="target-toolbar">
        <label>
          <span>
            {nativeAvailable
              ? "Bundled game profile"
              : "Synthetic review profile"}
          </span>
          <select
            value={gameProfileId}
            disabled={busy || gameProfiles.length === 0}
            onChange={(event) => onGameProfileChange(event.target.value)}
          >
            {gameProfiles.length === 0 ? (
              <option value={gameProfileId}>{gameProfileId}</option>
            ) : (
              gameProfiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.displayName}
                </option>
              ))
            )}
          </select>
        </label>
        <button
          className="secondary-action"
          disabled={!nativeAvailable || busy}
          onClick={discover}
        >
          {busy ? "Working…" : "Discover running windows"}
        </button>
        <button
          className="quiet-button"
          disabled={!selected || busy}
          onClick={clear}
        >
          Clear binding
        </button>
      </div>
      <label className="authorization-control">
        <input
          type="checkbox"
          checked={confirmed}
          disabled={!nativeAvailable || busy}
          onChange={(event) => setConfirmed(event.target.checked)}
        />
        <span>
          <b>I am using offline single-player</b>
          <small>
            This records an operator statement only. It does not authorize
            visual capture.
          </small>
        </span>
      </label>
      {selected && (
        <article className="selected-target-card">
          <div>
            <b>{selected.target.title}</b>
            <small>
              {selected.target.executableName} · PID {selected.target.processId}{" "}
              · HWND {selected.target.nativeWindow}
            </small>
          </div>
          <span className="badge bad">{selected.safetyState}</span>
          <p>{selected.safetyDetail}</p>
          <button
            className="quiet-button"
            disabled
            aria-describedby="ordinary-capture-blocked-reason"
          >
            Verify ordinary game capture
          </button>
          <small id="ordinary-capture-blocked-reason">
            Disabled: the native safety boundary has no trusted offline and
            protection evidence for this commercial target. The task-owned
            synthetic fixture uses a separate review-only verifier below.
          </small>
        </article>
      )}
      {candidates.kind === "loading" && (
        <div className="empty-state">
          <b>Loading native target state…</b>
        </div>
      )}
      {candidates.kind === "error" && (
        <div className="empty-state error">
          <b>Target state unavailable</b>
          <p>{candidates.detail}</p>
        </div>
      )}
      {candidates.kind === "ready" && (
        <div
          className="target-candidate-list"
          aria-label="Eligible game windows"
        >
          {candidates.value.map((candidate) => (
            <article key={`${candidate.processId}-${candidate.nativeWindow}`}>
              <div>
                <b>{candidate.title}</b>
                <small>
                  {candidate.executableName} · PID {candidate.processId} ·{" "}
                  {candidate.clientWidth}×{candidate.clientHeight}
                </small>
              </div>
              <button disabled={busy} onClick={() => void choose(candidate)}>
                Bind this process
              </button>
            </article>
          ))}
        </div>
      )}
      <div className="target-toolbar manual-actor-picker-controls">
        <button
          className="secondary-action"
          disabled={
            !nativeAvailable ||
            !selected ||
            busy ||
            actorPicker.state === "waiting"
          }
          onClick={startActorPicker}
        >
          Select character in game
        </button>
        <button
          className="quiet-button"
          disabled={busy || actorPicker.state !== "waiting"}
          onClick={cancelActorPicker}
        >
          Cancel character selection
        </button>
        <span
          className={
            actorPicker.state === "selected" ? "badge good" : "badge wait"
          }
        >
          {actorPicker.state === "waiting"
            ? "Waiting for native click"
            : actorPicker.state === "selected"
              ? "Character selected"
              : actorPicker.state === "cancelled"
                ? "Selection cancelled"
                : "Selection unavailable"}
        </span>
      </div>
      <p className="source-disclosure">
        {actorPicker.detail} Captured pixels, detected regions, pointer
        coordinates, native handles, and receipt proof remain outside this
        interface.
      </p>
      <p className="inline-status" role="status">
        {notice}
      </p>
    </section>
  );
}

export function CharacterDatabase({
  nativeAvailable = false,
  gameProfileId = "eclipse-harbor",
  onSelectionChange,
}: {
  nativeAvailable?: boolean;
  gameProfileId?: string;
  onSelectionChange?: (inspection: NativeCharacterInspection) => void;
}) {
  const [catalog, setCatalog] = useState<NativeCharacterCatalogSnapshot | null>(
    null,
  );
  const [state, setState] = useState<AsyncState<NativeCharacterInspection>>(
    nativeAvailable
      ? { kind: "loading" }
      : { kind: "ready", value: BROWSER_CHARACTER },
  );
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(
    nativeAvailable
      ? "Loading native-owned profile selection and delivered-memory scope."
      : "Browser preview fixture only. There are no editable or persisted native character controls here.",
  );
  const inspect = useCallback(
    async (requestedId?: string) => {
      if (!nativeAvailable) {
        setState({ kind: "ready", value: BROWSER_CHARACTER });
        onSelectionChange?.(BROWSER_CHARACTER);
        return;
      }
      setState({ kind: "loading" });
      try {
        const [nextCatalog, value] = await Promise.all([
          readCharacterDatabaseCatalog(gameProfileId),
          inspectCharacterDatabase(gameProfileId, requestedId),
        ]);
        if (!value || !nextCatalog)
          throw new Error(
            "Native character commands returned incomplete state.",
          );
        setCatalog(nextCatalog);
        setState({ kind: "ready", value });
        onSelectionChange?.(value);
        setNotice(
          "Canonical profile, authored knowledge, provenance, and delivered memory loaded from native authority.",
        );
      } catch (error) {
        const detail = errorText(error, "Character inspection failed.");
        setState({ kind: "error", detail });
        setNotice(detail);
      }
    },
    [gameProfileId, nativeAvailable, onSelectionChange],
  );
  useEffect(() => {
    void inspect();
  }, [inspect]);
  const value = state.kind === "ready" ? state.value : null;
  const choose = async (characterId: string) => {
    setBusy(true);
    try {
      const result = await persistSelectedCharacter(gameProfileId, characterId);
      if (!result?.persisted)
        throw new Error("Native selection was not persisted.");
      await inspect(characterId);
      setNotice(
        `Selected ${characterId} for ordinary turns in ${gameProfileId}.`,
      );
    } catch (error) {
      setNotice(
        errorText(error, "Character selection could not be persisted."),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      className="instrument-panel character-database"
      aria-labelledby="character-db-title"
    >
      <header className="panel-title character-database__title">
        <div>
          <span className="eyebrow">Game-scoped native authority</span>
          <h2 id="character-db-title">Character database</h2>
        </div>
        <span className={nativeAvailable ? "badge good" : "badge wait"}>
          {nativeAvailable ? "Native inspection" : "Browser fixture"}
        </span>
      </header>
      <p className="source-disclosure">
        {nativeAvailable
          ? "Bundled profiles are immutable. This view lists and inspects canonical authored records and persists only the selected character ID; no edit command is exposed."
          : "One synthetic preview record is shown so the layout remains inspectable. It is not runtime character authority and cannot be edited or selected natively."}
      </p>
      {catalog && (
        <aside
          className="native-character-list"
          aria-label={`${catalog.gameDisplayName} characters`}
        >
          {catalog.characters.length === 0 ? (
            <p>No canonical characters in this profile.</p>
          ) : (
            catalog.characters.map((entry) => (
              <button
                key={entry.id}
                className={value?.character.id === entry.id ? "selected" : ""}
                aria-pressed={value?.character.id === entry.id}
                disabled={busy}
                onClick={() => void inspect(entry.id)}
              >
                <span>{entry.displayName.slice(0, 2).toUpperCase()}</span>
                <span>
                  <b>{entry.displayName}</b>
                  <small>
                    {entry.backgroundNpc
                      ? "Background archetype"
                      : entry.identityStrategy}
                  </small>
                </span>
                <i>
                  {catalog.selectedCharacterId === entry.id
                    ? "Selected"
                    : entry.id}
                </i>
              </button>
            ))
          )}
        </aside>
      )}
      {state.kind === "loading" && (
        <div className="empty-state">
          <b>Loading native character record…</b>
        </div>
      )}
      {state.kind === "error" && (
        <div className="empty-state error">
          <b>Character record unavailable</b>
          <p>{state.detail}</p>
          <button onClick={() => void inspect()}>Retry profile default</button>
        </div>
      )}
      {value && (
        <div className="character-native-record">
          <div className="character-record__heading">
            <div>
              <span className="eyebrow">
                {value.gameDisplayName} · {value.character.id}
              </span>
              <h3>{value.character.displayName}</h3>
              <p>{value.character.promptRole}</p>
            </div>
            <button
              className="secondary-action"
              disabled={
                !nativeAvailable ||
                busy ||
                value.selectedCharacterId === value.character.id
              }
              onClick={() => void choose(value.character.id)}
            >
              {value.selectedCharacterId === value.character.id
                ? "Selected for turns"
                : "Use for ordinary turns"}
            </button>
          </div>
          <dl className="facts spacious character-native-facts">
            <div>
              <dt>Biography</dt>
              <dd>{value.character.biography || "No biography authored"}</dd>
            </div>
            <div>
              <dt>Personality</dt>
              <dd>
                {value.character.personality || "No personality authored"}
              </dd>
            </div>
            <div>
              <dt>Dialogue style</dt>
              <dd>
                {value.character.dialogueStyle || "No dialogue style authored"}
              </dd>
            </div>
            <div>
              <dt>Identity</dt>
              <dd>
                No face or demographic inference. Strategy:{" "}
                {value.character.identity.strategy} · automatic face recognition
                claimed:{" "}
                {value.character.identity.automaticFaceRecognitionClaimed
                  ? "yes"
                  : "no"}
              </dd>
            </div>
            <div>
              <dt>Memory scope</dt>
              <dd>
                {value.memoryScope.gameId} / {value.memoryScope.characterId} ·
                cross-game widening{" "}
                {value.memoryScope.crossGameWideningAllowed
                  ? "allowed"
                  : "blocked"}
              </dd>
            </div>
            <div>
              <dt>Delivered memory</dt>
              <dd>
                {value.deliveredMemory.length} delivered turn
                {value.deliveredMemory.length === 1 ? "" : "s"}
              </dd>
            </div>
          </dl>
          <details>
            <summary>
              Authored knowledge · {value.authoredKnowledge.length}
            </summary>
            {value.authoredKnowledge.length === 0 ? (
              <p>No authored records for this character.</p>
            ) : (
              <ul>
                {value.authoredKnowledge.map((record) => (
                  <li key={record.id}>
                    <b>
                      {record.authority} · {record.spoilerTier}
                    </b>
                    <span>{record.text}</span>
                    <small>provenance {record.provenanceId}</small>
                  </li>
                ))}
              </ul>
            )}
          </details>
          <details>
            <summary>Provenance · {value.provenance.length}</summary>
            {value.provenance.length === 0 ? (
              <p>No provenance rows.</p>
            ) : (
              <ul>
                {value.provenance.map((record) => (
                  <li key={record.id}>
                    <b>{record.title}</b>
                    <span>
                      {record.kind} ·{" "}
                      {record.reviewStatus ?? "review state absent"}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </details>
          <details>
            <summary>Delivered memory · {value.deliveredMemory.length}</summary>
            {value.deliveredMemory.length === 0 ? (
              <p>No delivered turns in the native memory scope.</p>
            ) : (
              <ol>
                {value.deliveredMemory.map((turn) => (
                  <li key={turn.turnId}>
                    <b>{turn.speaker}</b>
                    <span>{turn.deliveredText}</span>
                    <small>
                      Source {turn.provenanceSourceKind} ·{" "}
                      {turn.provenanceSourceId ??
                        turn.provenanceSourceUri ??
                        "source identifier unavailable"}
                    </small>
                    <small>
                      Stored record {turn.turnId} · receipt{" "}
                      {turn.deliveryReceiptId ?? "unavailable"}
                    </small>
                  </li>
                ))}
              </ol>
            )}
          </details>
          <CharacterMemoryLifecycle
            nativeAvailable={nativeAvailable}
            gameProfileId={value.gameProfileId}
            characterId={value.character.id}
          />
        </div>
      )}
      <p className="inline-status" role="status">
        {notice}
      </p>
    </section>
  );
}

function CharacterMemoryLifecycle({
  nativeAvailable,
  gameProfileId,
  characterId,
}: {
  nativeAvailable: boolean;
  gameProfileId: string;
  characterId: string;
}) {
  const [status, setStatus] = useState<NativeCharacterMemoryStatus | null>(
    null,
  );
  const [backups, setBackups] = useState<NativeLocalMemoryBackup[]>([]);
  const [eraseResult, setEraseResult] =
    useState<NativeCharacterMemoryEraseResult | null>(null);
  const [restoreResult, setRestoreResult] =
    useState<NativeCharacterMemoryRestoreResult | null>(null);
  const [removeAllResult, setRemoveAllResult] =
    useState<NativeRemoveAllLocalMemoryResult | null>(null);
  const [busy, setBusy] = useState<
    "load" | "backup" | "erase" | "delete" | "restore" | "removeAll" | null
  >(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [backupArmed, setBackupArmed] = useState(false);
  const [eraseArmed, setEraseArmed] = useState(false);
  const [deleteArmed, setDeleteArmed] = useState<string | null>(null);
  const [restoreArmed, setRestoreArmed] = useState<string | null>(null);
  const [removeAllArmed, setRemoveAllArmed] = useState(false);
  const [backupBeforeErasure, setBackupBeforeErasure] = useState(true);
  const [includeBackupsInRemoval, setIncludeBackupsInRemoval] = useState(false);

  const load = useCallback(async () => {
    if (!nativeAvailable) return;
    setBusy("load");
    setError(null);
    try {
      const [nextStatus, nextBackups] = await Promise.all([
        readCharacterMemoryStatus(gameProfileId, characterId),
        listLocalMemoryBackups(),
      ]);
      if (!nextStatus || !nextBackups)
        throw new Error("Native memory lifecycle returned incomplete state.");
      setStatus(nextStatus);
      setBackups(nextBackups);
    } catch (cause) {
      setStatus(null);
      setBackups([]);
      setError(errorText(cause, "Native memory lifecycle could not load."));
    } finally {
      setBusy(null);
    }
  }, [characterId, gameProfileId, nativeAvailable]);

  useEffect(() => {
    void load();
  }, [load]);

  const createBackup = async () => {
    if (!backupArmed) {
      setBackupArmed(true);
      setNotice(
        "Confirm whole-store backup. It contains all local character memory, not only the selected character, and stays on this PC.",
      );
      return;
    }
    setBusy("backup");
    setError(null);
    try {
      const result = await backupAllLocalMemory();
      if (!result) throw new Error("Native memory backup returned no receipt.");
      setBackupArmed(false);
      setNotice(
        `Whole-store local backup ${result.backupId} created · ${formatPackBytes(result.bytes)} · ${result.pagesCopied ?? "unreported"} pages copied.`,
      );
      await load();
    } catch (cause) {
      setError(errorText(cause, "Local memory backup failed."));
    } finally {
      setBusy(null);
    }
  };

  const erase = async () => {
    if (!eraseArmed) {
      setEraseArmed(true);
      setNotice(
        backupBeforeErasure
          ? "Confirm character-memory erasure with a whole-store backup first. That backup will still contain the history erased from the live store."
          : "Confirm irreversible erasure without creating a backup. Only the selected game/character scope is requested; cross-game widening stays blocked.",
      );
      return;
    }
    setBusy("erase");
    setError(null);
    try {
      const result = await eraseCharacterMemory(
        gameProfileId,
        characterId,
        backupBeforeErasure,
      );
      if (!result)
        throw new Error("Native character erasure returned no receipt.");
      setEraseResult(result);
      setEraseArmed(false);
      setNotice(
        `Erasure ${result.erasureId} completed for ${result.gameProfileId}/${result.characterId}.`,
      );
      await load();
    } catch (cause) {
      setError(errorText(cause, "Character memory erasure failed."));
    } finally {
      setBusy(null);
    }
  };

  const removeBackup = async (backupId: string) => {
    if (deleteArmed !== backupId) {
      setDeleteArmed(backupId);
      setNotice(
        `Confirm permanent deletion of local backup ${backupId}. The UI cannot recover it afterward.`,
      );
      return;
    }
    setBusy("delete");
    setError(null);
    try {
      const result = await deleteLocalMemoryBackup(backupId);
      if (!result?.deleted)
        throw new Error("Native backup deletion was not confirmed.");
      setDeleteArmed(null);
      setNotice(`Local backup ${result.backupId} was permanently deleted.`);
      await load();
    } catch (cause) {
      setError(errorText(cause, "Local backup deletion failed."));
    } finally {
      setBusy(null);
    }
  };

  const restoreBackup = async (backupId: string) => {
    if (restoreArmed !== backupId) {
      setRestoreArmed(backupId);
      setNotice(
        `Confirm restore from ${backupId}. It replaces the current live memory store, quarantines the prior store, preserves the backup, and restarts the runtime lazily on next use.`,
      );
      return;
    }
    setBusy("restore");
    setError(null);
    try {
      const result = await restoreLocalMemoryBackup(backupId);
      if (!result?.restored)
        throw new Error("Native memory restore was not confirmed.");
      setRestoreResult(result);
      setRestoreArmed(null);
      setNotice(
        `Restore ${result.operationId} completed from ${result.backupId}. Native integrity was rechecked; the backup remains available.`,
      );
      await load();
    } catch (cause) {
      setError(errorText(cause, "Local memory restore failed."));
    } finally {
      setBusy(null);
    }
  };

  const removeAll = async () => {
    if (!removeAllArmed) {
      setRemoveAllArmed(true);
      setNotice(
        includeBackupsInRemoval
          ? "Confirm complete local-memory removal including every backup. This is irreversible; the native runtime will restart lazily with an empty store."
          : "Confirm removal of the live store and quarantines. Existing backups are deliberately preserved and still contain local memory.",
      );
      return;
    }
    setBusy("removeAll");
    setError(null);
    try {
      const result = await removeAllLocalMemory(includeBackupsInRemoval);
      if (!result?.emptyStoreReopened || !result.reopenedIntegrityOk)
        throw new Error(
          "Native all-memory removal did not prove a healthy empty store.",
        );
      setRemoveAllResult(result);
      setRemoveAllArmed(false);
      setNotice(
        `Local-memory removal ${result.operationId} completed · ${result.removedArtifacts} artifacts · ${formatPackBytes(result.removedBytes)}.`,
      );
      await load();
    } catch (cause) {
      setError(errorText(cause, "Complete local-memory removal failed."));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section
      className="character-memory-lifecycle"
      aria-label="Local memory lifecycle"
    >
      <header>
        <div>
          <span className="eyebrow">Native-owned SQLite lifecycle</span>
          <h3>Local memory backup & erasure</h3>
        </div>
        <span className={status?.integrityOk ? "badge good" : "badge wait"}>
          {nativeAvailable
            ? status
              ? status.integrityOk
                ? "Integrity OK"
                : "Integrity warning"
              : "Loading"
            : "Native required"}
        </span>
      </header>
      <p className="source-disclosure">
        Native authority owns the database path, local principal, session, and
        integrity checks. The WebView submits only the selected game/character
        IDs and explicit confirmations; paths and raw database data never enter
        this control.
      </p>
      {!nativeAvailable && (
        <div className="empty-state">
          <b>No browser memory actions</b>
          <p>
            Backup, listing, deletion, and erasure require the installed app.
          </p>
        </div>
      )}
      {nativeAvailable && busy === "load" && !status && (
        <div className="empty-state" aria-live="polite">
          <b>Loading native memory status…</b>
        </div>
      )}
      {status && (
        <>
          <dl className="memory-status-grid">
            <div>
              <dt>Selected scope</dt>
              <dd>
                {status.gameProfileId} / {status.characterId}
              </dd>
            </div>
            <div>
              <dt>Delivered / structured / legacy</dt>
              <dd>
                {status.deliveredTurns} / {status.structuredMemories} /{" "}
                {status.legacyItems}
              </dd>
            </div>
            <div>
              <dt>Native store</dt>
              <dd>
                schema {status.storeSchemaVersion} ·{" "}
                {formatPackBytes(status.databaseBytes)}
              </dd>
            </div>
            <div>
              <dt>Authority boundary</dt>
              <dd>
                {status.crossGameWideningAllowed
                  ? "Cross-game widening allowed"
                  : "Cross-game widening blocked"}
              </dd>
            </div>
          </dl>
          {status.integrityMessages.length > 0 && (
            <ul className="memory-integrity-messages">
              {status.integrityMessages.map((message, index) => (
                <li key={`${index}-${message}`}>{message}</li>
              ))}
            </ul>
          )}
          <div className="memory-lifecycle-actions">
            <button
              className={backupArmed ? "quiet-button danger" : "quiet-button"}
              disabled={busy !== null}
              onClick={() => void createBackup()}
            >
              {busy === "backup"
                ? "Backing up…"
                : backupArmed
                  ? "Confirm whole-store backup"
                  : "Back up all local memory"}
            </button>
            <label>
              <input
                type="checkbox"
                checked={backupBeforeErasure}
                disabled={busy !== null || eraseArmed}
                onChange={(event) =>
                  setBackupBeforeErasure(event.target.checked)
                }
              />
              Back up the whole local store before erasing this character
            </label>
            <button
              className="quiet-button danger"
              disabled={busy !== null || !status.hasMemory}
              title={
                !status.hasMemory
                  ? "This character has no local memory to erase."
                  : undefined
              }
              onClick={() => void erase()}
            >
              {busy === "erase"
                ? "Erasing…"
                : eraseArmed
                  ? "Confirm irreversible character erasure"
                  : "Erase this character’s memory"}
            </button>
            <button
              className="quiet-button"
              disabled={busy !== null}
              onClick={() => void load()}
            >
              Refresh memory state
            </button>
          </div>
        </>
      )}

      <div className="memory-backup-list" aria-label="Local memory backups">
        <h4>Whole-store local backups · {backups.length}</h4>
        {backups.length === 0 ? (
          <p>No native local backups reported.</p>
        ) : (
          backups.map((backup) => (
            <article key={backup.backupId}>
              <div>
                <b>{backup.backupId}</b>
                <small>
                  {formatPackBytes(backup.bytes)} · all local memory · local
                  only
                </small>
              </div>
              <button
                className="quiet-button"
                disabled={busy !== null}
                onClick={() => void restoreBackup(backup.backupId)}
              >
                {busy === "restore" && restoreArmed === backup.backupId
                  ? "Restoring…"
                  : restoreArmed === backup.backupId
                    ? "Confirm replace live memory"
                    : "Restore backup"}
              </button>
              <button
                className="quiet-button danger"
                disabled={busy !== null}
                onClick={() => void removeBackup(backup.backupId)}
              >
                {deleteArmed === backup.backupId
                  ? "Confirm permanent deletion"
                  : "Delete backup"}
              </button>
            </article>
          ))
        )}
      </div>

      {status && (
        <section
          className="remove-all-memory-panel"
          aria-label="Complete local memory removal"
        >
          <div>
            <b>Remove local memory</b>
            <p>
              Removes the live store and native quarantines. Backups remain by
              default and must be explicitly included for complete local-data
              removal.
            </p>
          </div>
          <label>
            <input
              type="checkbox"
              checked={includeBackupsInRemoval}
              disabled={busy !== null || removeAllArmed}
              onChange={(event) =>
                setIncludeBackupsInRemoval(event.target.checked)
              }
            />
            Also permanently remove every local memory backup
          </label>
          <button
            className="quiet-button danger"
            disabled={busy !== null}
            onClick={() => void removeAll()}
          >
            {busy === "removeAll"
              ? "Removing…"
              : removeAllArmed
                ? includeBackupsInRemoval
                  ? "Confirm complete removal including backups"
                  : "Confirm removal while preserving backups"
                : includeBackupsInRemoval
                  ? "Remove all local memory and backups"
                  : "Remove live local memory"}
          </button>
        </section>
      )}

      {eraseResult && (
        <div className="memory-erasure-receipt">
          <b>Native erasure receipt · {eraseResult.erasureId}</b>
          <span>
            {eraseResult.deliveredTurnsDeleted} delivered turns ·{" "}
            {eraseResult.structuredMemoriesDeleted} structured ·{" "}
            {eraseResult.legacyItemsDeleted} legacy ·{" "}
            {eraseResult.outboxJobsDeleted} outbox jobs
          </span>
          <small>
            scope sha256 {eraseResult.scopeSha256} · cross-game widening{" "}
            {eraseResult.crossGameWideningAllowed ? "allowed" : "blocked"}
          </small>
          {eraseResult.backupRetainsErasedData && (
            <strong role="alert">
              The pre-erasure backup still contains the history deleted from the
              live store. Delete that backup separately if it must also be
              removed.
            </strong>
          )}
        </div>
      )}
      {restoreResult && (
        <div className="memory-erasure-receipt">
          <b>Native restore receipt · {restoreResult.operationId}</b>
          <span>
            {restoreResult.deliveredTurns} delivered turns ·{" "}
            {restoreResult.structuredMemories} structured memories · schema{" "}
            {restoreResult.storeSchemaVersion}
          </span>
          <small>
            Prior store quarantined{" "}
            {restoreResult.priorStoreQuarantined ? "yes" : "no"} · integrity{" "}
            {restoreResult.reopenedIntegrityOk ? "ok" : "failed"} · backup
            preserved {restoreResult.backupsPreserved ? "yes" : "no"} · audit
            persisted {restoreResult.auditPersisted ? "yes" : "no"}
          </small>
          <strong>
            The restored backup remains on disk, and the prior live store is
            quarantined. Runtime restarts on next use:{" "}
            {restoreResult.runtimeRestartsOnNextUse ? "yes" : "no"}.
          </strong>
        </div>
      )}
      {removeAllResult && (
        <div className="memory-erasure-receipt">
          <b>All-memory removal receipt · {removeAllResult.operationId}</b>
          <span>
            {removeAllResult.removedArtifacts} artifacts ·{" "}
            {formatPackBytes(removeAllResult.removedBytes)}
          </span>
          <small>
            Empty store reopened{" "}
            {removeAllResult.emptyStoreReopened ? "yes" : "no"} · integrity{" "}
            {removeAllResult.reopenedIntegrityOk ? "ok" : "failed"} · schema{" "}
            {removeAllResult.storeSchemaVersion} · audit persisted{" "}
            {removeAllResult.auditPersisted ? "yes" : "no"}
          </small>
          <strong>
            {removeAllResult.backupsPreserved
              ? "Local backups were preserved and still contain memory. Use complete removal including backups if those copies must also be deleted."
              : removeAllResult.backupsRemoved
                ? "All local backups were removed with the live store."
                : "Backup removal state was not proven."}
          </strong>
        </div>
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

export function EncounterLifecycleControls({
  nativeAvailable,
  encounter,
  authoredCharacter,
}: {
  nativeAvailable: boolean;
  encounter: NativeCharacterEncounterEvidence;
  authoredCharacter: {
    gameProfileId: string;
    characterId: string;
    displayName: string;
  } | null;
}) {
  const [confirmed, setConfirmed] = useState(false);
  const [destinationEncounterId, setDestinationEncounterId] = useState("");
  const [busy, setBusy] = useState<"correct" | "merge" | null>(null);
  const [result, setResult] = useState<NativeEncounterMutationResult | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const active = encounter.status === "active";
  const correctionReady =
    nativeAvailable &&
    active &&
    confirmed &&
    authoredCharacter?.gameProfileId === encounter.game_profile_id;
  const destination = destinationEncounterId.trim();
  const mergeReady =
    nativeAvailable &&
    active &&
    confirmed &&
    destination.length > 0 &&
    destination !== encounter.encounter_id;

  const correct = async () => {
    if (!correctionReady || !authoredCharacter) return;
    setBusy("correct");
    setError(null);
    try {
      const value = await correctEncounterToAuthoredCharacter({
        gameProfileId: encounter.game_profile_id,
        encounterId: encounter.encounter_id,
        characterId: authoredCharacter.characterId,
        explicitUserConfirmation: true,
      });
      if (!value) throw new Error("Native encounter correction unavailable.");
      setResult(value);
    } catch (reason) {
      setError(errorText(reason, "Encounter correction failed."));
    } finally {
      setBusy(null);
    }
  };

  const merge = async () => {
    if (!mergeReady) return;
    setBusy("merge");
    setError(null);
    try {
      const value = await mergeUnknownEncounters({
        gameProfileId: encounter.game_profile_id,
        sourceEncounterId: encounter.encounter_id,
        destinationEncounterId: destination,
        explicitUserConfirmation: true,
      });
      if (!value) throw new Error("Native encounter merge unavailable.");
      setResult(value);
    } catch (reason) {
      setError(errorText(reason, "Encounter merge failed."));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section
      className="encounter-lifecycle"
      aria-labelledby="encounter-lifecycle-title"
    >
      <div className="panel-title">
        <div>
          <span className="eyebrow">Manual native identity repair</span>
          <h3 id="encounter-lifecycle-title">Encounter lifecycle</h3>
        </div>
        <span className={active ? "badge wait" : "badge bad"}>
          {encounter.status}
        </span>
      </div>
      <p>
        Encounter {encounter.encounter_id} is native runtime evidence for{" "}
        {encounter.game_profile_id}. These actions update only the trusted
        encounter lifecycle; memory remains in its original authority scope.
      </p>
      <label className="authorization-control">
        <input
          type="checkbox"
          checked={confirmed}
          disabled={!nativeAvailable || !active || busy !== null}
          onChange={(event) => setConfirmed(event.target.checked)}
        />
        <span>
          <b>I explicitly confirm this manual identity correction</b>
          <small>
            This does not migrate delivered memory and does not submit face or
            identity evidence from the WebView.
          </small>
        </span>
      </label>
      <div className="encounter-actions">
        <div>
          <b>Correct to selected authored character</b>
          <small>
            {authoredCharacter
              ? `${authoredCharacter.displayName} · ${authoredCharacter.characterId}`
              : "Select an authored character for this game in World first."}
          </small>
          <button
            className="secondary-action"
            disabled={!correctionReady || busy !== null}
            onClick={() => void correct()}
          >
            {busy === "correct"
              ? "Correcting…"
              : "Correct encounter to authored character"}
          </button>
        </div>
        <div>
          <label>
            <span>Destination encounter ID</span>
            <input
              value={destinationEncounterId}
              disabled={!nativeAvailable || !active || busy !== null}
              placeholder="Existing active encounter UUID"
              onChange={(event) =>
                setDestinationEncounterId(event.target.value)
              }
            />
          </label>
          <button
            className="secondary-action"
            disabled={!mergeReady || busy !== null}
            onClick={() => void merge()}
          >
            {busy === "merge" ? "Merging…" : "Merge into destination"}
          </button>
        </div>
      </div>
      {!nativeAvailable || !active ? (
        <p className="control-reason">
          {!nativeAvailable
            ? "Requires an authenticated native runtime encounter."
            : "Expired encounters cannot be corrected or merged."}
        </p>
      ) : null}
      {result && (
        <div className="encounter-receipt" role="status">
          <b>{result.event.kind}</b>
          <span>{result.detail}</span>
          <small>
            Memory migration required: yes · performed: no · occurred{" "}
            {new Date(result.event.occurred_at_ms).toISOString()}
          </small>
        </div>
      )}
      {error && (
        <p className="inline-status error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

const bytesToGiB = (bytes: number) => bytes / 1024 ** 3;
const formatPackBytes = (bytes: number) =>
  bytes >= 1024 ** 3
    ? `${bytesToGiB(bytes).toFixed(2)} GiB`
    : `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
const gibToBytes = (gib: string) => Math.round(Number(gib) * 1024 ** 3);
const observationLabel = (observation: NativeTelemetryObservation<number>) =>
  observation.availability === "available"
    ? `${bytesToGiB(observation.value).toFixed(1)} GiB`
    : `Unavailable · ${typeof observation.reason === "string" ? observation.reason : "native API error"}`;

const humanizeBenchmarkKey = (value: string) =>
  value
    .replace(/_/g, " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());

const formatBenchmarkValue = (value: number, unit: string) => {
  if (unit === "bytes") return `${bytesToGiB(value).toFixed(2)} GiB`;
  if (unit === "percent") return `${value.toFixed(1)}%`;
  if (unit === "ms") return `${value.toFixed(1)} ms`;
  if (unit === "frames_per_second") return `${value.toFixed(1)} fps`;
  if (unit === "tokens_per_second") return `${value.toFixed(1)} tok/s`;
  if (unit === "tokens") return `${value.toFixed(1)} tokens`;
  if (unit === "count") return value.toFixed(1);
  return `${value.toFixed(2)} ${unit}`;
};

const terminalBenchmarkStates = new Set([
  "completed",
  "partial",
  "unavailable",
  "cancelled",
  "failed",
]);

function BenchmarkMetricCard({
  metric,
}: {
  metric: NativeBenchmarkMetricSummary;
}) {
  return (
    <article>
      <header>
        <div>
          <span>{humanizeBenchmarkKey(metric.metric)}</span>
          <b>
            {metric.sampleCount} sample{metric.sampleCount === 1 ? "" : "s"}
          </b>
        </div>
        <small>{metric.unit.replace(/_/g, " ")}</small>
      </header>
      <dl>
        <div>
          <dt>p50</dt>
          <dd>{formatBenchmarkValue(metric.p50, metric.unit)}</dd>
        </div>
        <div>
          <dt>p95</dt>
          <dd>{formatBenchmarkValue(metric.p95, metric.unit)}</dd>
        </div>
        <div>
          <dt>p99</dt>
          <dd>{formatBenchmarkValue(metric.p99, metric.unit)}</dd>
        </div>
      </dl>
      <small>
        range {formatBenchmarkValue(metric.minimum, metric.unit)}–
        {formatBenchmarkValue(metric.maximum, metric.unit)} · mean{" "}
        {formatBenchmarkValue(metric.mean, metric.unit)}
      </small>
    </article>
  );
}

export function ThisPcBenchmark({
  nativeAvailable = false,
}: {
  nativeAvailable?: boolean;
}) {
  const [status, setStatus] = useState<NativeBenchmarkStatus | null>(null);
  const [report, setReport] = useState<NativeBenchmarkReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"start" | "cancel" | null>(null);
  const [iterations, setIterations] = useState("10");
  const [timeoutSeconds, setTimeoutSeconds] = useState("120");
  const [baselineSeconds, setBaselineSeconds] = useState("3");

  const refreshStatus = useCallback(async () => {
    if (!nativeAvailable) return;
    try {
      const next = await readThisPcBenchmarkStatus();
      if (!next) throw new Error("Native benchmark status returned no state.");
      setStatus(next);
      setError(null);
    } catch (cause) {
      setError(errorText(cause, "This-PC benchmark status is unavailable."));
    }
  }, [nativeAvailable]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);
  useEffect(() => {
    if (status?.state !== "running" && status?.state !== "cancelling") return;
    const timer = window.setInterval(() => void refreshStatus(), 750);
    return () => window.clearInterval(timer);
  }, [refreshStatus, status?.state]);
  useEffect(() => {
    if (
      !status?.reportId ||
      !terminalBenchmarkStates.has(status.state) ||
      report?.reportId === status.reportId
    )
      return;
    let active = true;
    void readThisPcBenchmarkReport(status.reportId)
      .then((next) => {
        if (!active) return;
        if (!next)
          throw new Error("Native benchmark report returned no document.");
        setReport(next);
      })
      .catch((cause) => {
        if (active)
          setError(
            errorText(cause, "This-PC benchmark report could not be loaded."),
          );
      });
    return () => {
      active = false;
    };
  }, [report?.reportId, status?.reportId, status?.state]);

  const requestedIterations = Number(iterations);
  const timeoutMillis = Math.round(Number(timeoutSeconds) * 1000);
  const baselineWindowMillis = Math.round(Number(baselineSeconds) * 1000);
  const requestValid =
    Number.isInteger(requestedIterations) &&
    requestedIterations >= 3 &&
    requestedIterations <= 120 &&
    timeoutMillis >= 5_000 &&
    timeoutMillis <= 300_000 &&
    baselineWindowMillis >= 500 &&
    baselineWindowMillis <= 30_000 &&
    baselineWindowMillis < timeoutMillis;
  const active = status?.state === "running" || status?.state === "cancelling";

  const start = async () => {
    if (!nativeAvailable || !requestValid) return;
    setBusy("start");
    setError(null);
    setReport(null);
    try {
      const next = await startThisPcBenchmark({
        requestedIterations,
        timeoutMillis,
        baselineWindowMillis,
      });
      if (!next) throw new Error("Native benchmark start returned no state.");
      setStatus(next);
    } catch (cause) {
      setError(errorText(cause, "This-PC benchmark could not start."));
    } finally {
      setBusy(null);
    }
  };
  const cancel = async () => {
    setBusy("cancel");
    setError(null);
    try {
      const next = await cancelThisPcBenchmark();
      if (!next)
        throw new Error("Native benchmark cancellation returned no state.");
      setStatus(next);
    } catch (cause) {
      setError(errorText(cause, "This-PC benchmark could not be cancelled."));
    } finally {
      setBusy(null);
    }
  };

  const statusLabel = !nativeAvailable
    ? "Native unavailable"
    : status?.state
      ? humanizeBenchmarkKey(status.state)
      : "Loading";
  const progressMaximum = Math.max(
    status?.requestedIterations ?? requestedIterations,
    1,
  );
  const progressValue = Math.min(
    status?.completedIterations ?? 0,
    progressMaximum,
  );

  return (
    <section
      className="this-pc-benchmark instrument-panel"
      aria-labelledby="this-pc-benchmark-title"
    >
      <div className="panel-title">
        <div>
          <span className="eyebrow">Native receipt-backed measurement</span>
          <h2 id="this-pc-benchmark-title">This PC benchmark</h2>
        </div>
        <span
          className={
            status?.state === "completed"
              ? "badge good"
              : status?.state === "failed" || status?.state === "unavailable"
                ? "badge bad"
                : "badge wait"
          }
        >
          {statusLabel}
        </span>
      </div>
      <p className="source-disclosure">
        {nativeAvailable
          ? "The native shell binds this run to the current exact game target and resolved provider loadout. Missing receipt producers remain unavailable; the control app never substitutes fixture or browser timings."
          : "Browser preview cannot run or simulate this benchmark. Open the native Windows shell, select an exact game target, and use a qualified provider loadout."}
      </p>
      <div className="benchmark-controls">
        <label>
          <span>Iterations</span>
          <input
            aria-label="Benchmark iterations"
            type="number"
            min="3"
            max="120"
            step="1"
            value={iterations}
            disabled={!nativeAvailable || active}
            onChange={(event) => setIterations(event.target.value)}
          />
          <small>3–120</small>
        </label>
        <label>
          <span>Timeout</span>
          <div>
            <input
              aria-label="Benchmark timeout in seconds"
              type="number"
              min="5"
              max="300"
              step="1"
              value={timeoutSeconds}
              disabled={!nativeAvailable || active}
              onChange={(event) => setTimeoutSeconds(event.target.value)}
            />
            <i>sec</i>
          </div>
          <small>5–300 seconds</small>
        </label>
        <label>
          <span>Idle baseline</span>
          <div>
            <input
              aria-label="Benchmark baseline in seconds"
              type="number"
              min="0.5"
              max="30"
              step="0.5"
              value={baselineSeconds}
              disabled={!nativeAvailable || active}
              onChange={(event) => setBaselineSeconds(event.target.value)}
            />
            <i>sec</i>
          </div>
          <small>0.5–30; below timeout</small>
        </label>
        <div className="benchmark-actions">
          <button
            className="primary-action"
            disabled={
              !nativeAvailable || active || busy !== null || !requestValid
            }
            onClick={() => void start()}
          >
            {busy === "start" ? "Starting…" : "Start native benchmark"}
          </button>
          <button
            className="quiet-button danger"
            disabled={
              !nativeAvailable ||
              !active ||
              status?.state === "cancelling" ||
              busy !== null
            }
            onClick={() => void cancel()}
          >
            {busy === "cancel" ? "Cancelling…" : "Cancel run"}
          </button>
        </div>
      </div>
      {!requestValid && nativeAvailable && (
        <p className="inline-status error" role="alert">
          Use 3–120 iterations, a 5–300 second timeout, and a 0.5–30 second
          baseline shorter than the timeout.
        </p>
      )}
      {status && status.state !== "idle" && (
        <div className="benchmark-progress" aria-live="polite">
          <div>
            <b>
              {status.completedIterations} / {status.requestedIterations}{" "}
              iterations
            </b>
            <span>
              {(status.elapsedMillis / 1000).toFixed(1)} seconds elapsed
            </span>
          </div>
          <progress max={progressMaximum} value={progressValue}>
            {progressValue} of {progressMaximum}
          </progress>
          <small>
            {status.startedAtUtc
              ? `Started ${status.startedAtUtc}`
              : "Start timestamp unavailable"}
            {status.reportFileName ? ` · ${status.reportFileName}` : ""}
          </small>
        </div>
      )}
      {status &&
        (status.unavailableComponents.length > 0 ||
          status.actionCodes.length > 0) && (
          <div className="benchmark-unavailable-summary">
            <div>
              <b>Unavailable components</b>
              {status.unavailableComponents.length ? (
                <ul>
                  {status.unavailableComponents.map((value) => (
                    <li key={value}>{humanizeBenchmarkKey(value)}</li>
                  ))}
                </ul>
              ) : (
                <p>None reported.</p>
              )}
            </div>
            <div>
              <b>Native next actions</b>
              {status.actionCodes.length ? (
                <ul>
                  {status.actionCodes.map((value) => (
                    <li key={value}>{humanizeBenchmarkKey(value)}</li>
                  ))}
                </ul>
              ) : (
                <p>None reported.</p>
              )}
            </div>
          </div>
        )}
      {!nativeAvailable && (
        <div className="empty-state">
          <b>No browser benchmark data</b>
          <p>
            There are no illustrative percentile, resource, or frame-impact
            values.
          </p>
        </div>
      )}
      {nativeAvailable && !status && !error && (
        <div className="empty-state">
          <b>Loading native benchmark status…</b>
        </div>
      )}
      {error && (
        <p className="inline-status error" role="alert">
          {error}
        </p>
      )}
      {report && (
        <div className="benchmark-report">
          <header>
            <div>
              <span className="eyebrow">
                Timestamped native report · {report.generatedAtUtc}
              </span>
              <h3>{report.reportId}</h3>
              <p>{report.classification.reason}</p>
            </div>
            <span
              className={
                report.classification.acceptanceEligible
                  ? "badge good"
                  : "badge wait"
              }
            >
              {report.classification.acceptanceEligible
                ? "Review-eligible evidence"
                : "Not review-eligible"}
            </span>
          </header>
          <p className="source-disclosure">
            Review eligibility means the evidence may be reviewed; it is not an
            acceptance decision. Classification:{" "}
            {report.classification.executionMode} ·{" "}
            {report.classification.measurementKind.replace(/_/g, " ")} ·{" "}
            {report.state}.
          </p>
          <dl className="benchmark-facts">
            <div>
              <dt>Bound game/loadout</dt>
              <dd>
                {report.binding.gameProfileId} ·{" "}
                {report.binding.loadoutRevision}
              </dd>
            </div>
            <div>
              <dt>Completed bounds</dt>
              <dd>
                {report.bounds.completedIterations}/
                {report.bounds.requestedIterations} ·{" "}
                {(report.bounds.elapsedMillis / 1000).toFixed(1)} sec ·{" "}
                {report.bounds.completedWithinBounds
                  ? "within timeout"
                  : "outside timeout"}
              </dd>
            </div>
            <div>
              <dt>Hardware</dt>
              <dd>
                {report.hardware.adapterDescription ?? "Adapter unavailable"} ·{" "}
                {report.hardware.operatingSystem} {report.hardware.architecture}
              </dd>
            </div>
            <div>
              <dt>Provenance</dt>
              <dd>
                {report.provenance.timingClock} · runtime{" "}
                {report.provenance.runtimeRevision}
              </dd>
            </div>
            <div>
              <dt>Persistence</dt>
              <dd>
                {report.persistence.state} ·{" "}
                {report.persistence.reportFileName ?? "No report file"} ·{" "}
                {report.persistence.atomicWrite
                  ? "atomic write"
                  : "atomic write not proven"}
              </dd>
            </div>
            <div>
              <dt>Receipt quality</dt>
              <dd>
                {report.coverage.invalidReceiptCount} invalid ·{" "}
                {report.coverage.failedIterationCount} failed iterations
              </dd>
            </div>
          </dl>
          <div
            className="benchmark-coverage"
            aria-label="Benchmark component coverage"
          >
            {report.coverage.components.map((component) => (
              <article key={component.component}>
                <span
                  className={
                    component.availability === "ready"
                      ? "status-dot good"
                      : "status-dot"
                  }
                />
                <div>
                  <b>{humanizeBenchmarkKey(component.component)}</b>
                  <small>
                    {component.availability === "ready"
                      ? "Receipt source ready"
                      : `${humanizeBenchmarkKey(component.reason ?? "unavailable")}${component.action ? ` · ${humanizeBenchmarkKey(component.action)}` : ""}`}
                  </small>
                </div>
              </article>
            ))}
          </div>
          <div
            className="benchmark-metrics"
            aria-label="Benchmark percentile metrics"
          >
            {report.metrics.length ? (
              report.metrics.map((metric) => (
                <BenchmarkMetricCard key={metric.metric} metric={metric} />
              ))
            ) : (
              <div className="empty-state">
                <b>No measured percentile metrics</b>
                <p>
                  The native report did not include a qualifying receipt-backed
                  sample.
                </p>
              </div>
            )}
          </div>
          <section
            className="frame-impact-summary"
            aria-label="Game frame impact"
          >
            <h4>Game frame impact</h4>
            <dl>
              <div>
                <dt>Baseline frame p50</dt>
                <dd>
                  {report.frameImpact.baselineFrameTimeMs
                    ? formatBenchmarkValue(
                        report.frameImpact.baselineFrameTimeMs.p50,
                        report.frameImpact.baselineFrameTimeMs.unit,
                      )
                    : "Unavailable"}
                </dd>
              </div>
              <div>
                <dt>Active frame p50</dt>
                <dd>
                  {report.frameImpact.activeFrameTimeMs
                    ? formatBenchmarkValue(
                        report.frameImpact.activeFrameTimeMs.p50,
                        report.frameImpact.activeFrameTimeMs.unit,
                      )
                    : "Unavailable"}
                </dd>
              </div>
              <div>
                <dt>Frame-time p95 delta</dt>
                <dd>
                  {report.frameImpact.p95FrameTimeDeltaMs === null
                    ? "Unavailable"
                    : `${report.frameImpact.p95FrameTimeDeltaMs.toFixed(1)} ms`}
                </dd>
              </div>
              <div>
                <dt>FPS p50 impact</dt>
                <dd>
                  {report.frameImpact.p50FpsImpactPercent === null
                    ? "Unavailable"
                    : `${report.frameImpact.p50FpsImpactPercent.toFixed(1)}%`}
                </dd>
              </div>
            </dl>
          </section>
        </div>
      )}
    </section>
  );
}

export function LocalResourcePlanner({
  models,
  nativeAvailable = false,
}: {
  models: NativeModelSummary[];
  nativeAvailable?: boolean;
}) {
  const [settings, setSettings] = useState<NativeLocalResourceSettings | null>(
    null,
  );
  const [telemetry, setTelemetry] =
    useState<NativeResourceTelemetryResult | null>(null);
  const [pack, setPack] = useState<NativeExperimentalPackState | null>(null);
  const [loadoutPlanner, setLoadoutPlanner] =
    useState<NativeSelectedLoadoutPlannerResult | null>(null);
  const [loadoutAdmission, setLoadoutAdmission] =
    useState<NativeSelectedLoadoutAdmissionResult | null>(null);
  const [trustedCatalog, setTrustedCatalog] =
    useState<NativeTrustedLocalPackCatalog | null>(null);
  const [optionalLifecycle, setOptionalLifecycle] =
    useState<NativeTrustedOptionalPackLifecycle | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "error" | "browser">(
    nativeAvailable ? "loading" : "browser",
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [gameReserve, setGameReserve] = useState("");
  const [ramReserve, setRamReserve] = useState("");
  const [vramCeilingPercent, setVramCeilingPercent] = useState("");
  const [ramCeilingPercent, setRamCeilingPercent] = useState("");
  const [keepWarmSeconds, setKeepWarmSeconds] = useState("");
  const [unloadSeconds, setUnloadSeconds] = useState("");
  const [optionalConsent, setOptionalConsent] = useState<string | null>(null);
  const [optionalLicenseAccepted, setOptionalLicenseAccepted] = useState(false);
  const [optionalCancelReceipt, setOptionalCancelReceipt] = useState<{
    key: string;
    detail: string;
  } | null>(null);

  const refresh = useCallback(async () => {
    if (!nativeAvailable) {
      setState("browser");
      return;
    }
    setState("loading");
    setError(null);
    try {
      const [
        nextSettings,
        nextTelemetry,
        nextPack,
        nextLoadoutPlanner,
        nextTrustedCatalog,
        nextOptionalLifecycle,
      ] = await Promise.all([
        readLocalResourceSettings(),
        readLocalResourceTelemetry(),
        readExperimentalVisualPackStatus(),
        readSelectedLocalLoadoutPlanner(),
        readTrustedLocalPackCatalog(),
        readTrustedOptionalPackLifecycle(),
      ]);
      if (
        !nextSettings ||
        !nextTelemetry ||
        !nextPack ||
        !nextLoadoutPlanner ||
        !nextTrustedCatalog ||
        !nextOptionalLifecycle
      )
        throw new Error(
          "Native local-resource commands returned incomplete state.",
        );
      setSettings(nextSettings);
      setTelemetry(nextTelemetry);
      setPack(nextPack);
      setLoadoutPlanner(nextLoadoutPlanner);
      setTrustedCatalog(nextTrustedCatalog);
      setOptionalLifecycle(nextOptionalLifecycle);
      setGameReserve(bytesToGiB(nextSettings.gameReserveVramBytes).toFixed(1));
      setRamReserve(
        bytesToGiB(nextSettings.gameAdditionalReserveRamBytes).toFixed(1),
      );
      setVramCeilingPercent(
        (nextSettings.governor.vram_soft_ceiling_basis_points / 100).toFixed(0),
      );
      setRamCeilingPercent(
        (nextSettings.governor.ram_soft_ceiling_basis_points / 100).toFixed(0),
      );
      setKeepWarmSeconds(
        (nextSettings.governor.keep_warm_millis / 1000).toFixed(0),
      );
      setUnloadSeconds(
        (nextSettings.governor.unload_ttl_millis / 1000).toFixed(0),
      );
      setState("ready");
    } catch (cause) {
      setState("error");
      setError(errorText(cause, "Native local-resource state is unavailable."));
    }
  }, [nativeAvailable]);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = async () => {
    if (
      !settings ||
      !Number.isFinite(Number(gameReserve)) ||
      !Number.isFinite(Number(ramReserve)) ||
      !Number.isFinite(Number(vramCeilingPercent)) ||
      !Number.isFinite(Number(ramCeilingPercent)) ||
      !Number.isFinite(Number(keepWarmSeconds)) ||
      !Number.isFinite(Number(unloadSeconds))
    )
      return;
    setBusy("save");
    try {
      const next = await saveLocalResourceSettings({
        ...settings,
        governor: {
          ...settings.governor,
          vram_soft_ceiling_basis_points: Math.round(
            Number(vramCeilingPercent) * 100,
          ),
          ram_soft_ceiling_basis_points: Math.round(
            Number(ramCeilingPercent) * 100,
          ),
          keep_warm_millis: Math.round(Number(keepWarmSeconds) * 1000),
          unload_ttl_millis: Math.round(Number(unloadSeconds) * 1000),
        },
        gameReserveVramBytes: gibToBytes(gameReserve),
        gameAdditionalReserveRamBytes: gibToBytes(ramReserve),
      });
      if (!next) throw new Error("Native settings save returned no state.");
      setSettings(next);
      await refresh();
    } catch (cause) {
      setError(errorText(cause, "Native settings could not be saved."));
    } finally {
      setBusy(null);
    }
  };
  const mutate = async (
    action: "install" | "activate" | "repair" | "remove",
  ) => {
    if (!pack || !confirmed) return;
    setBusy(action);
    setError(null);
    try {
      const next = await mutateExperimentalVisualPack(action, pack);
      if (!next) throw new Error("Native pack command returned no state.");
      setPack(next);
    } catch (cause) {
      setError(errorText(cause, `${action} did not complete.`));
    } finally {
      setBusy(null);
    }
  };
  const mutateOptional = async (
    action: "install" | "repair" | "remove",
    identity: { pack_id: string; revision: string },
  ) => {
    const key = `${identity.pack_id}@${identity.revision}`;
    if (optionalConsent !== key) return;
    setBusy(`optional-${action}-${key}`);
    setError(null);
    try {
      const next = await mutateTrustedOptionalPack(
        action,
        identity,
        optionalLicenseAccepted,
      );
      if (!next)
        throw new Error("Native optional-pack command returned no state.");
      setOptionalLifecycle(next);
      setOptionalConsent(null);
      setOptionalLicenseAccepted(false);
    } catch (cause) {
      setError(errorText(cause, `${action} did not complete.`));
    } finally {
      setBusy(null);
    }
  };
  const cancelOptional = async (identity: {
    pack_id: string;
    revision: string;
  }) => {
    const key = `${identity.pack_id}@${identity.revision}`;
    setError(null);
    try {
      const cancelled = await cancelTrustedOptionalPackDownload(identity);
      setOptionalCancelReceipt({
        key,
        detail: cancelled
          ? `Cancellation requested for the active exact-pack transfer ${key}. Native verification and staging will not continue after cancellation acknowledgement.`
          : `No active exact-pack transfer exists for ${key}; nothing was cancelled.`,
      });
    } catch (cause) {
      setError(
        errorText(cause, "Optional-pack cancellation did not complete."),
      );
    }
  };
  const admitLoadout = async () => {
    if (!loadoutPlanner?.selected) return;
    setBusy("admit");
    setError(null);
    try {
      const next = await admitSelectedLocalLoadout(loadoutPlanner.selected);
      if (!next)
        throw new Error("Native selected-loadout admission returned no state.");
      setLoadoutAdmission(next);
      await refresh();
    } catch (cause) {
      setError(
        errorText(cause, "Selected local loadout could not be checked."),
      );
    } finally {
      setBusy(null);
    }
  };

  const adapter = telemetry?.snapshot.adapter;
  const resourceInputsValid =
    Number.isFinite(Number(gameReserve)) &&
    Number(gameReserve) >= 0 &&
    Number.isFinite(Number(ramReserve)) &&
    Number(ramReserve) >= 0 &&
    Number(vramCeilingPercent) > 0 &&
    Number(vramCeilingPercent) <= 100 &&
    Number(ramCeilingPercent) > 0 &&
    Number(ramCeilingPercent) <= 100 &&
    Number(keepWarmSeconds) >= 0 &&
    Number(unloadSeconds) >= Number(keepWarmSeconds);
  const inventoryCount = models.filter((model) =>
    /presence|lip|visual/i.test(`${model.purpose} ${model.displayName}`),
  ).length;
  const actionReason = !nativeAvailable
    ? "Requires the native desktop shell"
    : !confirmed
      ? "Check explicit local-review confirmation first"
      : pack?.trustDomain !== "localReviewDevOnly"
        ? "This pack is outside the local-review trust domain"
        : null;

  return (
    <section
      className="local-resource-workspace"
      aria-labelledby="local-packs-title"
    >
      <div className="resource-planner instrument-panel">
        <div className="panel-title">
          <div>
            <span className="eyebrow">Native soft-governor policy</span>
            <h2>Whole-loadout fit</h2>
          </div>
          <span
            className={telemetry?.admissionReady ? "badge good" : "badge wait"}
          >
            {state === "loading"
              ? "Loading"
              : telemetry?.admissionReady
                ? "Telemetry complete"
                : "Fail-closed"}
          </span>
        </div>
        <p className="source-disclosure">
          {nativeAvailable
            ? "Settings persist in native app data. Telemetry observations remain timestamped and unavailable values are never converted to zero."
            : "Browser preview has no hardware telemetry or persistence. No planning inputs are presented as measured device fit."}
        </p>
        {state === "loading" && (
          <div className="empty-state">
            <b>Reading native resource policy and telemetry…</b>
          </div>
        )}
        {state === "error" && (
          <div className="empty-state error">
            <b>Native resource state unavailable</b>
            <p>{error}</p>
            <button onClick={() => void refresh()}>Retry</button>
          </div>
        )}
        {state === "browser" && (
          <div className="empty-state">
            <b>Native desktop required</b>
            <p>
              Open the Windows shell to inspect detected RAM/VRAM and save game
              reserves.
            </p>
          </div>
        )}
        {settings && telemetry && (
          <>
            <div className="resource-inputs native-resource-inputs">
              <label>
                <span>Game VRAM reserve</span>
                <div>
                  <input
                    aria-label="Game VRAM reserve in GiB"
                    type="number"
                    min="0"
                    step="0.1"
                    value={gameReserve}
                    onChange={(event) => setGameReserve(event.target.value)}
                  />
                  <i>GiB</i>
                </div>
                <small>Persisted soft floor</small>
              </label>
              <label>
                <span>Additional game RAM</span>
                <div>
                  <input
                    aria-label="Additional game RAM reserve in GiB"
                    type="number"
                    min="0"
                    step="0.1"
                    value={ramReserve}
                    onChange={(event) => setRamReserve(event.target.value)}
                  />
                  <i>GiB</i>
                </div>
                <small>Persisted soft floor</small>
              </label>
              <label>
                <span>VRAM soft ceiling</span>
                <div>
                  <input
                    aria-label="VRAM soft ceiling percent"
                    type="number"
                    min="1"
                    max="100"
                    step="1"
                    value={vramCeilingPercent}
                    onChange={(event) =>
                      setVramCeilingPercent(event.target.value)
                    }
                  />
                  <i>%</i>
                </div>
                <small>Of the lower physical / OS budget</small>
              </label>
              <label>
                <span>RAM soft ceiling</span>
                <div>
                  <input
                    aria-label="RAM soft ceiling percent"
                    type="number"
                    min="1"
                    max="100"
                    step="1"
                    value={ramCeilingPercent}
                    onChange={(event) =>
                      setRamCeilingPercent(event.target.value)
                    }
                  />
                  <i>%</i>
                </div>
                <small>Still capped by currently available RAM</small>
              </label>
              <label>
                <span>Keep warm</span>
                <div>
                  <input
                    aria-label="Keep local models warm in seconds"
                    type="number"
                    min="0"
                    step="1"
                    value={keepWarmSeconds}
                    onChange={(event) => setKeepWarmSeconds(event.target.value)}
                  />
                  <i>s</i>
                </div>
                <small>Measured reload cost still decides residency</small>
              </label>
              <label>
                <span>Unload after</span>
                <div>
                  <input
                    aria-label="Unload idle local models in seconds"
                    type="number"
                    min={keepWarmSeconds || "0"}
                    step="1"
                    value={unloadSeconds}
                    onChange={(event) => setUnloadSeconds(event.target.value)}
                  />
                  <i>s</i>
                </div>
                <small>Must be at least the keep-warm period</small>
              </label>
              <label>
                <span>Residency preference</span>
                <select
                  aria-label="Preferred local model residency"
                  value={settings.preferredResidency}
                  onChange={(event) =>
                    setSettings({
                      ...settings,
                      preferredResidency: event.target
                        .value as NativeLocalResourceSettings["preferredResidency"],
                    })
                  }
                >
                  <option value="cpu_resident_gpu_cold">
                    CPU resident · GPU cold
                  </option>
                  <option value="cpu_resident">CPU resident</option>
                  <option value="gpu_resident">GPU resident</option>
                </select>
                <small>Admission may still reject it</small>
              </label>
            </div>
            <button
              className="secondary-action"
              disabled={busy !== null || !resourceInputsValid}
              onClick={() => void save()}
            >
              {busy === "save" ? "Saving…" : "Save native resource policy"}
            </button>
            <div className="telemetry-grid">
              <article>
                <span>Exact target PID</span>
                <b>{telemetry.snapshot.selected_game_pid ?? "Not selected"}</b>
                <small>Native GameTargetManager authority</small>
              </article>
              <article>
                <span>Adapter</span>
                <b>
                  {adapter?.availability === "available"
                    ? adapter.value.description
                    : "Unavailable"}
                </b>
                <small>{adapter?.provenance.source ?? "No observation"}</small>
              </article>
              <article>
                <span>Dedicated VRAM</span>
                <b>
                  {observationLabel(telemetry.snapshot.dedicated_vram_bytes)}
                </b>
              </article>
              <article>
                <span>OS local budget</span>
                <b>
                  {observationLabel(
                    telemetry.snapshot.os_local_vram_budget_bytes,
                  )}
                </b>
              </article>
              <article>
                <span>Game VRAM</span>
                <b>
                  {observationLabel(
                    telemetry.snapshot.selected_game_vram_bytes,
                  )}
                </b>
              </article>
              <article>
                <span>Game working set</span>
                <b>
                  {observationLabel(
                    telemetry.snapshot.selected_game_working_set_bytes,
                  )}
                </b>
              </article>
              <article>
                <span>Control-process VRAM</span>
                <b>
                  {observationLabel(
                    telemetry.snapshot.current_process_local_vram_bytes,
                  )}
                </b>
              </article>
              <article>
                <span>Total device pressure</span>
                <b>
                  {observationLabel(
                    telemetry.snapshot.total_device_pressure_vram_bytes,
                  )}
                </b>
              </article>
              <article>
                <span>Available RAM</span>
                <b>
                  {observationLabel(telemetry.snapshot.available_ram_bytes)}
                </b>
              </article>
            </div>
            <p className="inline-status" role="status">
              {telemetry.admissionDetail}
            </p>
            {loadoutPlanner && (
              <section
                className="selected-loadout-admission"
                aria-labelledby="selected-loadout-admission-title"
              >
                <div className="panel-title">
                  <div>
                    <span className="eyebrow">
                      Native selected-loadout authority
                    </span>
                    <h3 id="selected-loadout-admission-title">
                      Complete local loadout admission
                    </h3>
                  </div>
                  <span
                    className={
                      loadoutPlanner.ready ? "badge good" : "badge wait"
                    }
                  >
                    {loadoutPlanner.ready ? "Planner ready" : "Fail-closed"}
                  </span>
                </div>
                <p className="source-disclosure">{loadoutPlanner.detail}</p>
                {loadoutPlanner.selected ? (
                  <>
                    <div className="selected-loadout-summary">
                      <div>
                        <b>{loadoutPlanner.selected.selection_id}</b>
                        <small>
                          {loadoutPlanner.selected.roles.length} exact catalog
                          role
                          {loadoutPlanner.selected.roles.length === 1
                            ? ""
                            : "s"}
                          {" · "}idle horizon{" "}
                          {loadoutPlanner.selected.expected_idle_millis} ms
                        </small>
                      </div>
                      <span className="badge">Persisted selection</span>
                    </div>
                    <ol className="selected-loadout-roles">
                      {loadoutPlanner.selected.roles.map((role) => (
                        <li
                          key={`${role.role}-${role.identity.pack_id}-${role.identity.revision}`}
                        >
                          <span>{humanizeBenchmarkKey(role.role)}</span>
                          <b>{role.identity.pack_id}</b>
                          <small>
                            {role.identity.revision} ·{" "}
                            {humanizeBenchmarkKey(role.preferred_residency)}
                          </small>
                        </li>
                      ))}
                    </ol>
                  </>
                ) : (
                  <div className="empty-state compact">
                    <b>No native local loadout is selected</b>
                    <p>
                      The WebView cannot invent pack IDs or revisions. A signed
                      release catalog and an exact native selection must exist
                      first.
                    </p>
                  </div>
                )}
                {loadoutPlanner.planner && (
                  <dl className="benchmark-facts">
                    <div>
                      <dt>Trusted measurement streams</dt>
                      <dd>
                        {loadoutPlanner.planner.trusted_measurement_streams}
                      </dd>
                    </div>
                    <div>
                      <dt>Active measured evidence</dt>
                      <dd>{loadoutPlanner.planner.active_evidence.length}</dd>
                    </div>
                    <div>
                      <dt>Pending native work</dt>
                      <dd>{loadoutPlanner.planner.pending_work}</dd>
                    </div>
                  </dl>
                )}
                <button
                  className="secondary-action"
                  disabled={busy !== null || !loadoutPlanner.selected}
                  aria-describedby="selected-loadout-admit-reason"
                  onClick={() => void admitLoadout()}
                >
                  {busy === "admit"
                    ? "Checking exact loadout…"
                    : "Check and admit selected loadout"}
                </button>
                <p
                  id="selected-loadout-admit-reason"
                  className="control-reason"
                >
                  {loadoutPlanner.selected
                    ? "Native re-collects the exact target telemetry and verifies every signed current-device p99 envelope before persisting admission."
                    : "Disabled because there is no native-selected exact local loadout."}
                </p>
                {loadoutAdmission && (
                  <div
                    className={
                      loadoutAdmission.persisted
                        ? "loadout-admission-result admitted"
                        : "loadout-admission-result blocked"
                    }
                    role="status"
                  >
                    <b>
                      {loadoutAdmission.decision?.status ??
                        (loadoutAdmission.ready
                          ? "Decision unavailable"
                          : "Planner unavailable")}
                    </b>
                    <p>{loadoutAdmission.detail}</p>
                    {loadoutAdmission.decision?.reason_code && (
                      <code>{loadoutAdmission.decision.reason_code}</code>
                    )}
                    {loadoutAdmission.decision?.admission_receipt && (
                      <dl>
                        <div>
                          <dt>Projected VRAM</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .projected_total_vram_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                        <div>
                          <dt>VRAM ceiling</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .vram_soft_ceiling_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                        <div>
                          <dt>Desktop + protected game VRAM</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .protected_desktop_and_game_vram_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                        <div>
                          <dt>Selected resident + transient VRAM</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .selected_peak_vram_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                        <div>
                          <dt>VRAM safety margin</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .vram_safety_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                        <div>
                          <dt>Selected peak RAM</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .selected_peak_ram_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                        <div>
                          <dt>RAM ceiling</dt>
                          <dd>
                            {bytesToGiB(
                              loadoutAdmission.decision.admission_receipt
                                .ram_soft_ceiling_bytes,
                            ).toFixed(2)}{" "}
                            GiB
                          </dd>
                        </div>
                      </dl>
                    )}
                    {loadoutAdmission.decision?.residency_decisions.length ? (
                      <div className="loadout-residency-decisions">
                        <b>Measured idle policy</b>
                        <ol>
                          {loadoutAdmission.decision.residency_decisions.map(
                            (decision) => (
                              <li
                                key={`${decision.role}-${decision.identity.pack_id}-${decision.identity.revision}`}
                              >
                                <span>
                                  {humanizeBenchmarkKey(decision.role)} ·{" "}
                                  {humanizeBenchmarkKey(decision.disposition)}
                                </span>
                                <small>
                                  Expected idle {decision.expected_idle_millis}{" "}
                                  ms · measured reload p99{" "}
                                  {decision.compared_p99_reload_millis} ms ·{" "}
                                  {humanizeBenchmarkKey(
                                    decision.evidence_placement,
                                  )}
                                </small>
                              </li>
                            ),
                          )}
                        </ol>
                      </div>
                    ) : null}
                    {loadoutAdmission.decision?.pressure_cancellations
                      .length ? (
                      <p className="control-reason">
                        Pressure shed{" "}
                        {
                          loadoutAdmission.decision.pressure_cancellations
                            .length
                        }{" "}
                        queued operation
                        {loadoutAdmission.decision.pressure_cancellations
                          .length === 1
                          ? ""
                          : "s"}
                        :{" "}
                        {loadoutAdmission.decision.pressure_cancellations
                          .map(
                            (cancellation) =>
                              `${cancellation.work_id} (${humanizeBenchmarkKey(cancellation.reason)})`,
                          )
                          .join(" · ")}
                      </p>
                    ) : null}
                  </div>
                )}
              </section>
            )}
          </>
        )}
      </div>
      <section
        className="trusted-pack-catalog instrument-panel"
        aria-labelledby="trusted-pack-catalog-title"
      >
        <div className="panel-title">
          <div>
            <span className="eyebrow">Signed native release catalog</span>
            <h2 id="trusted-pack-catalog-title">Local runtime inventory</h2>
          </div>
          <span className={trustedCatalog?.ready ? "badge good" : "badge wait"}>
            {trustedCatalog?.ready
              ? trustedCatalog.productionTrust
                ? `${trustedCatalog.packs.length} production-trusted packs`
                : `${trustedCatalog.packs.length} review-bootstrap packs`
              : nativeAvailable
                ? "Fail-closed"
                : "Native required"}
          </span>
        </div>
        <p className="source-disclosure">
          {trustedCatalog?.detail ??
            (nativeAvailable
              ? "Loading the manager-owned signed pack catalog…"
              : "Browser preview does not invent local runtimes, licenses, downloads, or resource measurements.")}
        </p>
        {trustedCatalog?.ready && !trustedCatalog.productionTrust && (
          <p className="control-reason">
            Non-production review trust ·{" "}
            {humanizeBenchmarkKey(
              trustedCatalog.trustScope ?? "trust_scope_unavailable",
            )}
            {trustedCatalog.rotationRequiredBeforeRelease
              ? " · key rotation required before release"
              : ""}
            {!trustedCatalog.promotionSupported
              ? " · promotion unsupported"
              : ""}
            {!trustedCatalog.publicationSupported
              ? " · publication unsupported"
              : ""}
          </p>
        )}
        {trustedCatalog?.packs.length ? (
          <div className="trusted-pack-grid">
            {trustedCatalog.packs.map((candidate) => {
              const measurement = candidate.qualified_measurement;
              const measured =
                candidate.measurement_status === "qualified" &&
                measurement !== null;
              const lifecycleState = optionalLifecycle?.packs.find(
                (entry) =>
                  entry.identity.pack_id === candidate.identity.pack_id &&
                  entry.identity.revision === candidate.identity.revision,
              );
              const lifecycleKey = `${candidate.identity.pack_id}@${candidate.identity.revision}`;
              const lifecycleConfirmed = optionalConsent === lifecycleKey;
              return (
                <article
                  className="trusted-pack-card"
                  key={`${candidate.identity.pack_id}-${candidate.identity.revision}`}
                >
                  <header>
                    <div>
                      <span>
                        {humanizeBenchmarkKey(candidate.capability.kind)} ·{" "}
                        {humanizeBenchmarkKey(candidate.capability.scope)}
                      </span>
                      <h3>{candidate.display_name}</h3>
                      <small>
                        {candidate.identity.pack_id} ·{" "}
                        {candidate.identity.revision}
                      </small>
                    </div>
                    <span className={measured ? "badge good" : "badge wait"}>
                      {measured
                        ? trustedCatalog.productionTrust
                          ? "Production-qualified"
                          : "Review-qualified"
                        : "Unmeasured"}
                    </span>
                  </header>
                  <p>{candidate.description}</p>
                  <p className="pack-recommendation">
                    <b>Recommendation</b>
                    {candidate.recommendation_reason ??
                      "No signed product recommendation is attached."}
                  </p>
                  <dl>
                    <div>
                      <dt>Runtime / backend</dt>
                      <dd>
                        {candidate.runtime}
                        {candidate.runtime_revision
                          ? ` ${candidate.runtime_revision}`
                          : " · revision not reported"}
                        {" · "}
                        {candidate.backends.length
                          ? candidate.backends.join(" / ")
                          : "backend unavailable"}
                      </dd>
                    </div>
                    <div>
                      <dt>Download / installed</dt>
                      <dd>
                        {formatPackBytes(
                          candidate.exact_artifact_download_bytes,
                        )}{" "}
                        / {formatPackBytes(candidate.installed_bytes)}
                      </dd>
                    </div>
                    <div>
                      <dt>License</dt>
                      <dd>
                        <a
                          href={candidate.license.license_url}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {candidate.license.spdx_expression ??
                            candidate.license.license_name}
                        </a>
                        {" · "}
                        {candidate.license.redistributable
                          ? "redistributable"
                          : "not redistributable"}
                        {candidate.license.acceptance_required
                          ? " · acceptance required"
                          : ""}
                      </dd>
                    </div>
                    <div>
                      <dt>Residency candidates</dt>
                      <dd>
                        {candidate.allowed_residencies.length
                          ? candidate.allowed_residencies
                              .map(humanizeBenchmarkKey)
                              .join(" · ")
                          : "None admitted"}
                      </dd>
                    </div>
                    <div>
                      <dt>Lifecycle</dt>
                      <dd>
                        {humanizeBenchmarkKey(
                          candidate.lifecycle.install_strategy,
                        )}
                        {" · "}
                        {candidate.lifecycle.explicit_download_required
                          ? "explicit download"
                          : "download policy not explicit"}
                        {" · "}
                        {candidate.lifecycle.automatic_download_allowed
                          ? "automatic download allowed"
                          : "no automatic download"}
                      </dd>
                    </div>
                    <div>
                      <dt>Admission</dt>
                      <dd>
                        {humanizeBenchmarkKey(candidate.admission_state)} ·{" "}
                        {candidate.admission_reason}
                      </dd>
                    </div>
                  </dl>
                  <p className="pack-measurement-detail">
                    {candidate.measurement_detail} · qualified envelopes{" "}
                    {candidate.qualified_envelope_count} · non-qualifying review
                    evidence {candidate.non_qualifying_review_evidence_count}
                  </p>
                  {measured && measurement && (
                    <div className="pack-measurement-receipt">
                      <b>
                        {trustedCatalog.productionTrust
                          ? "Production-trusted current-device envelope"
                          : "Non-production review-bootstrap current-device envelope"}{" "}
                        · {measurement.sample_count} samples
                      </b>
                      <small>
                        {measurement.runtime} {measurement.runtime_revision} ·{" "}
                        {measurement.backend} · report {measurement.report_id}
                      </small>
                      <dl>
                        {Object.entries(measurement.placements).map(
                          ([placement, values]) =>
                            values ? (
                              <div key={placement}>
                                <dt>{humanizeBenchmarkKey(placement)}</dt>
                                <dd>
                                  RAM resident/p99{" "}
                                  {formatPackBytes(values.resident_ram_bytes)} /{" "}
                                  {formatPackBytes(values.p99_total_ram_bytes)}{" "}
                                  · VRAM resident/workspace{" "}
                                  {formatPackBytes(values.resident_vram_bytes)}{" "}
                                  /{" "}
                                  {formatPackBytes(
                                    values.p99_workspace_vram_bytes,
                                  )}
                                  {" · "}load/reload/operation p99{" "}
                                  {values.p99_load_millis}/
                                  {values.p99_reload_millis}/
                                  {values.p99_operation_millis} ms
                                </dd>
                              </div>
                            ) : null,
                        )}
                      </dl>
                    </div>
                  )}
                  <div className="trusted-pack-lifecycle-controls">
                    <p className="control-reason">
                      {lifecycleState?.detail ??
                        optionalLifecycle?.detail ??
                        "Native optional-pack lifecycle is unavailable."}
                    </p>
                    <label className="authorization-control">
                      <input
                        type="checkbox"
                        checked={lifecycleConfirmed}
                        disabled={!nativeAvailable || !lifecycleState}
                        onChange={(event) =>
                          setOptionalConsent(
                            event.target.checked ? lifecycleKey : null,
                          )
                        }
                      />
                      <span>
                        <b>Confirm exact signed-catalog mutation</b>
                        <small>
                          No silent download. Identity, URLs, sizes, hashes,
                          license, and lifecycle come from the verified native
                          catalog.
                        </small>
                      </span>
                    </label>
                    {lifecycleState?.licenseAcceptanceRequired &&
                      lifecycleConfirmed && (
                        <label className="authorization-control">
                          <input
                            type="checkbox"
                            checked={optionalLicenseAccepted}
                            onChange={(event) =>
                              setOptionalLicenseAccepted(event.target.checked)
                            }
                          />
                          <span>
                            <b>Accept the exact license shown above</b>
                            <small>
                              Required only for this explicit install or repair.
                            </small>
                          </span>
                        </label>
                      )}
                    <div className="pack-actions">
                      <button
                        disabled={
                          busy !== null ||
                          !lifecycleConfirmed ||
                          !lifecycleState?.canInstall ||
                          (lifecycleState.licenseAcceptanceRequired &&
                            !optionalLicenseAccepted)
                        }
                        onClick={() =>
                          void mutateOptional("install", candidate.identity)
                        }
                      >
                        {busy === `optional-install-${lifecycleKey}`
                          ? "Downloading…"
                          : "Install exact optional pack"}
                      </button>
                      <button
                        disabled={
                          busy !== null ||
                          !lifecycleConfirmed ||
                          !lifecycleState?.canRepair ||
                          (lifecycleState.licenseAcceptanceRequired &&
                            !optionalLicenseAccepted)
                        }
                        onClick={() =>
                          void mutateOptional("repair", candidate.identity)
                        }
                      >
                        Repair from signed catalog
                      </button>
                      <button
                        disabled={
                          busy !== null ||
                          !lifecycleConfirmed ||
                          !lifecycleState?.canRemove
                        }
                        onClick={() =>
                          void mutateOptional("remove", candidate.identity)
                        }
                      >
                        Remove exact pack
                      </button>
                      {(busy === `optional-install-${lifecycleKey}` ||
                        busy === `optional-repair-${lifecycleKey}` ||
                        lifecycleState?.phase === "downloading" ||
                        lifecycleState?.phase === "repairing") && (
                        <button
                          className="secondary-action"
                          onClick={() =>
                            void cancelOptional(candidate.identity)
                          }
                        >
                          Cancel active exact-pack transfer
                        </button>
                      )}
                    </div>
                    {optionalCancelReceipt?.key === lifecycleKey && (
                      <p className="control-reason" role="status">
                        {optionalCancelReceipt.detail}
                      </p>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        ) : (
          <div className="empty-state compact">
            <b>
              {trustedCatalog
                ? "No trusted local packs are available"
                : nativeAvailable
                  ? "Loading trusted pack catalog…"
                  : "Native signed catalog unavailable in browser preview"}
            </b>
            <p>
              API-first remains the default. Catalog presence alone never proves
              current-device fit or permits CUDA activation.
            </p>
          </div>
        )}
        <p className="control-reason">
          Whole-loadout game-reserve fit is shown only by the separate live
          admission decision above. Planning sizes and non-qualifying review
          evidence are never presented as measured fit.
        </p>
      </section>
      <ThisPcBenchmark nativeAvailable={nativeAvailable} />
      <div className="pack-manager instrument-panel">
        <div className="panel-title">
          <div>
            <span className="eyebrow">Explicit local-review lifecycle</span>
            <h2 id="local-packs-title">Experimental visual signal pack</h2>
          </div>
          <span className="badge wait">
            {pack?.phase ?? (nativeAvailable ? "Loading" : "Unavailable")}
          </span>
        </div>
        <p className="source-disclosure">
          This optional experimental OpenSeeFace visual-signal dependency is not
          a complete lip-sync model. Installation is explicit, SHA-256 pinned,
          and debug/local-review only. Activation remains fail-closed without a
          signed current-device envelope.
        </p>
        {pack ? (
          <article className="native-pack-card">
            <header>
              <div>
                <span>{pack.trustDomain}</span>
                <h3>{pack.packId}</h3>
              </div>
              <span className="badge wait">revision {pack.revision}</span>
            </header>
            <p>{pack.detail}</p>
            <dl>
              <div>
                <dt>Complete lip-sync model</dt>
                <dd>{pack.completeLipSyncModel ? "Yes" : "No"}</dd>
              </div>
              <div>
                <dt>Installed artifacts</dt>
                <dd>{pack.installedArtifactSha256.length}</dd>
              </div>
              <div>
                <dt>Related native inventory</dt>
                <dd>
                  {inventoryCount} catalog entr
                  {inventoryCount === 1 ? "y" : "ies"}
                </dd>
              </div>
            </dl>
            <label className="authorization-control">
              <input
                type="checkbox"
                checked={confirmed}
                onChange={(event) => setConfirmed(event.target.checked)}
              />
              <span>
                <b>Confirm this explicit local-review mutation</b>
                <small>No action runs until I press its button.</small>
              </span>
            </label>
            <div className="pack-actions">
              <button
                disabled={
                  busy !== null ||
                  Boolean(actionReason) ||
                  pack.phase !== "not_installed"
                }
                aria-describedby="pack-install-reason"
                onClick={() => void mutate("install")}
              >
                {busy === "install" ? "Installing…" : "Install exact pack"}
              </button>
              <button
                disabled={
                  busy !== null ||
                  Boolean(actionReason) ||
                  pack.phase !== "repair_required"
                }
                aria-describedby="pack-repair-reason"
                onClick={() => void mutate("repair")}
              >
                Repair installed pack
              </button>
              <button
                disabled={
                  busy !== null ||
                  Boolean(actionReason) ||
                  !telemetry?.admissionReady ||
                  (pack.phase !== "installed_inactive" &&
                    pack.phase !==
                      "activation_blocked_missing_measured_envelope")
                }
                aria-describedby="pack-activate-reason"
                onClick={() => void mutate("activate")}
              >
                Request activation check
              </button>
              <button
                disabled={
                  busy !== null ||
                  Boolean(actionReason) ||
                  pack.phase === "not_installed"
                }
                aria-describedby="pack-remove-reason"
                onClick={() => void mutate("remove")}
              >
                Remove local pack
              </button>
            </div>
            <div className="pack-unavailable-reasons">
              <p id="pack-install-reason">
                Install:{" "}
                {actionReason ??
                  (pack.phase === "not_installed"
                    ? "ready after confirmation"
                    : `blocked in ${pack.phase}`)}
                .
              </p>
              <p id="pack-repair-reason">
                Repair:{" "}
                {actionReason ??
                  (pack.phase === "repair_required"
                    ? "ready after confirmation"
                    : "only available after verification fails")}
                .
              </p>
              <p id="pack-activate-reason">
                Activate:{" "}
                {actionReason ??
                  (!telemetry?.admissionReady
                    ? (telemetry?.admissionDetail ?? "fresh telemetry required")
                    : "native command will still require a signed current-device measured envelope")}
                .
              </p>
              <p id="pack-remove-reason">
                Remove:{" "}
                {actionReason ??
                  (pack.phase === "not_installed"
                    ? "nothing is installed"
                    : "ready after confirmation")}
                .
              </p>
            </div>
          </article>
        ) : (
          <div className="empty-state">
            <b>
              {nativeAvailable
                ? "Loading pack state…"
                : "Native pack lifecycle unavailable in browser preview"}
            </b>
          </div>
        )}
        {error && state !== "error" && (
          <p className="inline-status error" role="alert">
            {error}
          </p>
        )}
      </div>
    </section>
  );
}

export function resetProductWorkspacesForTests() {
  // Native state is intentionally not mutated by frontend test cleanup.
}
