import type { SVGProps } from "react";

export type IconName =
  | "home"
  | "games"
  | "characters"
  | "conversation"
  | "presence"
  | "performance"
  | "models"
  | "diagnostics"
  | "settings"
  | "help"
  | "play"
  | "pause"
  | "arrow"
  | "check"
  | "warning"
  | "search"
  | "download"
  | "mic"
  | "shield"
  | "spark"
  | "chevron"
  | "close"
  | "more"
  | "headphones"
  | "folder"
  | "refresh"
  | "external";

const paths: Record<IconName, React.ReactNode> = {
  home: (
    <>
      <path d="M3.8 10.2 12 3.5l8.2 6.7" />
      <path d="M5.8 9.3v10.2h12.4V9.3M9.5 19.5v-6h5v6" />
    </>
  ),
  games: (
    <>
      <path d="M8.5 8.5h7c3.1 0 5.5 2.6 5.5 5.8 0 2.8-1.4 5.2-3.4 5.2-1.5 0-2.1-1.7-3.4-2.3h-4.4c-1.3.6-1.9 2.3-3.4 2.3-2 0-3.4-2.4-3.4-5.2 0-3.2 2.4-5.8 5.5-5.8Z" />
      <path d="M7 12v4M5 14h4M16 12.5h.01M18.5 15h.01" />
    </>
  ),
  characters: (
    <>
      <circle cx="9" cy="8" r="3.2" />
      <path d="M3.8 19.5c.5-4 2.2-6 5.2-6s4.7 2 5.2 6M15.5 4.8c2.7.3 4.1 1.8 4.1 4.4s-1.4 4-4.1 4.4M16.4 14.8c2.2.5 3.5 2.1 3.8 4.7" />
    </>
  ),
  conversation: (
    <>
      <path d="M4 5.5h16v11H9l-5 3v-14Z" />
      <path d="M8 10h8M8 13h5" />
    </>
  ),
  presence: (
    <>
      <path d="M4 7V4h3M17 4h3v3M20 17v3h-3M7 20H4v-3" />
      <circle cx="12" cy="11" r="3" />
      <path d="M7.5 18c.5-2.8 2-4.2 4.5-4.2s4 1.4 4.5 4.2" />
    </>
  ),
  performance: (
    <>
      <path d="M4 18.5h16M6 16V9M10 16V5M14 16v-3M18 16V7" />
    </>
  ),
  models: (
    <>
      <path d="m12 3 8 4.5-8 4.5-8-4.5L12 3Z" />
      <path d="m4 12 8 4.5 8-4.5M4 16.5l8 4.5 8-4.5" />
    </>
  ),
  diagnostics: (
    <>
      <path d="M5 4v16M19 4v16M5 8h5l2 7 2-4h5" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" />
    </>
  ),
  help: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M9.6 9a2.6 2.6 0 1 1 3 2.6c-.6.2-.6.7-.6 1.4M12 17h.01" />
    </>
  ),
  play: <path d="m8 5 11 7-11 7V5Z" />,
  pause: (
    <>
      <path d="M8 5v14M16 5v14" />
    </>
  ),
  arrow: (
    <>
      <path d="M5 12h14M14 7l5 5-5 5" />
    </>
  ),
  check: <path d="m5 12 4 4L19 6" />,
  warning: (
    <>
      <path d="M12 3 2.8 20h18.4L12 3Z" />
      <path d="M12 9v5M12 17.5h.01" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m15.5 15.5 5 5" />
    </>
  ),
  download: (
    <>
      <path d="M12 3v12M7.5 11 12 15.5l4.5-4.5M4 20h16" />
    </>
  ),
  mic: (
    <>
      <rect x="8" y="3" width="8" height="12" rx="4" />
      <path d="M5 11c0 4 2.3 7 7 7s7-3 7-7M12 18v3" />
    </>
  ),
  shield: (
    <>
      <path d="M12 3 4.5 6v5.5c0 4.4 2.5 7.6 7.5 9.5 5-1.9 7.5-5.1 7.5-9.5V6L12 3Z" />
      <path d="m8.5 12 2.2 2.2 4.8-5" />
    </>
  ),
  spark: (
    <>
      <path d="m12 3 1.5 5.5L19 10l-5.5 1.5L12 17l-1.5-5.5L5 10l5.5-1.5L12 3ZM18 16l.7 2.3L21 19l-2.3.7L18 22l-.7-2.3L15 19l2.3-.7L18 16Z" />
    </>
  ),
  chevron: <path d="m9 6 6 6-6 6" />,
  close: <path d="m6 6 12 12M18 6 6 18" />,
  more: (
    <>
      <circle cx="5" cy="12" r="1" />
      <circle cx="12" cy="12" r="1" />
      <circle cx="19" cy="12" r="1" />
    </>
  ),
  headphones: (
    <>
      <path d="M4 13v-2a8 8 0 0 1 16 0v2M4 13h4v7H6a2 2 0 0 1-2-2v-5ZM20 13h-4v7h2a2 2 0 0 0 2-2v-5Z" />
    </>
  ),
  folder: (
    <>
      <path d="M3 6h7l2 2h9v11H3V6Z" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 7v5h-5M4 17v-5h5" />
      <path d="M6 9a7 7 0 0 1 12-2l2 5M18 15a7 7 0 0 1-12 2l-2-5" />
    </>
  ),
  external: (
    <>
      <path d="M14 4h6v6M20 4l-9 9" />
      <path d="M18 13v7H4V6h7" />
    </>
  ),
};

export function Icon({
  name,
  size = 20,
  ...props
}: SVGProps<SVGSVGElement> & { name: IconName; size?: number }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {paths[name]}
    </svg>
  );
}
