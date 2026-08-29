import { useMemo, useState } from "react";
import { Button } from "react-aria-components";
import { CHARACTERS, GAME_PROFILES, MODEL_PACKS } from "./data";
import { Icon } from "./icons";
import type {
  AppPreferences,
  DemoState,
  ModelPack,
  PageId,
  StageId,
} from "./types";
import {
  ActionButton,
  Disclosure,
  EmptyState,
  IconButton,
  KeyboardKey,
  Metric,
  MiniBar,
  SectionTitle,
  Skeleton,
  StatusPill,
  Toggle,
} from "./components";
import { ProviderSettings } from "./ProviderSettings";
import { SyntheticReplayCaptureControl } from "./SyntheticReplayCaptureControl";
import {
  syntheticReplayCaptureAvailability,
  type NativeBootstrapHealth,
} from "./tauriBridge";
import {
  simulationEvidenceLabel,
  visibleSimulationText,
  type SimulationEvidence,
} from "./simulationEvidence";

interface PageProps {
  page: PageId;
  state: DemoState;
  preferences: AppPreferences;
  updatePreferences: (patch: Partial<AppPreferences>) => void;
  activeStage: StageId | null;
  isSimulating: boolean;
  startSimulation: () => void;
  simulationEvidence: SimulationEvidence;
  nativeBootstrap: NativeBootstrapHealth;
}

export function CurrentPage(props: PageProps) {
  if (props.state === "loading") return <PageLoading page={props.page} />;
  if (props.state === "empty") return <PageEmpty page={props.page} />;
  const content = (() => {
    switch (props.page) {
      case "home":
        return <HomePage {...props} />;
      case "games":
        return <GamesPage />;
      case "characters":
        return <CharactersPage />;
      case "conversation":
        return <ConversationPage {...props} />;
      case "presence":
        return (
          <PresencePage
            preferences={props.preferences}
            updatePreferences={props.updatePreferences}
          />
        );
      case "performance":
        return (
          <PerformancePage
            preferences={props.preferences}
            updatePreferences={props.updatePreferences}
          />
        );
      case "models":
        return <ModelsPage />;
      case "diagnostics":
        return (
          <DiagnosticsPage
            state={props.state}
            nativeBootstrap={props.nativeBootstrap}
          />
        );
      case "settings":
        return (
          <SettingsPage
            preferences={props.preferences}
            updatePreferences={props.updatePreferences}
          />
        );
      case "help":
        return <HelpPage />;
    }
  })();
  return (
    <>
      {props.state === "error" && (
        <StateBanner
          tone="danger"
          title="Simulated runtime interruption"
          detail="This fixture demonstrates a recovery state; no live worker status was returned."
          action="View fixture"
        />
      )}
      {props.state === "degraded" && (
        <StateBanner
          tone="warning"
          title="Simulated audio-and-subtitle fallback"
          detail="This fixture demonstrates the intended degradation order; no live GPU signal was returned."
          action="View fixture"
        />
      )}
      {content}
    </>
  );
}

function StateBanner({
  tone,
  title,
  detail,
  action,
}: {
  tone: "warning" | "danger";
  title: string;
  detail: string;
  action: string;
}) {
  return (
    <div className={`state-banner state-banner--${tone}`} role="alert">
      <Icon name={tone === "danger" ? "warning" : "performance"} />
      <div>
        <strong>{title}</strong>
        <span>{detail}</span>
      </div>
      <button>
        {action}
        <Icon name="arrow" size={15} />
      </button>
    </div>
  );
}

function HomePage({
  state,
  isSimulating,
  startSimulation,
  simulationEvidence,
  nativeBootstrap,
}: PageProps) {
  const active = isSimulating || state === "active";
  const nativeEvidence = simulationEvidence.source === "nativeRuntime";
  const runtimeText = visibleSimulationText(simulationEvidence);
  const evidenceLabel = simulationEvidenceLabel(simulationEvidence);
  const nativeSnapshot =
    nativeBootstrap.kind === "snapshot" ? nativeBootstrap.snapshot : null;
  const desktopAuthenticated = Boolean(
    nativeSnapshot?.runtime.connected && nativeSnapshot.mediaBroker.connected,
  );
  const debugCaptureEnabled =
    nativeSnapshot?.capabilities?.debugSyntheticReplayCapture === true;
  return (
    <div className="page page--home">
      <section
        className={`native-health-strip ${desktopAuthenticated ? "is-authenticated" : ""}`}
        aria-label="Native desktop health"
        aria-live="polite"
      >
        <span className="native-health-strip__signal" />
        <div>
          <span>DESKTOP CONTROL PATH</span>
          <strong>
            {desktopAuthenticated
              ? "Runtime and media broker authenticated"
              : nativeBootstrap.kind === "loading"
                ? "Checking the native runtime…"
                : nativeBootstrap.kind === "browserPreview"
                  ? "Browser preview · native health unavailable"
                  : nativeBootstrap.kind === "unavailable"
                    ? "Native bootstrap unavailable"
                    : "Native connection incomplete"}
          </strong>
        </div>
        <small>
          {nativeSnapshot
            ? `Runtime ${nativeSnapshot.runtime.state} · broker ${nativeSnapshot.mediaBroker.state} · attempt ${nativeBootstrap.attempts}`
            : nativeBootstrap.kind === "unavailable"
              ? nativeBootstrap.detail
              : "No authenticated process claim is shown until the desktop bridge responds."}
        </small>
      </section>
      <section className={`command-deck ${active ? "is-live" : ""}`}>
        <div className="command-deck__copy">
          <div className="eyebrow">
            {active
              ? nativeEvidence
                ? "NATIVE RUNTIME EVENTS · DETERMINISTIC FIXTURE INPUT"
                : simulationEvidence.source === "awaitingNative"
                  ? "CONNECTING TO NATIVE RUNTIME"
                  : "BROWSER-ONLY SIMULATION · FIXTURE"
              : "AUTHORED CATALOG · RUNTIME UNVERIFIED"}
          </div>
          <h1>
            {active ? (
              <>
                Mara is{" "}
                <em>
                  {nativeEvidence
                    ? "answering through native events."
                    : "answering."}
                </em>
              </>
            ) : (
              <>
                Your worlds are authored.
                <br />
                <em>Connect the runtime to verify.</em>
              </>
            )}
          </h1>
          <p>
            {active
              ? nativeEvidence
                ? "Eclipse Harbor fixture crossed the native Tauri bridge. This is runtime event evidence, not live-game certification."
                : "Eclipse Harbor · browser-authored timing preview · no native event evidence yet"
              : "No live game, model pack, capture source, or certified capability has been reported by the runtime."}
          </p>
          <div className="command-actions">
            <ActionButton
              icon={active ? "pause" : "play"}
              onPress={startSimulation}
            >
              {active ? "End simulation" : "Run a private simulation"}
            </ActionButton>
            <ActionButton variant="outline" icon="games">
              Choose a game
            </ActionButton>
          </div>
        </div>
        <div
          className="command-deck__scene"
          aria-label={
            active
              ? "Active Eclipse Harbor simulation"
              : "Ready signal visualization"
          }
        >
          <div className="scene-grid" />
          <div className="scene-horizon" />
          <div className="scene-radar">
            <i />
            <i />
            <i />
            <span />
          </div>
          <div className="scene-character">
            <span>MV</span>
            <i />
          </div>
          <div className="scene-transcript">
            {active ? (
              <>
                <span>{evidenceLabel} · MARA VENN</span>
                {runtimeText ??
                  (nativeEvidence
                    ? "Waiting for a sentence-ready event from the native runtime…"
                    : "The eastern lock is clear. I’ll keep the lantern on for you.")}
              </>
            ) : (
              <>
                <span>FIXTURE PREVIEW · NO LIVE CAPTURE</span>Connect the
                runtime before starting a real game session.
              </>
            )}
          </div>
          <div className="scene-readout">
            <span>
              {active
                ? nativeEvidence
                  ? `EVENT SEQ ${Math.max(0, simulationEvidence.sequence)}`
                  : "BROWSER CLOCK"
                : "NO LIVE TURN"}
            </span>
            <span>
              {active
                ? nativeEvidence
                  ? "NATIVE BRIDGE"
                  : "BROWSER FIXTURE"
                : "CATALOG ONLY"}
            </span>
          </div>
        </div>
      </section>
      {debugCaptureEnabled && (
        <SyntheticReplayCaptureControl
          availability={syntheticReplayCaptureAvailability(true)}
        />
      )}
      <section className="home-strip">
        <div>
          <span className="home-strip__label">ACTIVE PROFILE</span>
          <div className="active-profile">
            <span
              className="game-monogram"
              style={
                {
                  "--game-accent": active ? "#63d6c3" : "#e4c966",
                } as React.CSSProperties
              }
            >
              {active ? "EH" : "—"}
            </span>
            <div>
              <strong>
                {active
                  ? nativeEvidence
                    ? "Eclipse Harbor · native event fixture"
                    : "Eclipse Harbor · browser fixture"
                  : "No live game reported"}
              </strong>
              <small>
                {active
                  ? nativeEvidence
                    ? "Authenticated desktop runtime event path"
                    : "Illustrative UI path · no native evidence"
                  : "Authored profiles are not detection evidence"}
              </small>
            </div>
          </div>
        </div>
        <div>
          <span className="home-strip__label">CONFIGURED FIXTURE KEY</span>
          <strong className="key-readout">
            <KeyboardKey>V</KeyboardKey> Push to talk
          </strong>
        </div>
        <div>
          <span className="home-strip__label">
            PREFERENCE · NOT A LIVE ROUTE
          </span>
          <strong>
            Hybrid <StatusPill tone="neutral">Not connected</StatusPill>
          </strong>
        </div>
        <div>
          <span className="home-strip__label">ESTIMATED IMPACT</span>
          <strong>
            SIM 1.8% <small>fixture · not measured</small>
          </strong>
          <MiniBar value={18} label="Illustrative frame impact fixture" />
        </div>
      </section>
      <div className="home-grid">
        <section className="instrument-panel conversation-preview">
          <SectionTitle
            eyebrow={
              nativeEvidence
                ? "NATIVE RUNTIME SIGNAL"
                : "BROWSER FIXTURE SIGNAL"
            }
            title="Conversation memory"
            action={
              <button className="inline-link">
                Open conversation <Icon name="arrow" size={14} />
              </button>
            }
          />
          <div className="dialogue-line dialogue-line--player">
            <span>YOU · FIXTURE TURN</span>
            <p>Did you ever make it to the old lighthouse?</p>
          </div>
          <div className="dialogue-line dialogue-line--npc">
            <span>
              MARA VENN ·{" "}
              {nativeEvidence ? evidenceLabel : "BROWSER FIXTURE TEXT"}
            </span>
            <p>
              {runtimeText ??
                "Not yet. The eastern lock jammed again—but I remembered what you said about the service tunnel."}
            </p>
            <div className="memory-tags">
              <span>
                <Icon name="check" size={12} />
                {nativeEvidence
                  ? "Text returned through the native bridge"
                  : "Used 2 browser fixture memories"}
              </span>
              <span>
                {nativeEvidence &&
                simulationEvidence.fixtureFirstAudioMs != null
                  ? `Runtime fixture first-audio field · ${simulationEvidence.fixtureFirstAudioMs} ms`
                  : "Illustrative browser timing · 1.31 s"}
              </span>
            </div>
          </div>
        </section>
        <section className="instrument-panel headroom-panel">
          <SectionTitle
            eyebrow="ILLUSTRATIVE HEADROOM"
            title="Simulation model"
            action={<StatusPill tone="warn">Not measured</StatusPill>}
          />
          <HeadroomRow
            label="Game GPU"
            value="62%"
            amount={62}
            detail="Eclipse Harbor load fixture"
          />
          <HeadroomRow
            label="NPC GPU"
            value="18%"
            amount={18}
            detail="Illustrative 7.5 GB policy cap"
          />
          <HeadroomRow
            label="CPU"
            value="31%"
            amount={31}
            detail="Synthetic speech + voice load"
          />
          <HeadroomRow
            label="Frame impact"
            value="1.8%"
            amount={18}
            detail="Fixture only · run a benchmark"
          />
          <div className="degrade-note">
            <Icon name="shield" />
            <span>Visual features yield before the game or conversation.</span>
          </div>
        </section>
        <section className="instrument-panel readiness-panel">
          <SectionTitle eyebrow="FIXTURE READINESS" title="Before you play" />
          <ul className="readiness-list">
            <li>
              <span>
                <Icon name="check" />
              </span>
              <div>
                <strong>Microphone path modeled</strong>
                <small>Run the real rehearsal before play</small>
              </div>
            </li>
            <li>
              <span>
                <Icon name="check" />
              </span>
              <div>
                <strong>Zero local packs provisioned</strong>
                <small>Catalog candidates are not installed models</small>
              </div>
            </li>
            <li>
              <span>
                <Icon name="check" />
              </span>
              <div>
                <strong>Safety policy modeled</strong>
                <small>
                  Expected policy only · runtime check still required
                </small>
              </div>
            </li>
          </ul>
          <button className="panel-action">
            Open diagnostics <Icon name="arrow" />
          </button>
        </section>
      </div>
    </div>
  );
}

function HeadroomRow({
  label,
  value,
  amount,
  detail,
}: {
  label: string;
  value: string;
  amount: number;
  detail: string;
}) {
  return (
    <div className="headroom-row">
      <div>
        <strong>{label}</strong>
        <span>{detail}</span>
      </div>
      <b>{value}</b>
      <MiniBar value={amount} tone={amount > 75 ? "amber" : "teal"} />
    </div>
  );
}

function GamesPage() {
  const [filter, setFilter] = useState<"all" | "authored" | "offline">("all");
  const [query, setQuery] = useState("");
  const filtered = GAME_PROFILES.filter(
    (game) =>
      (filter === "all" ||
        (filter === "authored" && game.state === "authored") ||
        (filter === "offline" && game.state === "offline-policy")) &&
      game.title.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div className="page">
      <PageHero
        eyebrow="PROFILE LIBRARY · 20 AUTHORED WORLDS"
        title="Choose a world."
        description="These are authored catalog packages. Installation, game-build compatibility, character counts, and capabilities remain unverified until returned by the runtime."
        actions={
          <ActionButton icon="refresh" variant="outline" isDisabled>
            Connect runtime to scan
          </ActionButton>
        }
      />
      <div className="library-toolbar">
        <label className="search-field">
          <Icon name="search" />
          <span className="sr-only">Search profiles</span>
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search games, stores, or capabilities"
          />
        </label>
        <div className="filter-tabs" aria-label="Filter profiles">
          {(["all", "authored", "offline"] as const).map((item) => (
            <button
              key={item}
              className={filter === item ? "is-active" : ""}
              onClick={() => setFilter(item)}
            >
              {item === "all"
                ? "All 20"
                : item === "authored"
                  ? "Authored catalog"
                  : "Offline policy"}
            </button>
          ))}
        </div>
      </div>
      <div className="profile-grid">
        {filtered.map((game) => (
          <article key={game.id} className="profile-card">
            <div className="profile-card__top">
              <span
                className="game-monogram game-monogram--large"
                style={{ "--game-accent": game.accent } as React.CSSProperties}
              >
                {game.abbreviation}
              </span>
              <div className="profile-card__status">
                <StatusPill
                  tone={game.state === "offline-policy" ? "warn" : "neutral"}
                >
                  {game.state === "offline-policy"
                    ? "Authored · offline policy"
                    : "Authored · not live-certified"}
                </StatusPill>
                <IconButton
                  label={`More options for ${game.title}`}
                  icon="more"
                />
              </div>
            </div>
            <span className="profile-card__wave">WAVE {game.wave}</span>
            <h2>{game.title}</h2>
            <p>{game.description}</p>
            <dl>
              <div>
                <dt>Content</dt>
                <dd>Authored profile package</dd>
              </div>
              <div>
                <dt>Certification</dt>
                <dd>{game.capability}</dd>
              </div>
              <div>
                <dt>Installation</dt>
                <dd>{game.store}</dd>
              </div>
            </dl>
            <button className="profile-card__action">
              Open authored profile
              <Icon name="arrow" />
            </button>
          </article>
        ))}
        <article className="profile-card profile-card--generic">
          <div className="generic-orbit">
            <Icon name="games" />
          </div>
          <span className="profile-card__wave">EXPERIMENTAL</span>
          <h2>Another single-player game</h2>
          <p>
            Use a manual character name, conversation memory, audio, and
            subtitles with any safely capturable game.
          </p>
          <Disclosure tone="info" title="Clear limits">
            Identity and screen-space lip-sync remain experimental.
          </Disclosure>
          <button className="profile-card__action">
            Set up generic mode <Icon name="arrow" />
          </button>
        </article>
      </div>
    </div>
  );
}

function CharactersPage() {
  const [selected, setSelected] = useState(0);
  const character = CHARACTERS[selected];
  return (
    <div className="page">
      <PageHero
        eyebrow="CHARACTER DIRECTORY"
        title="Familiar voices, separate memories."
        description="Each character keeps their own voice direction, boundaries, relationship history, and game-specific context."
        actions={
          <ActionButton icon="characters">Add custom character</ActionButton>
        }
      />
      <div className="character-workspace">
        <div className="character-list">
          <label className="search-field search-field--compact">
            <Icon name="search" />
            <input
              aria-label="Search characters"
              placeholder="Search characters"
            />
          </label>
          {CHARACTERS.map((item, index) => (
            <button
              key={item.name}
              className={`character-row ${selected === index ? "is-selected" : ""}`}
              onClick={() => setSelected(index)}
            >
              <span
                className="character-avatar"
                style={
                  { "--avatar-accent": item.accent } as React.CSSProperties
                }
              >
                {item.monogram}
              </span>
              <span>
                <strong>{item.name}</strong>
                <small>
                  {item.game} · {item.last}
                </small>
              </span>
              <Icon name="chevron" />
            </button>
          ))}
        </div>
        <section className="character-detail">
          <div className="character-detail__hero">
            <div
              className="character-portrait"
              style={
                { "--avatar-accent": character.accent } as React.CSSProperties
              }
            >
              <span>{character.monogram}</span>
              <i />
              <i />
              <i />
            </div>
            <div>
              <span className="eyebrow">{character.game}</span>
              <h1>{character.name}</h1>
              <p>{character.role}</p>
              <div className="character-pills">
                <StatusPill tone="teal">{character.bond}</StatusPill>
                <StatusPill tone="neutral">
                  {character.memories} memories
                </StatusPill>
                <StatusPill tone="neutral">
                  Voice intent only · no pack
                </StatusPill>
              </div>
            </div>
            <IconButton label="Character options" icon="more" />
          </div>
          <div className="character-detail__grid">
            <div className="detail-sheet">
              <span className="detail-sheet__label">PERSONALITY DIRECTION</span>
              <h3>Composed under pressure, exact with details.</h3>
              <p>
                Speaks in short, practical observations. Warms gradually, avoids
                grand declarations, and never invents knowledge beyond the
                current spoiler tier.
              </p>
              <button>
                Edit direction <Icon name="arrow" />
              </button>
            </div>
            <div className="detail-sheet">
              <span className="detail-sheet__label">VOICE CHARACTER</span>
              <h3>{character.voice}</h3>
              <p>
                Provider-neutral direction. No game audio or performer likeness
                is used.
              </p>
              <button>
                Preview safe sample <Icon name="play" />
              </button>
            </div>
            <div className="detail-sheet detail-sheet--wide">
              <span className="detail-sheet__label">
                RECENT RELATIONSHIP THREAD
              </span>
              <div className="relationship-track">
                <span>First contact</span>
                <i />
                <span>Earned trust</span>
                <i />
                <span className="is-current">{character.bond}</span>
              </div>
              <p>
                Last remembered: You offered to help inspect the eastern service
                tunnel after the lock failed.
              </p>
              <button>
                Review memory ledger <Icon name="arrow" />
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

function ConversationPage({
  preferences,
  updatePreferences,
  simulationEvidence,
}: Pick<
  PageProps,
  "preferences" | "updatePreferences" | "simulationEvidence"
>) {
  const [text, setText] = useState("");
  const [showDetails, setShowDetails] = useState(false);
  const nativeEvidence = simulationEvidence.source === "nativeRuntime";
  const runtimeText = visibleSimulationText(simulationEvidence);
  const evidenceLabel = simulationEvidenceLabel(simulationEvidence);
  return (
    <div className="page">
      <PageHero
        eyebrow="CONVERSATION LEDGER"
        title="What was said. What was kept."
        description="Only delivered dialogue becomes history. Proposed memories stay inspectable and removable."
        actions={
          <>
            <ActionButton variant="outline" icon="download">
              Export visible history
            </ActionButton>
            <ActionButton variant="quiet" icon="more">
              Filters
            </ActionButton>
          </>
        }
      />
      <div className="conversation-layout">
        <aside className="conversation-sessions">
          <span className="panel-label">SESSIONS</span>
          {[
            "Today · Eclipse Harbor",
            "Yesterday · Cyberpunk 2077",
            "Aug 24 · Skyrim SE",
            "Aug 19 · Baldur’s Gate 3",
          ].map((session, index) => (
            <button key={session} className={index === 0 ? "is-selected" : ""}>
              <span>{session}</span>
              <small>
                {index === 0
                  ? nativeEvidence
                    ? "Mara Venn · native event evidence"
                    : "Mara Venn · browser fixture"
                  : index === 1
                    ? "Jackie Welles · 12 turns"
                    : index === 2
                      ? "Aela · 8 turns"
                      : "Shadowheart · 15 turns"}
              </small>
            </button>
          ))}
          <button className="conversation-sessions__archive">
            <Icon name="folder" />
            Archived sessions
          </button>
        </aside>
        <section className="conversation-ledger">
          <div
            className={`runtime-evidence-banner ${nativeEvidence ? "is-native" : "is-fixture"}`}
            role="status"
          >
            <Icon name={nativeEvidence ? "check" : "warning"} />
            <div>
              <span>{evidenceLabel}</span>
              <strong>
                {nativeEvidence
                  ? "Dialogue below is sourced from the desktop runtime event channel."
                  : "Dialogue below is authored browser fixture content, not runtime output."}
              </strong>
            </div>
          </div>
          <header>
            <div>
              <span className="eyebrow">
                ECLIPSE HARBOR ·{" "}
                {nativeEvidence ? "NATIVE EVENT FIXTURE" : "BROWSER FIXTURE"}
              </span>
              <h2>Mara Venn</h2>
              <p>
                {nativeEvidence
                  ? `Runtime simulation ${simulationEvidence.simulationId ?? "pending ID"} · event ${Math.max(0, simulationEvidence.sequence)}`
                  : "Illustrative session · no native event returned"}
              </p>
            </div>
            <StatusPill tone={nativeEvidence ? "ok" : "warn"}>
              {nativeEvidence
                ? simulationEvidence.phase === "delivered"
                  ? "Native delivered"
                  : "Native event"
                : "Browser fixture"}
            </StatusPill>
          </header>
          <div className="turn">
            <div className="turn__meta">
              <span>YOU</span>
              <time>3:41:08 PM</time>
            </div>
            <p>Did you ever make it to the old lighthouse?</p>
            <div className="turn__trace">
              <span>AUTHORED FIXTURE INPUT · NOT LIVE STT</span>
              <span>Deterministic test prompt</span>
            </div>
          </div>
          <div className="turn turn--npc">
            <div className="turn__meta">
              <span>MARA VENN</span>
              <time>3:41:10 PM</time>
            </div>
            <p>
              {runtimeText ??
                "The eastern lock jammed again, so not yet. But I remembered what you said about the service tunnel. If the tide stays low, I can try it before dark."}
            </p>
            <div className="turn__trace">
              <span>
                <Icon name={nativeEvidence ? "check" : "headphones"} />
                {nativeEvidence
                  ? evidenceLabel
                  : "BROWSER first-audio fixture · 1.31 s"}
              </span>
              <span>
                {nativeEvidence
                  ? simulationEvidence.fixtureFirstAudioMs != null
                    ? `${simulationEvidence.fixtureFirstAudioMs} ms runtime fixture field`
                    : "No audio timing event yet"
                  : "Illustrative duration · 4.2 s"}
              </span>
              <button onClick={() => setShowDetails(!showDetails)}>
                {showDetails ? "Hide trace" : "View trace"}{" "}
                <Icon name="chevron" />
              </button>
            </div>
            {showDetails && (
              <div className="trace-detail">
                <div>
                  <span>MEMORY</span>
                  <strong>2 passages · 53 ms</strong>
                </div>
                <div>
                  <span>REPLY</span>
                  <strong>
                    {nativeEvidence
                      ? `Native event sequence ${Math.max(0, simulationEvidence.sequence)}`
                      : "BROWSER SIM 478 ms fixture"}
                  </strong>
                </div>
                <div>
                  <span>VOICE</span>
                  <strong>
                    {nativeEvidence
                      ? "Not proven by the text event"
                      : "Illustrative Kokoro route"}
                  </strong>
                </div>
                <div>
                  <span>EFFECTS</span>
                  <strong>Calm · intensity 0.32</strong>
                </div>
              </div>
            )}
            <div className="memory-proposal">
              <Icon name="spark" />
              <div>
                <span>MEMORY PROPOSAL</span>
                <strong>
                  Mara plans to try the eastern service tunnel before dark.
                </strong>
                <small>
                  Not yet saved · relationship-neutral · expires in 30 days
                </small>
              </div>
              <div>
                <button className="accept">Keep</button>
                <button>Dismiss</button>
              </div>
            </div>
          </div>
          <div className="typed-reply">
            <textarea
              value={text}
              onChange={(event) => setText(event.target.value)}
              placeholder="Type a reply when speaking is not convenient…"
              aria-label="Typed reply"
            />
            <button disabled={!text}>
              <Icon name="arrow" />
            </button>
            <span>
              <KeyboardKey>Ctrl</KeyboardKey> + <KeyboardKey>Enter</KeyboardKey>{" "}
              to send
            </span>
          </div>
        </section>
        <aside className="conversation-inspector">
          <span className="panel-label">SESSION CONTROL</span>
          <Toggle
            label="Subtitles"
            description="Show every delivered line over the game."
            isSelected={preferences.subtitles}
            onChange={(subtitles) => updatePreferences({ subtitles })}
          />
          <div className="inspector-block">
            <span>ROUTE</span>
            <strong>Local speech → OpenAI reply → local voice</strong>
            <small>Conversation text leaves this PC. Audio does not.</small>
          </div>
          <div className="inspector-block">
            <span>SPOILER BOUNDARY</span>
            <strong>Current quest progress only</strong>
            <small>Later story knowledge remains unavailable.</small>
          </div>
          <Disclosure title="Memory is inspectable">
            Facts, episodes, relationship state, and summaries stay separate.
            You can remove any item without rewriting past dialogue.
          </Disclosure>
        </aside>
      </div>
    </div>
  );
}

function PresencePage({
  preferences,
  updatePreferences,
}: Pick<PageProps, "preferences" | "updatePreferences">) {
  return (
    <div className="page">
      <PageHero
        eyebrow="PRESENCE & PRESENTATION"
        title="Seen only when useful."
        description="Conversation never depends on a camera or visible face. Optional perception has strict boundaries and immediate fallbacks."
      />
      <div className="presence-hero">
        <div className="presence-preview">
          <div className="capture-frame">
            <span className="capture-corner capture-corner--tl" />
            <span className="capture-corner capture-corner--tr" />
            <span className="capture-corner capture-corner--bl" />
            <span className="capture-corner capture-corner--br" />
            <div className="face-anchor">
              <span>SIM MARA VENN · 96% FIXTURE</span>
              <i />
              <i />
              <i />
            </div>
            <div className="preview-subtitle">The eastern lock is clear.</div>
            <div className="capture-readout">
              <span>WGC · GAME WINDOW ONLY</span>
              <span>FRAME AGE 18 ms</span>
            </div>
          </div>
        </div>
        <div className="presence-control">
          <span className="eyebrow">AUTHORED CAPABILITY PROPOSAL</span>
          <h2>External capture candidate</h2>
          <p>
            The authored profile proposes game-window capture and exclusions. No
            live capture or build certification has been reported.
          </p>
          <div className="capability-ladder">
            <div className="is-available">
              <span>01</span>
              <div>
                <strong>External game-window capture</strong>
                <small>Windows capture only · no injection into the game</small>
              </div>
              <StatusPill tone="neutral">Candidate</StatusPill>
            </div>
            <div className="is-active">
              <span>02</span>
              <div>
                <strong>Screen-space mouth motion</strong>
                <small>Optional experiment · simulated confidence 96%</small>
              </div>
              <StatusPill tone="teal">Experimental</StatusPill>
            </div>
            <div>
              <span>03</span>
              <div>
                <strong>Audio + subtitles</strong>
                <small>Immediate fallback for uncertainty or occlusion</small>
              </div>
              <StatusPill tone="ok">Always ready</StatusPill>
            </div>
          </div>
        </div>
      </div>
      <section
        className="frame-continuity"
        aria-labelledby="frame-continuity-title"
      >
        <div className="frame-continuity__header">
          <div>
            <span className="eyebrow">CURRENT-FRAME CONTRACT</span>
            <h2 id="frame-continuity-title">The game frame stays in charge.</h2>
          </div>
          <p>
            A visual model may propose mouth motion. It never owns the face,
            pauses capture, or replaces the full frame.
          </p>
        </div>
        <div className="frame-continuity__rail">
          <div className="continuity-node continuity-node--source">
            <span>01 · LIVE</span>
            <strong>Newest game frame</strong>
            <small>Immutable source</small>
          </div>
          <i aria-hidden="true" />
          <div className="continuity-node">
            <span>02 · TRACK</span>
            <strong>Lower-mouth anchor</strong>
            <small>Current actor + pose</small>
          </div>
          <i aria-hidden="true" />
          <div className="continuity-node continuity-node--residual">
            <span>03 · PROPOSE</span>
            <strong>Mouth residual only</strong>
            <small>No full-face pixels</small>
          </div>
          <i aria-hidden="true" />
          <div className="continuity-node continuity-node--display">
            <span>04 · DISPLAY</span>
            <strong>Composite if fresh</strong>
            <small>Strict mask + frame age</small>
          </div>
        </div>
        <div className="fail-open-rule">
          <span className="fail-open-rule__mark">
            <Icon name="shield" size={19} />
            FAIL OPEN
          </span>
          <strong>Uncertain, occluded, stale, or wrong actor?</strong>
          <p>
            Drop the residual and show the untouched game on the next displayed
            frame. Voice and subtitles continue.
          </p>
        </div>
      </section>
      <div className="settings-sections">
        <section className="settings-sheet">
          <SectionTitle eyebrow="GAME AWARENESS" title="What can be observed" />
          <Toggle
            label="Read the selected game window"
            description="Capture only the manually selected game window for character and context cues; never inject into its process."
            isSelected={preferences.screenPresence}
            onChange={(screenPresence) => updatePreferences({ screenPresence })}
            privacy="Frames discarded after local analysis"
          />
          <Toggle
            label="Track the speaking character"
            description="Keep a temporary in-session actor identity across brief occlusion."
            isSelected={preferences.screenPresence}
            onChange={() => undefined}
            disabled={!preferences.screenPresence}
            privacy="No biometric identity database"
          />
          <Toggle
            label="Read interface-safe context"
            description="Exclude known HUD, chat, password, and notification regions."
            isSelected={true}
            onChange={() => undefined}
            privacy="Generic local exclusion masks"
          />
        </section>
        <section className="settings-sheet">
          <SectionTitle eyebrow="FACIAL RESPONSE" title="Optional motion" />
          <Toggle
            label="Keep presentation external to the game"
            description="Use overlays, audio, and subtitles only—no mods, hooks, injectors, or native-rig control."
            isSelected={true}
            onChange={() => undefined}
            disabled
          />
          <Toggle
            label="Try screen-space mouth motion"
            description="Edit only a tracked mouth residual; restore the untouched frame on uncertainty."
            isSelected={preferences.screenPresence}
            onChange={(screenPresence) => updatePreferences({ screenPresence })}
            privacy="Experimental · GPU lease required"
          />
          <div className="confidence-rule">
            <div>
              <span>Minimum face confidence</span>
              <strong>88%</strong>
            </div>
            <input
              type="range"
              min="70"
              max="99"
              defaultValue="88"
              aria-label="Minimum face confidence"
            />
            <small>
              Below this point, the original game frame returns within one
              displayed frame.
            </small>
          </div>
        </section>
        <section className="settings-sheet">
          <SectionTitle eyebrow="YOUR CAMERA" title="Player presence" />
          <Toggle
            label="Let characters react to my presence"
            description="Opt-in local pose and broad expression cues. Never infers age, race, gender, or identity."
            isSelected={false}
            onChange={() => undefined}
            privacy="Off · local only · no recording"
          />
          <Disclosure title="Designed to stay off">
            Camera access is not requested during normal setup. Enabling it
            requires a separate Windows permission and live preview.
          </Disclosure>
        </section>
      </div>
    </div>
  );
}

function PerformancePage({
  preferences,
  updatePreferences,
}: Pick<PageProps, "preferences" | "updatePreferences">) {
  const modes = [
    "competitive",
    "fast",
    "balanced",
    "immersive",
    "maximum",
    "custom",
  ] as const;
  return (
    <div className="page">
      <PageHero
        eyebrow="RESOURCE BROKER"
        title="The game gets first claim."
        description="This fixture demonstrates the intended budget presentation. No live hardware measurements have been returned."
        actions={
          <ActionButton icon="play" isDisabled>
            Connect runtime to benchmark
          </ActionButton>
        }
      />
      <div className="performance-summary">
        <div className="performance-gauge">
          <svg viewBox="0 0 160 90" aria-hidden="true">
            <path d="M15 78a65 65 0 0 1 130 0" />
            <path className="gauge-value" d="M15 78a65 65 0 0 1 98-55" />
          </svg>
          <strong>
            SIM 1.8<small>%</small>
          </strong>
          <span>ILLUSTRATIVE GAME IMPACT</span>
        </div>
        <div className="performance-verdict">
          <StatusPill tone="warn">Simulation fixture</StatusPill>
          <h2>8.2 ms of illustrative headroom</h2>
          <p>
            This previews how a controlled benchmark will report frame lows and
            fallback decisions; no live run is represented here.
          </p>
        </div>
        <div className="reference-compare">
          <span>ILLUSTRATIVE REFERENCE vs THIS PC</span>
          <div>
            <small>First audio</small>
            <strong>SIM 1.31 s</strong>
            <em>fixture</em>
          </div>
          <div>
            <small>Frame impact</small>
            <strong>SIM 1.8%</strong>
            <em>fixture</em>
          </div>
          <div>
            <small>VRAM peak</small>
            <strong>SIM 6.7 GB</strong>
            <em>fixture</em>
          </div>
        </div>
      </div>
      <section className="admission-console" aria-labelledby="admission-title">
        <div className="admission-console__copy">
          <span className="eyebrow">LOCAL RESOURCE ADMISSION</span>
          <h2 id="admission-title">Measure before a model loads.</h2>
          <p>
            Admission uses live game headroom and the selected performance
            reserve. An installed-but-cold alternative is never silently loaded
            or moved to cloud.
          </p>
        </div>
        <div
          className="admission-console__states"
          aria-label="Admission states"
        >
          <AdmissionState
            value="Fits"
            detail="Verified inside the game reserve"
          />
          <AdmissionState
            value="CPU-only"
            detail="Verified without a GPU lease"
          />
          <AdmissionState
            value="Conflicts"
            detail="Would consume protected headroom"
          />
          <AdmissionState
            value="Unverified"
            detail="Manifest or live measurement missing"
          />
        </div>
      </section>
      <section className="mode-selector">
        <div className="mode-selector__head">
          <div>
            <span className="eyebrow">PERFORMANCE MODE</span>
            <h2>Balanced</h2>
          </div>
          <p>Preference only; the runtime has not verified this target.</p>
        </div>
        <div className="mode-rail">
          {modes.map((mode, index) => (
            <button
              key={mode}
              className={preferences.performance === mode ? "is-selected" : ""}
              onClick={() => updatePreferences({ performance: mode })}
            >
              <i />
              <span>
                {mode === "maximum"
                  ? "Maximum"
                  : mode[0].toUpperCase() + mode.slice(1)}
              </span>
              <small>
                {["2%", "3%", "5%", "10%", "15%", "Custom"][index]} avg cap
              </small>
            </button>
          ))}
        </div>
      </section>
      <div className="performance-grid">
        <section className="instrument-panel">
          <SectionTitle
            eyebrow="ILLUSTRATIVE TURN · P95"
            title="Latency budget"
          />
          <div className="waterfall">
            <WaterfallRow
              label="End speech"
              start={0}
              width={9}
              value="SIM 412 ms"
              tone="neutral"
            />
            <WaterfallRow
              label="Transcribe"
              start={9}
              width={14}
              value="SIM 286 ms"
              tone="teal"
            />
            <WaterfallRow
              label="Identify + memory"
              start={23}
              width={8}
              value="SIM 94 ms"
              tone="purple"
            />
            <WaterfallRow
              label="First reply clause"
              start={31}
              width={25}
              value="SIM 478 ms"
              tone="teal"
            />
            <WaterfallRow
              label="First voice audio"
              start={56}
              width={14}
              value="SIM 221 ms"
              tone="amber"
            />
          </div>
          <div className="latency-total">
            <span>SPEECH END → FIRST AUDIO</span>
            <strong>SIM 1.31 s</strong>
            <StatusPill tone="neutral">Fixture comparison only</StatusPill>
          </div>
        </section>
        <section className="instrument-panel">
          <SectionTitle eyebrow="ILLUSTRATIVE BUDGETS" title="Resources" />
          <HeadroomRow
            label="VRAM ceiling"
            value="SIM 6.7 / 7.5 GB"
            amount={89}
            detail="May overshoot 256 MB for ≤ 2 sec"
          />
          <HeadroomRow
            label="System memory"
            value="SIM 9.8 / 13.6 GB"
            amount={72}
            detail="2.4 GB reserved for game spikes"
          />
          <HeadroomRow
            label="NPC CPU"
            value="SIM 16%"
            amount={32}
            detail="8 performance cores available"
          />
          <HeadroomRow
            label="Frame time"
            value="SIM 8.9 / 11.4 ms"
            amount={78}
            detail="87 FPS 1% low target"
          />
        </section>
        <section className="instrument-panel degradation-order">
          <SectionTitle
            eyebrow="AUTOMATIC DEGRADATION"
            title="What yields first"
          />
          <ol>
            <li>
              <span>01</span>
              <div>
                <strong>Reduce continuous vision</strong>
                <small>Keep recent context; sample fewer frames</small>
              </div>
            </li>
            <li>
              <span>02</span>
              <div>
                <strong>Disable screen-space motion</strong>
                <small>Continue voice and subtitles</small>
              </div>
            </li>
            <li>
              <span>03</span>
              <div>
                <strong>Neutralize optional effects</strong>
                <small>Emotion metadata becomes a no-op</small>
              </div>
            </li>
            <li>
              <span>04</span>
              <div>
                <strong>Use recent + text memory</strong>
                <small>Skip vector retrieval for this turn</small>
              </div>
            </li>
          </ol>
        </section>
      </div>
    </div>
  );
}

function AdmissionState({
  value,
  detail,
}: {
  value: ModelPack["admission"];
  detail: string;
}) {
  const tone =
    value === "Fits"
      ? "ok"
      : value === "CPU-only"
        ? "teal"
        : value === "Conflicts"
          ? "danger"
          : "neutral";
  return (
    <div className={`admission-state admission-state--${value.toLowerCase()}`}>
      <StatusPill tone={tone}>{value}</StatusPill>
      <small>{detail}</small>
    </div>
  );
}

function WaterfallRow({
  label,
  start,
  width,
  value,
  tone,
}: {
  label: string;
  start: number;
  width: number;
  value: string;
  tone: string;
}) {
  return (
    <div className="waterfall-row">
      <span>{label}</span>
      <div>
        <i
          className={`tone-${tone}`}
          style={{ marginLeft: `${start}%`, width: `${width}%` }}
        />
      </div>
      <strong>{value}</strong>
    </div>
  );
}

function ModelsPage() {
  const models = MODEL_PACKS;
  return (
    <div className="page">
      <PageHero
        eyebrow="MODEL MANAGER"
        title="Local motion is still research."
        description="The product remains API-first. These three generic visual paths are comparison lanes—not installed packs, selectable routes, or download offers."
        actions={
          <ActionButton icon="download" variant="outline" isDisabled>
            No qualified visual packs
          </ActionButton>
        }
      />
      <div className="model-summary">
        <div>
          <span>INSTALLED FOOTPRINT</span>
          <strong>
            0 <small>optional lip-sync packs</small>
          </strong>
          <MiniBar value={0} />
          <small>No runtime installation inventory was returned.</small>
        </div>
        <div>
          <span>BASELINE ROUTE</span>
          <strong>
            API first <small>no local model required</small>
          </strong>
          <StatusPill tone="teal">User connects providers</StatusPill>
          <small>LLM, STT, and TTS use explicitly selected API routes.</small>
        </div>
        <div>
          <span>VISUAL SAFETY</span>
          <strong>
            Current frame wins <Icon name="shield" size={19} />
          </strong>
          <small>Every proposed residual is disposable and fail-open.</small>
        </div>
      </div>
      <section
        className="visual-research model-table"
        aria-labelledby="visual-research-title"
      >
        <div className="visual-research__heading">
          <div>
            <span className="eyebrow">THREE-LANE QUALIFICATION</span>
            <h2 id="visual-research-title">
              From motion signal to mouth residual.
            </h2>
          </div>
          <p>
            None of these paths can be selected. A route appears only after
            license, artifact, Windows, latency, quality, and game-contention
            gates pass.
          </p>
        </div>
        <div className="visual-research__grid">
          {models.map((model) => (
            <article
              className={`research-path research-path--${model.id}`}
              key={model.id}
            >
              <header>
                <div className="research-path__index">
                  {String(models.indexOf(model) + 1).padStart(2, "0")}
                </div>
                <div>
                  <span>{model.lane}</span>
                  <strong>{model.name}</strong>
                </div>
                <AdmissionState value={model.admission} detail={model.fit} />
              </header>
              <p className="research-path__decision">{model.decision}</p>
              <dl>
                <div>
                  <dt>OUTPUT CONTRACT</dt>
                  <dd>{model.output}</dd>
                </div>
                <div>
                  <dt>LATENCY EVIDENCE</dt>
                  <dd>{model.latency}</dd>
                </div>
                <div>
                  <dt>ACCESS</dt>
                  <dd>{model.access}</dd>
                </div>
              </dl>
              <footer>
                <span>
                  <Icon name="shield" size={15} /> Research path · not installed
                </span>
                <small>{model.license}</small>
              </footer>
            </article>
          ))}
        </div>
      </section>
      <div className="visual-pack-boundary" role="note">
        <Icon name="help" size={18} />
        <div>
          <strong>No visual route is selectable or installed.</strong>
          <p>
            Qualification does not trigger a download. A future pack requires an
            explicit user choice after immutable artifacts, hashes, signatures,
            license review, a self-test, and Windows game-load contention
            results exist.
          </p>
        </div>
      </div>
      <Disclosure title="No pack is required or installed automatically">
        Generic lip-sync is optional and experimental. A user must choose a
        reviewed pack explicitly after immutable artifacts, hashes, signatures,
        licenses, resource envelopes, and self-tests exist. Advanced local LLM,
        STT, and TTS alternatives are unavailable here and are not needed for
        the API-first baseline.
      </Disclosure>
    </div>
  );
}

function DiagnosticsPage({
  state,
  nativeBootstrap,
}: {
  state: DemoState;
  nativeBootstrap: NativeBootstrapHealth;
}) {
  const snapshot =
    nativeBootstrap.kind === "snapshot" ? nativeBootstrap.snapshot : null;
  const authenticated = Boolean(
    snapshot?.runtime.connected && snapshot.mediaBroker.connected,
  );
  const checks = snapshot
    ? [
        ["Control application", "Native bootstrap returned", "ok"],
        [
          "Runtime coordinator",
          snapshot.runtime.connected
            ? `Authenticated · ${snapshot.runtime.state} · protocol ${snapshot.runtime.protocolVersion ?? "unknown"}`
            : snapshot.runtime.detail,
          snapshot.runtime.connected ? "ok" : "warn",
        ],
        [
          "Media broker",
          snapshot.mediaBroker.connected
            ? `Authenticated · ${snapshot.mediaBroker.state} · protocol ${snapshot.mediaBroker.protocolVersion ?? "unknown"}`
            : snapshot.mediaBroker.detail,
          snapshot.mediaBroker.connected ? "ok" : "warn",
        ],
        ["Speech recognition", "Provider not connected", "warn"],
        ["Language model", "Provider not connected", "warn"],
        ["Voice synthesis", "Provider not connected", "warn"],
        [
          "Game capture",
          snapshot.mediaBroker.captureAvailable
            ? "Native capture capability available"
            : "No live capture capability",
          snapshot.mediaBroker.captureAvailable ? "ok" : "warn",
        ],
        ["Memory database", "No live database check requested", "warn"],
      ]
    : state === "error"
      ? [
          ["Control application", "Fixture modeled", "ok"],
          ["Runtime coordinator", "Fixture restarting", "warn"],
          ["Media broker", "Fixture modeled", "ok"],
          ["Speech recognition", "Provider not connected", "warn"],
          ["Language model", "Simulated failure", "danger"],
          ["Voice synthesis", "Provider not connected", "warn"],
          ["Game capture", "No live capture", "warn"],
          ["Memory database", "Fixture schema", "ok"],
        ]
      : [
          ["Control application", "Fixture modeled", "ok"],
          ["Runtime coordinator", "No live snapshot", "warn"],
          ["Media broker", "No live snapshot", "warn"],
          ["Speech recognition", "Provider not connected", "warn"],
          ["Language model", "Provider not connected", "warn"],
          ["Voice synthesis", "Provider not connected", "warn"],
          ["Game capture", "No live capture", "warn"],
          ["Memory database", "Fixture schema", "ok"],
        ];
  return (
    <div className="page">
      <PageHero
        eyebrow="DIAGNOSTICS"
        title={
          snapshot
            ? "Desktop runtime health."
            : state === "error"
              ? "Simulated worker recovery."
              : "Illustrative diagnostic fixture."
        }
        description={
          snapshot
            ? "Runtime and broker rows below come from the native bootstrap snapshot. Provider and game checks remain explicit placeholders until their own live probes run."
            : "Readable checks first, detailed traces when you need them. Logs stay local unless you explicitly export a redacted bundle."
        }
        actions={
          <>
            <ActionButton icon="refresh">Run all checks</ActionButton>
            <ActionButton variant="outline" icon="download">
              Export redacted bundle
            </ActionButton>
          </>
        }
      />
      {state === "error" && (
        <div className="incident-card">
          <div className="incident-card__icon">
            <Icon name="warning" />
          </div>
          <div>
            <span>SIMULATED INCIDENT · FIXTURE RETRY 2 OF 3</span>
            <h2>Cloud reply connection ended before the first clause.</h2>
            <p>
              This fixture models clean cancellation. It is not evidence that a
              fallback provider or memory runtime is available.
            </p>
            <div>
              <ActionButton icon="refresh">Retry provider</ActionButton>
              <ActionButton variant="outline" isDisabled>
                Advanced local fallback unavailable
              </ActionButton>
            </div>
          </div>
          <div className="incident-clock">
            <span>Next retry</span>
            <strong>00:08</strong>
          </div>
        </div>
      )}
      <div className="diagnostics-grid">
        <section className="instrument-panel system-checks">
          <SectionTitle
            eyebrow={
              snapshot ? "NATIVE PROCESS HEALTH" : "SIMULATED PROCESS HEALTH"
            }
            title={snapshot ? "Desktop sidecars" : "Fixture systems"}
            action={
              <StatusPill
                tone={
                  authenticated ? "ok" : state === "error" ? "warn" : "neutral"
                }
              >
                {authenticated
                  ? "Authenticated"
                  : snapshot
                    ? "Connection incomplete"
                    : nativeBootstrap.kind === "loading"
                      ? "Checking runtime"
                      : "No live checks"}
              </StatusPill>
            }
          />
          {checks.map(([name, status, tone]) => (
            <div className="system-check" key={name}>
              <span className={`health-led health-led--${tone}`} />
              <strong>{name}</strong>
              <span>{status}</span>
              <button aria-label={`Details for ${name}`}>
                <Icon name="chevron" />
              </button>
            </div>
          ))}
        </section>
        <section className="instrument-panel timeline-panel">
          <SectionTitle
            eyebrow="SIMULATED LAST TURN"
            title="Trace timeline"
            action={
              <button className="inline-link">
                Open full trace <Icon name="arrow" />
              </button>
            }
          />
          <div className="trace-timeline">
            <TraceItem
              time="15:41:08.104"
              title="PTT released"
              detail="Authoritative endpoint · generation 24"
            />
            <TraceItem
              time="15:41:08.390"
              title="Transcript final"
              detail="SIM Moonshine candidate · 286 ms fixture"
            />
            <TraceItem
              time="15:41:08.484"
              title="Context ready"
              detail="SIM 2 fixture memories · identity 96% fixture"
            />
            <TraceItem
              time="15:41:08.962"
              title="First clause ready"
              detail="Validated and sanitized"
            />
            <TraceItem
              time="15:41:09.183"
              title="First audio delivered"
              detail="SIM 1.079 s after fixture PTT release"
              active
            />
          </div>
        </section>
        <section className="instrument-panel environment-panel">
          <SectionTitle
            eyebrow="ILLUSTRATIVE ENVIRONMENT"
            title="Fixture session"
          />
          <dl>
            <div>
              <dt>App</dt>
              <dd>2.0.0-alpha.1</dd>
            </div>
            <div>
              <dt>Protocol</dt>
              <dd>EnvelopeV1 · ABI 2</dd>
            </div>
            <div>
              <dt>Windows</dt>
              <dd>11 · 24H2 · x64</dd>
            </div>
            <div>
              <dt>Game</dt>
              <dd>Eclipse Harbor · simulation</dd>
            </div>
            <div>
              <dt>Safety</dt>
              <dd>
                <StatusPill tone="neutral">Fixture policy only</StatusPill>
              </dd>
            </div>
            <div>
              <dt>Network</dt>
              <dd>Fixture route · no request sent</dd>
            </div>
            <div>
              <dt>Database</dt>
              <dd>SQLite WAL · fixture schema only</dd>
            </div>
          </dl>
        </section>
      </div>
      <Disclosure title="Support bundles are redacted before export">
        API keys, full prompts, transcript text, memory content, Windows
        usernames, paths, and captured frames are excluded by default. You see
        the manifest before saving.
      </Disclosure>
    </div>
  );
}

function TraceItem({
  time,
  title,
  detail,
  active,
}: {
  time: string;
  title: string;
  detail: string;
  active?: boolean;
}) {
  return (
    <div className={`trace-item ${active ? "is-active" : ""}`}>
      <span>{time}</span>
      <i />
      <div>
        <strong>{title}</strong>
        <small>{detail}</small>
      </div>
    </div>
  );
}

function SettingsPage({
  preferences,
  updatePreferences,
}: Pick<PageProps, "preferences" | "updatePreferences">) {
  const sections = [
    "general",
    "privacy",
    "providers",
    "audio",
    "overlay",
    "accessibility",
    "storage",
    "updates",
  ];
  const requestedSection = new URLSearchParams(window.location.search).get(
    "settingsSection",
  );
  const [section, setSection] = useState(
    requestedSection && sections.includes(requestedSection)
      ? requestedSection
      : "general",
  );
  return (
    <div className="page">
      <PageHero
        eyebrow="SETTINGS"
        title="One default. Precise exceptions."
        description="Settings inherit from global → game → character. Every override is visible where it takes effect."
      />
      <div
        className={`settings-workspace ${section === "providers" ? "settings-workspace--providers" : ""}`}
      >
        <nav className="settings-nav" aria-label="Settings sections">
          {sections.map((item) => (
            <button
              key={item}
              className={section === item ? "is-selected" : ""}
              onClick={() => setSection(item)}
            >
              {item[0].toUpperCase() + item.slice(1)}
              <Icon name="chevron" />
            </button>
          ))}
        </nav>
        <section className="settings-main">
          {section === "general" && (
            <>
              <SectionTitle
                eyebrow="GLOBAL DEFAULTS"
                title="General"
                description="These choices apply unless a game or character says otherwise."
              />
              <div className="setting-group">
                <h3>Conversation</h3>
                <Toggle
                  label="Push-to-talk"
                  description="Hold V to speak; release ends the turn immediately."
                  isSelected={preferences.ptt}
                  onChange={(ptt) => updatePreferences({ ptt })}
                />
                <Toggle
                  label="Subtitles"
                  description="Show delivered character speech over the selected game."
                  isSelected={preferences.subtitles}
                  onChange={(subtitles) => updatePreferences({ subtitles })}
                />
                <Toggle
                  label="Start minimized with Windows"
                  description="Keep the response runtime ready without opening the console."
                  isSelected={false}
                  onChange={() => undefined}
                />
              </div>
              <div className="setting-group">
                <h3>Default run mode</h3>
                <div className="segmented-control">
                  {(["cloud", "hybrid", "local"] as const).map((mode) => (
                    <button
                      key={mode}
                      className={
                        preferences.execution === mode ? "is-selected" : ""
                      }
                      onClick={() =>
                        updatePreferences({
                          execution: mode,
                          localOnly: mode === "local",
                        })
                      }
                    >
                      {mode === "local"
                        ? "Fully local"
                        : mode[0].toUpperCase() + mode.slice(1)}
                    </button>
                  ))}
                </div>
                <Disclosure title="No silent rerouting">
                  Provider routes and privacy consequences are confirmed before
                  the first use.
                </Disclosure>
              </div>
            </>
          )}
          {section === "privacy" && (
            <>
              <SectionTitle
                eyebrow="DATA BOUNDARIES"
                title="Privacy"
                description="See and control every path that can leave this PC."
              />
              <div className="privacy-map">
                <div className="privacy-node privacy-node--pc">
                  <Icon name="shield" />
                  <strong>This PC</strong>
                  <span>Speech · memory · voices</span>
                </div>
                <div className="privacy-route">
                  <i />
                  <span>Conversation text only</span>
                </div>
                <div className="privacy-node">
                  <Icon name="spark" />
                  <strong>OpenAI</strong>
                  <span>Reply model · allowed</span>
                </div>
              </div>
              <div className="setting-group">
                <Toggle
                  label="Offline mode"
                  description="Block all application network attempts, including catalog and update checks."
                  isSelected={preferences.localOnly}
                  onChange={(localOnly) =>
                    updatePreferences({
                      localOnly,
                      execution: localOnly ? "local" : preferences.execution,
                    })
                  }
                  privacy="Hard application-level boundary"
                />
                <Toggle
                  label="Store delivered conversations"
                  description="Keep a local, inspectable history after each session."
                  isSelected={true}
                  onChange={() => undefined}
                  privacy="SQLite on this PC"
                />
                <Toggle
                  label="Local diagnostic logs"
                  description="Keep redacted timing and process health for seven days."
                  isSelected={preferences.diagnostics}
                  onChange={(diagnostics) => updatePreferences({ diagnostics })}
                  privacy="No transcript or memory content"
                />
              </div>
            </>
          )}
          {section === "providers" && <ProviderSettings />}
          {section === "accessibility" && (
            <>
              <SectionTitle
                eyebrow="ACCESSIBILITY"
                title="Make the console comfortable"
                description="Designed for keyboard, controller, Narrator, high contrast, reduced motion, and Windows scaling."
              />
              <div className="setting-group">
                <Toggle
                  label="Follow Windows high contrast"
                  description="Use system colors and reinforced borders when high contrast is active."
                  isSelected={true}
                  onChange={() => undefined}
                />
                <Toggle
                  label="Reduce motion"
                  description="Replace ambient and stage transitions with immediate state changes."
                  isSelected={false}
                  onChange={() => undefined}
                />
                <Toggle
                  label="Announce response stages"
                  description="Narrator speaks meaningful stage and fallback changes."
                  isSelected={true}
                  onChange={() => undefined}
                />
                <div className="slider-setting">
                  <div>
                    <strong>Interface scale</strong>
                    <span>100%</span>
                  </div>
                  <input
                    type="range"
                    min="100"
                    max="200"
                    step="25"
                    defaultValue="100"
                  />
                  <small>
                    Windows scaling remains supported independently.
                  </small>
                </div>
              </div>
            </>
          )}
          {!["general", "privacy", "providers", "accessibility"].includes(
            section,
          ) && <SettingsPlaceholder section={section} />}
        </section>
        <aside className="settings-context">
          <span className="panel-label">INHERITANCE</span>
          <div className="inheritance">
            <div className="is-current">
              <i />
              Global default
            </div>
            <div>
              <i />
              Cyberpunk 2077
            </div>
            <div>
              <i />
              Jackie Welles
            </div>
          </div>
          <p>
            You are editing the global layer. Game and character exceptions
            remain unchanged.
          </p>
          <button>
            View 4 overrides <Icon name="arrow" />
          </button>
        </aside>
      </div>
    </div>
  );
}

function SettingsPlaceholder({ section }: { section: string }) {
  const copy: Record<string, [string, string]> = {
    providers: [
      "Connected providers",
      "Manage credentials through Windows, models, consented routes, and spending visibility.",
    ],
    audio: [
      "Microphone & playback",
      "Choose devices, push-to-talk, endpoint behavior, subtitle timing, and interruption handling.",
    ],
    overlay: [
      "Game overlay",
      "Position subtitles, response status, safe regions, controller navigation, and HDR behavior.",
    ],
    storage: [
      "Storage & retention",
      "Manage models, rebuildable embeddings, conversations, caches, and retention windows.",
    ],
    updates: [
      "Updates",
      "Review signed app, profile, model catalog, and rollback policies before activation.",
    ],
  };
  const [title, desc] = copy[section] ?? ["Settings", ""];
  return (
    <>
      <SectionTitle
        eyebrow={section.toUpperCase()}
        title={title}
        description={desc}
      />
      <div className="setting-group">
        <Toggle
          label={`${title} enabled`}
          description={`Use the recommended ${section} configuration for this PC.`}
          isSelected={true}
          onChange={() => undefined}
        />
        <Toggle
          label="Ask before changing a route"
          description="Show the exact resource, privacy, or compatibility impact before applying."
          isSelected={true}
          onChange={() => undefined}
        />
        <Disclosure title="A safe default is active">
          Detailed provider and device controls will appear here when their
          runtime capability is connected.
        </Disclosure>
      </div>
    </>
  );
}

function HelpPage() {
  const [query, setQuery] = useState("");
  const topics = [
    "Start a conversation",
    "Connect a supported game",
    "Understand cloud, hybrid, and local",
    "Fix a microphone problem",
    "Keep game performance steady",
    "Review or remove a memory",
    "Use generic game mode",
    "Create a redacted support bundle",
  ];
  const visible = topics.filter((topic) =>
    topic.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div className="page">
      <section className="help-hero">
        <span className="eyebrow">HELP CENTER · LOCAL DOCUMENTATION</span>
        <h1>What do you want to do?</h1>
        <label className="help-search">
          <Icon name="search" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search setup, games, privacy, performance…"
            autoComplete="off"
          />
          <span>
            <KeyboardKey>Ctrl</KeyboardKey>
            <KeyboardKey>K</KeyboardKey>
          </span>
        </label>
        <div className="quick-help">
          <button>
            <Icon name="mic" />
            <span>
              <strong>Test my microphone</strong>
              <small>Run a local rehearsal</small>
            </span>
            <Icon name="arrow" />
          </button>
          <button>
            <Icon name="diagnostics" />
            <span>
              <strong>Check what’s wrong</strong>
              <small>Open guided diagnostics</small>
            </span>
            <Icon name="arrow" />
          </button>
          <button>
            <Icon name="shield" />
            <span>
              <strong>See privacy routes</strong>
              <small>What can leave this PC</small>
            </span>
            <Icon name="arrow" />
          </button>
        </div>
      </section>
      <div className="help-grid">
        <section>
          <SectionTitle eyebrow="POPULAR GUIDES" title="Get something done" />
          <div className="guide-list">
            {visible.map((topic, index) => (
              <button key={topic}>
                <span>{String(index + 1).padStart(2, "0")}</span>
                <strong>{topic}</strong>
                <small>{index % 2 === 0 ? "3 min read" : "Guided check"}</small>
                <Icon name="arrow" />
              </button>
            ))}
          </div>
        </section>
        <aside>
          <div className="support-card">
            <span className="support-card__icon">
              <Icon name="headphones" />
            </span>
            <h2>Need a closer look?</h2>
            <p>
              Create a redacted support bundle, inspect exactly what it
              contains, then save it wherever you choose.
            </p>
            <ActionButton variant="outline" icon="download">
              Prepare support bundle
            </ActionButton>
          </div>
          <div className="support-card controller-help-card">
            <span className="support-card__icon">
              <Icon name="games" />
            </span>
            <h2>Experimental controller navigation</h2>
            <p>
              D-pad or left stick moves keyboard focus, A activates the focused
              control, and B uses an explicit Back or Close control. It runs
              only while this window is visible. If WebView2 exposes no Gamepad
              API, the console silently remains keyboard-first.
            </p>
          </div>
          <div className="version-card">
            <span>NPC 2.0 PRIVATE PREVIEW</span>
            <strong>2.0.0-alpha.1</strong>
            <small>Protocol ABI 2 · profiles 2.0.14</small>
            <button>
              Read release notes <Icon name="external" />
            </button>
          </div>
        </aside>
      </div>
    </div>
  );
}

function PageHero({
  eyebrow,
  title,
  description,
  actions,
}: {
  eyebrow: string;
  title: string;
  description: string;
  actions?: React.ReactNode;
}) {
  return (
    <header className="page-hero">
      <div>
        <span className="eyebrow">{eyebrow}</span>
        <span className="fixture-label">
          ILLUSTRATIVE UI FIXTURE · VALUES ARE NOT RELEASE MEASUREMENTS
        </span>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {actions && <div className="page-hero__actions">{actions}</div>}
    </header>
  );
}
function PageLoading({ page }: { page: PageId }) {
  return (
    <div className="page">
      <PageHero
        eyebrow="CONNECTING"
        title={`Loading ${page}…`}
        description="Reading local state. No provider requests are made for this view."
      />
      <div className="loading-grid">
        <Skeleton lines={5} />
        <Skeleton lines={4} />
        <Skeleton lines={6} />
        <Skeleton lines={3} />
      </div>
    </div>
  );
}
function PageEmpty({ page }: { page: PageId }) {
  const map: Partial<Record<PageId, [any, string, string, string]>> = {
    games: [
      "games",
      "No games connected yet",
      "Scan local libraries or choose a single-player executable manually.",
      "Scan this PC",
    ],
    characters: [
      "characters",
      "No characters here yet",
      "Connect a game profile or create an original character to begin.",
      "Choose a game",
    ],
    conversation: [
      "conversation",
      "No delivered conversations",
      "Run the private Eclipse Harbor simulation to see how history and memory work.",
      "Run simulation",
    ],
    models: [
      "models",
      "No local models installed",
      "Choose a recommended pipeline sized for this PC, or stay cloud-only.",
      "Browse model packs",
    ],
    diagnostics: [
      "diagnostics",
      "No diagnostic history",
      "Run a local health check to establish the first baseline.",
      "Run all checks",
    ],
  };
  const [icon, title, description, action] = map[page] ?? [
    "spark",
    `No ${page} data yet`,
    "This view will fill in as you use NPC 2.0.",
    "Run private simulation",
  ];
  return (
    <div className="page">
      <EmptyState
        icon={icon}
        title={title}
        description={description}
        action={<ActionButton icon="arrow">{action}</ActionButton>}
      />
    </div>
  );
}
