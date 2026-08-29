import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "react-aria-components";
import { CurrentPage } from "./Pages";
import { Onboarding } from "./Onboarding";
import { NAV_ITEMS, RESPONSE_STAGES } from "./data";
import { Icon } from "./icons";
import type { AppPreferences, DemoState, PageId, StageId } from "./types";
import { ActionButton, IconButton, StatusPill } from "./components";
import {
  cancelNativeSimulation,
  loadNativeBootstrapHealth,
  LOADING_NATIVE_BOOTSTRAP,
  startNativeSimulation,
  type NativeBootstrapHealth,
} from "./tauriBridge";
import { ThemeSpecimen } from "./ThemeSpecimen";
import { useExperimentalControllerNavigation } from "./controllerNavigation";
import {
  applyNativeSimulationEvent,
  awaitingNativeEvidence,
  browserFixtureEvidence,
  EMPTY_SIMULATION_EVIDENCE,
  nativeCompletionNotice,
  type SimulationEvidence,
} from "./simulationEvidence";

const isPage = (value: string | null): value is PageId =>
  Boolean(value && NAV_ITEMS.some((item) => item.id === value));
const isState = (value: string | null): value is DemoState =>
  ["ready", "active", "loading", "empty", "error", "degraded"].includes(
    value ?? "",
  );
const initialPage = (params: URLSearchParams): PageId => {
  const value = params.get("page");
  return isPage(value) ? value : "home";
};
const initialState = (params: URLSearchParams): DemoState => {
  const value = params.get("state");
  return isState(value) ? value : "ready";
};

export function App() {
  if (
    new URLSearchParams(window.location.search).get("themeSpecimen") === "1"
  ) {
    document.documentElement.dataset.contrast = "normal";
    document.documentElement.dataset.theme = "dark";
    document.documentElement.dataset.largeText = "false";
    return <ThemeSpecimen />;
  }
  return <ResponseConsoleApp />;
}

function ResponseConsoleApp() {
  useExperimentalControllerNavigation();
  const params = useMemo(() => new URLSearchParams(window.location.search), []);
  const [page, setPage] = useState<PageId>(() => initialPage(params));
  const [demoState, setDemoState] = useState<DemoState>(() =>
    initialState(params),
  );
  const [onboarding, setOnboarding] = useState(
    params.get("onboarding") === "1",
  );
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);
  const [activeStageIndex, setActiveStageIndex] = useState(
    demoState === "active" ? 4 : -1,
  );
  const [isSimulating, setIsSimulating] = useState(demoState === "active");
  const [toast, setToast] = useState<string | null>(null);
  const [simulationEvidence, setSimulationEvidence] =
    useState<SimulationEvidence>(() =>
      demoState === "active"
        ? browserFixtureEvidence()
        : EMPTY_SIMULATION_EVIDENCE,
    );
  const [nativeBootstrap, setNativeBootstrap] = useState<NativeBootstrapHealth>(
    LOADING_NATIVE_BOOTSTRAP,
  );
  const simulationTimer = useRef<number | null>(null);
  const completionTimer = useRef<number | null>(null);
  const nativeSimulation = useRef(false);
  const [preferences, setPreferences] = useState<AppPreferences>({
    execution: "cloud",
    performance: "balanced",
    subtitles: true,
    ptt: true,
    localOnly: false,
    screenPresence: true,
    diagnostics: true,
  });

  const updatePreferences = (patch: Partial<AppPreferences>) =>
    setPreferences((value) => ({ ...value, ...patch }));
  const activeStage =
    activeStageIndex >= 0
      ? (RESPONSE_STAGES[activeStageIndex]?.id ?? null)
      : null;

  useEffect(() => {
    document.documentElement.dataset.contrast =
      params.get("contrast") === "high" ? "high" : "normal";
    document.documentElement.dataset.theme =
      params.get("theme") === "light" ? "light" : "dark";
    document.documentElement.dataset.largeText =
      params.get("largeText") === "1" ? "true" : "false";
    if (params.get("motion") === "reduce")
      document.documentElement.classList.add("force-reduced-motion");
  }, [params]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setCommandOpen(true);
      }
      if (event.key === "Escape") {
        setCommandOpen(false);
        setSidebarOpen(false);
      }
      if (event.altKey && /^[1-9]$/.test(event.key)) {
        const item = NAV_ITEMS[Number(event.key) - 1];
        if (item) setPage(item.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(
    () => () => {
      if (simulationTimer.current)
        window.clearInterval(simulationTimer.current);
      if (completionTimer.current) window.clearTimeout(completionTimer.current);
    },
    [],
  );

  useEffect(() => {
    let current = true;
    loadNativeBootstrapHealth().then((health) => {
      if (current) setNativeBootstrap(health);
    });
    return () => {
      current = false;
    };
  }, []);

  const runtimeConnected =
    nativeBootstrap.kind === "snapshot" &&
    nativeBootstrap.snapshot.runtime.connected;
  const brokerConnected =
    nativeBootstrap.kind === "snapshot" &&
    nativeBootstrap.snapshot.mediaBroker.connected;

  const selectPage = (nextPage: PageId) => {
    setPage(nextPage);
    setSidebarOpen(false);
    const next = new URL(window.location.href);
    next.searchParams.set("page", nextPage);
    window.history.replaceState(null, "", next);
  };

  const startSimulation = async () => {
    if (isSimulating) {
      if (nativeSimulation.current)
        await cancelNativeSimulation().catch(() => false);
      nativeSimulation.current = false;
      if (simulationTimer.current)
        window.clearInterval(simulationTimer.current);
      simulationTimer.current = null;
      if (completionTimer.current) window.clearTimeout(completionTimer.current);
      completionTimer.current = null;
      setIsSimulating(false);
      setActiveStageIndex(-1);
      setDemoState("ready");
      setSimulationEvidence((evidence) => ({
        ...evidence,
        phase: "cancelled",
      }));
      setToast("Illustrative simulation ended. No conversation was saved.");
      return;
    }
    setIsSimulating(true);
    if (completionTimer.current) window.clearTimeout(completionTimer.current);
    completionTimer.current = null;
    setDemoState("active");
    setActiveStageIndex(0);
    setSimulationEvidence(awaitingNativeEvidence());
    setToast("Private Eclipse Harbor simulation started.");
    const startedNatively = await startNativeSimulation(
      preferences.execution,
      (event) => {
        setSimulationEvidence((evidence) =>
          applyNativeSimulationEvent(evidence, event),
        );
        if (event.type === "stageStarted")
          setActiveStageIndex(
            RESPONSE_STAGES.findIndex((stage) => stage.id === event.stage),
          );
        if (event.type === "completed") {
          nativeSimulation.current = false;
          setActiveStageIndex(RESPONSE_STAGES.length - 1);
          completionTimer.current = window.setTimeout(() => {
            setIsSimulating(false);
            setDemoState("ready");
            setActiveStageIndex(-1);
            setToast(nativeCompletionNotice(event.fixtureFirstAudioMs));
            completionTimer.current = null;
          }, 900);
        }
        if (event.type === "cancelled") {
          nativeSimulation.current = false;
          if (completionTimer.current)
            window.clearTimeout(completionTimer.current);
          completionTimer.current = null;
          setIsSimulating(false);
          setDemoState("ready");
          setActiveStageIndex(-1);
        }
      },
    ).catch(() => false);
    if (startedNatively) {
      nativeSimulation.current = true;
      return;
    }
    setSimulationEvidence(browserFixtureEvidence());
    let index = 0;
    simulationTimer.current = window.setInterval(() => {
      index += 1;
      if (index >= RESPONSE_STAGES.length) {
        if (simulationTimer.current)
          window.clearInterval(simulationTimer.current);
        simulationTimer.current = null;
        setActiveStageIndex(RESPONSE_STAGES.length - 1);
        completionTimer.current = window.setTimeout(() => {
          setIsSimulating(false);
          setDemoState("ready");
          setActiveStageIndex(-1);
          setSimulationEvidence(browserFixtureEvidence("delivered"));
          setToast(
            "Illustrative simulation complete · virtual first audio 1.31 s",
          );
          completionTimer.current = null;
        }, 1200);
        return;
      }
      setActiveStageIndex(index);
    }, 650);
  };

  return (
    <div className={`app-shell ${sidebarOpen ? "sidebar-open" : ""}`}>
      <a className="skip-link" href="#main-content">
        Skip to main content
      </a>
      <aside className="sidebar" aria-label="Primary">
        <div className="brand">
          <div className="brand__mark">
            <span>N</span>
            <i />
          </div>
          <div>
            <strong>NPC 2.0</strong>
            <small>Response Console</small>
          </div>
        </div>
        <nav className="nav-list">
          <span className="nav-label">CONSOLE</span>
          {NAV_ITEMS.filter((item) => item.group === "primary").map(
            (item, index) => (
              <Button
                key={item.id}
                className={`nav-item ${page === item.id ? "is-current" : ""}`}
                onPress={() => selectPage(item.id)}
                aria-current={page === item.id ? "page" : undefined}
              >
                <Icon name={item.icon} />
                <span>{item.label}</span>
                <kbd>Alt {index + 1}</kbd>
              </Button>
            ),
          )}
          <span className="nav-label nav-label--system">SYSTEM</span>
          {NAV_ITEMS.filter((item) => item.group === "system").map((item) => (
            <Button
              key={item.id}
              className={`nav-item ${page === item.id ? "is-current" : ""}`}
              onPress={() => selectPage(item.id)}
              aria-current={page === item.id ? "page" : undefined}
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </Button>
          ))}
        </nav>
        <div className="sidebar__session">
          <div className="session-led">
            <i className={isSimulating ? "is-live" : ""} />
            <div>
              <strong>
                {isSimulating
                  ? "Simulation fixture active"
                  : runtimeConnected && brokerConnected
                    ? "Runtime + broker authenticated"
                    : nativeBootstrap.kind === "loading"
                      ? "Checking native runtime"
                      : "Runtime connection incomplete"}
              </strong>
              <span>
                {isSimulating
                  ? "Eclipse Harbor · simulated"
                  : runtimeConnected && brokerConnected
                    ? `Protocol ${nativeBootstrap.kind === "snapshot" ? (nativeBootstrap.snapshot.runtime.protocolVersion ?? "connected") : "connected"}`
                    : nativeBootstrap.kind === "browserPreview"
                      ? "Browser preview · no desktop bridge"
                      : "See Diagnostics for native health"}
              </span>
            </div>
          </div>
          <button onClick={() => selectPage("diagnostics")}>
            View health <Icon name="chevron" />
          </button>
        </div>
        <div className="sidebar__profile">
          <span>AI</span>
          <div>
            <strong>API-first profile</strong>
            <small>
              {preferences.execution === "local"
                ? "Advanced offline route"
                : preferences.execution === "hybrid"
                  ? "Advanced hybrid route"
                  : "Explicit provider routes"}
            </small>
          </div>
          <IconButton label="Open profile menu" icon="more" />
        </div>
      </aside>
      {sidebarOpen && (
        <button
          className="sidebar-scrim"
          aria-label="Close navigation"
          onClick={() => setSidebarOpen(false)}
        />
      )}
      <div className="workspace">
        <header className="topbar">
          <button
            className="mobile-menu"
            onClick={() => setSidebarOpen(true)}
            aria-label="Open navigation"
          >
            <span />
            <span />
            <span />
          </button>
          <div className="topbar__crumb">
            <span>{NAV_ITEMS.find((item) => item.id === page)?.label}</span>
            <i />
            <strong>
              {isSimulating
                ? simulationEvidence.source === "nativeRuntime"
                  ? "Native runtime simulation"
                  : simulationEvidence.source === "awaitingNative"
                    ? "Connecting to runtime"
                    : "Browser simulation"
                : demoState === "degraded"
                  ? "Degraded safely"
                  : demoState === "error"
                    ? "Recovering"
                    : runtimeConnected && brokerConnected
                      ? "Runtime authenticated"
                      : "No runtime snapshot"}
            </strong>
            <b className="fixture-badge">
              {simulationEvidence.source === "nativeRuntime"
                ? "NATIVE EVENT EVIDENCE"
                : "ILLUSTRATIVE FIXTURE"}
            </b>
          </div>
          <div className="topbar__actions">
            <button
              className="command-button"
              aria-label="Find anything"
              onClick={() => setCommandOpen(true)}
            >
              <Icon name="search" />
              <span>Find anything</span>
              <kbd>Ctrl K</kbd>
            </button>
            <div className="safety-badge">
              <Icon name="shield" />
              <span>
                <strong>Single-player policy</strong>
                <small>Runtime verification required</small>
              </span>
            </div>
            <IconButton
              label="Notifications"
              icon={demoState === "error" ? "warning" : "spark"}
            />
            <IconButton label="Minimize to tray" icon="more" />
          </div>
        </header>
        <ResponseSpine
          activeStage={activeStage}
          isSimulating={isSimulating}
          state={demoState}
          simulationEvidence={simulationEvidence}
        />
        <main id="main-content" tabIndex={-1}>
          <CurrentPage
            page={page}
            state={demoState}
            preferences={preferences}
            updatePreferences={updatePreferences}
            activeStage={activeStage}
            isSimulating={isSimulating}
            startSimulation={startSimulation}
            simulationEvidence={simulationEvidence}
            nativeBootstrap={nativeBootstrap}
            showSyntheticReplayCaptureTest={
              params.get("syntheticReplayTest") === "1"
            }
          />
        </main>
      </div>
      {onboarding && (
        <Onboarding
          initialStep={Number(params.get("step") ?? 0)}
          preferences={preferences}
          updatePreferences={updatePreferences}
          onClose={() => setOnboarding(false)}
        />
      )}
      {commandOpen && (
        <CommandPalette
          onClose={() => setCommandOpen(false)}
          onSelect={(value) => {
            if (isPage(value)) selectPage(value);
            else if (value === "onboarding") setOnboarding(true);
            else if (value === "simulate") startSimulation();
            setCommandOpen(false);
          }}
        />
      )}
      {toast && (
        <div className="toast" role="status">
          <Icon name="check" />
          <span>{toast}</span>
          <button
            aria-label="Dismiss notification"
            onClick={() => setToast(null)}
          >
            <Icon name="close" />
          </button>
        </div>
      )}
      <DemoControls
        state={demoState}
        onState={setDemoState}
        onOnboarding={() => setOnboarding(true)}
      />
    </div>
  );
}

function ResponseSpine({
  activeStage,
  isSimulating,
  state,
  simulationEvidence,
}: {
  activeStage: StageId | null;
  isSimulating: boolean;
  state: DemoState;
  simulationEvidence: SimulationEvidence;
}) {
  const activeIndex = RESPONSE_STAGES.findIndex(
    (stage) => stage.id === activeStage,
  );
  return (
    <section
      className={`response-spine ${isSimulating ? "is-live" : ""} response-spine--${state}`}
      aria-label="Response pipeline"
      aria-live="polite"
    >
      <div className="response-spine__identity">
        <span className="response-spine__pulse" />
        <div>
          <span>
            {isSimulating
              ? simulationEvidence.source === "nativeRuntime"
                ? "NATIVE RUNTIME TURN · FIXTURE INPUT"
                : simulationEvidence.source === "awaitingNative"
                  ? "CONNECTING TO NATIVE RUNTIME"
                  : "SIMULATED TURN · BROWSER FIXTURE"
              : "RESPONSE SPINE · NO LIVE TURN"}
          </span>
          <strong>
            {isSimulating
              ? (RESPONSE_STAGES[activeIndex]?.label ?? "Starting")
              : state === "degraded"
                ? "Simulated audio fallback"
                : state === "error"
                  ? "Simulated recovery state"
                  : "No runtime turn reported"}
          </strong>
        </div>
      </div>
      <ol>
        {RESPONSE_STAGES.map((stage, index) => (
          <li
            key={stage.id}
            className={
              index === activeIndex
                ? "is-active"
                : index < activeIndex
                  ? "is-complete"
                  : ""
            }
            aria-current={index === activeIndex ? "step" : undefined}
          >
            <span className="stage-node">
              {index < activeIndex ? <Icon name="check" size={11} /> : <i />}
            </span>
            <div>
              <strong>{stage.label}</strong>
              <small
                className="response-spine__stage-status"
                title={
                  isSimulating
                    ? simulationEvidence.source === "nativeRuntime"
                      ? "Stage event returned by the native runtime"
                      : "Illustrative browser simulation timing"
                    : "Illustrative preview · no live measurement"
                }
              >
                {!isSimulating
                  ? "PREVIEW"
                  : index < activeIndex
                    ? simulationEvidence.source === "nativeRuntime"
                      ? "NATIVE EVENT"
                      : `SIM ${stage.fixtureDuration} ms`
                    : index === activeIndex
                      ? simulationEvidence.source === "nativeRuntime"
                        ? "NATIVE ACTIVE"
                        : "SIM ACTIVE"
                      : simulationEvidence.source === "nativeRuntime"
                        ? "AWAITING EVENT"
                        : "SIM NEXT"}
              </small>
            </div>
            {index < RESPONSE_STAGES.length - 1 && <b />}
          </li>
        ))}
      </ol>
      <div className="response-spine__compact-stage">
        {isSimulating
          ? `${RESPONSE_STAGES[activeIndex]?.label ?? "Starting fixture"} · ${Math.max(0, activeIndex + 1)}/7`
          : "No live stage · —"}
      </div>
      <div className="response-spine__total">
        <span>
          {isSimulating
            ? simulationEvidence.source === "nativeRuntime"
              ? "NATIVE EVENTS"
              : "SIM FIXTURE"
            : "NO LIVE TURN"}
        </span>
        <strong>
          {isSimulating ? `${Math.max(0, activeIndex + 1)} / 7` : "—"}
        </strong>
      </div>
    </section>
  );
}

function CommandPalette({
  onClose,
  onSelect,
}: {
  onClose: () => void;
  onSelect: (value: string) => void;
}) {
  const [query, setQuery] = useState("");
  const actions = [
    ...NAV_ITEMS.map((item) => ({
      id: item.id,
      label: `Open ${item.label}`,
      meta: "Page",
      icon: item.icon,
    })),
    {
      id: "simulate",
      label: "Run private simulation",
      meta: "Action",
      icon: "play" as const,
    },
    {
      id: "onboarding",
      label: "Open setup guide",
      meta: "Action",
      icon: "spark" as const,
    },
  ];
  const visible = actions.filter((action) =>
    action.label.toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div
      className="command-overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Find anything"
      >
        <div className="command-palette__input">
          <Icon name="search" />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Go to a page or run an action…"
          />
          <button onClick={onClose}>
            <kbd>Esc</kbd>
          </button>
        </div>
        <div className="command-palette__results">
          <span>{query ? "MATCHES" : "QUICK ACCESS"}</span>
          {visible.map((action) => (
            <button key={action.id} onClick={() => onSelect(action.id)}>
              <Icon name={action.icon} />
              <strong>{action.label}</strong>
              <small>{action.meta}</small>
              <Icon name="arrow" />
            </button>
          ))}
          {visible.length === 0 && <p>No pages or actions match “{query}”.</p>}
        </div>
      </div>
    </div>
  );
}

function DemoControls({
  state,
  onState,
  onOnboarding,
}: {
  state: DemoState;
  onState: (state: DemoState) => void;
  onOnboarding: () => void;
}) {
  const params = new URLSearchParams(window.location.search);
  if (params.get("demoControls") !== "1") return null;
  return (
    <div className="demo-controls" aria-label="Demo state controls">
      <span>DEMO</span>
      {(
        [
          "ready",
          "active",
          "loading",
          "empty",
          "error",
          "degraded",
        ] as DemoState[]
      ).map((item) => (
        <button
          key={item}
          className={state === item ? "is-active" : ""}
          onClick={() => onState(item)}
        >
          {item}
        </button>
      ))}
      <button onClick={onOnboarding}>setup</button>
    </div>
  );
}
