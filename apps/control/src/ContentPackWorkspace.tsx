import { useEffect, useRef, useState } from "react";
import {
  CONTENT_PACK_MAX_BYTES,
  activateContentPack,
  inspectContentPackJson,
  readContentPackState,
  type ContentPackPreview,
  type ContentPackState,
} from "./contentPacks";

const errorText = (error: unknown, fallback: string) =>
  error instanceof Error ? error.message : fallback;

const roleLabel = (role: string) =>
  ({
    llm: "Reply model",
    stt: "Speech recognition",
    tts: "Voice",
    embeddings: "Memory search",
  })[role] ?? role;

const readFileText = (file: File) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () =>
      typeof reader.result === "string"
        ? resolve(reader.result)
        : reject(new Error("The selected file was not readable text.")),
    );
    reader.addEventListener("error", () =>
      reject(reader.error ?? new Error("The selected file could not be read.")),
    );
    reader.readAsText(file);
  });

export function ContentPackWorkspace({
  nativeAvailable,
  gameProfileId,
  onApplied,
  onOpenProviders,
}: {
  nativeAvailable: boolean;
  gameProfileId: string;
  onApplied: () => void;
  onOpenProviders: () => void;
}) {
  const [packText, setPackText] = useState<string | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [preview, setPreview] = useState<ContentPackPreview | null>(null);
  const [state, setState] = useState<ContentPackState | null>(null);
  const [busy, setBusy] = useState<"preview" | "apply" | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let active = true;
    if (!nativeAvailable) return;
    void readContentPackState()
      .then((value) => {
        if (active) setState(value);
      })
      .catch((error) => {
        if (active)
          setNotice(
            errorText(error, "Active content packs could not be read."),
          );
      });
    return () => {
      active = false;
    };
  }, [nativeAvailable]);

  const chooseFile = async (file?: File) => {
    setPreview(null);
    setNotice(null);
    setPackText(null);
    setFileName(file?.name ?? null);
    if (!file) return;
    if (!file.name.toLocaleLowerCase().endsWith(".json")) {
      setNotice(
        "Choose a .json content-pack file. Archives and executable files are not accepted.",
      );
      return;
    }
    if (file.size === 0 || file.size > CONTENT_PACK_MAX_BYTES) {
      setNotice("The content-pack file must be between 1 byte and 2 MiB.");
      return;
    }
    try {
      setPackText(await readFileText(file));
    } catch (error) {
      setNotice(errorText(error, "The selected file could not be read."));
    }
  };

  const inspect = async () => {
    if (!packText) return;
    setBusy("preview");
    setNotice(null);
    try {
      const value = await inspectContentPackJson(packText);
      setPreview(value);
      setNotice(
        value.gameProfileId === gameProfileId
          ? "Pack validated. Review its content, rights, and provider suggestions before applying it."
          : `Pack validated for ${value.displayName}. Select that game to use its content after activation.`,
      );
    } catch (error) {
      setPreview(null);
      setNotice(errorText(error, "The content pack did not pass validation."));
    } finally {
      setBusy(null);
    }
  };

  const apply = async () => {
    if (!packText || !preview) return;
    setBusy("apply");
    setNotice(null);
    try {
      const active = await activateContentPack(packText, preview.contentSha256);
      const nextState = await readContentPackState();
      setState(nextState);
      setNotice(
        `${active.title} is active for ${active.gameProfileId}. Saved character, memory, voice, and model choices were preserved.`,
      );
      onApplied();
    } catch (error) {
      setNotice(errorText(error, "The content pack could not be activated."));
    } finally {
      setBusy(null);
    }
  };

  const activeForGame = state?.active.find(
    (pack) => pack.gameProfileId === gameProfileId,
  );

  return (
    <section
      className="instrument-panel content-pack-workspace"
      aria-labelledby="content-pack-title"
    >
      <header className="panel-title workspace-heading">
        <div>
          <span className="eyebrow">Optional authored content</span>
          <h2 id="content-pack-title">Game & character packs</h2>
        </div>
        <span className={activeForGame ? "badge good" : "badge"}>
          {activeForGame ? "Pack active" : "Bundled profile"}
        </span>
      </header>

      {activeForGame && (
        <div className="content-pack-active" aria-label="Active content pack">
          <span className="content-pack-spine" aria-hidden="true" />
          <div>
            <small>ACTIVE FOR {gameProfileId.toLocaleUpperCase()}</small>
            <b>{activeForGame.title}</b>
            <span>
              {activeForGame.characterCount} characters ·{" "}
              {activeForGame.knowledgeCount} lore records · v
              {activeForGame.version}
            </span>
          </div>
          <code>{activeForGame.contentSha256.slice(0, 12)}</code>
        </div>
      )}

      <div className="content-pack-import">
        <div>
          <h3>Review a local data pack</h3>
          <p>
            Import one JSON file with game lore, character backstories, dialogue
            guidance, and provider suggestions.
          </p>
        </div>
        <div className="content-pack-file-row">
          <input
            ref={inputRef}
            type="file"
            accept=".json,application/json"
            disabled={!nativeAvailable || busy !== null}
            aria-label="Choose content-pack JSON"
            onChange={(event) => void chooseFile(event.target.files?.[0])}
          />
          <button
            className="secondary-action"
            disabled={!nativeAvailable || !packText || busy !== null}
            onClick={() => void inspect()}
          >
            {busy === "preview" ? "Checking…" : "Preview pack"}
          </button>
        </div>
        <small className="control-reason">
          {nativeAvailable
            ? `${fileName ?? "No file selected"} · JSON only · 2 MiB maximum · no scripts, executables, archives, credentials, or undeclared files`
            : "Open the installed Windows app to review and activate local content packs."}
        </small>
      </div>

      {preview && (
        <article className="content-pack-preview">
          <div className="content-pack-preview__heading">
            <div>
              <span className="eyebrow">
                {preview.namespace}/{preview.packId} · v{preview.version}
              </span>
              <h3>{preview.title}</h3>
              <p>{preview.summary}</p>
            </div>
            <span className="badge good">Validated locally</span>
          </div>
          <dl className="facts content-pack-facts">
            <div>
              <dt>Game</dt>
              <dd>{preview.displayName}</dd>
            </div>
            <div>
              <dt>Authored data</dt>
              <dd>
                {preview.characterCount} characters · {preview.knowledgeCount}{" "}
                lore records
              </dd>
            </div>
            <div>
              <dt>Rights</dt>
              <dd>
                {preview.rights.license_name} ·{" "}
                {preview.rights.redistributable
                  ? "redistributable"
                  : "local use only"}
              </dd>
            </div>
            <div>
              <dt>Provider changes</dt>
              <dd>None during import or activation</dd>
            </div>
          </dl>
          <p className="source-disclosure">{preview.applyDetail}</p>

          {preview.providerRecommendations.length > 0 && (
            <div className="content-pack-recommendations">
              <div className="content-pack-recommendations__heading">
                <div>
                  <h4>Author suggestions</h4>
                  <p>
                    Suggestions stay below every saved player choice. Review and
                    adopt them intentionally in Voice & models.
                  </p>
                </div>
                <button className="quiet-button" onClick={onOpenProviders}>
                  Review in Voice & models
                </button>
              </div>
              <ul>
                {preview.providerRecommendations.map((recommendation) => (
                  <li
                    key={`${recommendation.role}:${recommendation.providerId}:${recommendation.modelId}:${recommendation.characterId ?? "game"}`}
                  >
                    <span
                      className={
                        recommendation.catalogAvailable
                          ? "status-dot good"
                          : "status-dot"
                      }
                    />
                    <div>
                      <b>
                        {roleLabel(recommendation.role)} ·{" "}
                        {recommendation.providerId}
                      </b>
                      <span>
                        {recommendation.modelId}
                        {recommendation.voiceId
                          ? ` · ${recommendation.voiceId}`
                          : ""}
                      </span>
                      <small>
                        {recommendation.rationale} {recommendation.detail}
                      </small>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="content-pack-actions">
            <button
              className="primary-action"
              disabled={busy !== null}
              onClick={() => void apply()}
            >
              {busy === "apply" ? "Applying…" : "Use pack content"}
            </button>
            <code title="Reviewed content digest">
              sha256 {preview.contentSha256.slice(0, 16)}…
            </code>
          </div>
        </article>
      )}

      <details className="technical-disclosure">
        <summary>What this pack does not install</summary>
        <p>
          Authored references are biography, lore, aliases, and dialogue
          guidance. This JSON format does not enroll face images, identity
          embeddings, mouth atlases, model binaries, or arbitrary Cyberpunk
          visuals. Those require a separately qualified, rights-bound appearance
          pack.
        </p>
      </details>
      {notice && (
        <p className="inline-status" role="status">
          {notice}
        </p>
      )}
    </section>
  );
}
