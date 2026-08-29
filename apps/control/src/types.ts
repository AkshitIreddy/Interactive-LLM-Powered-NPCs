export type PageId =
  | "home"
  | "games"
  | "characters"
  | "conversation"
  | "presence"
  | "performance"
  | "models"
  | "diagnostics"
  | "settings"
  | "help";

export type DemoState =
  | "ready"
  | "active"
  | "loading"
  | "empty"
  | "error"
  | "degraded";
export type ExecutionMode = "cloud" | "hybrid" | "local";
export type PerformanceMode =
  | "competitive"
  | "fast"
  | "balanced"
  | "immersive"
  | "maximum"
  | "custom";
export type StageId =
  | "listening"
  | "transcribing"
  | "identifying"
  | "remembering"
  | "responding"
  | "voicing"
  | "animating";

export interface Stage {
  id: StageId;
  label: string;
  shortLabel: string;
  fixtureDuration: number;
}

export interface GameProfile {
  id: string;
  title: string;
  abbreviation: string;
  accent: string;
  wave: "A" | "B" | "C" | "Risk-gated";
  state: "authored" | "offline-policy";
  store: string;
  characters?: number;
  capability: string;
  description: string;
}

export interface ModelPack {
  id: string;
  name: string;
  lane: "Performance" | "High Fidelity" | "Offline comparator";
  purpose: "Speech in" | "Thinking" | "Voice out" | "Memory" | "Presence";
  size: string;
  fit: string;
  admission: "Fits" | "CPU-only" | "Conflicts" | "Unverified";
  state: "candidate";
  license: string;
  availability:
    | "conditional"
    | "experimental"
    | "baseline"
    | "deferred"
    | "offline";
  access: string;
  latency: string;
  decision: string;
  output: string;
}

export interface AppPreferences {
  execution: ExecutionMode;
  performance: PerformanceMode;
  subtitles: boolean;
  ptt: boolean;
  localOnly: boolean;
  screenPresence: boolean;
  diagnostics: boolean;
}
