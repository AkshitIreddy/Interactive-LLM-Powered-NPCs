import type { GameProfile, ModelPack, PageId, Stage } from "./types";
import type { IconName } from "./icons";

export const NAV_ITEMS: Array<{
  id: PageId;
  label: string;
  icon: IconName;
  group: "primary" | "system";
}> = [
  { id: "home", label: "Home", icon: "home", group: "primary" },
  { id: "games", label: "Games", icon: "games", group: "primary" },
  {
    id: "characters",
    label: "Characters",
    icon: "characters",
    group: "primary",
  },
  {
    id: "conversation",
    label: "Conversation",
    icon: "conversation",
    group: "primary",
  },
  { id: "presence", label: "Presence", icon: "presence", group: "primary" },
  {
    id: "performance",
    label: "Performance",
    icon: "performance",
    group: "primary",
  },
  { id: "models", label: "Models", icon: "models", group: "primary" },
  {
    id: "diagnostics",
    label: "Diagnostics",
    icon: "diagnostics",
    group: "system",
  },
  { id: "settings", label: "Settings", icon: "settings", group: "system" },
  { id: "help", label: "Help", icon: "help", group: "system" },
];

export const RESPONSE_STAGES: Stage[] = [
  {
    id: "listening",
    label: "Listening",
    shortLabel: "Fixture",
    fixtureDuration: 412,
  },
  {
    id: "transcribing",
    label: "Transcribing",
    shortLabel: "Fixture",
    fixtureDuration: 286,
  },
  {
    id: "identifying",
    label: "Identifying",
    shortLabel: "Fixture",
    fixtureDuration: 41,
  },
  {
    id: "remembering",
    label: "Remembering",
    shortLabel: "Fixture",
    fixtureDuration: 53,
  },
  {
    id: "responding",
    label: "Responding",
    shortLabel: "Fixture",
    fixtureDuration: 478,
  },
  {
    id: "voicing",
    label: "Voicing",
    shortLabel: "Fixture",
    fixtureDuration: 221,
  },
  {
    id: "animating",
    label: "Animating",
    shortLabel: "Fixture",
    fixtureDuration: 77,
  },
];

type HonestProfileSeed = Omit<GameProfile, "state" | "store" | "capability"> & {
  offlinePolicy?: boolean;
  catalogSource: string;
};
const seeds: HonestProfileSeed[] = [
  {
    id: "skyrim",
    title: "Skyrim Special Edition",
    abbreviation: "SK",
    accent: "#9fc9c0",
    wave: "A",
    catalogSource: "Steam metadata",
    description:
      "Authored lore, prompts, safety policy, and external-capture guidance.",
  },
  {
    id: "cyberpunk",
    title: "Cyberpunk 2077",
    abbreviation: "77",
    accent: "#ff4c5f",
    wave: "A",
    catalogSource: "GOG metadata",
    description:
      "Authored Night City content and proposed external-capture policy.",
  },
  {
    id: "bg3",
    title: "Baldur’s Gate 3",
    abbreviation: "BG",
    accent: "#c98a68",
    wave: "A",
    catalogSource: "Steam metadata",
    description: "Authored party, memory, and spoiler-boundary content.",
  },
  {
    id: "fallout4",
    title: "Fallout 4",
    abbreviation: "F4",
    accent: "#9abe85",
    wave: "B",
    catalogSource: "Steam metadata",
    description: "Authored companion and settlement context package.",
  },
  {
    id: "new-vegas",
    title: "Fallout: New Vegas",
    abbreviation: "NV",
    accent: "#c7aa70",
    wave: "B",
    catalogSource: "Manual metadata",
    description: "Authored faction and companion content package.",
  },
  {
    id: "witcher3",
    title: "The Witcher 3",
    abbreviation: "W3",
    accent: "#b9c4cb",
    wave: "B",
    catalogSource: "GOG metadata",
    description: "Authored spoiler-tiered conversation package.",
  },
  {
    id: "starfield",
    title: "Starfield",
    abbreviation: "SF",
    accent: "#6ca6cf",
    wave: "B",
    catalogSource: "Xbox metadata",
    description: "Authored crew, faction, and exploration content.",
  },
  {
    id: "bannerlord",
    title: "Mount & Blade II: Bannerlord",
    abbreviation: "MB",
    accent: "#c0a57f",
    wave: "B",
    catalogSource: "Manual metadata",
    description: "Authored clan and kingdom content package.",
  },
  {
    id: "kcd2",
    title: "Kingdom Come: Deliverance II",
    abbreviation: "KC",
    accent: "#b6915f",
    wave: "B",
    catalogSource: "Steam metadata",
    description: "Authored period-aware and spoiler-boundary content.",
  },
  {
    id: "oblivion",
    title: "Oblivion Remastered",
    abbreviation: "OR",
    accent: "#a99dd4",
    wave: "B",
    catalogSource: "Steam metadata",
    description: "Authored disposition and world-lore content.",
  },
  {
    id: "sims4",
    title: "The Sims 4",
    abbreviation: "S4",
    accent: "#7fc6a7",
    wave: "C",
    catalogSource: "EA metadata",
    description: "Authored trait and household-memory content.",
  },
  {
    id: "stardew",
    title: "Stardew Valley",
    abbreviation: "SV",
    accent: "#9fc16f",
    wave: "C",
    catalogSource: "Steam metadata",
    description: "Authored season and friendship content.",
  },
  {
    id: "minecraft",
    title: "Minecraft Java Edition",
    abbreviation: "MC",
    accent: "#78a263",
    wave: "C",
    catalogSource: "Launcher metadata",
    description: "Authored named-character and world-context package.",
  },
  {
    id: "dos2",
    title: "Divinity: Original Sin 2",
    abbreviation: "D2",
    accent: "#c78368",
    wave: "C",
    catalogSource: "Manual metadata",
    description: "Authored party and quest-boundary content.",
  },
  {
    id: "mass-effect",
    title: "Mass Effect Legendary Edition",
    abbreviation: "ME",
    accent: "#61a5ca",
    wave: "C",
    catalogSource: "EA metadata",
    description: "Authored squad and mission-context package.",
  },
  {
    id: "dragon-age",
    title: "Dragon Age: Inquisition",
    abbreviation: "DA",
    accent: "#a57b65",
    wave: "C",
    catalogSource: "EA metadata",
    description: "Authored party and world-state content.",
  },
  {
    id: "kenshi",
    title: "Kenshi",
    abbreviation: "KE",
    accent: "#a18f72",
    wave: "C",
    catalogSource: "Manual metadata",
    description: "Authored recruit and settlement content.",
  },
  {
    id: "rdr2",
    title: "Red Dead Redemption 2 — Story",
    abbreviation: "R2",
    accent: "#b16e5c",
    wave: "Risk-gated",
    catalogSource: "Rockstar metadata",
    offlinePolicy: true,
    description:
      "Authored Story Mode policy; live safety certification is absent.",
  },
  {
    id: "gta5",
    title: "Grand Theft Auto V — Story",
    abbreviation: "V",
    accent: "#72a281",
    wave: "Risk-gated",
    catalogSource: "Rockstar metadata",
    offlinePolicy: true,
    description:
      "Authored Story Mode policy; live safety certification is absent.",
  },
  {
    id: "elden-ring",
    title: "Elden Ring — Offline",
    abbreviation: "ER",
    accent: "#b9a46e",
    wave: "Risk-gated",
    catalogSource: "Steam metadata",
    offlinePolicy: true,
    description:
      "Authored offline-only policy; live safety certification is absent.",
  },
];

export const GAME_PROFILES: GameProfile[] = seeds.map(
  ({ offlinePolicy, catalogSource, ...seed }) => ({
    ...seed,
    state: offlinePolicy ? "offline-policy" : "authored",
    store: `${catalogSource} only · not detected`,
    capability: "Not live-certified",
  }),
);

export const CHARACTERS = [
  {
    name: "Aela the Huntress",
    game: "Skyrim Special Edition",
    role: "Authored catalog character",
    bond: "Fixture only",
    voice: "Voice intent only · no pack installed",
    memories: 0,
    accent: "#9fc9c0",
    monogram: "AE",
    last: "Not run",
  },
  {
    name: "Jackie Welles",
    game: "Cyberpunk 2077",
    role: "Authored catalog character",
    bond: "Fixture only",
    voice: "Voice intent only · no pack installed",
    memories: 0,
    accent: "#ff4c5f",
    monogram: "JW",
    last: "Not run",
  },
  {
    name: "Shadowheart",
    game: "Baldur’s Gate 3",
    role: "Authored catalog character",
    bond: "Fixture only",
    voice: "Voice intent only · no pack installed",
    memories: 0,
    accent: "#c98a68",
    monogram: "SH",
    last: "Not run",
  },
  {
    name: "Nick Valentine",
    game: "Fallout 4",
    role: "Authored catalog character",
    bond: "Fixture only",
    voice: "Voice intent only · no pack installed",
    memories: 0,
    accent: "#9abe85",
    monogram: "NV",
    last: "Not run",
  },
  {
    name: "Mara Venn",
    game: "Eclipse Harbor",
    role: "Synthetic demonstration character",
    bond: "Simulated",
    voice: "Synthetic voice fixture",
    memories: 2,
    accent: "#5ffbff",
    monogram: "MV",
    last: "Fixture",
  },
];

export const MODEL_PACKS: ModelPack[] = [
  {
    id: "character-mouth-atlas",
    name: "Character mouth atlas",
    lane: "Performance",
    purpose: "Presence",
    size: "943 KB raw / 287 KB compressed per-character proof",
    fit: "No resident visual AI model",
    admission: "Headless-qualified",
    state: "candidate",
    license: "Project artifact · teacher provenance required",
    availability: "experimental",
    access:
      "Generated per confirmed character during enrollment; no downloadable runtime model",
    latency:
      "Four native 1080p actor-bound atlas select + compose runs measured 4.549-4.724 ms mean / 5.101-6.692 ms p95 with 0 GPU VRAM; the separate moving-video artifact gate passed",
    decision:
      "Preferred runtime architecture. Real observed or enrollment-teacher mouth states are warped into the exact current game frame; production still requires per-character enrollment and installed live-game qualification before general enablement.",
    output:
      "Identity-bound mouth atlas → authenticated bounded current-frame residual",
  },
  {
    id: "a2f3d-regression",
    name: "Audio2Face-3D regression",
    lane: "Performance",
    purpose: "Presence",
    size: "Manifest not provisioned",
    fit: "Low-latency coefficient candidate",
    admission: "Unverified",
    state: "candidate",
    license: "NVIDIA SDK and model terms · review required",
    availability: "baseline",
    access: "Research candidate only; no reviewed Windows pack exists",
    latency:
      "Streaming coefficients are promising; this PC under game load is unmeasured",
    decision:
      "Map jaw and lip coefficients onto a tracked lower-mouth mesh over the newest game frame.",
    output: "Mouth and jaw coefficients → tiny 2D residual",
  },
  {
    id: "nvidia-ar-lipsync",
    name: "NVIDIA AR SDK LipSync",
    lane: "High Fidelity",
    purpose: "Presence",
    size: "Manifest not provisioned",
    fit: "Windows RTX candidate · 14-frame pre-roll",
    admission: "Unverified",
    state: "candidate",
    license: "NVIDIA model terms · review required",
    availability: "conditional",
    access:
      "Private NGC access required; a standard NVIDIA API key does not unlock it",
    latency:
      "Streaming vendor path; visual look-ahead and 12 GB contention remain unqualified",
    decision:
      "Derive a strict mouth-only difference from synchronized live frames, never replace the full face.",
    output: "Same-frame video result → clipped mouth residual",
  },
  {
    id: "musetalk",
    name: "MuseTalk 1.5",
    lane: "Performance",
    purpose: "Presence",
    size: "Manifest not provisioned",
    fit: "7,617 MiB peak before game · conflicts with coexistence envelope",
    admission: "Conflicts",
    state: "candidate",
    license: "Model + dependency review",
    availability: "conditional",
    access:
      "Public artifacts; no reviewed downloadable pack or complete app route exists",
    latency:
      "Corrected-crop persistent probe measured 7.66 FPS at batch 1 and 8.95 FPS best complete hot path",
    decision:
      "Reject as a resident gameplay renderer on this device. Retain only as an optional offline enrollment teacher for the tiny mouth atlas.",
    output: "Enrollment teacher frames → mouth atlas · unavailable",
  },
];

export const ONBOARDING_STEPS = [
  {
    id: "welcome",
    label: "Welcome",
    description: "Meet your Response Console",
  },
  {
    id: "scan",
    label: "This PC",
    description: "Illustrative hardware and privacy scan",
  },
  {
    id: "execution",
    label: "Run mode",
    description: "Choose a preferred execution boundary",
  },
  {
    id: "game",
    label: "First game",
    description: "Choose an authored profile",
  },
  {
    id: "providers",
    label: "Voice & mind",
    description: "Review candidates and providers",
  },
  {
    id: "microphone",
    label: "Talk",
    description: "Rehearse a simulated push-to-talk turn",
  },
  {
    id: "presence",
    label: "Presence",
    description: "Choose optional privacy preferences",
  },
  {
    id: "performance",
    label: "Performance",
    description: "Choose an illustrative target",
  },
  {
    id: "simulation",
    label: "Test run",
    description: "Run a deterministic fixture",
  },
  {
    id: "ready",
    label: "Setup complete",
    description: "Await runtime verification",
  },
];
