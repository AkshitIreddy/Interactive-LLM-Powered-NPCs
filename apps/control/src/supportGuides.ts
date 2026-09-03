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
    summary: "Recheck the native boundary, world, providers, and one turn.",
    detail:
      "Rerunning setup does not silently activate a route or local pack. Each step reads the current native state and preserves explicit selections.",
    searchTerms: ["first run", "onboarding", "reset", "rerun"],
    actionLabel: "Rerun guided setup",
    action: { kind: "setup" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "diagnostics",
    title: "Native diagnostics",
    summary: "Inspect measured, unmeasured, blocked, and recovery evidence.",
    detail:
      "Diagnostics never turn missing measurements into zero. The matrix distinguishes native measurements from unavailable producers and exports only after an explicit local action.",
    searchTerms: ["health", "latency", "error", "export", "recovery"],
    actionLabel: "Run native diagnostics",
    action: { kind: "diagnostics" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "privacy",
    title: "Privacy and provider egress",
    summary:
      "Review what selected hosted routes can send and what stays local.",
    detail:
      "Credentials remain in the native vault. Provider disclosures identify selected data classes; automatic provider fallback stays false, and a preference mutation cannot activate a route.",
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
      "A configured role is not proof that a turn consumed it. Use the loadout editor for route intent and the Session receipt panels for authoritative consumption and delivery evidence.",
    searchTerms: ["tts", "stt", "llm", "voice", "model", "loadout", "magpie"],
    actionLabel: "Open Voice & models",
    action: { kind: "navigate", page: "voice" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "game",
    title: "Game and character troubleshooting",
    summary:
      "Inspect current-session target binding and persisted character selection.",
    detail:
      "Commercial capture remains fail-closed without native safety evidence. The task-owned synthetic review path is labeled separately and never grants authority to an ordinary game target.",
    searchTerms: [
      "world",
      "character",
      "target",
      "process",
      "capture",
      "memory",
    ],
    actionLabel: "Open World",
    action: { kind: "navigate", page: "world" },
    reviewedRevision: "support-2026-08-30",
  },
  {
    id: "overlay",
    title: "Overlay and subtitle troubleshooting",
    summary:
      "Check exact-window capture, overlay exclusion, and subtitle receipts.",
    detail:
      "Exact selected-window capture receipts are the evidence source. Perceived desktop brightness is not capture or color proof; subtitle delivery requires a committed native presentation receipt.",
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
