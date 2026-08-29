import { useCallback, useEffect, useMemo, useState } from "react";
import type { AppPreferences, ExecutionMode } from "./types";
import {
  promptAndSaveProviderCredential,
  testProviderCredential,
} from "./providerCredentials";
import {
  cancelNativeSimulation,
  loadNativeBootstrapHealth,
  LOADING_NATIVE_BOOTSTRAP,
  readDiagnosticSummary,
  readMediaBrokerDiagnostics,
  readRuntimeDoctor,
  runSyntheticReplayCapture,
  saveOnboarding,
  startNativeSimulation,
  syntheticReplayCaptureAvailability,
  type NativeBootstrapHealth,
  type NativeDiagnosticSummary,
  type NativeDoctorReport,
  type NativeMediaBrokerDiagnostics,
  type NativeOnboardingStep,
  type NativeSimulationEvent,
  type OnboardingSnapshot,
} from "./tauriBridge";

type ProductPage = "session" | "world" | "voice" | "diagnostics" | "settings";

const PAGE_ALIASES: Record<string, ProductPage> = {
  home: "session",
  conversation: "session",
  games: "world",
  characters: "world",
  presence: "world",
  models: "voice",
  performance: "diagnostics",
  help: "settings",
};

const DEFAULT_PREFERENCES: AppPreferences = {
  execution: "cloud",
  performance: "balanced",
  subtitles: true,
  ptt: true,
  localOnly: false,
  screenPresence: false,
  diagnostics: true,
};

const NAV: Array<{ id: ProductPage; label: string; index: string }> = [
  { id: "session", label: "Session deck", index: "01" },
  { id: "world", label: "World", index: "02" },
  { id: "voice", label: "Voice & models", index: "03" },
  { id: "diagnostics", label: "Diagnostics", index: "04" },
  { id: "settings", label: "Settings & guide", index: "05" },
];

const ONBOARDING_STEPS: Array<{ id: NativeOnboardingStep; label: string }> = [
  { id: "scan", label: "System" },
  { id: "game", label: "World" },
  { id: "providers", label: "Voice" },
  { id: "simulation", label: "Test" },
];

const nowSnapshot = (
  completed: boolean,
  currentStep: NativeOnboardingStep,
  preferences: AppPreferences,
): OnboardingSnapshot => ({
  schemaVersion: 1,
  completed,
  currentStep,
  selectedGameId: "eclipse-harbor",
  preferences,
  updatedAtEpochMs: Date.now(),
});

function initialPage(): ProductPage {
  const requested = new URLSearchParams(window.location.search).get("page");
  if (!requested) return "session";
  if (NAV.some((item) => item.id === requested))
    return requested as ProductPage;
  return PAGE_ALIASES[requested] ?? "session";
}

function runtimeSummary(health: NativeBootstrapHealth) {
  if (health.kind === "loading")
    return { tone: "wait", label: "Starting native services" };
  if (health.kind === "browserPreview")
    return {
      tone: "muted",
      label: "Browser preview — native actions unavailable",
    };
  if (health.kind === "unavailable")
    return { tone: "bad", label: "Native control unavailable" };
  const connected =
    health.snapshot.runtime.connected && health.snapshot.mediaBroker.connected;
  return connected
    ? { tone: "good", label: "Runtime and media broker authenticated" }
    : { tone: "wait", label: "Native services are still connecting" };
}

export function ProductConsole() {
  const query = useMemo(() => new URLSearchParams(window.location.search), []);
  const [page, setPage] = useState<ProductPage>(initialPage);
  const [bootstrap, setBootstrap] = useState<NativeBootstrapHealth>(
    LOADING_NATIVE_BOOTSTRAP,
  );
  const [preferences, setPreferences] =
    useState<AppPreferences>(DEFAULT_PREFERENCES);
  const [setupOpen, setSetupOpen] = useState(query.get("onboarding") === "1");
  const [setupStep, setSetupStep] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [turnEvents, setTurnEvents] = useState<NativeSimulationEvent[]>([]);
  const [deliveredTurn, setDeliveredTurn] = useState<
    (NativeSimulationEvent & { type: "completed" }) | null
  >(null);
  const [authorizeLiveVoice, setAuthorizeLiveVoice] = useState(false);
  const [doctor, setDoctor] = useState<NativeDoctorReport | null>(null);
  const [broker, setBroker] = useState<NativeMediaBrokerDiagnostics | null>(
    null,
  );
  const [diagnostics, setDiagnostics] =
    useState<NativeDiagnosticSummary | null>(null);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [providerBusy, setProviderBusy] = useState(false);
  const [captureProof, setCaptureProof] = useState<{
    pid: number;
    frames: number;
  } | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    void loadNativeBootstrapHealth({ signal: controller.signal })
      .then((health) => {
        setBootstrap(health);
        if (health.kind !== "snapshot") return;
        const onboarding = health.snapshot.onboarding;
        if (onboarding) {
          setPreferences(onboarding.preferences);
          if (!onboarding.completed) {
            const knownStep = ONBOARDING_STEPS.findIndex(
              (item) => item.id === onboarding.currentStep,
            );
            setSetupStep(Math.max(0, knownStep));
            setSetupOpen(true);
          }
        }
      })
      .catch((error: unknown) => {
        if (!(error instanceof Error && error.name === "AbortError")) {
          setNotice("Native bootstrap did not complete.");
        }
      });
    return () => controller.abort();
  }, []);

  const snapshot = bootstrap.kind === "snapshot" ? bootstrap.snapshot : null;
  const provider = snapshot?.providers?.find(
    (item) => item.providerId === "elevenlabs",
  );
  const providerPresent = provider?.status === "present";
  const captureDebugAvailable = syntheticReplayCaptureAvailability(
    Boolean(snapshot?.capabilities?.debugSyntheticReplayCapture),
  );
  const activeEvent = turnEvents.at(-1);
  const activeStage =
    activeEvent?.type === "stageStarted" ||
    activeEvent?.type === "stageCompleted"
      ? activeEvent.stage
      : activeEvent?.type === "sentenceReady"
        ? "voicing"
        : activeEvent?.type === "completed"
          ? "animating"
          : null;

  const navigate = (next: ProductPage) => {
    setPage(next);
    const url = new URL(window.location.href);
    url.searchParams.set("page", next);
    url.searchParams.delete("state");
    window.history.replaceState({}, "", url);
  };

  const persistSetup = useCallback(
    async (completed: boolean, step: NativeOnboardingStep) => {
      const result = await saveOnboarding(
        nowSnapshot(completed, step, preferences),
      );
      if (result) setPreferences(result.onboarding.preferences);
    },
    [preferences],
  );

  const nextSetupStep = async () => {
    if (setupStep < ONBOARDING_STEPS.length - 1) {
      const next = setupStep + 1;
      setSetupStep(next);
      await persistSetup(false, ONBOARDING_STEPS[next].id);
      return;
    }
    await persistSetup(true, "ready");
    setSetupOpen(false);
    setNotice("Setup saved. Session deck is ready.");
  };

  const runTurn = async () => {
    setDeliveredTurn(null);
    setTurnEvents([]);
    setRunning(true);
    setNotice(null);
    try {
      const started = await startNativeSimulation(
        preferences.execution,
        (event) => {
          setTurnEvents((current) => [...current, event].slice(-24));
          if (event.type === "completed") {
            setDeliveredTurn(event);
            setRunning(false);
          }
          if (event.type === "cancelled") setRunning(false);
        },
        {
          gameProfileId: "eclipse-harbor",
          characterId: "mara-venn",
          characterName: "Mara Venn",
          transcript: "Did you ever make it to the old lighthouse?",
          ...(authorizeLiveVoice && providerPresent
            ? {
                devLiveTts: {
                  providerId: "elevenlabs",
                  modelId: "eleven_flash_v2_5",
                  voiceId: "EXAVITQu4vr4xnSDxMaL",
                  explicitUserAuthorization: true as const,
                },
              }
            : {}),
        },
      );
      if (!started) {
        setRunning(false);
        setNotice("Native desktop runtime is required for a delivered turn.");
      }
    } catch (error) {
      setRunning(false);
      setNotice(
        error instanceof Error ? error.message : "Turn failed before delivery.",
      );
    }
  };

  const stopTurn = async () => {
    await cancelNativeSimulation();
    setRunning(false);
  };

  const refreshDiagnostics = async () => {
    setDiagnosticsBusy(true);
    const [nextDoctor, nextBroker, nextDiagnostics] = await Promise.all([
      readRuntimeDoctor(),
      readMediaBrokerDiagnostics(),
      readDiagnosticSummary(),
    ]);
    setDoctor(nextDoctor);
    setBroker(nextBroker);
    setDiagnostics(nextDiagnostics);
    setDiagnosticsBusy(false);
  };

  const selectSyntheticTarget = async () => {
    try {
      const result = await runSyntheticReplayCapture(captureDebugAvailable);
      setCaptureProof({
        pid: result.targetProcessId,
        frames: result.diagnostics.framesReceived,
      });
      setNotice(
        `Synthetic target selected · PID ${result.targetProcessId} · ${result.diagnostics.framesReceived} frames received`,
      );
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Synthetic capture failed.",
      );
    }
  };

  const runProviderAction = async (action: "save" | "validate") => {
    setProviderBusy(true);
    try {
      const result =
        action === "save"
          ? await promptAndSaveProviderCredential("elevenlabs")
          : await testProviderCredential("elevenlabs");
      setNotice(result.detail);
      if (action === "save" && result.status === "present") {
        const health = await loadNativeBootstrapHealth({ maxAttempts: 1 });
        setBootstrap(health);
      }
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : "Credential action failed.",
      );
    } finally {
      setProviderBusy(false);
    }
  };

  return (
    <div className="product-shell">
      <aside className="product-rail" aria-label="Primary navigation">
        <button
          className="brand-lockup"
          onClick={() => navigate("session")}
          aria-label="Open session deck"
        >
          <span className="brand-mark">N2</span>
          <span>
            <b>NPC 2.0</b>
            <small>session instrument</small>
          </span>
        </button>
        <nav>
          {NAV.map((item) => (
            <button
              key={item.id}
              className={page === item.id ? "nav-item selected" : "nav-item"}
              onClick={() => navigate(item.id)}
            >
              <span>{item.index}</span>
              <b>{item.label}</b>
            </button>
          ))}
        </nav>
        <div className="rail-safety">
          <span className="status-dot good" />
          Single-player only<small>Anti-cheat modes stay blocked.</small>
        </div>
      </aside>

      <main className="product-main">
        <header className="product-topbar">
          <div>
            <span className="eyebrow">
              NPC 2.0 / {NAV.find((item) => item.id === page)?.label}
            </span>
          </div>
          <div className={`runtime-chip ${runtimeSummary(bootstrap).tone}`}>
            <span className="status-dot" />
            {runtimeSummary(bootstrap).label}
          </div>
          <button className="quiet-button" onClick={() => setSetupOpen(true)}>
            Setup
          </button>
        </header>

        {notice && (
          <div className="notice" role="status">
            <span>{notice}</span>
            <button onClick={() => setNotice(null)} aria-label="Dismiss notice">
              ×
            </button>
          </div>
        )}

        <SignalRail
          activeStage={activeStage}
          running={running}
          delivered={Boolean(deliveredTurn)}
          providerPresent={providerPresent}
          captureProof={captureProof}
        />

        {page === "session" && (
          <SessionPage
            running={running}
            deliveredTurn={deliveredTurn}
            events={turnEvents}
            providerPresent={providerPresent}
            captureProof={captureProof}
            authorizeLiveVoice={authorizeLiveVoice}
            setAuthorizeLiveVoice={setAuthorizeLiveVoice}
            onRun={runTurn}
            onStop={stopTurn}
            onNavigate={navigate}
          />
        )}
        {page === "world" && (
          <WorldPage
            captureProof={captureProof}
            captureAvailable={captureDebugAvailable.available}
            onCapture={selectSyntheticTarget}
          />
        )}
        {page === "voice" && (
          <VoicePage
            providerPresent={providerPresent}
            providerDetail={provider?.detail}
            execution={preferences.execution}
            setExecution={(execution) =>
              setPreferences((current) => ({ ...current, execution }))
            }
            nativeAvailable={bootstrap.kind === "snapshot"}
            providerBusy={providerBusy}
            onProviderAction={runProviderAction}
            onSave={async () => {
              await persistSetup(true, "ready");
              setNotice("Execution default saved.");
            }}
          />
        )}
        {page === "diagnostics" && (
          <DiagnosticsPage
            bootstrap={bootstrap}
            doctor={doctor}
            broker={broker}
            diagnostics={diagnostics ?? snapshot?.diagnostics ?? null}
            busy={diagnosticsBusy}
            nativeAvailable={bootstrap.kind === "snapshot"}
            onRefresh={refreshDiagnostics}
          />
        )}
        {page === "settings" && (
          <SettingsPage
            preferences={preferences}
            setPreferences={setPreferences}
            onSave={async () => {
              await persistSetup(true, "ready");
              setNotice("Supported preferences saved to the native store.");
            }}
            onSetup={() => {
              setSetupStep(0);
              setSetupOpen(true);
            }}
            onDiagnostics={() => navigate("diagnostics")}
            nativeAvailable={bootstrap.kind === "snapshot"}
          />
        )}
      </main>

      {setupOpen && (
        <OnboardingOverlay
          step={setupStep}
          bootstrap={bootstrap}
          providerPresent={providerPresent}
          authorizeLiveVoice={authorizeLiveVoice}
          setAuthorizeLiveVoice={setAuthorizeLiveVoice}
          nativeAvailable={bootstrap.kind === "snapshot"}
          captureAvailable={captureDebugAvailable.available}
          captureProof={captureProof}
          deliveredTurn={deliveredTurn}
          running={running}
          preferences={preferences}
          setPreferences={setPreferences}
          onBack={() => setSetupStep((current) => Math.max(0, current - 1))}
          onNext={nextSetupStep}
          onCapture={selectSyntheticTarget}
          onProviderAction={runProviderAction}
          onRun={runTurn}
          onClose={
            snapshot?.onboarding?.completed || query.get("onboarding") === "1"
              ? () => setSetupOpen(false)
              : undefined
          }
        />
      )}
    </div>
  );
}

function SignalRail({
  activeStage,
  running,
  delivered,
  providerPresent,
  captureProof,
}: {
  activeStage: string | null;
  running: boolean;
  delivered: boolean;
  providerPresent: boolean;
  captureProof: { pid: number; frames: number } | null;
}) {
  const nodes = [
    {
      label: "Target",
      value: captureProof
        ? `PID ${captureProof.pid} · ${captureProof.frames}f`
        : "Eclipse Harbor · not selected",
      ready: Boolean(captureProof),
    },
    { label: "Actor", value: "Mara Venn", ready: true },
    {
      label: "Route",
      value: providerPresent ? "ElevenLabs ready" : "Fixture-safe",
      ready: providerPresent,
    },
    { label: "Voice", value: "Stock · Sarah", ready: providerPresent },
    {
      label: "Delivery",
      value: delivered
        ? "Committed"
        : running
          ? (activeStage ?? "Starting")
          : "Standby",
      ready: delivered,
    },
  ];
  return (
    <section
      className={running ? "signal-rail running" : "signal-rail"}
      aria-label="Session signal rail"
    >
      {nodes.map((node, index) => (
        <div className="signal-node" key={node.label}>
          <span className={node.ready ? "signal-index ready" : "signal-index"}>
            {String(index + 1).padStart(2, "0")}
          </span>
          <span>
            <small>{node.label}</small>
            <b>{node.value}</b>
          </span>
        </div>
      ))}
    </section>
  );
}

function SessionPage({
  running,
  deliveredTurn,
  events,
  providerPresent,
  captureProof,
  authorizeLiveVoice,
  setAuthorizeLiveVoice,
  onRun,
  onStop,
  onNavigate,
}: {
  running: boolean;
  deliveredTurn: (NativeSimulationEvent & { type: "completed" }) | null;
  events: NativeSimulationEvent[];
  providerPresent: boolean;
  captureProof: { pid: number; frames: number } | null;
  authorizeLiveVoice: boolean;
  setAuthorizeLiveVoice: (value: boolean) => void;
  onRun: () => void;
  onStop: () => void;
  onNavigate: (page: ProductPage) => void;
}) {
  const completionLabel = deliveredTurn?.runtimeFixtureOnly
    ? "Deterministic runtime fixture — no audible delivery claimed"
    : deliveredTurn
      ? "Live provider audio delivered to the native sink"
      : "No delivered turn in this session";
  return (
    <div className="page-grid session-grid">
      <section className="instrument-panel primary-instrument">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Selected conversation</span>
            <h1>Mara Venn</h1>
            <p>
              Eclipse Harbor · lighthouse keeper · explicit game-scoped identity
            </p>
          </div>
          <span className="identity-seal">MV</span>
        </div>
        <div className="prompt-block">
          <small>Bounded test prompt</small>
          <p>“Did you ever make it to the old lighthouse?”</p>
        </div>
        <div className="session-controls">
          <button className="primary-action" onClick={onRun} disabled={running}>
            {running ? "Turn in progress" : "Run spoken turn"}
            <span>PTT rehearsal prompt</span>
          </button>
          {running && (
            <button className="stop-action" onClick={onStop}>
              Cancel generation
            </button>
          )}
          <label
            className={
              providerPresent
                ? "authorization-control"
                : "authorization-control disabled"
            }
          >
            <input
              type="checkbox"
              checked={authorizeLiveVoice}
              disabled={!providerPresent || running}
              onChange={(event) => setAuthorizeLiveVoice(event.target.checked)}
            />
            <span>
              <b>Authorize one live stock-voice call</b>
              <small>
                {providerPresent
                  ? "Sends only the generated reply text to ElevenLabs."
                  : "Add an ElevenLabs credential in Voice & models first."}
              </small>
            </span>
          </label>
        </div>
        <div className="delivery-proof">
          <span
            className={
              deliveredTurn && !deliveredTurn.runtimeFixtureOnly
                ? "proof-icon delivered"
                : "proof-icon"
            }
          />
          <div>
            <small>Delivery ledger</small>
            <b>{completionLabel}</b>
            {deliveredTurn && <p>{deliveredTurn.deliveredText}</p>}
          </div>
        </div>
      </section>

      <aside className="session-side">
        <section className="instrument-panel compact-panel">
          <div className="panel-title">
            <h2>Ready decision</h2>
            <span className={providerPresent ? "badge good" : "badge wait"}>
              {providerPresent ? "Live route" : "Fixture route"}
            </span>
          </div>
          <dl className="facts">
            <div>
              <dt>Capture</dt>
              <dd>
                {captureProof
                  ? `Selected · PID ${captureProof.pid}`
                  : "Not selected"}
              </dd>
            </div>
            <div>
              <dt>Identity</dt>
              <dd>Explicit selection</dd>
            </div>
            <div>
              <dt>Input</dt>
              <dd>PTT required</dd>
            </div>
            <div>
              <dt>Subtitles</dt>
              <dd>Bottom-center safe area</dd>
            </div>
          </dl>
          <button className="text-action" onClick={() => onNavigate("world")}>
            Inspect world selection →
          </button>
        </section>
        <section className="instrument-panel trace-panel">
          <div className="panel-title">
            <h2>Turn trace</h2>
            <span className="mono">{events.length} events</span>
          </div>
          {events.length === 0 ? (
            <p className="empty-copy">
              No current trace. Run a turn to populate runtime-owned events.
            </p>
          ) : (
            <ol>
              {events.slice(-7).map((event) => (
                <li key={`${event.sequence}-${event.type}`}>
                  <span>{event.sequence.toString().padStart(2, "0")}</span>
                  <b>{event.type}</b>
                  <small>
                    {"stage" in event
                      ? event.stage
                      : "measurementBasis" in event
                        ? event.measurementBasis
                        : ""}
                  </small>
                </li>
              ))}
            </ol>
          )}
        </section>
      </aside>
    </div>
  );
}

function WorldPage({
  captureAvailable,
  captureProof,
  onCapture,
}: {
  captureAvailable: boolean;
  captureProof: { pid: number; frames: number } | null;
  onCapture: () => void;
}) {
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">Target and identity boundary</span>
        <h1>World</h1>
        <p>
          One task-owned synthetic game, one selected character, and provenance
          at every boundary.
        </p>
      </header>
      <div className="world-layout">
        <section className="instrument-panel world-visual">
          <div className="world-horizon">
            <div className="moon" />
            <div className="lighthouse">
              <i />
              <span />
            </div>
            <div className="harbor-lines" />
          </div>
          <div className="world-overlay">
            <span className={captureProof ? "badge good" : "badge wait"}>
              {captureProof ? "Capture selected" : "Configured test world"}
            </span>
            <h2>Eclipse Harbor</h2>
            <p>
              Synthetic validation target · offline · task-owned GUI executable
            </p>
          </div>
        </section>
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>Capture contract</h2>
            <span className="badge">Debug only</span>
          </div>
          <dl className="facts spacious">
            <div>
              <dt>Executable</dt>
              <dd>synthetic-game.exe</dd>
            </div>
            <div>
              <dt>Policy</dt>
              <dd>Single-player only</dd>
            </div>
            <div>
              <dt>Evidence</dt>
              <dd>PID · HWND · frame counters</dd>
            </div>
            <div>
              <dt>Backend</dt>
              <dd>Windows Graphics Capture</dd>
            </div>
          </dl>
          <button
            className="secondary-action"
            disabled={!captureAvailable}
            onClick={onCapture}
          >
            {captureProof
              ? "Refresh synthetic capture evidence"
              : captureAvailable
                ? "Select synthetic target"
                : "Open native debug build to test capture"}
          </button>
        </section>
      </div>
      <section className="instrument-panel character-row">
        <div className="character-avatar">MV</div>
        <div>
          <span className="eyebrow">Game-scoped roster · 1 character</span>
          <h2>Mara Venn</h2>
          <p>
            Lighthouse keeper. Identity authority is the explicit selection in
            the Eclipse Harbor profile; no face or demographic inference is
            used.
          </p>
        </div>
        <span className="badge good">Identity locked</span>
      </section>
    </div>
  );
}

function VoicePage({
  providerPresent,
  providerDetail,
  execution,
  setExecution,
  nativeAvailable,
  providerBusy,
  onProviderAction,
  onSave,
}: {
  providerPresent: boolean;
  providerDetail?: string;
  execution: ExecutionMode;
  setExecution: (mode: ExecutionMode) => void;
  nativeAvailable: boolean;
  providerBusy: boolean;
  onProviderAction: (action: "save" | "validate") => void;
  onSave: () => void;
}) {
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">Effective route</span>
        <h1>Voice & models</h1>
        <p>
          Every role names its execution boundary. Credentials remain in the
          native vault and never enter this WebView.
        </p>
      </header>
      <section className="route-map" aria-label="Effective model route">
        {[
          {
            role: "Speech in",
            model: "Native rehearsal",
            provider: "Device input · pending qualification",
          },
          {
            role: "Reply",
            model: "Runtime fixture",
            provider: "Deterministic until a hosted LLM is qualified",
          },
          {
            role: "Voice out",
            model: "eleven_flash_v2_5",
            provider: "ElevenLabs · stock Sarah",
          },
          {
            role: "Memory",
            model: "Delivered-turn ledger",
            provider: "Local SQLite boundary",
          },
        ].map((route) => (
          <article key={route.role} className="route-card">
            <span>{route.role}</span>
            <h2>{route.model}</h2>
            <p>{route.provider}</p>
          </article>
        ))}
      </section>
      <div className="voice-layout">
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>ElevenLabs</h2>
            <span className={providerPresent ? "badge good" : "badge bad"}>
              {providerPresent ? "Credential present" : "Credential missing"}
            </span>
          </div>
          <p className="body-copy">
            The approved live slice uses a provider stock voice. It never clones
            or implies a game performer.
          </p>
          <dl className="facts spacious">
            <div>
              <dt>Voice</dt>
              <dd>Sarah · stock</dd>
            </div>
            <div>
              <dt>Model</dt>
              <dd className="mono">eleven_flash_v2_5</dd>
            </div>
            <div>
              <dt>Data sent</dt>
              <dd>Generated reply text only</dd>
            </div>
            <div>
              <dt>Vault</dt>
              <dd>{providerDetail ?? "Native status unavailable"}</dd>
            </div>
          </dl>
          <div className="provider-actions">
            <button
              className="secondary-action"
              disabled={!nativeAvailable || providerBusy}
              onClick={() => onProviderAction("save")}
            >
              {providerBusy
                ? "Native prompt open…"
                : providerPresent
                  ? "Replace credential"
                  : "Add credential securely"}
            </button>
            <button
              className="quiet-button"
              disabled={!providerPresent || providerBusy}
              onClick={() => onProviderAction("validate")}
            >
              Validate vault binding
            </button>
          </div>
          {!nativeAvailable && (
            <small className="control-reason">
              Credential controls require the native desktop shell.
            </small>
          )}
        </section>
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>Execution default</h2>
            <span className="badge">Native preference</span>
          </div>
          <div className="segmented-control">
            {(["cloud", "hybrid", "local"] as ExecutionMode[]).map((mode) => (
              <button
                key={mode}
                className={execution === mode ? "active" : ""}
                onClick={() => setExecution(mode)}
              >
                {mode}
              </button>
            ))}
          </div>
          <p className="body-copy">
            Local packs stay unavailable until a model, license manifest, device
            fit, and measured quality have all been qualified.
          </p>
          <button
            className="secondary-action route-save"
            disabled={!nativeAvailable}
            onClick={onSave}
          >
            Save execution default
          </button>
          <div className="blocked-state">
            <b>Local visual packs not qualified</b>
            <span>No download or enable control is shown yet.</span>
          </div>
        </section>
      </div>
    </div>
  );
}

function DiagnosticsPage({
  bootstrap,
  doctor,
  broker,
  diagnostics,
  busy,
  nativeAvailable,
  onRefresh,
}: {
  bootstrap: NativeBootstrapHealth;
  doctor: NativeDoctorReport | null;
  broker: NativeMediaBrokerDiagnostics | null;
  diagnostics: NativeDiagnosticSummary | null;
  busy: boolean;
  nativeAvailable: boolean;
  onRefresh: () => void;
}) {
  const checks = diagnostics?.checks ?? [];
  return (
    <div className="page-stack">
      <header className="page-heading split">
        <div>
          <span className="eyebrow">Native truth, timestamped</span>
          <h1>Diagnostics</h1>
          <p>
            No illustrative CPU, GPU, latency, or provider values are rendered
            here.
          </p>
        </div>
        <button
          className="secondary-action"
          onClick={onRefresh}
          disabled={busy || !nativeAvailable}
        >
          {busy
            ? "Refreshing…"
            : nativeAvailable
              ? "Refresh native checks"
              : "Native desktop required"}
        </button>
      </header>
      <div className="diagnostic-summary">
        <article>
          <span>Control bootstrap</span>
          <b>{bootstrap.kind}</b>
          <small>
            {bootstrap.kind === "snapshot"
              ? `${bootstrap.attempts} attempt${bootstrap.attempts === 1 ? "" : "s"}`
              : "No native measurement"}
          </small>
        </article>
        <article>
          <span>Runtime doctor</span>
          <b>{doctor?.status ?? "Not run"}</b>
          <small>
            {doctor
              ? `${doctor.profileCount} profiles · ${doctor.providerCount} providers`
              : "Refresh to call authenticated runtime"}
          </small>
        </article>
        <article>
          <span>Broker frames</span>
          <b>{broker ? broker.framesReceived.toLocaleString() : "—"}</b>
          <small>
            {broker
              ? `${broker.framesDropped} dropped · generation ${broker.deviceGeneration}`
              : "Refresh to read native counters"}
          </small>
        </article>
        <article>
          <span>Evidence class</span>
          <b>
            {diagnostics?.measurements.currentResultsAreReleaseEvidence
              ? "Release"
              : "Not release"}
          </b>
          <small>
            {diagnostics?.measurements.reason ?? "No diagnostic summary"}
          </small>
        </article>
      </div>
      <section className="instrument-panel check-table">
        <div className="panel-title">
          <h2>Current checks</h2>
          <span className="mono">
            {diagnostics
              ? new Date(diagnostics.generatedAtEpochMs).toLocaleTimeString()
              : "not refreshed"}
          </span>
        </div>
        {checks.length === 0 ? (
          <div className="empty-state">
            <b>No native results yet</b>
            <p>
              Open the Windows desktop build and refresh. Browser preview
              deliberately invents nothing.
            </p>
          </div>
        ) : (
          <div role="table">
            {checks.map((check) => (
              <div className="check-row" role="row" key={check.id}>
                <span className={`check-status ${check.status}`} />
                <div>
                  <b>{check.title}</b>
                  <p>{check.detail}</p>
                  {check.remediation && <small>{check.remediation}</small>}
                </div>
                <code>{check.id}</code>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function SettingsPage({
  preferences,
  setPreferences,
  onSave,
  onSetup,
  onDiagnostics,
  nativeAvailable,
}: {
  preferences: AppPreferences;
  setPreferences: (
    update: AppPreferences | ((current: AppPreferences) => AppPreferences),
  ) => void;
  onSave: () => void;
  onSetup: () => void;
  onDiagnostics: () => void;
  nativeAvailable: boolean;
}) {
  const toggle = (key: "subtitles" | "ptt" | "diagnostics") =>
    setPreferences((current) => ({ ...current, [key]: !current[key] }));
  return (
    <div className="page-stack">
      <header className="page-heading">
        <span className="eyebrow">Persisted support only</span>
        <h1>Settings & guide</h1>
        <p>
          Every control below has a native persistence effect or a direct
          product destination.
        </p>
      </header>
      <div className="settings-layout">
        <section className="instrument-panel">
          <div className="panel-title">
            <h2>Conversation defaults</h2>
            <span className="badge">Native store</span>
          </div>
          <ToggleRow
            label="Push to talk"
            detail="A turn begins only while the bound PTT input is held."
            checked={preferences.ptt}
            onChange={() => toggle("ptt")}
          />
          <ToggleRow
            label="Subtitles"
            detail="Show only runtime-delivered text in the safe area."
            checked={preferences.subtitles}
            onChange={() => toggle("subtitles")}
          />
          <ToggleRow
            label="Diagnostic detail"
            detail="Retain bounded local turn traces and native checks."
            checked={preferences.diagnostics}
            onChange={() => toggle("diagnostics")}
          />
          <button
            className="primary-action small"
            disabled={!nativeAvailable}
            onClick={onSave}
          >
            {nativeAvailable
              ? "Save supported preferences"
              : "Native desktop required to save"}
          </button>
        </section>
        <section className="instrument-panel guide-list">
          <div className="panel-title">
            <h2>Setup guide</h2>
            <span className="badge good">4 steps</span>
          </div>
          <button onClick={onSetup}>
            <span>01</span>
            <div>
              <b>Rerun guided setup</b>
              <small>System → World → Voice → Test</small>
            </div>
            <i>→</i>
          </button>
          <button onClick={onDiagnostics}>
            <span>02</span>
            <div>
              <b>Run native diagnostics</b>
              <small>
                Runtime, broker, capture, provider, and evidence state
              </small>
            </div>
            <i>→</i>
          </button>
          <div className="guide-note">
            <b>Privacy boundary</b>
            <p>
              Credential values never enter the WebView. Live voice is
              authorized one bounded call at a time in this development slice.
            </p>
          </div>
        </section>
      </div>
    </div>
  );
}

function ToggleRow({
  label,
  detail,
  checked,
  onChange,
}: {
  label: string;
  detail: string;
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <label className="toggle-row">
      <span>
        <b>{label}</b>
        <small>{detail}</small>
      </span>
      <input type="checkbox" checked={checked} onChange={onChange} />
      <i aria-hidden="true" />
    </label>
  );
}

function OnboardingOverlay({
  step,
  bootstrap,
  providerPresent,
  authorizeLiveVoice,
  setAuthorizeLiveVoice,
  nativeAvailable,
  captureAvailable,
  captureProof,
  deliveredTurn,
  running,
  preferences,
  setPreferences,
  onBack,
  onNext,
  onCapture,
  onProviderAction,
  onRun,
  onClose,
}: {
  step: number;
  bootstrap: NativeBootstrapHealth;
  providerPresent: boolean;
  authorizeLiveVoice: boolean;
  setAuthorizeLiveVoice: (value: boolean) => void;
  nativeAvailable: boolean;
  captureAvailable: boolean;
  captureProof: { pid: number; frames: number } | null;
  deliveredTurn: (NativeSimulationEvent & { type: "completed" }) | null;
  running: boolean;
  preferences: AppPreferences;
  setPreferences: (
    update: AppPreferences | ((current: AppPreferences) => AppPreferences),
  ) => void;
  onBack: () => void;
  onNext: () => void;
  onCapture: () => void;
  onProviderAction: (action: "save" | "validate") => void;
  onRun: () => void;
  onClose?: () => void;
}) {
  return (
    <div className="setup-scrim" role="presentation">
      <section
        className="setup-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="setup-title"
      >
        <header className="setup-header">
          <div>
            <span className="brand-mark">N2</span>
            <span>
              <b>Guided setup</b>
              <small>Real state only</small>
            </span>
          </div>
          {onClose && (
            <button onClick={onClose} aria-label="Close setup">
              ×
            </button>
          )}
        </header>
        <ol className="setup-progress">
          {ONBOARDING_STEPS.map((item, index) => (
            <li
              key={item.id}
              className={
                index === step ? "current" : index < step ? "complete" : ""
              }
            >
              <span>{index < step ? "✓" : index + 1}</span>
              <b>{item.label}</b>
            </li>
          ))}
        </ol>
        <div className="setup-content">
          {step === 0 && (
            <>
              <span className="eyebrow">01 / system</span>
              <h1 id="setup-title">Prove the native boundary</h1>
              <p>
                NPC 2.0 needs its authenticated runtime and media broker before
                it can select a window or deliver audio.
              </p>
              <div className="setup-check">
                <span
                  className={
                    bootstrap.kind === "snapshot"
                      ? "check-status passed"
                      : "check-status warning"
                  }
                />
                <div>
                  <b>{runtimeSummary(bootstrap).label}</b>
                  <small>
                    {bootstrap.kind === "snapshot"
                      ? bootstrap.snapshot.runtime.detail
                      : "Open the native desktop shell for a real check."}
                  </small>
                </div>
              </div>
            </>
          )}
          {step === 1 && (
            <>
              <span className="eyebrow">02 / world</span>
              <h1 id="setup-title">Select the safe test world</h1>
              <p>
                The first run uses only the task-owned Eclipse Harbor window.
                Online games and detected anti-cheat remain blocked.
              </p>
              <div className="setup-selection selected">
                <span>EH</span>
                <div>
                  <b>Eclipse Harbor</b>
                  <small>
                    Synthetic GUI target · single-player · Mara Venn
                  </small>
                </div>
                <i>{captureProof ? `PID ${captureProof.pid}` : "Configured"}</i>
              </div>
              <button
                className="secondary-action setup-inline-action"
                disabled={!captureAvailable}
                onClick={onCapture}
              >
                {captureProof
                  ? "Refresh capture evidence"
                  : captureAvailable
                    ? "Select running synthetic target"
                    : "Native debug capture unavailable"}
              </button>
            </>
          )}
          {step === 2 && (
            <>
              <span className="eyebrow">03 / voice</span>
              <h1 id="setup-title">Choose the execution boundary</h1>
              <p>
                A credential summary can report presence, never the secret
                value. Live voice remains explicitly authorized per test turn.
              </p>
              <div className="segmented-control large">
                {(["cloud", "hybrid", "local"] as ExecutionMode[]).map(
                  (mode) => (
                    <button
                      key={mode}
                      className={preferences.execution === mode ? "active" : ""}
                      onClick={() =>
                        setPreferences((current) => ({
                          ...current,
                          execution: mode,
                        }))
                      }
                    >
                      {mode}
                    </button>
                  ),
                )}
              </div>
              <div className="setup-check">
                <span
                  className={
                    providerPresent
                      ? "check-status passed"
                      : "check-status warning"
                  }
                />
                <div>
                  <b>ElevenLabs stock voice</b>
                  <small>
                    {providerPresent
                      ? "Credential reference is present in the native vault."
                      : "No native credential reference is present. Fixture-safe testing remains available."}
                  </small>
                </div>
              </div>
              <button
                className="secondary-action setup-inline-action"
                disabled={!nativeAvailable}
                onClick={() =>
                  onProviderAction(providerPresent ? "validate" : "save")
                }
              >
                {providerPresent
                  ? "Validate vault binding"
                  : "Add credential securely"}
              </button>
              {providerPresent && (
                <label className="setup-live-authorization">
                  <input
                    type="checkbox"
                    checked={authorizeLiveVoice}
                    onChange={(event) =>
                      setAuthorizeLiveVoice(event.target.checked)
                    }
                  />
                  <span>
                    <b>Authorize the next test as a live stock-voice call</b>
                    <small>
                      Sends only the generated reply text once; disable it to
                      use the deterministic fixture route.
                    </small>
                  </span>
                </label>
              )}
            </>
          )}
          {step === 3 && (
            <>
              <span className="eyebrow">04 / test</span>
              <h1 id="setup-title">Prove one delivered turn</h1>
              <p>
                The session starts with Mara Venn and the lighthouse prompt
                selected. A turn is committed only after runtime completion.
              </p>
              <div className="rehearsal-card">
                <span
                  className={
                    deliveredTurn ? "pulse-ring complete" : "pulse-ring"
                  }
                >
                  {deliveredTurn ? "✓" : "PTT"}
                </span>
                <div>
                  <b>
                    {deliveredTurn
                      ? "Runtime completion received"
                      : running
                        ? "Turn in progress"
                        : "Bounded lighthouse rehearsal"}
                  </b>
                  <small>
                    {deliveredTurn
                      ? deliveredTurn.runtimeFixtureOnly
                        ? "Deterministic delivery completed; audible speech was not claimed."
                        : "Receipt-backed live stock-voice delivery completed."
                      : "Run the selected route and inspect its delivered-only result."}
                  </small>
                </div>
              </div>
              <button
                className="secondary-action setup-inline-action"
                disabled={!nativeAvailable || running}
                onClick={onRun}
              >
                {running
                  ? "Turn in progress…"
                  : deliveredTurn
                    ? "Run the turn again"
                    : "Run bounded turn"}
              </button>
            </>
          )}
        </div>
        <footer className="setup-footer">
          <div>
            <b>{ONBOARDING_STEPS[step].label}</b>
            <small>
              Step {step + 1} of {ONBOARDING_STEPS.length}
            </small>
          </div>
          <div>
            {step > 0 && (
              <button className="quiet-button" onClick={onBack}>
                Back
              </button>
            )}
            <button
              className="primary-action small"
              onClick={onNext}
              disabled={
                step === ONBOARDING_STEPS.length - 1 &&
                nativeAvailable &&
                !deliveredTurn
              }
            >
              {step === ONBOARDING_STEPS.length - 1
                ? "Finish setup"
                : "Continue"}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
