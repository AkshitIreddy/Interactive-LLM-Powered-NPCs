# Product architecture and acceptance map

Status: implementation contract  
Date: 2026-08-29

## Product thesis

NPC 2.0 is a Windows session instrument for talking to one explicitly selected
character in one safely selected single-player window. It is not a game
catalog landing page. The main view answers five questions at a glance:

1. Which window is selected and are frames advancing?
2. Which character will answer and why is that identity trusted?
3. Which exact STT/LLM/TTS/stock-voice routes will run?
4. Is PTT/audio/subtitle delivery ready?
5. What action can the player take now?

## New information architecture

```text
Session Deck
├── Start/stop, PTT, compact response trace
├── Selected world + advancing capture evidence
├── Selected character + identity authority
└── Delivered-only current conversation

World
├── Synthetic test target
├── Detected/manual single-player targets
├── Runtime profile detail
└── Game-scoped character roster

Voice & Models
├── Named loadouts and inheritance
├── STT / LLM / TTS / stock voice / retrieval
├── Credential presence and real bounded tests
└── Optional local visual packs (unavailable until qualified)

Diagnostics
├── Runtime/broker/capture/audio/provider checks
├── Measured benchmark evidence
├── Turn trace and failure remediation
└── Redacted export when implemented

Settings & Guide
├── Persisted PTT/subtitle/privacy defaults
├── Subtitle style preview and accessibility
├── Rerun/reset setup
└── Real task links only
```

Old routes remain redirectable for saved links, but they render the owning new
surface rather than duplicate fixture pages: Characters and Presence → World;
Conversation → Session Deck; Performance → Diagnostics; Models → Voice &
Models; Help → Settings & Guide.

## State model

Every retained surface has explicit `loading`, `empty`, `blocked`, `ready`,
`running`, `degraded`, and `error` semantics where applicable. No query-string
demo state is product state.

The trusted ID chain is:

```text
capture session
  → target generation
  → frame ID + QPC timestamp
  → actor track ID
  → identity-lock revision
  → turn ID + cancellation generation
  → audio/subtitle segment IDs
  → delivered-turn commit
```

Configuration has one source of truth and explicit inheritance:

```text
global loadout → game override → character override
```

Each effective route records provider, endpoint kind, model ID, stock voice ID,
metadata timestamp, credential-reference presence, privacy/cost disclosure,
and last bounded test result. Credentials never enter the WebView.

## Visual direction

Subject: a cockpit-grade conversation instrument for a player already running
a game. Audience: PC players who understand game settings but should not need
to understand runtime architecture.

Palette:

- `Carbon void #080b0d`: calm primary field.
- `Gunmetal #11191d`: raised instrument surfaces.
- `Signal cyan #55f6e8`: ready/selected/current data.
- `Arc amber #f3c969`: attention and player-controlled action.
- `Fault coral #ff6a6f`: blocked/error only.
- `Paper white #eef6f4`: functional text with accessible contrast.

Type roles:

- Display: a narrow open-licensed industrial sans used sparingly for selected
  target/character and primary state.
- Body: an open-licensed highly legible sans with Windows Segoe UI fallback.
- Utility: a tabular/monospace face for IDs, timings, counters, and provenance.
- Noto script-specific fallbacks are bundled only with OFL notices and tested
  per script; Segoe files are never redistributed.

Layout: fixed compact rail, one primary Session Deck, narrow telemetry rail.
The first viewport contains the entire start decision. Dense information uses
alignment and consistent rails, not tiny text or decorative card grids.

Signature: the **signal rail** is one horizontal chain of target → actor →
provider → voice → delivery. Each node is sourced, actionable, and changes
state; it replaces the oversized marketing hero and decorative pipeline prose.

Motion: one orchestrated turn pulse moves along the signal rail. No perpetual
decoration. Reduced motion swaps it for immediate state changes.

## Vertical slice acceptance map

| Stage | Required implementation | Passing evidence |
| --- | --- | --- |
| Clean launch | Native bootstrap returns persisted setup state | On a clean app-data root, onboarding opens automatically without a query flag |
| System check | Runtime/broker doctor called; no invented hardware values | Timestamped native results or one plain blocker |
| Synthetic game | Launch/detect exact task-owned GUI EXE, validate PID/HWND/path/hash, select WGC target | No console; selection generation plus at least two positive frame-counter deltas |
| Character | Select Mara Venn inside Eclipse Harbor | Game-scoped ID and explicit-selection identity evidence persist |
| Providers | Select/test exact LLM/STT/TTS/retrieval and approved stock voice | Native loadout resolved; credentials checked; bounded provider tests show timestamp/outcome/network provenance |
| PTT/audio | Select input/output, set PTT, run mic rehearsal | Device IDs persist; captured sample/level/noise/endpoint evidence is measured, not simulated |
| Spoken turn | Run the lighthouse prompt through the selected real TTS and audio sink | Non-silent PCM receipt, first-audible timestamp, completed device drain, and cancellation test |
| Subtitles | Show only delivered text, speaker label, locale/direction, safe-area preset | Rendered capture at 100/150/200%, HDR/SDR, long text, CJK, Arabic/Hebrew, offscreen fallback |
| Conversation | Commit after delivery | Fresh DB is empty; the one delivered turn appears with game/character/provider/audio provenance |
| Diagnostics | Refresh real checks | Timestamped runtime/broker/capture/provider/audio rows; no simulated timeline |

## Surface implementation map

| Old surface | New owner | Retain now | Defer/remove until backed |
| --- | --- | --- | --- |
| Home | Session Deck | Native health, selected state, real turn action/trace | Marketing hero, fixture headroom/memory/readiness |
| Games | World | Runtime profile summaries, synthetic target state | Static detected claims, inert card menus |
| Characters | World | Game-scoped Mara selection | Cross-game fixtures, add/edit/preview/memory controls |
| Conversation | Session Deck | Current delivered evidence | Invented sessions, quests, relationships, send/export |
| Presence | World + Settings | Capture diagnostics, subtitle preset | Fake tracking confidence, webcam, mouth toggles |
| Performance | Diagnostics | Unmeasured state | Illustrative CPU/GPU/latency values |
| Models | Voice & Models | Vault and real loadout CRUD | Unselectable research catalog in primary flow |
| Diagnostics | Diagnostics | Bootstrap, doctor, broker diagnostics | Fake provider rows/timeline/export |
| Settings | Settings & Guide | Persisted supported preferences | Placeholder toggles and false offline enforcement |
| Help | Settings & Guide | Real task navigation | Inert guide/support buttons |

## Subtitle and actor acceptance

- Subtitle presets include foreground, outline, shadow/backplate, opacity,
  speaker label, alignment, safe region, animation, HDR treatment, and DPI
  scale. Speaker changes never rely on color alone.
- Default subtitle text meets 4.5:1 contrast and can scale to 200% without
  horizontal page overflow.
- Delivered text carries language/direction metadata and bidi-isolates labels,
  numbers, models, and game terms.
- Actor-following placement is optional. Uncertain/offscreen tracking falls
  back to stable bottom-center within one refresh.
- A visual worker receives and returns the exact track/frame/timestamp. Stale,
  low-confidence, mismatched, occluded, or over-budget output is discarded.

## Release gate

No installer becomes a user-review candidate until:

1. the clean-state vertical slice above passes without terminal/file editing;
2. the control and synthetic target are Windows GUI subsystem executables;
3. runtime/broker/helpers create no visible console and write bounded logs;
4. every visible product control has a command, persistence effect, navigation
   outcome, or disabled reason;
5. fixture/developer evidence is absent from normal product navigation;
6. full frames plus 4–6 inspected close-ups exist for every retained screen at
   normal, narrow, 150%, and 200% scaling;
7. the exact installed binary is rerun from a clean local state.

