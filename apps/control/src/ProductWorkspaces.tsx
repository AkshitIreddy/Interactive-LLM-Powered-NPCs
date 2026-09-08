import { useCallback, useEffect, useRef, useState } from "react";
import { GameOverlaySetup } from "./GameOverlaySetup";
import {
  readCharacterContentOverride,
  resetCharacterContentOverride,
  saveCharacterContentOverride,
  type CharacterContentOverrideSnapshot,
} from "./characterContentOverrides";
import {
  disableCharacterMouthPack,
  enableCharacterMouthPack,
  importCharacterMouthPack,
  inspectCharacterMouthPack,
  readCharacterMouthPackState,
  readMouthPackFiles,
  type CharacterMouthPackFiles,
  type CharacterMouthPackPreview,
  type CharacterMouthPackState,
  type InstalledCharacterMouthPack,
} from "./characterMouthPacks";
import type {
  NativeGameProfileSummary,
  NativeModelSummary,
  NativeProductPreferenceSnapshot,
} from "./tauriBridge";
import {
  activateTrustedOptionalPack,
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
  prepareSupportedVisualLoadout,
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
  verifySelectedGameCapture,
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
  type NativeTrustedOptionalPackActivationReceipt,
  type NativeTrustedOptionalPackLifecycle,
} from "./tauriBridge";

const BROWSER_CHARACTER: NativeCharacterInspection = {
  schemaVersion: 1,
  gameProfileId: "cyberpunk-2077",
  gameDisplayName: "Cyberpunk 2077",
  selectedCharacterId: "misty-olzewski",
  character: {
    id: "misty-olzewski",
    displayName: "Misty Olszewski",
    aliases: ["Misty"],
    biography:
      "The proprietor of an esoteric shop near Viktor's clinic and a close member of Jackie's circle. She interprets experience through spiritual symbols while remaining attentive to grief and human needs.",
    personality:
      "Gentle, perceptive, patient, and resilient. She offers meaning without demanding belief and can sit with uncertainty rather than forcing reassurance.",
    dialogueStyle:
      "Soft, deliberate, image-rich, and emotionally attentive. Symbolic readings are framed as invitations, never objective predictions.",
    styleExamples: [],
    openingLines: [],
    backgroundNpc: false,
    promptRole:
      "Speak as Misty with relationship and story facts bounded by trusted progress.",
    promptObjectives: [
      "Listen before interpreting.",
      "Offer symbolic reflection as optional perspective.",
      "Acknowledge grief without exploiting it.",
    ],
    promptConstraints: [
      "Do not claim supernatural certainty.",
      "Do not predict locked story outcomes.",
      "Do not imitate recorded dialogue or performer.",
    ],
    knowledgeRefs: [],
    voice: {
      description:
        "Original gentle mid voice with spacious pacing and grounded warmth.",
      locale: "en-US",
      styleTags: ["gentle", "reflective", "grounded"],
      providerVoiceId: null,
      adapterId: null,
      catalogVersion: null,
      license: null,
      userOverrideAllowed: false,
    },
    identity: {
      strategy: "explicit_selection",
      evidence: ["Bundled Cyberpunk profile preview"],
      fallback: "explicit_selection",
      automaticFaceRecognitionClaimed: false,
    },
  },
  authoredKnowledge: [],
  provenance: [],
  deliveredMemory: [],
  memoryScope: {
    userId: "browser-preview",
    profileId: "cyberpunk-2077",
    gameId: "cyberpunk-2077",
    characterId: "misty-olzewski",
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

const CYBERPUNK_PROFILE_ID = "cyberpunk-2077";

export function GameTargetWorkspace({
  onSetupMouthTracking,
  nativeAvailable,
  gameProfiles,
  gameProfileId,
  onSelectionChange,
  onPreferencesSnapshot,
}: {
  onSetupMouthTracking?: () => void;
  nativeAvailable: boolean;
  gameProfiles: NativeGameProfileSummary[];
  gameProfileId: string;
  onGameProfileChange: (gameProfileId: string) => void;
  onSelectionChange: (selection: NativeGameTargetSelection | null) => void;
  onPreferencesSnapshot?: (snapshot: NativeProductPreferenceSnapshot) => void;
}) {
  const cyberpunkProfile = gameProfiles.find(
    (profile) => profile.id === CYBERPUNK_PROFILE_ID,
  );
  const activeGameProfileId = cyberpunkProfile?.id ?? gameProfileId;
  const [candidates, setCandidates] = useState<
    AsyncState<NativeGameTargetCandidate[]>
  >(nativeAvailable ? { kind: "loading" } : { kind: "empty" });
  const [selected, setSelected] = useState<NativeGameTargetSelection | null>(
    null,
  );
  const [confirmed, setConfirmed] = useState(false);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  useEffect(
    () => () => {
      selectedRef.current = null;
    },
    [],
  );
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState(
    nativeAvailable
      ? "Checking this session for a selected game."
      : "Game discovery is available in the installed Windows app.",
  );
  const [actorPicker, setActorPicker] =
    useState<NativeManualActorPickerPresentation>({
      schemaVersion: 1,
      state: "unavailable",
      detail: nativeAvailable
        ? "Select an NPC to start mouth tracking."
        : "Native actor selection requires the Windows desktop shell.",
    });

  useEffect(() => {
    let active = true;
    if (!nativeAvailable) return;
    void readSelectedGameTarget()
      .then((value) => {
        if (!active) return;
        setSelected(value);
        setConfirmed(value?.userConfirmedOfflineSinglePlayer ?? false);
        onSelectionChange(value);
        setCandidates({ kind: "empty" });
        setNotice(
          value
            ? `Connected to ${value.target.title} for this session.`
            : "No game process is selected in this native session. Start the game, then find its window.",
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
    if (
      !nativeAvailable ||
      !["waiting", "selected"].includes(actorPicker.state)
    )
      return;
    let active = true;
    let pending = false;
    const timer = window.setInterval(
      () => {
        if (pending) return;
        pending = true;
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
          })
          .finally(() => {
            pending = false;
          });
      },
      actorPicker.state === "waiting" ? 250 : 1000,
    );
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [actorPicker.state, nativeAvailable]);

  const discover = async () => {
    setBusy(true);
    setCandidates({ kind: "loading" });
    try {
      const value = await discoverGameTargets(activeGameProfileId);
      if (!value?.length) {
        setCandidates({ kind: "empty" });
        setNotice(
          `Cyberpunk 2077 is not running yet. Start the game, reach the main game window, then scan again.`,
        );
      } else {
        setCandidates({ kind: "ready", value });
        setNotice(
          `${value.length} matching game window${value.length === 1 ? "" : "s"} found. Choose the one you are playing.`,
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
        activeGameProfileId,
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
        "Game window disconnected. You can reconnect without changing the selected profile or character.",
      );
    } catch (error) {
      setNotice(errorText(error, "Game target could not be cleared."));
    } finally {
      setBusy(false);
    }
  };
  const verifyCapture = async () => {
    const expected = selectedRef.current;
    if (!nativeAvailable || !expected || busy) return;
    setBusy(true);
    try {
      const receipt = await verifySelectedGameCapture();
      if (selectedRef.current !== expected) return;
      if (
        !receipt ||
        receipt.gameProfileId !== expected.gameProfileId ||
        receipt.target.processId !== expected.target.processId ||
        receipt.target.nativeWindow !== expected.target.nativeWindow ||
        receipt.target.executablePathSha256 !==
          expected.target.executablePathSha256
      )
        throw new Error("The capture check did not match the connected game.");
      const next = await readSelectedGameTarget();
      if (selectedRef.current !== expected) return;
      if (
        !next ||
        next.gameProfileId !== expected.gameProfileId ||
        next.target.processId !== expected.target.processId ||
        next.target.nativeWindow !== expected.target.nativeWindow ||
        next.target.executablePathSha256 !==
          expected.target.executablePathSha256 ||
        !next.captureAuthorized
      )
        throw new Error("Game capture is not ready yet. Try checking again.");
      setSelected(next);
      onSelectionChange(next);
      setNotice("Game capture checked. You can now select the NPC on screen.");
    } catch (error) {
      if (selectedRef.current === expected)
        setNotice(errorText(error, "Game capture could not be checked."));
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

  const cyberpunkProfileReady = !nativeAvailable || Boolean(cyberpunkProfile);

  return (
    <section
      className="instrument-panel native-target-workspace"
      aria-labelledby="target-workspace-title"
    >
      <div className="panel-title workspace-heading">
        <div>
          <span className="eyebrow">Game link</span>
          <h2 id="target-workspace-title">Cyberpunk 2077</h2>
        </div>
        <span className={selected ? "badge good" : "badge"}>
          {selected ? "Connected" : "Not connected"}
        </span>
      </div>
      <div className="game-target-compact">
        <p>
          {selected
            ? "The game window is connected for this session."
            : "Start Cyberpunk 2077, then connect its window."}
        </p>
        <div className="game-target-compact__actions">
          <button
            className="primary-action"
            disabled={!nativeAvailable || !cyberpunkProfileReady || busy}
            onClick={discover}
          >
            {busy ? "Scanning…" : "Scan for Cyberpunk 2077"}
          </button>
          <label className="authorization-control compact">
            <input
              type="checkbox"
              checked={confirmed}
              disabled={!nativeAvailable || busy}
              onChange={(event) => setConfirmed(event.target.checked)}
            />
            <span>
              <b>Single-player session</b>
            </span>
          </label>
        </div>

        {!nativeAvailable && (
          <div className="empty-state compact">
            <b>Open the Windows app to scan for the game.</b>
          </div>
        )}

        {nativeAvailable && !cyberpunkProfile && (
          <div className="empty-state error compact">
            <b>The Cyberpunk profile did not load.</b>
          </div>
        )}

        {candidates.kind === "loading" && (
          <div className="empty-state compact" aria-live="polite">
            <b>Looking for Cyberpunk2077.exe…</b>
          </div>
        )}
        {candidates.kind === "error" && (
          <div className="empty-state error">
            <b>Could not read running game windows</b>
            <p>{candidates.detail}</p>
            <button className="quiet-button" onClick={() => void discover()}>
              Try again
            </button>
          </div>
        )}
        {candidates.kind === "empty" && nativeAvailable && !selected && (
          <div className="empty-state compact">
            <b>No Cyberpunk game window connected.</b>
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
                    {candidate.executableName} · {candidate.clientWidth}×
                    {candidate.clientHeight}
                  </small>
                </div>
                <button
                  disabled={busy || !confirmed}
                  onClick={() => void choose(candidate)}
                >
                  Connect this window
                </button>
              </article>
            ))}
          </div>
        )}
      </div>

      {selected && (
        <article className="selected-target-card selected-target-card--primary">
          <header>
            <div>
              <span className="eyebrow">Connected game</span>
              <b>{selected.target.title}</b>
              <small>{selected.target.executableName}</small>
            </div>
            <span className="badge good">Window connected</span>
          </header>
          <div className="target-toolbar">
            <button className="quiet-button" disabled={busy} onClick={clear}>
              Disconnect game
            </button>
          </div>
        </article>
      )}

      {selected && (
        <div className="game-visual-setup">
          <GameOverlaySetup
            nativeAvailable={nativeAvailable}
            gameProfileId={activeGameProfileId}
            onSnapshot={onPreferencesSnapshot}
          />
          <section className="game-actor-setup" aria-label="NPC tracking setup">
            <span className="eyebrow">On-screen character</span>
            <h3>NPC tracking</h3>
            <p>
              Select the NPC you are facing. The character profile below
              controls voice and memory.
            </p>
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
                Select NPC on screen
              </button>
              {actorPicker.state === "waiting" && (
                <button
                  className="quiet-button"
                  disabled={busy}
                  onClick={cancelActorPicker}
                >
                  Cancel selection
                </button>
              )}
              <span
                className={
                  actorPicker.state === "selected" ? "badge good" : "badge wait"
                }
              >
                {actorPicker.state === "waiting"
                  ? "Waiting for click"
                  : actorPicker.state === "selected"
                    ? "NPC selected"
                    : actorPicker.state === "cancelled"
                      ? "Selection cancelled"
                      : "Not selected"}
              </span>
            </div>
            <p className="source-disclosure">{actorPicker.detail}</p>
            {onSetupMouthTracking && (
              <button className="text-action" onClick={onSetupMouthTracking}>
                Set up mouth tracking →
              </button>
            )}
          </section>
        </div>
      )}
      <details className="technical-disclosure actor-picker-disclosure">
        <summary>Connection details</summary>
        {selected && (
          <div className="connection-evidence">
            <dl className="facts">
              <div>
                <dt>Process</dt>
                <dd>PID {selected.target.processId}</dd>
              </div>
              <div>
                <dt>Window</dt>
                <dd>HWND {selected.target.nativeWindow}</dd>
              </div>
              <div>
                <dt>Safety state</dt>
                <dd>{selected.safetyState}</dd>
              </div>
            </dl>
            <p>{selected.safetyDetail}</p>
            <button
              className="quiet-button"
              disabled={!nativeAvailable || busy}
              onClick={() => void verifyCapture()}
            >
              Check game capture
            </button>
          </div>
        )}
      </details>
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
  const [mouthPackRegistry, setMouthPackRegistry] =
    useState<CharacterMouthPackState | null>(null);
  const [state, setState] = useState<AsyncState<NativeCharacterInspection>>(
    nativeAvailable
      ? { kind: "loading" }
      : { kind: "ready", value: BROWSER_CHARACTER },
  );
  const [busy, setBusy] = useState(false);
  const onSelectionChangeRef = useRef(onSelectionChange);
  const [notice, setNotice] = useState(
    nativeAvailable
      ? "Loading this game's character selection and saved-turn count."
      : "Browser preview only. Character changes are unavailable here.",
  );
  useEffect(() => {
    onSelectionChangeRef.current = onSelectionChange;
  }, [onSelectionChange]);
  const inspect = useCallback(
    async (requestedId?: string) => {
      if (!nativeAvailable) {
        setState({ kind: "ready", value: BROWSER_CHARACTER });
        onSelectionChangeRef.current?.(BROWSER_CHARACTER);
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
        onSelectionChangeRef.current?.(value);
        setNotice("Character profile and saved turns loaded.");
      } catch (error) {
        const detail = errorText(error, "Character inspection failed.");
        setState({ kind: "error", detail });
        setNotice(detail);
      }
    },
    [gameProfileId, nativeAvailable],
  );
  useEffect(() => {
    void inspect();
  }, [inspect]);
  useEffect(() => {
    if (!nativeAvailable) return;
    let active = true;
    void readCharacterMouthPackState()
      .then((nextRegistry) => {
        if (active) setMouthPackRegistry(nextRegistry);
      })
      .catch(() => {
        if (active) setMouthPackRegistry(null);
      });
    return () => {
      active = false;
    };
  }, [nativeAvailable]);
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
      <header className="panel-title character-database__title workspace-heading">
        <div>
          <span className="eyebrow">Night City roster</span>
          <h2 id="character-db-title">Choose who answers</h2>
        </div>
        <span className={nativeAvailable ? "badge good" : "badge wait"}>
          {nativeAvailable ? "Cyberpunk profiles" : "Preview"}
        </span>
      </header>
      <p className="source-disclosure">
        {nativeAvailable
          ? "Open a profile to inspect it, then choose Use this character to make it active. Voice and mouth motion are shown separately because they have different setup requirements."
          : "The preview shows the layout. Character choices are saved by the Windows app."}
      </p>
      {catalog && (
        <div className="character-roster-block">
          <div className="workspace-step__heading">
            <div>
              <span className="eyebrow">{catalog.gameDisplayName}</span>
              <h3>People you can configure</h3>
            </div>
            <span className="badge">{catalog.characters.length}</span>
          </div>
          <aside
            className="native-character-list"
            aria-label={`${catalog.gameDisplayName} characters`}
          >
            {catalog.characters.length === 0 ? (
              <div className="empty-state compact">
                <b>No authored characters in this profile</b>
                <p>Choose another bundled game profile.</p>
              </div>
            ) : (
              catalog.characters.map((entry) => {
                const enabledMouthPack = mouthPackRegistry?.enabled.some(
                  (packEntry) =>
                    packEntry.gameProfileId === gameProfileId &&
                    packEntry.characterId === entry.id,
                );
                const installedMouthPack = mouthPackRegistry?.installed.some(
                  (packEntry) =>
                    packEntry.gameProfileId === gameProfileId &&
                    packEntry.characterId === entry.id,
                );
                return (
                  <button
                    key={entry.id}
                    className={
                      value?.character.id === entry.id ? "selected" : ""
                    }
                    aria-pressed={value?.character.id === entry.id}
                    disabled={busy}
                    onClick={() => void inspect(entry.id)}
                  >
                    <span>{entry.displayName.slice(0, 2).toUpperCase()}</span>
                    <span>
                      <b>{entry.displayName}</b>
                      <small>
                        {entry.backgroundNpc
                          ? "Encounter profile for unlisted NPCs"
                          : entry.voiceDescription
                            ? "Voice direction included"
                            : "Choose a voice"}
                      </small>
                      <small className="character-mouth-pack-state">
                        {enabledMouthPack
                          ? "Full mouth pack enabled"
                          : installedMouthPack
                            ? "Full mouth pack installed"
                            : "Basic motion · no full pack"}
                      </small>
                    </span>
                    <i>
                      {catalog.selectedCharacterId === entry.id
                        ? "Active"
                        : entry.backgroundNpc
                          ? "Street NPC mode"
                          : "View"}
                    </i>
                  </button>
                );
              })
            )}
          </aside>
        </div>
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
          <button onClick={() => void inspect()}>Try again</button>
        </div>
      )}
      {value && (
        <div className="character-native-record">
          <div className="character-record__heading character-record__hero">
            <div>
              <span className="eyebrow">
                {value.gameDisplayName} ·{" "}
                {value.character.backgroundNpc
                  ? "Background character"
                  : "Authored character"}
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
                ? "Active character"
                : "Use this character"}
            </button>
          </div>
          <dl
            className="facts spacious character-native-facts character-summary-grid"
            aria-label={`${value.character.displayName} setup overview`}
          >
            <div>
              <dt>Dialogue profile</dt>
              <dd>
                {value.character.biography || value.character.promptRole
                  ? "Ready"
                  : "Needs character details"}
              </dd>
            </div>
            <div>
              <dt>Voice</dt>
              <dd>Chosen in Voice setup · {value.character.voice.locale}</dd>
            </div>
            <div>
              <dt>Mouth motion</dt>
              <dd>
                {mouthPackRegistry?.enabled.some(
                  (entry) =>
                    entry.gameProfileId === value.gameProfileId &&
                    entry.characterId === value.character.id,
                )
                  ? "Full pack enabled · visual tracking still required"
                  : mouthPackRegistry?.installed.some(
                        (entry) =>
                          entry.gameProfileId === value.gameProfileId &&
                          entry.characterId === value.character.id,
                      )
                    ? "Full pack installed · enable below"
                    : mouthPackRegistry || !nativeAvailable
                      ? "Basic motion · select NPC on screen"
                      : "Checking available motion"}
              </dd>
            </div>
            <div>
              <dt>Memory</dt>
              <dd>
                {value.deliveredMemory.length} saved turn
                {value.deliveredMemory.length === 1 ? "" : "s"}
              </dd>
            </div>
          </dl>
          {value.character.backgroundNpc && (
            <section
              className="street-npc-mode"
              aria-labelledby="street-npc-mode-title"
            >
              <div>
                <span className="eyebrow">For everyone else in Night City</span>
                <h4 id="street-npc-mode-title">Stable street NPC mode</h4>
              </div>
              <p>
                An unlisted NPC gets one encounter identity, a consistent
                profile voice, and separate memory. The app keeps that identity
                while you stay with the same person; it does not guess who they
                are from their appearance.
              </p>
              <dl className="facts">
                <div>
                  <dt>Conversation</dt>
                  <dd>Available through voice and subtitles</dd>
                </div>
                <div>
                  <dt>Mouth motion</dt>
                  <dd>
                    Voice and subtitles only; basic source motion is in testing
                  </dd>
                </div>
              </dl>
            </section>
          )}
          <details className="character-profile-disclosure">
            <summary>Story and speaking style</summary>
            <dl className="facts spacious">
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
                  {value.character.dialogueStyle ||
                    "No dialogue style authored"}
                </dd>
              </div>
            </dl>
          </details>
          <CharacterContentOverrideEditor
            nativeAvailable={nativeAvailable}
            editable={catalog?.editableAuthoredData ?? nativeAvailable}
            inspection={value}
            onChanged={() => inspect(value.character.id)}
          />
          <CharacterMouthPackWorkspace
            key={`${value.gameProfileId}/${value.character.id}`}
            nativeAvailable={nativeAvailable}
            gameProfileId={value.gameProfileId}
            characterId={value.character.id}
          />
          <details className="technical-disclosure character-data-disclosure">
            <summary>Advanced character data</summary>
            <details>
              <summary>Identity, voice, and memory scope</summary>
              <dl className="facts spacious">
                <div>
                  <dt>Identity method</dt>
                  <dd>
                    {value.character.identity.strategy.replaceAll("_", " ")}
                  </dd>
                </div>
                <div>
                  <dt>Automatic face recognition</dt>
                  <dd>
                    {value.character.identity.automaticFaceRecognitionClaimed
                      ? "Available in this profile"
                      : "Manual selection"}
                  </dd>
                </div>
                <div>
                  <dt>Voice binding</dt>
                  <dd>
                    {value.character.voice.providerVoiceId ??
                      "Resolved by the selected voice setup"}
                  </dd>
                </div>
                <div>
                  <dt>Memory scope</dt>
                  <dd>
                    {value.memoryScope.gameId} / {value.memoryScope.characterId}
                  </dd>
                </div>
              </dl>
            </details>
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
              <summary>
                Delivered memory · {value.deliveredMemory.length}
              </summary>
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
          </details>
        </div>
      )}
      <p className="inline-status" role="status">
        {notice}
      </p>
    </section>
  );
}

const CHARACTER_NAME_LIMIT = 160;
const CHARACTER_BIOGRAPHY_LIMIT = 16 * 1024;
const CHARACTER_PROMPT_CONTEXT_LIMIT = 1024;
const textBytes = (value: string) => new TextEncoder().encode(value).byteLength;

export function CharacterContentOverrideEditor({
  nativeAvailable,
  editable,
  inspection,
  onChanged,
}: {
  nativeAvailable: boolean;
  editable: boolean;
  inspection: NativeCharacterInspection;
  onChanged: () => Promise<void>;
}) {
  const { character, gameProfileId } = inspection;
  const [snapshot, setSnapshot] =
    useState<CharacterContentOverrideSnapshot | null>(null);
  const [displayName, setDisplayName] = useState(character.displayName);
  const [biography, setBiography] = useState(character.biography);
  const [promptContext, setPromptContext] = useState("");
  const [busy, setBusy] = useState<"load" | "save" | "reset" | null>(null);
  const [notice, setNotice] = useState(
    nativeAvailable
      ? "Open the editor to load this character's player-authored layer."
      : "Browser preview is read-only. Editing is available in the installed Windows app.",
  );

  const load = useCallback(async () => {
    if (!nativeAvailable) {
      setSnapshot(null);
      setDisplayName(character.displayName);
      setBiography(character.biography);
      setPromptContext("");
      return;
    }
    setBusy("load");
    try {
      const next = await readCharacterContentOverride(
        gameProfileId,
        character.id,
      );
      setSnapshot(next);
      setDisplayName(next?.saved?.displayName ?? character.displayName);
      setBiography(next?.saved?.biography ?? character.biography);
      setPromptContext(next?.saved?.promptContext ?? "");
      setNotice(
        next?.detail ?? "The current content pack supplies these fields.",
      );
    } catch (error) {
      setNotice(errorText(error, "Character customization could not load."));
    } finally {
      setBusy(null);
    }
  }, [
    character.biography,
    character.displayName,
    character.id,
    gameProfileId,
    nativeAvailable,
  ]);

  useEffect(() => {
    void load();
  }, [load]);

  const validation = (() => {
    if (!displayName.trim()) return "Enter a character name.";
    if (textBytes(displayName) > CHARACTER_NAME_LIMIT)
      return "Character name exceeds 160 bytes.";
    if (!biography.trim()) return "Enter a backstory or biography.";
    if (textBytes(biography) > CHARACTER_BIOGRAPHY_LIMIT)
      return "Backstory exceeds 16 KiB.";
    if (textBytes(promptContext) > CHARACTER_PROMPT_CONTEXT_LIMIT)
      return "Prompt context exceeds 1,024 bytes.";
    return null;
  })();

  const save = async () => {
    if (validation) {
      setNotice(validation);
      return;
    }
    setBusy("save");
    try {
      const result = await saveCharacterContentOverride({
        gameProfileId,
        characterId: character.id,
        displayName: displayName.trim(),
        biography: biography.trim(),
        promptContext: promptContext.trim(),
      });
      if (!result.persisted || !result.saved)
        throw new Error("Native override save did not return persisted state.");
      setSnapshot({
        schemaVersion: 1,
        gameProfileId,
        characterId: character.id,
        saved: result.saved,
        detail: result.detail,
      });
      setNotice(result.detail);
      await onChanged();
    } catch (error) {
      setNotice(
        errorText(error, "Character customization could not be saved."),
      );
    } finally {
      setBusy(null);
    }
  };

  const reset = async () => {
    setBusy("reset");
    try {
      const result = await resetCharacterContentOverride(
        gameProfileId,
        character.id,
      );
      if (!result.persisted || result.saved)
        throw new Error("Native override reset did not return cleared state.");
      setSnapshot({
        schemaVersion: 1,
        gameProfileId,
        characterId: character.id,
        saved: null,
        detail: result.detail,
      });
      setPromptContext("");
      setNotice(result.detail);
      await onChanged();
    } catch (error) {
      setNotice(
        errorText(error, "Character customization could not be reset."),
      );
    } finally {
      setBusy(null);
    }
  };

  const disabled = !nativeAvailable || !editable || busy !== null;
  return (
    <details className="character-content-editor technical-disclosure">
      <summary>
        <span>Customize this character</span>
        <span className={snapshot?.saved ? "badge good" : "badge"}>
          {snapshot?.saved ? "Player layer active" : "Pack values"}
        </span>
      </summary>
      <p className="source-disclosure">
        Your changes stay in place when this pack updates. Reset restores the
        latest pack defaults.
      </p>
      <div className="character-content-editor__fields">
        <label>
          <span>Character name</span>
          <input
            value={displayName}
            disabled={disabled}
            maxLength={CHARACTER_NAME_LIMIT}
            onChange={(event) => setDisplayName(event.target.value)}
          />
          <small>
            {textBytes(displayName)} / {CHARACTER_NAME_LIMIT} bytes
          </small>
        </label>
        <label>
          <span>Backstory / biography</span>
          <textarea
            value={biography}
            disabled={disabled}
            rows={6}
            maxLength={CHARACTER_BIOGRAPHY_LIMIT}
            onChange={(event) => setBiography(event.target.value)}
          />
          <small>
            {textBytes(biography).toLocaleString()} /{" "}
            {CHARACTER_BIOGRAPHY_LIMIT.toLocaleString()} bytes
          </small>
        </label>
        <label>
          <span>Extra prompt context</span>
          <textarea
            value={promptContext}
            disabled={disabled}
            rows={4}
            maxLength={CHARACTER_PROMPT_CONTEXT_LIMIT}
            placeholder="Optional relationship, roleplay, or situation guidance for this character."
            onChange={(event) => setPromptContext(event.target.value)}
          />
          <small>
            Used when this character responds · {textBytes(promptContext)} /{" "}
            {CHARACTER_PROMPT_CONTEXT_LIMIT} bytes
          </small>
        </label>
      </div>
      <div className="character-content-editor__actions">
        <button
          className="primary-action"
          disabled={disabled || validation !== null}
          onClick={() => void save()}
        >
          {busy === "save" ? "Saving…" : "Save changes"}
        </button>
        <button
          className="secondary-action"
          disabled={disabled || !snapshot?.saved}
          onClick={() => void reset()}
        >
          {busy === "reset" ? "Resetting…" : "Reset to pack values"}
        </button>
      </div>
      {validation && nativeAvailable && (
        <p className="field-error">{validation}</p>
      )}
      <p className="inline-status" aria-live="polite">
        {notice}
      </p>
    </details>
  );
}

export function CharacterMouthPackWorkspace({
  nativeAvailable,
  gameProfileId,
  characterId,
}: {
  nativeAvailable: boolean;
  gameProfileId: string;
  characterId: string;
}) {
  const [atlasFile, setAtlasFile] = useState<File | null>(null);
  const [textureFile, setTextureFile] = useState<File | null>(null);
  const [reviewedFiles, setReviewedFiles] =
    useState<CharacterMouthPackFiles | null>(null);
  const [preview, setPreview] = useState<CharacterMouthPackPreview | null>(
    null,
  );
  const [installed, setInstalled] =
    useState<InstalledCharacterMouthPack | null>(null);
  const [registry, setRegistry] = useState<CharacterMouthPackState | null>(
    null,
  );
  const [busy, setBusy] = useState<
    "review" | "import" | "enable" | "disable" | null
  >(null);
  const [notice, setNotice] = useState(
    nativeAvailable
      ? "Choose the prepared atlas.json and its .bin texture. You can review and import while the game is closed; enabling requires the matching current native actor."
      : "Browser preview is read-only. Review and import require the installed Windows app.",
  );

  const refresh = useCallback(async () => {
    try {
      setRegistry(await readCharacterMouthPackState());
    } catch (error) {
      setNotice(errorText(error, "Mouth-pack state could not load."));
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const clearReview = () => {
    setReviewedFiles(null);
    setPreview(null);
    setInstalled(null);
  };
  const review = async () => {
    if (!atlasFile || !textureFile) {
      setNotice("Choose both atlas.json and the .bin texture it names.");
      return;
    }
    setBusy("review");
    try {
      const files = await readMouthPackFiles(
        gameProfileId,
        atlasFile,
        textureFile,
      );
      const next = await inspectCharacterMouthPack(files);
      if (
        next.gameProfileId !== gameProfileId ||
        next.characterId !== characterId
      )
        throw new Error(
          "Native mouth-pack review returned a different selected character scope.",
        );
      setReviewedFiles(files);
      setPreview(next);
      setInstalled(null);
      setNotice(
        "Exact manifest and texture bytes passed schema and enrollment-binding checks. Inspect the result yourself before importing it.",
      );
    } catch (error) {
      clearReview();
      setNotice(errorText(error, "Mouth-pack review failed."));
    } finally {
      setBusy(null);
    }
  };
  const importReviewed = async () => {
    if (!reviewedFiles || !preview) return;
    setBusy("import");
    try {
      const result = await importCharacterMouthPack(
        reviewedFiles,
        preview.contentSha256,
      );
      setInstalled(result);
      setNotice(
        "Reviewed revision imported locally. Enable it explicitly while the same selected actor remains current.",
      );
      await refresh();
    } catch (error) {
      setNotice(errorText(error, "Mouth-pack import failed."));
    } finally {
      setBusy(null);
    }
  };
  const enable = async (requestedDigest?: string) => {
    const digest =
      requestedDigest ?? installed?.contentSha256 ?? preview?.contentSha256;
    if (!digest) return;
    setBusy("enable");
    try {
      await enableCharacterMouthPack(gameProfileId, digest);
      setNotice(
        "Pack applied to your selected NPC. This is your assignment, not automatic face recognition. Select and apply again when tracking expires.",
      );
      await refresh();
    } catch (error) {
      setNotice(errorText(error, "Mouth-pack enable failed."));
    } finally {
      setBusy(null);
    }
  };
  const disable = async () => {
    if (!scopedEnabled) return;
    setBusy("disable");
    try {
      await disableCharacterMouthPack(
        gameProfileId,
        scopedEnabled.contentSha256,
      );
      setNotice(
        "Mouth pack disabled for this character. Its reviewed local revision remains installed.",
      );
      await refresh();
    } catch (error) {
      setNotice(errorText(error, "Mouth-pack disable failed."));
    } finally {
      setBusy(null);
    }
  };

  const scopedInstalled =
    registry?.installed.filter(
      (entry) =>
        entry.gameProfileId === gameProfileId &&
        entry.characterId === characterId,
    ) ?? [];
  const scopedEnabled = registry?.enabled.find(
    (entry) =>
      entry.gameProfileId === gameProfileId &&
      entry.characterId === characterId,
  );

  return (
    <details className="character-mouth-pack technical-disclosure">
      <summary>
        <span>Mouth motion</span>
        <span className={scopedEnabled ? "badge good" : "badge wait"}>
          {scopedEnabled
            ? "Full pack enabled"
            : scopedInstalled.length
              ? "Full pack installed"
              : registry || !nativeAvailable
                ? "No full mouth pack"
                : "Checking"}
        </span>
      </summary>
      <p className="source-disclosure">
        Select an NPC on screen for basic motion using its current mouth
        appearance. A prepared pack adds richer mouth detail for that character.
        Both need the game overlay and local face tracking; voice and subtitles
        work independently.
      </p>
      {!scopedEnabled && scopedInstalled.length === 0 && registry && (
        <div className="empty-state compact character-mouth-pack__empty">
          <b>No full mouth pack for this character yet</b>
          <p>
            Basic motion needs no character pack. It moves the visible mouth
            subtly; it cannot add teeth or other detail hidden in the game
            frame.
          </p>
        </div>
      )}
      <div className="character-mouth-pack__files">
        <label>
          <span>Atlas manifest</span>
          <input
            aria-label="Choose mouth atlas JSON"
            type="file"
            accept=".json,application/json"
            disabled={!nativeAvailable || busy !== null}
            onChange={(event) => {
              setAtlasFile(event.target.files?.[0] ?? null);
              clearReview();
            }}
          />
          <small>atlas.json · up to 64 KiB</small>
        </label>
        <label>
          <span>Atlas texture</span>
          <input
            aria-label="Choose mouth atlas texture"
            type="file"
            accept=".bin,application/octet-stream"
            disabled={!nativeAvailable || busy !== null}
            onChange={(event) => {
              setTextureFile(event.target.files?.[0] ?? null);
              clearReview();
            }}
          />
          <small>.bin file named by atlas.json · up to 16 MiB</small>
        </label>
      </div>
      <button
        className="secondary-action character-mouth-pack__review"
        disabled={
          !nativeAvailable || !atlasFile || !textureFile || busy !== null
        }
        onClick={() => void review()}
      >
        {busy === "review" ? "Checking files…" : "Check selected files"}
      </button>
      {preview && (
        <section
          className="character-mouth-pack__preview"
          aria-label="Reviewed mouth pack"
        >
          <div>
            <span className="eyebrow">Pack belongs to this profile</span>
            <h4>
              {preview.gameProfileId} / {preview.characterId}
            </h4>
          </div>
          <dl className="facts spacious">
            <div>
              <dt>Atlas</dt>
              <dd>
                schema {preview.atlasSchemaVersion} · identity revision{" "}
                {preview.identityRevision}
              </dd>
            </div>
            <div>
              <dt>Texture</dt>
              <dd>
                {preview.textureFileName} ·{" "}
                {(preview.textureSizeBytes / 1024).toFixed(1)} KiB
              </dd>
            </div>
            <div>
              <dt>Reviewed content</dt>
              <dd>
                <code>{preview.contentSha256.slice(0, 16)}…</code>
              </dd>
            </div>
            <div>
              <dt>Private review evidence</dt>
              <dd>
                {preview.privateReviewBindingValidated
                  ? "Enrollment digest present · "
                  : "Binding missing · "}
                <code>{preview.enrollmentBindingSha256.slice(0, 16)}…</code>
              </dd>
            </div>
          </dl>
          <div className="character-mouth-pack__actions">
            <button
              className="primary-action"
              disabled={busy !== null || installed !== null}
              onClick={() => void importReviewed()}
            >
              {installed
                ? "Pack imported"
                : busy === "import"
                  ? "Importing…"
                  : "Import this pack"}
            </button>
            <button
              className="secondary-action"
              disabled={busy !== null || installed === null}
              onClick={() => void enable()}
            >
              {busy === "enable" ? "Applying…" : "Apply to selected NPC"}
            </button>
          </div>
        </section>
      )}
      {scopedInstalled.length > 0 && (
        <div
          className="character-mouth-pack__installed"
          aria-label="Installed mouth-pack revisions"
        >
          <div>
            <b>Installed packs</b>
            <small>
              Select the matching NPC on screen, then apply its pack here. The
              selection lasts up to 15 seconds; reselect if it expires.
            </small>
          </div>
          {scopedInstalled.map((entry) => {
            const active = scopedEnabled?.contentSha256 === entry.contentSha256;
            return (
              <article key={entry.contentSha256}>
                <span>
                  <code>{entry.contentSha256.slice(0, 16)}…</code>
                  <small>
                    identity revision {entry.identityRevision} ·{" "}
                    {entry.textureFileName}
                  </small>
                </span>
                <button
                  className="secondary-action"
                  disabled={!nativeAvailable || busy !== null}
                  onClick={() => void enable(entry.contentSha256)}
                >
                  {active ? "Reapply to selected NPC" : "Apply to selected NPC"}
                </button>
              </article>
            );
          })}
          {scopedEnabled && (
            <button
              className="quiet-button danger"
              disabled={busy !== null}
              onClick={() => void disable()}
            >
              Disable mouth pack
            </button>
          )}
        </div>
      )}
      <p className="inline-status" aria-live="polite">
        {notice}
      </p>
    </details>
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
          <span className="eyebrow">Saved conversation tools</span>
          <h3>Back up or delete memory</h3>
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
        These actions apply to the selected game and character. Backups and
        removal always require confirmation.
      </p>
      {!nativeAvailable && (
        <div className="empty-state">
          <b>Open the installed app</b>
          <p>
            The Windows app can create backups or delete saved conversation
            memory.
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
  const [setupError, setSetupError] = useState<string | null>(null);
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
  const [optionalActivationReceipt, setOptionalActivationReceipt] = useState<{
    key: string;
    value: NativeTrustedOptionalPackActivationReceipt;
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
  const activateOptional = async (identity: {
    pack_id: string;
    revision: string;
  }) => {
    const key = `${identity.pack_id}@${identity.revision}`;
    const selection = loadoutPlanner?.selected;
    if (
      !selection ||
      selection.roles.filter(
        (role) =>
          role.identity.pack_id === identity.pack_id &&
          role.identity.revision === identity.revision,
      ).length !== 1
    )
      return;
    setBusy(`optional-activate-${key}`);
    setError(null);
    try {
      const result = await activateTrustedOptionalPack(identity, selection);
      if (!result?.receipt || !result.lifecycle)
        throw new Error("Native provider-load self-test returned no receipt.");
      if (
        result.receipt.identity.pack_id !== identity.pack_id ||
        result.receipt.identity.revision !== identity.revision
      )
        throw new Error(
          "Native provider-load self-test returned a different pack identity.",
        );
      setOptionalLifecycle(result.lifecycle);
      setOptionalActivationReceipt({ key, value: result.receipt });
      setOptionalConsent(null);
      setOptionalLicenseAccepted(false);
    } catch (cause) {
      let message = errorText(
        cause,
        "Provider-load self-test did not complete.",
      );
      try {
        const latestLifecycle = await readTrustedOptionalPackLifecycle();
        if (!latestLifecycle)
          throw new Error("Native local model status returned no state.");
        setOptionalLifecycle(latestLifecycle);
      } catch (refreshCause) {
        message = `${message} ${errorText(
          refreshCause,
          "Latest local model status could not be refreshed.",
        )}`;
      }
      setError(message);
    } finally {
      setBusy(null);
    }
  };
  const prepareMouthTracking = async () => {
    setBusy("prepare-visual");
    setSetupError(null);
    try {
      const next = await prepareSupportedVisualLoadout();
      if (!next?.selected)
        throw new Error(
          "Mouth tracking setup did not return a selected model.",
        );
      setLoadoutPlanner(next);
      await refresh();
    } catch (cause) {
      setSetupError(
        errorText(cause, "Mouth tracking setup could not be prepared."),
      );
    } finally {
      setBusy(null);
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
      <section
        className="instrument-panel mouth-tracking-setup"
        aria-label="Mouth tracking model setup"
      >
        <span className="eyebrow">Local face tracking</span>
        <h2>Set up mouth tracking</h2>
        <p>
          Choose the supported tracking model, install it, then test and
          activate it. With Cyberpunk connected, check the loadout below and
          return to Games to select an NPC.
        </p>
        <div className="target-toolbar">
          <button
            className="primary-action"
            disabled={!nativeAvailable || busy !== null}
            onClick={() => void prepareMouthTracking()}
          >
            {busy === "prepare-visual" ? "Preparing…" : "Choose tracking model"}
          </button>
          <a className="text-action" href="#trusted-pack-catalog-title">
            Model downloads ↓
          </a>
        </div>
        <small>
          Your existing local model selection is preserved. This step does not
          download or start a model.
        </small>
        {setupError && <p role="alert">{setupError}</p>}
      </section>
      <div className="resource-planner instrument-panel">
        <div className="panel-title">
          <div>
            <span className="eyebrow">PC resources</span>
            <h2>Memory & performance</h2>
          </div>
          <span
            className={telemetry?.admissionReady ? "badge good" : "badge wait"}
          >
            {state === "loading"
              ? "Loading"
              : telemetry?.admissionReady
                ? "Measured"
                : "Not ready"}
          </span>
        </div>
        <p className="source-disclosure">
          {nativeAvailable
            ? "Reserve memory for your game and choose how local models stay loaded. These settings are saved on this PC."
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
            <details className="technical-disclosure resource-telemetry-disclosure">
              <summary>Hardware readings and provenance</summary>
              <div className="telemetry-grid">
                <article>
                  <span>Exact target PID</span>
                  <b>
                    {telemetry.snapshot.selected_game_pid ?? "Not selected"}
                  </b>
                  <small>Native GameTargetManager authority</small>
                </article>
                <article>
                  <span>Adapter</span>
                  <b>
                    {adapter?.availability === "available"
                      ? adapter.value.description
                      : "Unavailable"}
                  </b>
                  <small>
                    {adapter?.provenance.source ?? "No observation"}
                  </small>
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
            </details>
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
                    {loadoutPlanner.ready ? "Planner ready" : "Not ready"}
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
                    <details className="technical-disclosure">
                      <summary>Exact pack identities and residency</summary>
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
                    </details>
                  </>
                ) : (
                  <div className="empty-state compact">
                    <b>Choose a tracking model first</b>
                    <p>
                      Use Choose tracking model above to prepare the supported
                      local setup.
                    </p>
                  </div>
                )}
                {loadoutPlanner.planner && (
                  <details className="technical-disclosure">
                    <summary>Planner evidence</summary>
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
                  </details>
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
                    {(loadoutAdmission.decision?.reason_code ||
                      loadoutAdmission.decision?.admission_receipt ||
                      loadoutAdmission.decision?.residency_decisions.length ||
                      loadoutAdmission.decision?.pressure_cancellations
                        .length) && (
                      <details className="technical-disclosure">
                        <summary>
                          Admission receipt and resource arithmetic
                        </summary>
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
                        {loadoutAdmission.decision?.residency_decisions
                          .length ? (
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
                                      {humanizeBenchmarkKey(
                                        decision.disposition,
                                      )}
                                    </span>
                                    <small>
                                      Expected idle{" "}
                                      {decision.expected_idle_millis} ms ·
                                      measured reload p99{" "}
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
                      </details>
                    )}
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
            <span className="eyebrow">Available models</span>
            <h2 id="trusted-pack-catalog-title">Model downloads</h2>
          </div>
          <span className={trustedCatalog?.ready ? "badge good" : "badge wait"}>
            {trustedCatalog?.ready
              ? trustedCatalog.productionTrust
                ? `${trustedCatalog.packs.length} models`
                : `${trustedCatalog.packs.length} review models`
              : nativeAvailable
                ? "Not ready"
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
              const selectedDraftMatches =
                loadoutPlanner?.selected?.roles.filter(
                  (role) =>
                    role.identity.pack_id === candidate.identity.pack_id &&
                    role.identity.revision === candidate.identity.revision,
                ).length ?? 0;
              const exactSetupDraftReady = Boolean(
                loadoutPlanner?.ready &&
                  loadoutPlanner.selected &&
                  selectedDraftMatches === 1,
              );
              const awaitingProviderLoadSelfTest =
                lifecycleState?.phase ===
                "installed_inactive_awaiting_self_test";
              const lifecycleSummary =
                lifecycleState?.phase ===
                "installed_inactive_awaiting_self_test"
                  ? "Installed and ready to test."
                  : lifecycleState?.phase === "active"
                    ? "Installed and active."
                    : (lifecycleState?.detail ??
                      optionalLifecycle?.detail ??
                      "Local model status is unavailable.");
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
                    </div>
                    <span className={measured ? "badge good" : "badge wait"}>
                      {measured
                        ? trustedCatalog.productionTrust
                          ? "Resources qualified"
                          : "Resources measured"
                        : "Not measured"}
                    </span>
                  </header>
                  <p>{candidate.description}</p>
                  <p className="pack-recommendation">
                    <b>Recommendation</b>
                    {candidate.recommendation_reason ??
                      "No signed product recommendation is attached."}
                  </p>
                  <details className="technical-disclosure pack-technical-disclosure">
                    <summary>Runtime, license, and measurement details</summary>
                    <dl>
                      <div>
                        <dt>Pack identity</dt>
                        <dd>
                          {candidate.identity.pack_id} ·{" "}
                          {candidate.identity.revision}
                        </dd>
                      </div>
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
                      {candidate.qualified_envelope_count} · non-qualifying
                      review evidence{" "}
                      {candidate.non_qualifying_review_evidence_count}
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
                                    {formatPackBytes(values.resident_ram_bytes)}{" "}
                                    /{" "}
                                    {formatPackBytes(
                                      values.p99_total_ram_bytes,
                                    )}{" "}
                                    · VRAM resident/workspace{" "}
                                    {formatPackBytes(
                                      values.resident_vram_bytes,
                                    )}{" "}
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
                  </details>
                  <div className="trusted-pack-lifecycle-controls">
                    <p className="control-reason">{lifecycleSummary}</p>
                    {awaitingProviderLoadSelfTest && (
                      <>
                        <div className="pack-actions">
                          <button
                            className="primary-action"
                            disabled={busy !== null || !exactSetupDraftReady}
                            aria-describedby={`provider-load-self-test-reason-${candidate.identity.pack_id}`}
                            onClick={() =>
                              void activateOptional(candidate.identity)
                            }
                          >
                            {busy === `optional-activate-${lifecycleKey}`
                              ? "Loading model…"
                              : "Test & activate"}
                          </button>
                        </div>
                        <p
                          id={`provider-load-self-test-reason-${candidate.identity.pack_id}`}
                          className="control-reason provider-load-self-test-reason"
                        >
                          {exactSetupDraftReady
                            ? "Loads and unloads the model on this PC. No game needs to be running."
                            : "Select this model once in the local loadout above before testing."}
                        </p>
                      </>
                    )}
                    {(lifecycleState?.canInstall ||
                      lifecycleState?.canRepair ||
                      lifecycleState?.canRemove) && (
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
                          <b>Allow changes to this local model</b>
                          <small>
                            Required for the install, repair, or remove actions
                            shown below.
                          </small>
                        </span>
                      </label>
                    )}
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
                      {lifecycleState?.canInstall && (
                        <button
                          disabled={
                            busy !== null ||
                            !lifecycleConfirmed ||
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
                      )}
                      {lifecycleState?.canRepair && (
                        <button
                          disabled={
                            busy !== null ||
                            !lifecycleConfirmed ||
                            (lifecycleState.licenseAcceptanceRequired &&
                              !optionalLicenseAccepted)
                          }
                          onClick={() =>
                            void mutateOptional("repair", candidate.identity)
                          }
                        >
                          Repair from signed catalog
                        </button>
                      )}
                      {lifecycleState?.canRemove && (
                        <button
                          disabled={busy !== null || !lifecycleConfirmed}
                          onClick={() =>
                            void mutateOptional("remove", candidate.identity)
                          }
                        >
                          Remove exact pack
                        </button>
                      )}
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
                    {optionalActivationReceipt?.key === lifecycleKey && (
                      <div
                        className="provider-load-self-test-receipt"
                        role="status"
                      >
                        <div>
                          <b>Install check passed</b>
                          <span className="badge good">
                            {optionalActivationReceipt.value.providerLoadDurationMillis.toLocaleString()}{" "}
                            ms
                          </span>
                        </div>
                        <p>The model loaded successfully.</p>
                        <small>
                          Animation quality is not rated by this check.
                        </small>
                        <details className="technical-disclosure">
                          <summary>Technical receipt details</summary>
                          <dl>
                            <div>
                              <dt>Pack identity</dt>
                              <dd>
                                {
                                  optionalActivationReceipt.value.identity
                                    .pack_id
                                }{" "}
                                ·{" "}
                                {
                                  optionalActivationReceipt.value.identity
                                    .revision
                                }
                              </dd>
                            </div>
                            <div>
                              <dt>Native result</dt>
                              <dd>{optionalActivationReceipt.value.detail}</dd>
                            </div>
                            <div>
                              <dt>Scope</dt>
                              <dd>
                                Confirms the packaged provider loaded from its
                                installed files. It does not rate inference,
                                lip-sync, frame rate, or game coexistence.
                              </dd>
                            </div>
                            <div>
                              <dt>Trust domain</dt>
                              <dd>
                                {humanizeBenchmarkKey(
                                  optionalActivationReceipt.value.trustDomain,
                                )}
                              </dd>
                            </div>
                            <div>
                              <dt>Manifest</dt>
                              <dd>
                                {optionalActivationReceipt.value.manifestSha256.slice(
                                  0,
                                  16,
                                )}
                              </dd>
                            </div>
                            <div>
                              <dt>Installed tree</dt>
                              <dd>
                                {optionalActivationReceipt.value.installedContentTreeSha256.slice(
                                  0,
                                  16,
                                )}
                              </dd>
                            </div>
                            <div>
                              <dt>Attestation</dt>
                              <dd>
                                {optionalActivationReceipt.value.attestationSha256.slice(
                                  0,
                                  16,
                                )}
                              </dd>
                            </div>
                          </dl>
                        </details>
                      </div>
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
          Developer review tool. This visual-signal dependency is not a complete
          lip-sync model and is unavailable to normal product sessions.
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
            <details className="technical-disclosure">
              <summary>Experimental pack evidence</summary>
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
            </details>
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
            <details className="technical-disclosure pack-unavailable-reasons">
              <summary>Why actions are available or blocked</summary>
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
            </details>
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
