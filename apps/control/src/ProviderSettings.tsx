import { useEffect, useMemo, useState } from "react";
import { Button } from "react-aria-components";
import {
  ActionButton,
  Disclosure,
  SectionTitle,
  StatusPill,
} from "./components";
import { Icon } from "./icons";
import { ProviderLoadoutEditor } from "./ProviderLoadoutEditor";
import {
  deleteProviderCredential,
  listProviderCredentialStatus,
  promptAndSaveProviderCredential,
  testProviderCredential,
  type CredentialStatus,
  type ProviderCredentialSummary,
} from "./providerCredentials";

interface ProviderDefinition {
  id: string;
  name: string;
  execution: "Hosted" | "Local";
  modalities: string[];
  data: string;
  guidance: string;
  guidanceUrl?: string;
}

const PROVIDERS: ProviderDefinition[] = [
  {
    id: "openai",
    name: "OpenAI",
    execution: "Hosted",
    modalities: ["Reply", "Speech in"],
    data: "Conversation text; audio only when cloud transcription is selected",
    guidance:
      "API usage is billed separately. Check current pricing and account credits.",
    guidanceUrl:
      "https://platform.openai.com/settings/organization/billing/overview",
  },
  {
    id: "gemini",
    name: "Google Gemini",
    execution: "Hosted",
    modalities: ["Reply"],
    data: "Conversation text and selected context",
    guidance:
      "Google AI Studio may offer a limited free tier subject to current regional limits.",
    guidanceUrl: "https://aistudio.google.com/app/apikey",
  },
  {
    id: "anthropic",
    name: "Anthropic",
    execution: "Hosted",
    modalities: ["Reply"],
    data: "Conversation text and selected context",
    guidance:
      "API access requires an Anthropic Console account and may require prepaid credits.",
    guidanceUrl: "https://console.anthropic.com/settings/keys",
  },
  {
    id: "cohere",
    name: "Cohere",
    execution: "Hosted",
    modalities: ["Reply"],
    data: "Conversation text and selected context",
    guidance:
      "Evaluation keys are limited to experimentation and are not production entitlements.",
    guidanceUrl: "https://dashboard.cohere.com/api-keys",
  },
  {
    id: "nvidia-nim",
    name: "NVIDIA NIM",
    execution: "Hosted",
    modalities: [
      "LLM chat · backend landing",
      "Embeddings/rerank · implementing",
      "Stock voice discovery available",
    ],
    data: "Do not send confidential, sensitive, or personal data. Service-specific retention and security-abuse logging may apply.",
    guidance:
      "One NVIDIA Developer API key covers eligible NIM account routes. Stock Magpie voice discovery is available: character bindings choose provider-neutral qualities and map them to a runtime-returned stock voice, never a cloned or implied game performer. ASR remains pending gRPC endpoint qualification.",
    guidanceUrl: "https://build.nvidia.com/",
  },
  {
    id: "groq",
    name: "Groq",
    execution: "Hosted",
    modalities: ["Reply"],
    data: "Conversation text and selected context",
    guidance:
      "Developer access and limits change; review the console before choosing it as a default.",
    guidanceUrl: "https://console.groq.com/keys",
  },
  {
    id: "openai-compatible",
    name: "OpenAI-compatible endpoint",
    execution: "Hosted",
    modalities: ["Reply"],
    data: "Depends on the endpoint you configure",
    guidance:
      "Advanced: confirm the endpoint operator, TLS, privacy policy, and costs yourself.",
  },
  {
    id: "deepgram",
    name: "Deepgram",
    execution: "Hosted",
    modalities: ["Speech in", "Voice out"],
    data: "Microphone audio or reply text, depending on selected role",
    guidance:
      "New accounts may offer trial credit; availability and limits can change.",
    guidanceUrl: "https://console.deepgram.com/",
  },
  {
    id: "assemblyai",
    name: "AssemblyAI",
    execution: "Hosted",
    modalities: ["Speech in"],
    data: "Microphone audio and resulting transcript",
    guidance:
      "Review current trial credit and streaming availability in your dashboard.",
    guidanceUrl: "https://www.assemblyai.com/dashboard/signup",
  },
  {
    id: "elevenlabs",
    name: "ElevenLabs",
    execution: "Hosted",
    modalities: ["Speech in", "Voice out"],
    data: "Microphone audio or reply text, depending on selected role",
    guidance:
      "A limited free plan may be available. Do not clone actors or game performers.",
    guidanceUrl: "https://elevenlabs.io/app/settings/api-keys",
  },
  {
    id: "cartesia",
    name: "Cartesia",
    execution: "Hosted",
    modalities: ["Voice out"],
    data: "Reply text sent for speech generation",
    guidance:
      "Check current trial and usage limits before enabling automatic routes.",
    guidanceUrl: "https://play.cartesia.ai/keys",
  },
  {
    id: "inworld",
    name: "Inworld",
    execution: "Hosted",
    modalities: ["Voice out"],
    data: "Reply text sent for speech generation",
    guidance: "Check current account eligibility, credits, and usage terms.",
    guidanceUrl: "https://studio.inworld.ai/",
  },
  {
    id: "moonshine-local",
    name: "Moonshine local",
    execution: "Local",
    modalities: ["Speech in"],
    data: "Stays on this PC",
    guidance: "No account or API key. Requires a qualified local model pack.",
  },
  {
    id: "nemotron-local",
    name: "Nemotron Speech local",
    execution: "Local",
    modalities: ["Speech in"],
    data: "Stays on this PC",
    guidance:
      "No API key. Windows qualification and license review are required.",
  },
  {
    id: "whispercpp-local",
    name: "whisper.cpp local",
    execution: "Local",
    modalities: ["Speech in"],
    data: "Stays on this PC",
    guidance: "No API key. Import a compatible, reviewed GGML model pack.",
  },
  {
    id: "kokoro-local",
    name: "Kokoro ONNX local",
    execution: "Local",
    modalities: ["Voice out"],
    data: "Stays on this PC",
    guidance: "No API key. The pack remains qualification-gated.",
  },
  {
    id: "llamacpp-local",
    name: "llama.cpp local",
    execution: "Local",
    modalities: ["Reply"],
    data: "Stays on this PC",
    guidance:
      "No API key. Import a compatible GGUF model with its license manifest.",
  },
  {
    id: "onnx-embedding-local",
    name: "ONNX embedding worker",
    execution: "Local",
    modalities: ["Memory"],
    data: "Stays on this PC",
    guidance: "No API key. Embeddings remain local and rebuildable.",
  },
];

const EXPERIMENT_PATHS = [
  {
    name: "Cohere",
    offer: "1,000 calls per month for evaluation only",
    note: "Not a production entitlement.",
    url: "https://cohere.com/pricing",
  },
  {
    name: "NVIDIA NIM",
    offer:
      "Free hosted endpoints for individual prototyping/development/testing",
    note: "Model-specific rate limits; not a production/commercial entitlement.",
    url: "https://build.nvidia.com/",
  },
  {
    name: "ElevenLabs",
    offer: "Recurring free allowance",
    note: "Noncommercial use and attribution requirements apply.",
    url: "https://elevenlabs.io/pricing",
  },
  {
    name: "AssemblyAI",
    offer: "$50 in introductory credits",
    note: "One-time credit; current account terms apply.",
    url: "https://www.assemblyai.com/pricing",
  },
  {
    name: "Gemini",
    offer: "Recurring eligible free tier",
    note: "Free-tier data may be used to improve Google products.",
    url: "https://ai.google.dev/gemini-api/docs/pricing",
  },
  {
    name: "Groq",
    offer: "Recurring rate-limited free access",
    note: "Model-specific limits apply.",
    url: "https://console.groq.com/docs/rate-limits",
  },
  {
    name: "Deepgram",
    offer: "$200 no-expiry introductory credit",
    note: "Available under current signup terms.",
    url: "https://deepgram.com/pricing",
  },
  {
    name: "Cartesia",
    offer: "20,000 recurring Free credits",
    note: "Free plan is noncommercial.",
    url: "https://cartesia.ai/pricing",
  },
  {
    name: "Inworld",
    offer: "Free prototype allowance",
    note: "For prototyping within published limits.",
    url: "https://inworld.ai/pricing",
  },
  {
    name: "OpenAI",
    offer: "One quickstart test",
    note: "No recurring general free API tier.",
    url: "https://platform.openai.com/docs/quickstart",
  },
  {
    name: "Anthropic",
    offer: "Paid API access",
    note: "No general free API tier.",
    url: "https://www.anthropic.com/pricing#anthropic-api",
  },
];

type ActionState = {
  kind: "idle" | "working" | "success" | "error";
  message: string;
};

export interface ProviderPublicState {
  selectedProviderId: string;
  statuses: Record<string, CredentialStatus>;
}

export const serializeProviderPublicState = (state: ProviderPublicState) =>
  JSON.stringify(state);

export function ProviderSettings() {
  const requestedProvider = new URLSearchParams(window.location.search).get(
    "provider",
  );
  const [selectedId, setSelectedId] = useState(
    requestedProvider &&
      PROVIDERS.some((provider) => provider.id === requestedProvider)
      ? requestedProvider
      : "openai",
  );
  const [statuses, setStatuses] = useState<Record<string, CredentialStatus>>(
    {},
  );
  const [loading, setLoading] = useState(true);
  const [action, setAction] = useState<ActionState>({
    kind: "idle",
    message: "",
  });
  const selected =
    PROVIDERS.find((provider) => provider.id === selectedId) ?? PROVIDERS[0];
  const hosted = PROVIDERS.filter(
    (provider) => provider.execution === "Hosted",
  );
  const local = PROVIDERS.filter((provider) => provider.execution === "Local");
  const publicState = useMemo<ProviderPublicState>(
    () => ({ selectedProviderId: selectedId, statuses }),
    [selectedId, statuses],
  );

  useEffect(() => {
    let current = true;
    listProviderCredentialStatus()
      .then((summaries) => {
        if (!current) return;
        setStatuses(
          Object.fromEntries(
            summaries.map((summary: ProviderCredentialSummary) => [
              summary.providerId,
              summary.status,
            ]),
          ),
        );
      })
      .catch(() => {
        if (current)
          setAction({
            kind: "error",
            message:
              "Credential status is unavailable. The vault may not be ready.",
          });
      })
      .finally(() => {
        if (current) setLoading(false);
      });
    return () => {
      current = false;
    };
  }, []);

  useEffect(() => {
    setAction({ kind: "idle", message: "" });
  }, [selectedId]);

  const runCredentialAction = async (kind: "save" | "test" | "delete") => {
    setAction({
      kind: "working",
      message:
        kind === "save"
          ? "Opening the secure Windows credential prompt…"
          : kind === "test"
            ? "Checking saved reference and provider contract…"
            : "Removing credential…",
    });
    try {
      const result =
        kind === "save"
          ? await promptAndSaveProviderCredential(selected.id)
          : kind === "test"
            ? await testProviderCredential(selected.id)
            : await deleteProviderCredential(selected.id);
      setStatuses((current) => ({ ...current, [selected.id]: result.status }));
      setAction({
        kind: "success",
        message:
          kind === "save"
            ? result.outcome === "cancelled"
              ? "Secure Windows prompt cancelled. No credential status changed."
              : "Secure prompt completed. This page received presence status only."
            : kind === "test"
              ? "Setup check passed: saved reference and provider contract are present. No provider request was made."
              : "Credential removed from the Windows vault.",
      });
    } catch (error) {
      setAction({
        kind: "error",
        message:
          error instanceof Error
            ? error.message
            : "The provider action failed.",
      });
    }
  };

  return (
    <div className="providers-settings">
      <SectionTitle
        eyebrow="CLOUD & LOCAL ROUTES"
        title="Providers"
        description="Compose named provider/model routes, then connect only the services you intend to use. Credential values never enter this WebView."
      />
      <ProviderLoadoutEditor />
      <SectionTitle
        eyebrow="CONNECTION VAULT"
        title="Provider accounts"
        description="Loadouts store provider and model IDs—not key values. Connect each hosted service through the native Windows prompt when you are ready to use it."
      />
      <div className="provider-layout">
        <div className="provider-directory">
          <div className="provider-directory__group">
            <span>HOSTED · CREDENTIAL REQUIRED</span>
            {hosted.map((provider) => (
              <ProviderDirectoryRow
                key={provider.id}
                provider={provider}
                status={statuses[provider.id] ?? "missing"}
                selected={selectedId === provider.id}
                onSelect={() => setSelectedId(provider.id)}
              />
            ))}
          </div>
          <div className="provider-directory__group">
            <span>ADVANCED LOCAL · UNAVAILABLE IN THIS PREVIEW</span>
            {local.map((provider) => (
              <ProviderDirectoryRow
                key={provider.id}
                provider={provider}
                status="notRequired"
                selected={selectedId === provider.id}
                onSelect={() => setSelectedId(provider.id)}
              />
            ))}
          </div>
        </div>
        <section className="provider-detail" aria-live="polite">
          <header>
            <div>
              <span
                className={`provider-letter provider-letter--${selected.execution.toLowerCase()}`}
              >
                {selected.name.slice(0, 2).toUpperCase()}
              </span>
              <div>
                <span className="eyebrow">{selected.execution} PROVIDER</span>
                <h2>{selected.name}</h2>
              </div>
            </div>
            <CredentialPill
              status={
                selected.execution === "Local"
                  ? "notRequired"
                  : (statuses[selected.id] ?? "missing")
              }
              loading={loading}
            />
          </header>
          <div className="provider-facts">
            <div>
              <span>CAPABILITIES</span>
              <p>
                {selected.modalities.map((item) => (
                  <b key={item}>{item}</b>
                ))}
              </p>
            </div>
            <div>
              <span>DATA SENT</span>
              <strong>{selected.data}</strong>
            </div>
          </div>
          {selected.execution === "Hosted" ? (
            <>
              <div className="credential-entry credential-entry--native">
                <div className="native-prompt-boundary">
                  <Icon name="shield" />
                  <div>
                    <strong>
                      Credential values stay outside the Response Console
                    </strong>
                    <p>
                      Windows opens a native secure prompt. This WebView sends
                      only the provider ID and receives only presence or
                      cancellation status; it never receives, stores,
                      serializes, or reveals the credential value.
                    </p>
                  </div>
                </div>
              </div>
              <div className="credential-actions">
                <ActionButton
                  icon="shield"
                  onPress={() => runCredentialAction("save")}
                  isDisabled={action.kind === "working"}
                >
                  Open secure Windows key prompt
                </ActionButton>
                <ActionButton
                  variant="outline"
                  icon="refresh"
                  onPress={() => runCredentialAction("test")}
                  isDisabled={
                    statuses[selected.id] !== "present" ||
                    action.kind === "working"
                  }
                >
                  Check setup
                </ActionButton>
                {statuses[selected.id] === "present" && (
                  <ActionButton
                    variant="danger"
                    icon="close"
                    onPress={() => runCredentialAction("delete")}
                    isDisabled={action.kind === "working"}
                  >
                    Delete
                  </ActionButton>
                )}
              </div>
              {action.kind !== "idle" && (
                <div
                  className={`credential-result credential-result--${action.kind}`}
                  role={action.kind === "error" ? "alert" : "status"}
                >
                  <Icon
                    name={
                      action.kind === "error"
                        ? "warning"
                        : action.kind === "working"
                          ? "refresh"
                          : "check"
                    }
                  />
                  <span>{action.message}</span>
                </div>
              )}
              <div className="provider-guidance">
                <Icon name="help" />
                <div>
                  <strong>Plans, trials, and keys</strong>
                  <p>{selected.guidance}</p>
                  {selected.guidanceUrl && (
                    <a
                      href={selected.guidanceUrl}
                      target="_blank"
                      rel="noreferrer"
                    >
                      Open official account page{" "}
                      <Icon name="external" size={13} />
                    </a>
                  )}
                  {selected.id === "nvidia-nim" && (
                    <a
                      href="https://docs.api.nvidia.com/nim/docs/api-quickstart"
                      target="_blank"
                      rel="noreferrer"
                    >
                      Read official NIM API quickstart{" "}
                      <Icon name="external" size={13} />
                    </a>
                  )}
                </div>
              </div>
            </>
          ) : (
            <div className="local-provider-ready">
              <div className="local-provider-ready__signal">
                <Icon name="shield" />
                <i />
              </div>
              <div>
                <span>NO CREDENTIAL NEEDED</span>
                <h3>Advanced local route unavailable in this preview.</h3>
                <p>{selected.guidance}</p>
                <ActionButton variant="outline" icon="models" isDisabled>
                  Local conversation models unavailable
                </ActionButton>
              </div>
            </div>
          )}
          <Disclosure title="No silent provider fallback">
            A failed provider never changes to another cloud service or uploads
            local-only data unless you pre-authorize that exact route.
          </Disclosure>
        </section>
      </div>
      <section className="experiment-paths">
        <div className="experiment-paths__head">
          <div>
            <span className="eyebrow">
              PROVIDER-PUBLISHED EXPERIMENTATION PATHS · CHECKED 2026-08-28
            </span>
            <h3>Try a route without hiding the terms.</h3>
          </div>
          <p>
            Published offers and eligibility change. Use one account normally;
            NPC 2.0 never automates quota rotation or evasion.
          </p>
        </div>
        <div className="experiment-paths__grid">
          {EXPERIMENT_PATHS.map((path) => (
            <a key={path.name} href={path.url} target="_blank" rel="noreferrer">
              <span>{path.name}</span>
              <strong>{path.offer}</strong>
              <small>{path.note}</small>
              <Icon name="external" size={13} />
            </a>
          ))}
        </div>
      </section>
      <div className="sr-only" data-testid="provider-public-state">
        {serializeProviderPublicState(publicState)}
      </div>
    </div>
  );
}

function ProviderDirectoryRow({
  provider,
  status,
  selected,
  onSelect,
}: {
  provider: ProviderDefinition;
  status: CredentialStatus;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <Button
      className={`provider-directory__row ${selected ? "is-selected" : ""}`}
      onPress={onSelect}
    >
      <span
        className={`provider-mini-status provider-mini-status--${status}`}
      />
      <span>
        <strong>{provider.name}</strong>
        <small>
          {provider.id === "nvidia-nim"
            ? "Recommended for trying 2.0 · one account/key"
            : provider.modalities.join(" · ")}
        </small>
      </span>
      {status === "present" && <Icon name="check" size={13} />}{" "}
      {status === "notRequired" && <Icon name="shield" size={13} />}
    </Button>
  );
}

function CredentialPill({
  status,
  loading,
}: {
  status: CredentialStatus;
  loading: boolean;
}) {
  if (loading) return <StatusPill tone="neutral">Checking vault…</StatusPill>;
  if (status === "present")
    return <StatusPill tone="ok">Saved reference reported</StatusPill>;
  if (status === "notRequired")
    return <StatusPill tone="teal">No key needed</StatusPill>;
  if (status === "unavailable")
    return <StatusPill tone="danger">Vault unavailable</StatusPill>;
  return <StatusPill tone="warn">Not connected</StatusPill>;
}
