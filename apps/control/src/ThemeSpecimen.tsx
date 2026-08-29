import { Icon } from "./icons";

const variants = [
  {
    id: "settings",
    name: "01 / Settings field",
    primary: "#5FFBFF",
    signal: "#FF4C5F",
    danger: "#B72A3E",
    verdict: "FORM-FIRST",
  },
  {
    id: "cyberware",
    name: "02 / Cyberware field",
    primary: "#5FFBFF",
    signal: "#FF6574",
    danger: "#B72A3E",
    verdict: "SYSTEM-FIRST",
  },
  {
    id: "scanner",
    name: "03 / Scanner field",
    primary: "#5FFBFF",
    signal: "#FF334F",
    danger: "#B72A3E",
    verdict: "TRACE-FIRST",
  },
];

export function ThemeSpecimen() {
  return (
    <main className="theme-specimen">
      <header>
        <span>NPC 2.0 / REFERENCE-LED CALIBRATION</span>
        <h1>
          Response Console
          <br />
          field language.
        </h1>
        <p>
          Three original surfaces derived from the supplied menu, cyberware, and
          scanner structures.
        </p>
      </header>
      <div className="specimen-grid">
        {variants.map((variant) => (
          <article
            key={variant.id}
            className={`specimen-card specimen-card--${variant.id}`}
            style={
              {
                "--spec-primary": variant.primary,
                "--spec-signal": variant.signal,
                "--spec-danger": variant.danger,
              } as React.CSSProperties
            }
          >
            <div className="specimen-card__index">
              <span>{variant.name}</span>
              <b>{variant.verdict}</b>
            </div>
            <div className="specimen-card__trace">
              <i />
              <i />
              <i />
              <i />
              <i />
              <i />
              <i />
            </div>
            <div className="specimen-card__body">
              <span className="specimen-kicker">LIVE / TURN 024</span>
              <h2>
                MARA IS
                <br />
                <em>ANSWERING</em>
              </h2>
              <p>
                The eastern lock is clear. I’ll keep the lantern on for you.
              </p>
              <div className="specimen-readouts">
                <div>
                  <span>FIRST AUDIO</span>
                  <strong>1.31 S</strong>
                </div>
                <div>
                  <span>GAME IMPACT</span>
                  <strong>1.8%</strong>
                </div>
              </div>
              <button>
                <Icon name="pause" /> END SIMULATION
              </button>
            </div>
            <footer>
              <span>LISTEN</span>
              <span>MEMORY</span>
              <span className="is-active">RESPOND</span>
              <span>VOICE</span>
            </footer>
          </article>
        ))}
      </div>
      <aside>
        <span>DECISION RULE</span>
        <p>
          Coral defines structure and inactive controls. Cyan is selected, live,
          or actionable. Burgundy creates depth; yellow is absent from normal
          operation.
        </p>
      </aside>
    </main>
  );
}
