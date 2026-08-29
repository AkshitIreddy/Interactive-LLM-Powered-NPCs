import { useEffect, useRef, useState } from "react";
import { Button } from "react-aria-components";
import { GAME_PROFILES, ONBOARDING_STEPS } from "./data";
import { Icon } from "./icons";
import type { AppPreferences, ExecutionMode, PerformanceMode } from "./types";
import {
  ActionButton,
  Disclosure,
  KeyboardKey,
  Metric,
  MiniBar,
  StatusPill,
  Toggle,
} from "./components";

interface OnboardingProps {
  initialStep?: number;
  preferences: AppPreferences;
  updatePreferences: (patch: Partial<AppPreferences>) => void;
  onClose: () => void;
}

export function Onboarding({
  initialStep = 0,
  preferences,
  updatePreferences,
  onClose,
}: OnboardingProps) {
  const [step, setStep] = useState(
    Math.max(0, Math.min(ONBOARDING_STEPS.length - 1, initialStep)),
  );
  const [selectedGame, setSelectedGame] = useState("skyrim");
  const [rehearsed, setRehearsed] = useState(false);
  const [simulation, setSimulation] = useState<"idle" | "running" | "done">(
    "idle",
  );
  const [hasMoreContent, setHasMoreContent] = useState(false);
  const contentRef = useRef<HTMLDivElement>(null);
  const current = ONBOARDING_STEPS[step];

  useEffect(() => {
    const content = contentRef.current;
    if (!content) return;

    content.scrollTop = 0;
    const updateScrollCue = () => {
      const remaining =
        content.scrollHeight - content.scrollTop - content.clientHeight;
      setHasMoreContent(remaining > 12);
    };
    const timer = window.setTimeout(updateScrollCue, 0);
    content.addEventListener("scroll", updateScrollCue, { passive: true });
    window.addEventListener("resize", updateScrollCue);

    return () => {
      window.clearTimeout(timer);
      content.removeEventListener("scroll", updateScrollCue);
      window.removeEventListener("resize", updateScrollCue);
    };
  }, [step]);

  const next = () => {
    if (step === 8 && simulation !== "done") {
      setSimulation("running");
      window.setTimeout(() => setSimulation("done"), 1350);
      return;
    }
    if (step === ONBOARDING_STEPS.length - 1) onClose();
    else setStep((value) => value + 1);
  };

  return (
    <div
      className="onboarding"
      role="dialog"
      aria-modal="true"
      aria-labelledby="onboarding-title"
      data-testid="onboarding"
    >
      <div className="onboarding__rail">
        <div className="brand brand--onboarding">
          <div className="brand__mark">
            <span>N</span>
            <i />
          </div>
          <div>
            <strong>NPC 2.0</strong>
            <small>Private preview</small>
          </div>
        </div>
        <ol className="onboarding-steps">
          {ONBOARDING_STEPS.map((item, index) => (
            <li
              key={item.id}
              className={
                index === step ? "is-active" : index < step ? "is-complete" : ""
              }
            >
              <button
                onClick={() => index <= step && setStep(index)}
                disabled={index > step}
                aria-current={index === step ? "step" : undefined}
              >
                <span>
                  {index < step ? (
                    <Icon name="check" size={13} />
                  ) : (
                    String(index + 1).padStart(2, "0")
                  )}
                </span>
                <div>
                  <strong>{item.label}</strong>
                  <small>{item.description}</small>
                </div>
              </button>
            </li>
          ))}
        </ol>
        <div className="onboarding__privacy">
          <Icon name="shield" size={16} />
          <span>
            Nothing leaves this PC unless you choose a cloud provider.
          </span>
        </div>
      </div>
      <main className="onboarding__main">
        <header className="onboarding__header">
          <div>
            <span>SETUP / {String(step + 1).padStart(2, "0")}</span>
            <strong>{current.label}</strong>
            <b className="fixture-badge">SIMULATED SETUP DATA</b>
          </div>
          <button className="text-button" onClick={onClose}>
            Finish later
          </button>
        </header>
        <div className="onboarding__content" ref={contentRef}>
          {step === 0 && <Welcome />}
          {step === 1 && <HardwareScan />}
          {step === 2 && (
            <ExecutionChoice
              value={preferences.execution}
              onChange={(execution) =>
                updatePreferences({
                  execution,
                  localOnly: execution === "local",
                })
              }
            />
          )}
          {step === 3 && (
            <GameChoice selected={selectedGame} onChange={setSelectedGame} />
          )}
          {step === 4 && <ProviderChoice />}
          {step === 5 && (
            <MicrophoneRehearsal
              rehearsed={rehearsed}
              onChange={setRehearsed}
              ptt={preferences.ptt}
              setPtt={(ptt) => updatePreferences({ ptt })}
            />
          )}
          {step === 6 && (
            <PresenceChoice
              preferences={preferences}
              update={updatePreferences}
            />
          )}
          {step === 7 && (
            <PerformanceChoice
              value={preferences.performance}
              onChange={(performance) => updatePreferences({ performance })}
            />
          )}
          {step === 8 && <Simulation state={simulation} />}
          {step === 9 && (
            <Ready preferences={preferences} onSimulation={() => setStep(8)} />
          )}
        </div>
        <footer className="onboarding__footer">
          <Button
            className="action-button action-button--quiet"
            onPress={() => setStep((value) => Math.max(0, value - 1))}
            isDisabled={step === 0}
          >
            Back
          </Button>
          <div className="onboarding__progress-column">
            {hasMoreContent && (
              <div className="onboarding__scroll-hint" aria-hidden="true">
                <span>Scroll for the remaining options</span>
                <i />
              </div>
            )}
            <div
              className="onboarding__progress"
              aria-label={`Step ${step + 1} of ${ONBOARDING_STEPS.length}`}
            >
              <span
                style={{
                  width: `${((step + 1) / ONBOARDING_STEPS.length) * 100}%`,
                }}
              />
            </div>
          </div>
          <ActionButton
            onPress={next}
            icon={
              step === 9
                ? "check"
                : step === 8 && simulation === "idle"
                  ? "play"
                  : "arrow"
            }
          >
            {step === 9
              ? "Return to console"
              : step === 8 && simulation === "idle"
                ? "Run private test"
                : step === 8 && simulation === "running"
                  ? "Testing…"
                  : step === 8 && simulation === "done"
                    ? "Review result"
                    : "Continue"}
          </ActionButton>
        </footer>
      </main>
    </div>
  );
}

function Welcome() {
  return (
    <div className="welcome-panel">
      <div className="welcome-panel__signal" aria-hidden="true">
        <div className="signal-orbit signal-orbit--one" />
        <div className="signal-orbit signal-orbit--two" />
        <div className="signal-center">
          <span>N</span>
          <i />
        </div>
        <div className="signal-wave">
          <i />
          <i />
          <i />
          <i />
          <i />
          <i />
          <i />
        </div>
      </div>
      <div className="welcome-panel__copy">
        <div className="eyebrow">A CONVERSATION LAYER FOR YOUR GAMES</div>
        <h1 id="onboarding-title">
          Let the world
          <br />
          <em>answer back.</em>
        </h1>
        <p>
          NPC 2.0 gives single-player characters a voice, memory, and sense of
          the moment—while you stay in control of where every model runs.
        </p>
        <ul className="welcome-points">
          <li>
            <Icon name="mic" size={17} />
            <span>
              <strong>Talk naturally</strong> with push-to-talk or optional
              voice activity
            </span>
          </li>
          <li>
            <Icon name="shield" size={17} />
            <span>
              <strong>Choose your boundary:</strong> cloud, hybrid, or fully
              local
            </span>
          </li>
          <li>
            <Icon name="performance" size={17} />
            <span>
              <strong>Protect frame rate</strong> with measured performance
              limits
            </span>
          </li>
        </ul>
        <Disclosure title="Built for single-player games">
          NPC 2.0 refuses protected online modes and never attempts anti-cheat
          bypasses.
        </Disclosure>
      </div>
    </div>
  );
}

function HardwareScan() {
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">PRIVATE SYSTEM READ</span>
        <h1 id="onboarding-title">This PC is ready for hybrid play.</h1>
        <p>
          The scan stays on this device. It helps us recommend models that fit
          without crowding your game.
        </p>
      </div>
      <div className="hardware-layout">
        <div className="hardware-radar">
          <div className="hardware-radar__rings" />
          <div className="hardware-radar__core">
            <span>12</span>
            <small>GB VRAM</small>
          </div>
          <span className="radar-label radar-label--gpu">RTX 4080 Laptop</span>
          <span className="radar-label radar-label--cpu">i9-13980HX</span>
          <span className="radar-label radar-label--ram">16 GB RAM</span>
        </div>
        <div className="scan-results">
          <div className="scan-row">
            <Icon name="check" />
            <div>
              <strong>Local voice pipeline</strong>
              <span>
                Speech recognition and voice synthesis can stay on CPU.
              </span>
            </div>
            <StatusPill tone="ok">Great fit</StatusPill>
          </div>
          <div className="scan-row">
            <Icon name="check" />
            <div>
              <strong>Compact local intelligence</strong>
              <span>A 3–4B model fits beside most games on this GPU.</span>
            </div>
            <StatusPill tone="ok">Fits</StatusPill>
          </div>
          <div className="scan-row">
            <Icon name="warning" />
            <div>
              <strong>Visual mouth motion</strong>
              <span>
                Available as an experiment; it may yield under heavy GPU load.
              </span>
            </div>
            <StatusPill tone="warn">Optional</StatusPill>
          </div>
        </div>
      </div>
      <Disclosure title="What was checked">
        Windows version, processor, memory, graphics adapter, available VRAM,
        audio devices, and local storage. No serial numbers or file contents
        were read.
      </Disclosure>
    </div>
  );
}

function ExecutionChoice({
  value,
  onChange,
}: {
  value: ExecutionMode;
  onChange: (value: ExecutionMode) => void;
}) {
  const options: Array<{
    id: ExecutionMode;
    title: string;
    tag: string;
    description: string;
    latency: string;
    privacy: string;
    needs: string;
    recommended?: boolean;
    disabled?: boolean;
  }> = [
    {
      id: "cloud",
      title: "API first",
      tag: "DEFAULT · NO MODEL DOWNLOAD",
      description: "Speech, thinking, and voice use providers you connect.",
      latency: "Usually fastest",
      privacy: "Audio and text leave this PC",
      needs: "Internet + provider keys",
      recommended: true,
    },
    {
      id: "hybrid",
      title: "Hybrid",
      tag: "ADVANCED · UNAVAILABLE",
      description:
        "Local speech alternatives are not included in this API-first preview.",
      latency: "Unavailable in preview",
      privacy: "Requires future review",
      needs: "No selectable local pack",
      disabled: true,
    },
    {
      id: "local",
      title: "Fully local",
      tag: "ADVANCED · UNAVAILABLE",
      description:
        "A local conversation pipeline is not shipped in this preview.",
      latency: "Unavailable in preview",
      privacy: "Requires future review",
      needs: "No selectable local pack",
      disabled: true,
    },
  ];
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">EXECUTION BOUNDARY</span>
        <h1 id="onboarding-title">Where should conversations happen?</h1>
        <p>
          This choice sets a default, not a trap. You can override it for any
          game or character later.
        </p>
      </div>
      <div className="choice-grid choice-grid--three">
        {options.map((option) => (
          <Button
            key={option.id}
            className={`choice-card ${value === option.id ? "is-selected" : ""}`}
            onPress={() => onChange(option.id)}
            isDisabled={option.disabled}
            aria-pressed={value === option.id}
          >
            <div className="choice-card__top">
              <span className={`mode-glyph mode-glyph--${option.id}`}>
                <i />
                <i />
                <i />
              </span>
              {option.recommended && (
                <StatusPill tone="teal">Recommended</StatusPill>
              )}
            </div>
            <span className="choice-card__tag">{option.tag}</span>
            <h2>{option.title}</h2>
            <p>{option.description}</p>
            <dl>
              <div>
                <dt>Response</dt>
                <dd>{option.latency}</dd>
              </div>
              <div>
                <dt>Privacy</dt>
                <dd>{option.privacy}</dd>
              </div>
              <div>
                <dt>Needs</dt>
                <dd>{option.needs}</dd>
              </div>
            </dl>
            <span className="choice-card__select">
              {value === option.id ? (
                <>
                  <Icon name="check" size={15} /> Selected
                </>
              ) : option.disabled ? (
                "Unavailable in this preview"
              ) : (
                "Choose mode"
              )}
            </span>
          </Button>
        ))}
      </div>
      <Disclosure title="Cloud use is never automatic">
        NPC 2.0 will not switch from local to cloud, or between providers,
        unless you pre-authorize the exact route.
      </Disclosure>
    </div>
  );
}

function GameChoice({
  selected,
  onChange,
}: {
  selected: string;
  onChange: (id: string) => void;
}) {
  const games = GAME_PROFILES.slice(0, 6);
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">FIRST PROFILE</span>
        <h1 id="onboarding-title">Choose a world to connect.</h1>
        <p>
          This preview shows six authored profile examples. Runtime detection
          has not run, so none represents a detected installation.
        </p>
      </div>
      <div className="detected-banner">
        <Icon name="spark" />
        <div>
          <strong>
            6 authored profile examples · runtime detection pending
          </strong>
          <span>No detected installations in this fixture.</span>
        </div>
        <button disabled>Detection pending</button>
      </div>
      <div className="game-choice-grid">
        {games.map((game) => (
          <Button
            key={game.id}
            className={`game-choice ${selected === game.id ? "is-selected" : ""}`}
            onPress={() => onChange(game.id)}
          >
            <span
              className="game-monogram"
              style={{ "--game-accent": game.accent } as React.CSSProperties}
            >
              {game.abbreviation}
            </span>
            <span className="game-choice__copy">
              <strong>{game.title}</strong>
              <small>
                {game.store} · {game.capability}
              </small>
            </span>
            <StatusPill tone="neutral">Authored · unverified</StatusPill>
          </Button>
        ))}
      </div>
      <button className="generic-choice">
        <Icon name="games" />
        <span>
          <strong>Try another single-player game</strong>
          <small>
            Experimental generic mode · manual character name, audio, subtitles,
            and memory
          </small>
        </span>
        <Icon name="arrow" />
      </button>
    </div>
  );
}

function ProviderChoice() {
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">VOICE & MIND</span>
        <h1 id="onboarding-title">
          Choose API providers after runtime verification.
        </h1>
        <p>
          The baseline is API-first and requires no model download. This setup
          preview does not select or contact a hosted provider; every route
          remains explicit and user-controlled.
        </p>
      </div>
      <div className="pipeline-stack">
        <PipelineRow
          number="01"
          title="Hear you"
          selected="Speech recognition API"
          meta="Explicit selection required"
          detail="Choose a hosted speech provider later; no local model is required."
        />
        <PipelineRow
          number="02"
          title="Form a reply"
          selected="Hosted reply API"
          meta="Explicit selection required"
          detail="Connect one provider contract; no local language model is required."
        />
        <PipelineRow
          number="03"
          title="Speak back"
          selected="Voice synthesis API"
          meta="Explicit selection required"
          detail="Choose a hosted voice route; no local speech pack is required."
        />
      </div>
      <div className="provider-continuation" role="note">
        <span>Provider guide</span>
        <strong>Review account, privacy, and model notes below.</strong>
        <small>Continue scrolling before you finish this step.</small>
      </div>
      <Disclosure title="Recommended for trying 2.0 · NVIDIA NIM · one account/key">
        Select NVIDIA NIM explicitly in Settings after setup. Free hosted
        endpoints are for individual prototyping, development, and testing with
        model-specific limits—not production or commercial entitlement. LLM chat
        is pending backend landing; embeddings and rerank are being implemented.
        Stock Magpie voice discovery is available, with provider-neutral
        character qualities mapped only to runtime-returned stock voices—never
        cloned game performers. ASR remains pending gRPC qualification. Do not
        send confidential, sensitive, or personal data; service-specific
        retention and security-abuse logging may apply. The key is entered only
        through a native Windows prompt.
      </Disclosure>
      <Disclosure title="Local conversation models are advanced and unavailable">
        Local LLM, speech recognition, and voice alternatives are not selectable
        in this preview and are not required for the API-first baseline. The
        Models page is only for optional generic screen-space lip-sync packs.
      </Disclosure>
    </div>
  );
}

function PipelineRow({
  number,
  title,
  selected,
  meta,
  detail,
}: {
  number: string;
  title: string;
  selected: string;
  meta: string;
  detail: string;
}) {
  return (
    <div className="pipeline-row">
      <span className="pipeline-row__number">{number}</span>
      <div className="pipeline-row__role">
        <strong>{title}</strong>
        <small>{detail}</small>
      </div>
      <div className="pipeline-row__choice">
        <strong>{selected}</strong>
        <small>{meta}</small>
      </div>
      <button aria-label={`Change ${title}`}>
        <Icon name="chevron" />
      </button>
    </div>
  );
}

function MicrophoneRehearsal({
  rehearsed,
  onChange,
  ptt,
  setPtt,
}: {
  rehearsed: boolean;
  onChange: (value: boolean) => void;
  ptt: boolean;
  setPtt: (value: boolean) => void;
}) {
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">PUSH TO TALK</span>
        <h1 id="onboarding-title">Set a key. Say one line.</h1>
        <p>
          Push-to-talk is the safest default: nothing is listened to until you
          hold the key.
        </p>
      </div>
      <div className="rehearsal">
        <div className={`rehearsal__orb ${rehearsed ? "is-done" : ""}`}>
          <div className="orb-rings" />
          <Icon name={rehearsed ? "check" : "mic"} size={34} />
        </div>
        <div>
          <span className="rehearsal__label">
            {rehearsed ? "HEARD CLEARLY" : "HOLD TO REHEARSE"}
          </span>
          <h2>
            {rehearsed ? (
              "“Can you hear me?”"
            ) : (
              <>
                <KeyboardKey>V</KeyboardKey> then say “Can you hear me?”
              </>
            )}
          </h2>
          <p>
            {rehearsed
              ? "Microphone level, noise floor, and endpoint timing look good."
              : "The rehearsal is processed locally and is not saved."}
          </p>
          <ActionButton
            icon={rehearsed ? "refresh" : "mic"}
            variant={rehearsed ? "outline" : "primary"}
            onPress={() => onChange(!rehearsed)}
          >
            {rehearsed ? "Try again" : "Simulate key hold"}
          </ActionButton>
        </div>
      </div>
      <div className="device-strip">
        <div>
          <span>Input</span>
          <strong>Microphone Array (Realtek Audio)</strong>
        </div>
        <div>
          <span>Level</span>
          <div className="level-meter">
            <i />
            <i />
            <i />
            <i />
            <i />
            <i />
            <i />
            <i />
          </div>
        </div>
        <button>Change</button>
      </div>
      <div className="rehearsal-toggle">
        <Toggle
          label="Use push-to-talk"
          description="Hold one key while speaking. Releasing it immediately ends your turn."
          isSelected={ptt}
          onChange={setPtt}
          privacy="Recommended privacy boundary"
        />
      </div>
    </div>
  );
}

function PresenceChoice({
  preferences,
  update,
}: {
  preferences: AppPreferences;
  update: (patch: Partial<AppPreferences>) => void;
}) {
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">OPTIONAL PRESENCE</span>
        <h1 id="onboarding-title">Decide what the character can sense.</h1>
        <p>
          Conversation works without visual access. Turn on only the cues that
          improve your experience.
        </p>
      </div>
      <div className="presence-options">
        <Toggle
          label="See the game window"
          description="Reads selected frames to identify the speaker and on-screen context. Frames are discarded after analysis."
          isSelected={preferences.screenPresence}
          onChange={(screenPresence) => update({ screenPresence })}
          privacy="Local by default · never captures other windows"
        />
        <Toggle
          label="Animate visible speech"
          description="Adds experimental mouth motion when a face is confidently tracked. Falls back instantly when uncertain."
          isSelected={preferences.screenPresence}
          onChange={(screenPresence) => update({ screenPresence })}
          privacy="Experimental · may use up to 5.2 GB VRAM"
          disabled={!preferences.screenPresence}
        />
        <Toggle
          label="Use my camera for presence"
          description="Lets characters respond to opt-in pose and expression cues. No demographic inference, recognition, or recording."
          isSelected={false}
          onChange={() => undefined}
          privacy="Off by default · local only"
        />
      </div>
      <Disclosure title="Audio-only is a complete experience">
        If a character is obscured or offscreen, select their name and continue
        with voice and subtitles. Facial animation never blocks a conversation.
      </Disclosure>
    </div>
  );
}

function PerformanceChoice({
  value,
  onChange,
}: {
  value: PerformanceMode;
  onChange: (value: PerformanceMode) => void;
}) {
  const modes: Array<{
    id: PerformanceMode;
    title: string;
    fps: string;
    latency: string;
    visual: string;
  }> = [
    {
      id: "competitive",
      title: "Competitive",
      fps: "≤ 2% avg",
      latency: "Fastest",
      visual: "Audio only",
    },
    {
      id: "fast",
      title: "Fast",
      fps: "≤ 3% avg",
      latency: "Very fast",
      visual: "Light context",
    },
    {
      id: "balanced",
      title: "Balanced",
      fps: "≤ 5% avg",
      latency: "Fast",
      visual: "Adaptive",
    },
    {
      id: "immersive",
      title: "Immersive",
      fps: "≤ 10% avg",
      latency: "Moderate",
      visual: "Full presence",
    },
    {
      id: "maximum",
      title: "Maximum quality",
      fps: "≤ 15% avg",
      latency: "Quality first",
      visual: "All features",
    },
    {
      id: "custom",
      title: "Custom",
      fps: "You decide",
      latency: "You decide",
      visual: "Per feature",
    },
  ];
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">PERFORMANCE TARGET</span>
        <h1 id="onboarding-title">Give your game first claim on the PC.</h1>
        <p>
          NPC 2.0 measures live headroom and yields optional work before it
          harms your target.
        </p>
      </div>
      <div className="performance-choices">
        <div className="performance-choices__header" aria-hidden="true">
          <span />
          <span>Mode</span>
          <span>Game impact</span>
          <span>Response</span>
          <span>Presence</span>
        </div>
        {modes.map((mode) => (
          <Button
            key={mode.id}
            className={`performance-choice ${value === mode.id ? "is-selected" : ""}`}
            onPress={() => onChange(mode.id)}
          >
            <span className="performance-choice__radio" />
            <strong>{mode.title}</strong>
            <span>{mode.fps}</span>
            <span>{mode.latency}</span>
            <small>{mode.visual}</small>
          </Button>
        ))}
      </div>
      <div className="budget-preview">
        <div>
          <span>Recommended VRAM ceiling</span>
          <strong>
            {value === "competitive"
              ? "3.5"
              : value === "maximum"
                ? "9.5"
                : "7.5"}{" "}
            <small>/ 12 GB</small>
          </strong>
        </div>
        <MiniBar
          value={value === "competitive" ? 29 : value === "maximum" ? 79 : 63}
        />
        <p>Optional visual work pauses before the conversation or game does.</p>
      </div>
    </div>
  );
}

function Simulation({ state }: { state: "idle" | "running" | "done" }) {
  return (
    <div className="setup-sheet">
      <div className="setup-intro">
        <span className="step-kicker">PRIVATE TEST RUN</span>
        <h1 id="onboarding-title">A conversation, without launching a game.</h1>
        <p>
          Eclipse Harbor is a synthetic scene. It checks timing, memory, speech,
          and fallback behavior without any copyrighted game content.
        </p>
      </div>
      <div className={`simulation-card simulation-card--${state}`}>
        <div className="simulation-card__scene">
          <div className="harbor-sky">
            <i />
            <i />
            <i />
          </div>
          <div className="harbor-water" />
          <div className="harbor-character">
            <span>MV</span>
          </div>
          <div className="simulation-caption">
            {state === "idle"
              ? "Ready when you are"
              : state === "running"
                ? "Mara is forming a reply…"
                : "“The eastern lock is clear. I’ll keep the lantern on for you.”"}
          </div>
        </div>
        <div className="simulation-card__telemetry">
          <div>
            <span>Listening</span>
            <i className={state !== "idle" ? "is-lit" : ""} />
          </div>
          <div>
            <span>Transcript</span>
            <i
              className={
                state === "running" || state === "done" ? "is-lit" : ""
              }
            />
          </div>
          <div>
            <span>Memory</span>
            <i
              className={
                state === "running" || state === "done" ? "is-lit" : ""
              }
            />
          </div>
          <div>
            <span>Response</span>
            <i className={state === "done" ? "is-lit" : ""} />
          </div>
          <div>
            <span>Voice</span>
            <i className={state === "done" ? "is-lit" : ""} />
          </div>
        </div>
      </div>
      {state === "done" && (
        <div className="simulation-result">
          <Icon name="check" />
          <div>
            <strong>Round trip complete in 1.31 seconds</strong>
            <span>
              Voice started before the full reply finished. No cloud route or
              visual feature was needed.
            </span>
          </div>
          <StatusPill tone="ok">All checks passed</StatusPill>
        </div>
      )}
    </div>
  );
}

function Ready({
  preferences,
  onSimulation,
}: {
  preferences: AppPreferences;
  onSimulation: () => void;
}) {
  return (
    <div className="ready-panel">
      <div className="ready-panel__check">
        <Icon name="check" size={42} />
        <i />
        <i />
      </div>
      <span className="step-kicker">SETUP PREVIEW COMPLETE</span>
      <h1 id="onboarding-title">
        Preferences saved. Runtime verification remains.
      </h1>
      <p>
        No game, model pack, provider connection, capture path, or performance
        target has been live-verified. Connect the runtime or revisit the
        private simulation before attempting a real session.
      </p>
      <div className="ready-panel__actions">
        <div className="ready-panel__action-path ready-panel__action-path--blocked">
          <span>Required before a real session</span>
          <ActionButton icon="diagnostics" variant="quiet" isDisabled>
            Connect and verify runtime
          </ActionButton>
          <small>Unavailable in this preview</small>
        </div>
        <div className="ready-panel__action-path">
          <span>Optional fixture replay</span>
          <ActionButton icon="play" variant="outline" onPress={onSimulation}>
            Run private simulation
          </ActionButton>
          <small>No live connection required</small>
        </div>
      </div>
      <div className="ready-summary">
        <Metric
          label="Run mode"
          value={
            preferences.execution === "local"
              ? "Fully local"
              : preferences.execution[0].toUpperCase() +
                preferences.execution.slice(1)
          }
          detail="Preference only"
        />
        <Metric
          label="Performance"
          value={
            preferences.performance[0].toUpperCase() +
            preferences.performance.slice(1)
          }
          detail="Not benchmarked"
        />
        <Metric label="Talk key" value="V" detail="Fixture preference" />
        <Metric
          label="Privacy"
          value={preferences.localOnly ? "No network" : "Visible routes"}
          detail="Not connection evidence"
        />
      </div>
      <Disclosure title="Runtime verification is required">
        The authored policy is single-player only, but this preview does not
        prove that external capture, manual target selection, or the online-mode
        detector is running.
      </Disclosure>
    </div>
  );
}
