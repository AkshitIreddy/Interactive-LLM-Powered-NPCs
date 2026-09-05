import { useState } from "react";
import { HOSTED_PROVIDER_IDS } from "./providerCredentials";
import type { NativeProviderCredentialSummary } from "./tauriBridge";

export type AccountAction = "save" | "validate" | "delete";

const ACCOUNT_NAMES: Record<string, string> = {
  "nvidia-nim": "NVIDIA NIM",
  elevenlabs: "ElevenLabs",
  assemblyai: "AssemblyAI",
  openai: "OpenAI",
  gemini: "Google Gemini",
  anthropic: "Anthropic",
  cohere: "Cohere",
  groq: "Groq",
  mistral: "Mistral AI",
  openrouter: "OpenRouter",
  deepgram: "Deepgram",
  cartesia: "Cartesia",
  inworld: "Inworld",
};

/** Presence comes exclusively from native bootstrap; a browser has no vault. */
export function ProviderAccounts({
  accounts,
  nativeAvailable,
  busy,
  onAction,
  selectedProviderId,
  onProviderChange,
}: {
  accounts: NativeProviderCredentialSummary[];
  nativeAvailable: boolean;
  busy: string | null;
  onAction: (providerId: string, action: AccountAction) => void;
  selectedProviderId?: string;
  onProviderChange?: (id: string) => void;
}) {
  const [localSelectedId, setSelectedId] = useState("nvidia-nim");
  const selectedId = selectedProviderId ?? localSelectedId;
  const [confirmRemoval, setConfirmRemoval] = useState(false);
  const account = accounts.find((item) => item.providerId === selectedId);
  const present = nativeAvailable && account?.status === "present";
  return (
    <section
      className="instrument-panel account-workspace"
      aria-label="Provider accounts"
    >
      <div className="panel-title">
        <div>
          <span className="eyebrow">Connections</span>
          <h2>Provider accounts</h2>
        </div>
        <span className="badge">Windows credential vault</span>
      </div>
      <p className="body-copy">
        Connect the services used by your loadout. Keys are entered in a secure
        Windows prompt.
      </p>
      <label className="account-selector">
        <span>Provider</span>
        <select
          aria-label="Provider account"
          value={selectedId}
          onChange={(event) => {
            setSelectedId(event.target.value);
            onProviderChange?.(event.target.value);
            setConfirmRemoval(false);
          }}
        >
          {HOSTED_PROVIDER_IDS.map((id) => (
            <option key={id} value={id}>
              {ACCOUNT_NAMES[id] ?? id}
            </option>
          ))}
        </select>
      </label>
      <div className="account-status">
        <b>{ACCOUNT_NAMES[selectedId]}</b>
        <span className={`badge ${present ? "good" : "wait"}`}>
          {present
            ? "Connected key"
            : nativeAvailable
              ? "Key needed"
              : "Desktop app required"}
        </span>
      </div>
      <p className="control-reason">
        {nativeAvailable
          ? (account?.detail ?? "No saved key is reported for this provider.")
          : "Open the Windows application to connect an account."}
      </p>
      <div className="provider-actions">
        <button
          className="secondary-action"
          disabled={!nativeAvailable || busy !== null}
          onClick={() => onAction(selectedId, "save")}
        >
          {busy === selectedId
            ? "Working…"
            : present
              ? "Replace key"
              : "Connect account"}
        </button>
        <button
          className="quiet-button"
          disabled={!present || busy !== null}
          onClick={() => onAction(selectedId, "validate")}
        >
          Check saved key
        </button>
        {present && (
          <button
            className="text-action"
            disabled={busy !== null}
            onClick={() => setConfirmRemoval((value) => !value)}
          >
            Remove key
          </button>
        )}
      </div>
      {confirmRemoval && (
        <div
          className="inline-confirmation"
          role="group"
          aria-label="Confirm key removal"
        >
          <p>
            Remove the saved {ACCOUNT_NAMES[selectedId]} key? Loadouts using
            this account will need a new key.
          </p>
          <button
            className="stop-action"
            disabled={busy !== null}
            onClick={() => {
              setConfirmRemoval(false);
              onAction(selectedId, "delete");
            }}
          >
            Remove saved key
          </button>
          <button
            className="quiet-button"
            onClick={() => setConfirmRemoval(false)}
          >
            Keep key
          </button>
        </div>
      )}
      <small className="control-reason">
        Checking the saved key verifies local configuration. Test model requests
        from your loadout; provider charges and limits apply.
      </small>
    </section>
  );
}
