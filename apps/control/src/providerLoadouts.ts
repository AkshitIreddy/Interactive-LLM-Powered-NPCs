export type LoadoutScope = "global" | "game" | "character";
export type ProviderRole = "llm" | "stt" | "tts" | "retrieval" | "lipSync";

export interface RouteChoice {
  providerId: string;
  modelId: string;
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
  retrieval: {
    label: "Memory retrieval",
    short: "MEM",
    description: "Finds relevant lore and delivered memories.",
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
          id: "nvidia/nemotron-3-nano-30b-a3b",
          name: "Nemotron 3 Nano 30B A3B",
        },
        { id: "meta/llama-3.1-8b-instruct", name: "Llama 3.1 8B Instruct" },
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
      models: [{ id: "command-a", name: "Command A" }],
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
      models: [{ id: "universal-streaming", name: "Universal Streaming" }],
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
      note: "Visible for planning; live gRPC qualification is still pending.",
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
        { id: "eleven-flash-v2.5", name: "Flash v2.5" },
        { id: "eleven-multilingual-v2", name: "Multilingual v2" },
      ],
    },
    {
      id: "cartesia",
      name: "Cartesia",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → Cartesia",
      cost: "Free credits or metered usage",
      models: [{ id: "sonic-2", name: "Sonic 2" }],
    },
    {
      id: "deepgram",
      name: "Deepgram",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → Deepgram",
      cost: "Trial credit or metered usage",
      models: [{ id: "aura-2", name: "Aura-2" }],
    },
    {
      id: "inworld",
      name: "Inworld",
      execution: "Cloud",
      privacy: "Reply text leaves this PC",
      egress: "Text → Inworld",
      cost: "Prototype allowance or metered usage",
      models: [{ id: "inworld-tts-1.5-max", name: "TTS 1.5 Max" }],
    },
    {
      id: "nvidia-nim-magpie",
      name: "NVIDIA NIM Magpie",
      execution: "Cloud",
      privacy: "Reply text enters NVIDIA's development API",
      egress: "Text → NVIDIA",
      cost: "Development preview; limits apply",
      note: "Stock voices only. No performer cloning or uploaded voice prompts.",
      models: [
        {
          id: "magpie-tts-multilingual",
          name: "Magpie multilingual · stock voice",
        },
      ],
    },
  ],
  retrieval: [
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
          id: "a2f3d-regression",
          name: "Audio2Face-3D regression · unverified",
          selectable: false,
        },
        {
          id: "nvidia-ar-lipsync",
          name: "NVIDIA AR SDK LipSync · private access",
          selectable: false,
        },
        {
          id: "musetalk",
          name: "MuseTalk 1.5 · offline comparator",
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
      tts: { providerId: "elevenlabs", modelId: "eleven-flash-v2.5" },
      retrieval: { providerId: "fts-only", modelId: "sqlite-fts5" },
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
      tts: { providerId: "elevenlabs", modelId: "eleven-flash-v2.5" },
      retrieval: { providerId: "fts-only", modelId: "sqlite-fts5" },
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
        modelId: "nvidia/nemotron-3-nano-30b-a3b",
      },
      stt: { providerId: "assemblyai", modelId: "universal-streaming" },
      tts: {
        providerId: "nvidia-nim-magpie",
        modelId: "magpie-tts-multilingual",
      },
      retrieval: {
        providerId: "nvidia-nim",
        modelId: "nvidia/nemotron-3-embed-1b",
      },
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
      const parsed = JSON.parse(stored) as ProviderLoadout[];
      if (Array.isArray(parsed) && parsed.length > 0) return parsed;
    }
  } catch {
    // A blocked browser storage surface should leave the safe fixture usable.
  }
  return structuredClone(initialLoadouts);
}

export function writeBrowserLoadouts(loadouts: ProviderLoadout[]) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(loadouts));
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
