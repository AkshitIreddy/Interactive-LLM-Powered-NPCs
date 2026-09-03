export type LoadoutScope = "global" | "game" | "character";
export type ProviderRole =
  | "llm"
  | "stt"
  | "tts"
  | "embeddings"
  | "vision"
  | "lipSync";

export interface RouteChoice {
  providerId: string;
  modelId: string;
  voiceId?: string;
}

export interface ManualFallback extends RouteChoice {
  authorized: boolean;
}

export interface ProviderLoadout {
  id: string;
  name: string;
  scope: LoadoutScope;
  targetId?: string;
  targetLabel?: string;
  active: boolean;
  routes: Record<ProviderRole, RouteChoice>;
  fallbacks: Partial<Record<ProviderRole, ManualFallback>>;
  revision: number;
}

export interface RouteModelOption {
  id: string;
  name: string;
  selectable?: boolean;
  note?: string;
}

export interface RouteProviderOption {
  id: string;
  name: string;
  execution: "Cloud" | "Local" | "Off";
  privacy: string;
  egress: string;
  cost: string;
  selectable?: boolean;
  note?: string;
  models: RouteModelOption[];
}

export const ROLE_META: Record<
  ProviderRole,
  { label: string; short: string; description: string }
> = {
  llm: {
    label: "Reply model",
    short: "LLM",
    description: "Writes the NPC response from delivered context.",
  },
  stt: {
    label: "Speech recognition",
    short: "STT",
    description: "Turns push-to-talk audio into a transcript.",
  },
  tts: {
    label: "Character voice",
    short: "TTS",
    description: "Streams the selected response as speech.",
  },
  embeddings: {
    label: "Memory embeddings",
    short: "EMB",
    description: "Indexes and finds relevant lore and delivered memories.",
  },
  vision: {
    label: "Optional vision",
    short: "IMG",
    description: "Adds an explicitly captured frame to selected turns.",
  },
  lipSync: {
    label: "Optional lip-sync",
    short: "VIS",
    description: "Edits only the tracked mouth region of external capture.",
  },
};

export const ROLE_ORDER = Object.keys(ROLE_META) as ProviderRole[];

const offLipSync: RouteProviderOption = {
  id: "disabled",
  name: "Off",
  execution: "Off",
  privacy: "No visual model runs",
  egress: "None",
  cost: "None",
  models: [{ id: "disabled", name: "Audio and subtitles only" }],
};

const offVision: RouteProviderOption = {
  id: "disabled",
  name: "Off",
  execution: "Off",
  privacy: "No captured image enters a model route",
  egress: "None",
  cost: "None",
  models: [{ id: "disabled", name: "No visual context" }],
};

export const ROUTE_OPTIONS: Record<ProviderRole, RouteProviderOption[]> = {
  llm: [
    {
      id: "openai",
      name: "OpenAI",
      execution: "Cloud",
      privacy: "Conversation text and selected context leave this PC",
      egress: "Text → OpenAI",
      cost: "Metered API usage",
      models: [
        { id: "gpt-4.1-mini", name: "GPT-4.1 mini" },
        { id: "gpt-4.1", name: "GPT-4.1" },
      ],
    },
    {
      id: "nvidia-nim",
      name: "NVIDIA NIM",
      execution: "Cloud",
      privacy: "Conversation text enters NVIDIA's development API",
      egress: "Text → NVIDIA",
      cost: "Free development preview; limits apply",
      models: [
        {
          id: "nvidia/nemotron-3.5-lightning-30b-a3b",
          name: "Nemotron 3.5 Lightning 30B A3B",
        },
        {
          id: "nvidia/nemotron-3-nano-30b-a3b",
          name: "Nemotron 3 Nano 30B A3B",
        },
      ],
    },
    {
      id: "gemini",
      name: "Google Gemini",
      execution: "Cloud",
      privacy: "Conversation text and selected context leave this PC",
      egress: "Text → Google",
      cost: "Free tier or metered usage",
      models: [
        { id: "gemini-2.5-flash", name: "Gemini 2.5 Flash" },
        { id: "gemini-2.5-pro", name: "Gemini 2.5 Pro" },
      ],
    },
    {
      id: "anthropic",
      name: "Anthropic",
      execution: "Cloud",
      privacy: "Conversation text and selected context leave this PC",
      egress: "Text → Anthropic",
      cost: "Metered API usage",
      models: [
        { id: "claude-sonnet-4", name: "Claude Sonnet 4" },
        { id: "claude-haiku-3.5", name: "Claude 3.5 Haiku" },
      ],
    },
    {
      id: "groq",
      name: "Groq",
      execution: "Cloud",
      privacy: "Conversation text and selected context leave this PC",
      egress: "Text → Groq",
      cost: "Rate-limited trial or metered usage",
      models: [
        { id: "llama-3.3-70b-versatile", name: "Llama 3.3 70B Versatile" },
      ],
    },
    {
      id: "cohere",
      name: "Cohere",
      execution: "Cloud",
      privacy: "Conversation text and selected context leave this PC",
      egress: "Text → Cohere",
      cost: "Evaluation limits or metered usage",
      models: [{ id: "command-a-plus-05-2026", name: "Command A+" }],
    },
  ],
  stt: [
    {
      id: "openai",
      name: "OpenAI",
      execution: "Cloud",
      privacy: "Push-to-talk audio leaves this PC",
      egress: "Audio → OpenAI",
      cost: "Per audio minute",
      models: [
        { id: "gpt-4o-mini-transcribe", name: "GPT-4o mini Transcribe" },
      ],
    },
    {
      id: "deepgram",
      name: "Deepgram",
      execution: "Cloud",
      privacy: "Push-to-talk audio leaves this PC",
      egress: "Audio → Deepgram",
      cost: "Trial credit or per minute",
      models: [{ id: "nova-3", name: "Nova-3 streaming" }],
    },
    {
      id: "assemblyai",
      name: "AssemblyAI",
      execution: "Cloud",
      privacy: "Push-to-talk audio leaves this PC",
      egress: "Audio → AssemblyAI",
      cost: "Introductory credit or per minute",
      models: [{ id: "u3-rt-pro", name: "Universal-3 Pro Streaming" }],
    },
    {
      id: "elevenlabs",
      name: "ElevenLabs",
      execution: "Cloud",
      privacy: "Push-to-talk audio leaves this PC",
      egress: "Audio → ElevenLabs",
      cost: "Free allowance or metered usage",
      models: [{ id: "scribe-v1", name: "Scribe v1" }],
    },
    {
      id: "nvidia-nim-asr",
      name: "NVIDIA NIM ASR",
      execution: "Cloud",
      privacy: "Push-to-talk audio enters NVIDIA's development API",
      egress: "Audio → NVIDIA",
      cost: "Development preview; limits apply",
      selectable: false,
      note: "The same key is accepted by the official gRPC route, but two bounded live probes timed out. Keep this route disabled until it passes the in-app latency test.",
      models: [
        {
          id: "nemotron-asr-streaming",
          name: "Nemotron Streaming ASR",
          selectable: false,
        },
      ],
    },
  ],
  tts: [
    {
      id: "elevenlabs",
      name: "ElevenLabs",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → ElevenLabs",
      cost: "Characters or credits",
      models: [
        { id: "eleven_flash_v2_5", name: "Flash v2.5" },
        { id: "eleven-multilingual-v2", name: "Multilingual v2" },
      ],
    },
    {
      id: "cartesia",
      name: "Cartesia · runtime adapter unavailable",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → Cartesia",
      cost: "Free credits or metered usage",
      selectable: false,
      note: "The ordinary runtime does not construct a Cartesia TTS adapter. Keep this route unavailable until an end-to-end provider receipt is qualified.",
      models: [{ id: "sonic-2", name: "Sonic 2", selectable: false }],
    },
    {
      id: "deepgram",
      name: "Deepgram Aura · runtime adapter unavailable",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → Deepgram",
      cost: "Trial credit or metered usage",
      selectable: false,
      note: "Deepgram STT is configurable, but the ordinary runtime does not construct its TTS adapter. This voice route is unavailable.",
      models: [{ id: "aura-2", name: "Aura-2", selectable: false }],
    },
    {
      id: "inworld",
      name: "Inworld · runtime adapter unavailable",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → Inworld",
      cost: "Prototype allowance or metered usage",
      selectable: false,
      note: "The ordinary runtime does not construct an Inworld TTS adapter. Keep this route unavailable until an end-to-end provider receipt is qualified.",
      models: [
        {
          id: "inworld-tts-1.5-max",
          name: "TTS 1.5 Max",
          selectable: false,
        },
      ],
    },
    {
      id: "nvidia-nim-magpie",
      name: "NVIDIA NIM Magpie · private evaluation only",
      execution: "Cloud",
      privacy: "Reply text enters NVIDIA's private-evaluation API route",
      egress: "Text → NVIDIA",
      cost: "Private evaluation; provider limits apply",
      note: "Only the isolated native .debug/.review namespace may use authenticated stock-voice discovery after exact current-terms acknowledgement. The base production namespace, promotion, publication, performer cloning, and uploaded voice prompts remain blocked.",
      models: [
        {
          id: "magpie-tts-multilingual",
          name: "Magpie multilingual · stock voice",
        },
      ],
    },
  ],
  embeddings: [
    {
      id: "nvidia-nim",
      name: "NVIDIA NIM",
      execution: "Cloud",
      privacy: "Lore and memory query text leave this PC for embedding",
      egress: "Text → NVIDIA",
      cost: "Development preview; limits apply",
      models: [
        { id: "nvidia/nemotron-3-embed-1b", name: "Nemotron 3 Embed 1B" },
      ],
    },
    {
      id: "cohere",
      name: "Cohere",
      execution: "Cloud",
      privacy: "Lore and memory query text leave this PC for embedding",
      egress: "Text → Cohere",
      cost: "Evaluation limits or metered usage",
      selectable: false,
      note: "Catalog metadata only; no Cohere embedding runtime is implemented.",
      models: [{ id: "embed-v4.0", name: "Embed v4.0" }],
    },
    {
      id: "fts-only",
      name: "Local keyword memory",
      execution: "Local",
      privacy: "Memory stays on this PC",
      egress: "None",
      cost: "No provider charge",
      models: [{ id: "sqlite-fts5", name: "SQLite FTS5 · no embeddings" }],
    },
  ],
  vision: [
    offVision,
    {
      id: "openai",
      name: "OpenAI",
      execution: "Cloud",
      privacy:
        "Only an explicitly captured frame and selected context leave this PC",
      egress: "Selected frame → OpenAI",
      cost: "Metered API usage",
      models: [{ id: "gpt-4.1-mini", name: "GPT-4.1 mini vision" }],
    },
    {
      id: "nvidia-nim-vision",
      name: "NVIDIA NIM Vision",
      execution: "Cloud",
      selectable: false,
      privacy:
        "Only an explicitly captured frame and selected context enter NVIDIA's development API",
      egress: "Selected frame → NVIDIA",
      cost: "Development preview; exact endpoint qualification pending",
      note: "Visible for profile planning only. No compatible live vision endpoint has passed the in-app route test.",
      models: [
        {
          id: "nvidia-vision-unqualified",
          name: "Exact model not qualified",
          selectable: false,
        },
      ],
    },
  ],
  lipSync: [
    offLipSync,
    {
      id: "local-visual-worker",
      name: "Local visual worker",
      execution: "Local",
      selectable: false,
      privacy: "Captured face frames and delivered audio stay on this PC",
      egress: "None",
      cost: "GPU/VRAM and game frame-time",
      note: "Research paths remain unavailable until a signed, qualified pack exists and the live resource-admission gate passes.",
      models: [
        {
          id: "character-mouth-atlas",
          name: "Character mouth atlas · 287 KB static proof; live route pending",
          selectable: false,
        },
        {
          id: "a2f3d-regression",
          name: "Audio2Face-3D regression · unverified",
          selectable: false,
        },
        {
          id: "nvidia-ar-lipsync",
          name: "NVIDIA LipSync AR SDK · AI for Media private access",
          selectable: false,
        },
        {
          id: "musetalk",
          name: "MuseTalk 1.5 · 8.95 FPS / 7,617 MiB; enrollment teacher only",
          selectable: false,
        },
      ],
    },
  ],
};

const initialLoadouts: ProviderLoadout[] = [
  {
    id: "global-balanced-api",
    name: "Balanced API",
    scope: "global",
    targetLabel: "Every game",
    active: true,
    revision: 3,
    routes: {
      llm: { providerId: "openai", modelId: "gpt-4.1-mini" },
      stt: { providerId: "openai", modelId: "gpt-4o-mini-transcribe" },
      tts: {
        providerId: "elevenlabs",
        modelId: "eleven_flash_v2_5",
        voiceId: "EXAVITQu4vr4xnSDxMaL",
      },
      embeddings: { providerId: "fts-only", modelId: "sqlite-fts5" },
      vision: { providerId: "disabled", modelId: "disabled" },
      lipSync: { providerId: "disabled", modelId: "disabled" },
    },
    fallbacks: {},
  },
  {
    id: "game-night-city-fast",
    name: "Night City fast response",
    scope: "game",
    targetId: "cyberpunk-2077",
    targetLabel: "Cyberpunk 2077",
    active: false,
    revision: 1,
    routes: {
      llm: { providerId: "groq", modelId: "llama-3.3-70b-versatile" },
      stt: { providerId: "deepgram", modelId: "nova-3" },
      tts: {
        providerId: "elevenlabs",
        modelId: "eleven_flash_v2_5",
        voiceId: "EXAVITQu4vr4xnSDxMaL",
      },
      embeddings: { providerId: "fts-only", modelId: "sqlite-fts5" },
      vision: { providerId: "disabled", modelId: "disabled" },
      lipSync: { providerId: "disabled", modelId: "disabled" },
    },
    fallbacks: {},
  },
  {
    id: "character-mara-cinematic",
    name: "Mara · cinematic voice",
    scope: "character",
    targetId: "eclipse-harbor/mara-venn",
    targetLabel: "Eclipse Harbor · Mara Venn",
    active: false,
    revision: 1,
    routes: {
      llm: {
        providerId: "nvidia-nim",
        modelId: "nvidia/nemotron-3.5-lightning-30b-a3b",
      },
      stt: { providerId: "assemblyai", modelId: "u3-rt-pro" },
      tts: {
        providerId: "nvidia-nim-magpie",
        modelId: "magpie-tts-multilingual",
        voiceId: "Magpie-Multilingual.EN-US.Aria",
      },
      embeddings: {
        providerId: "nvidia-nim",
        modelId: "nvidia/nemotron-3-embed-1b",
      },
      vision: { providerId: "disabled", modelId: "disabled" },
      lipSync: { providerId: "disabled", modelId: "disabled" },
    },
    fallbacks: {},
  },
];

const STORAGE_KEY = "npc2.provider-loadouts.v1";

export function readBrowserLoadouts(): ProviderLoadout[] {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored) as unknown;
      if (Array.isArray(parsed) && parsed.length > 0) {
        return parsed.map(normalizeBrowserLoadout);
      }
    }
  } catch {
    // A blocked browser storage surface should leave the safe fixture usable.
  }
  return structuredClone(initialLoadouts);
}

type LegacyProviderLoadout = Omit<ProviderLoadout, "routes" | "fallbacks"> & {
  routes: Partial<Record<ProviderRole | "retrieval", RouteChoice>>;
  fallbacks?: Partial<Record<ProviderRole | "retrieval", ManualFallback>>;
};

function normalizeBrowserLoadout(value: unknown): ProviderLoadout {
  const candidate = value as LegacyProviderLoadout;
  const fallback = structuredClone(initialLoadouts[0]);
  const legacyEmbeddings = candidate.routes?.retrieval;
  const legacyEmbeddingFallback = candidate.fallbacks?.retrieval;
  const routes = Object.fromEntries(
    ROLE_ORDER.map((role) => [
      role,
      role === "embeddings"
        ? (candidate.routes?.embeddings ??
          legacyEmbeddings ??
          fallback.routes.embeddings)
        : (candidate.routes?.[role] ?? fallback.routes[role]),
    ]),
  ) as ProviderLoadout["routes"];
  if (routes.tts.providerId === "nvidia-nim-magpie" && !routes.tts.voiceId) {
    routes.tts = {
      ...routes.tts,
      voiceId: "Magpie-Multilingual.EN-US.Aria",
    };
  } else if (routes.tts.providerId === "elevenlabs" && !routes.tts.voiceId) {
    routes.tts = {
      ...routes.tts,
      voiceId: "EXAVITQu4vr4xnSDxMaL",
    };
  }
  const fallbacks = Object.fromEntries(
    ROLE_ORDER.flatMap((role) => {
      const selected =
        role === "embeddings"
          ? (candidate.fallbacks?.embeddings ?? legacyEmbeddingFallback)
          : candidate.fallbacks?.[role];
      return selected ? [[role, selected]] : [];
    }),
  ) as ProviderLoadout["fallbacks"];
  return {
    ...fallback,
    ...candidate,
    routes,
    fallbacks,
  };
}

export function writeBrowserLoadouts(loadouts: ProviderLoadout[]) {
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(loadouts.map(normalizeBrowserLoadout)),
    );
  } catch {
    // Native persistence owns production state; browser storage is preview-only.
  }
}

export function resetBrowserLoadoutsForTests() {
  window.localStorage.removeItem(STORAGE_KEY);
}

export function providerFor(role: ProviderRole, providerId: string) {
  return (
    ROUTE_OPTIONS[role].find((provider) => provider.id === providerId) ??
    ROUTE_OPTIONS[role][0]
  );
}

export function modelFor(role: ProviderRole, route: RouteChoice) {
  const provider = providerFor(role, route.providerId);
  return (
    provider.models.find((model) => model.id === route.modelId) ??
    provider.models[0]
  );
}

export function makeLoadout(
  scope: LoadoutScope,
  sequence: number,
): ProviderLoadout {
  const base = initialLoadouts[0];
  return {
    ...structuredClone(base),
    id: `loadout-${Date.now()}-${sequence}`,
    name: `New ${scope} loadout`,
    scope,
    targetId:
      scope === "global"
        ? undefined
        : scope === "game"
          ? "cyberpunk-2077"
          : "eclipse-harbor/mara-venn",
    targetLabel:
      scope === "global"
        ? "Every game"
        : scope === "game"
          ? "Cyberpunk 2077"
          : "Eclipse Harbor · Mara Venn",
    active: false,
    revision: 1,
    fallbacks: {},
  };
}

export function activeBrowserLoadoutFor(
  gameId: string,
  characterId: string,
): ProviderLoadout {
  const loadouts = readBrowserLoadouts();
  return (
    loadouts.find(
      (loadout) =>
        loadout.active &&
        loadout.scope === "character" &&
        loadout.targetId === `${gameId}/${characterId}`,
    ) ??
    loadouts.find(
      (loadout) =>
        loadout.active &&
        loadout.scope === "game" &&
        loadout.targetId === gameId,
    ) ??
    loadouts.find((loadout) => loadout.active && loadout.scope === "global") ??
    loadouts[0]
  );
}
