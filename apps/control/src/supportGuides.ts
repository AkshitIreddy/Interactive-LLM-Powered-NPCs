export type SupportGuideAction =
  | { kind: "setup" }
  | { kind: "diagnostics" }
  | { kind: "navigate"; page: "world" | "voice" | "diagnostics" };

export interface SupportGuideEntry {
  id: "setup" | "diagnostics" | "privacy" | "voice" | "game" | "overlay";
  title: string;
  summary: string;
  detail: string;
  searchTerms: string[];
  actionLabel: string;
  action: SupportGuideAction;
  reviewedRevision: string;
}

/**
 * Bundled, product-reviewed support metadata. Entries intentionally link only
 * to owned in-app states; no network destination or placeholder URL is kept.
 */
export const SUPPORT_GUIDES: SupportGuideEntry[] = [
  {
    id: "setup",
    title: "Guided setup",
    summary: "Connect a game, choose a voice and try a conversation.",
    detail:
      "Walk through your game, provider and microphone settings again. Your saved choices stay in place.",
    searchTerms: ["first run", "onboarding", "reset", "rerun"],
    actionLabel: "Rerun guided setup",
    action: { kind: "setup" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "diagnostics",
    title: "Troubleshooting",
    summary: "Find out why a connection or conversation is not working.",
    detail:
      "Run connection checks, inspect timing and export a report when you need help. Detailed results are available here when you need them.",
    searchTerms: ["health", "latency", "error", "export", "recovery"],
    actionLabel: "Run native diagnostics",
    action: { kind: "diagnostics" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "privacy",
    title: "Cloud and local processing",
    summary:
      "Review what selected hosted routes can send and what stays local.",
    detail:
      "Cloud models use your connected provider accounts. Local models run on this PC. Change providers or review what each receives in your loadout.",
    searchTerms: [
      "credential",
      "vault",
      "cloud",
      "data",
      "retention",
      "egress",
    ],
    actionLabel: "Review privacy controls",
    action: { kind: "navigate", page: "voice" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "voice",
    title: "Voice and model troubleshooting",
    summary:
      "Check the exact provider, model, stock voice, and local admission.",
    detail:
      "Select Listening, Intelligence or Voice in your loadout to change its provider and model. Connect a missing account, then check the selected configuration.",
    searchTerms: ["tts", "stt", "llm", "voice", "model", "loadout", "magpie"],
    actionLabel: "Open loadout",
    action: { kind: "navigate", page: "voice" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "game",
    title: "Game and character troubleshooting",
    summary:
      "Inspect current-session target binding and persisted character selection.",
    detail:
      "Launch Cyberpunk 2077, connect its window and choose your character. The included practice game is available separately. Mouth packs add character-specific lip-sync; voice and memory settings are separate.",
    searchTerms: [
      "world",
      "character",
      "target",
      "process",
      "capture",
      "memory",
    ],
    actionLabel: "Open Games",
    action: { kind: "navigate", page: "world" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "overlay",
    title: "Overlay and subtitle troubleshooting",
    summary:
      "Check exact-window capture, overlay exclusion, and subtitle receipts.",
    detail:
      "Choose subtitle size, placement and appearance in Settings. If subtitles do not appear in the game, check the selected window and run the connection checks.",
    searchTerms: [
      "subtitle",
      "wgc",
      "window",
      "brightness",
      "display",
      "receipt",
    ],
    actionLabel: "Open overlay diagnostics",
    action: { kind: "navigate", page: "diagnostics" },
    reviewedRevision: "support-2026-08-30",
  },
];
