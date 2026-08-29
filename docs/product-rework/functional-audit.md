# Product rework functional audit

Status: implementation baseline  
Date: 2026-08-29  
Source of truth: `see me.md`

## Executive finding

The current shell contains a credible authenticated Tauri-to-runtime boundary,
real provider-credential references, atomic onboarding/loadout stores, a real
debug WGC target-selection path, and an honest deterministic runtime fixture.
It does not contain one connected product flow. Most visible product state is
static presentation data, several enabled controls are inert, and the one
native “completed” turn uses zero-filled fixture PCM consumed by a non-device
sink. A completed fixture is therefore not evidence of audible speech.

Broad UI work must start from the real seams below. Any surface not backed by
one of them is removed, disabled with a specific reason, or quarantined as
developer evidence.

## Classification

- **Working**: produces a meaningful tested local/native effect.
- **Fixture-only**: deterministic demonstration data with no product claim.
- **Inert**: enabled control with no handler or a no-op handler.
- **Misleading**: wording implies persistence, measurement, or enforcement
  that is absent.
- **Missing**: required capability has no connected command/data path.

## Native seams that are real today

| Seam | Current outcome | Limits |
| --- | --- | --- |
| `bootstrap_snapshot` | Authenticated runtime/broker health, cached profile/model summaries, onboarding state, provider credential presence, safety flags, and diagnostic summary | Several values are cached; screen/microphone capabilities remain false; no selected target/character/voice contract |
| `save_onboarding` | Validates and atomically persists `onboarding-v1.json` in private app data | Frontend never called it; schema omits character, loadout, voice, device, subtitle preset, and test evidence |
| Provider credentials | Windows Credential Manager reference status, native secure prompt, delete, and contract-only setup check | “Check setup” performs no provider request and returns no model/voice evidence |
| Provider loadouts | Real bounded JSON persistence, recovery, CRUD, global/game/character inheritance, route validation | Runtime simulation never resolves or consumes the active loadout; no stock voice binding in the loadout |
| Runtime fixture | Authenticated typed events, cancellation, scoped SQLite memory, deterministic Mara reply | Always fixture-only; fixture TTS is silent PCM and fixture audio only counts frames |
| Synthetic target selection | Exact debug metadata schema, PID/HWND/basename checks, broker policy, WGC counters | Debug-only, separate from setup/session, no launch/discovery action, no frame-delta proof, no path/hash identity |
| Runtime doctor/broker diagnostics | Real bounded native checks and counters | Existing frontend does not call them; broker block reason and several recovery fields are dropped |

## Global shell

| Item | Class | Evidence and disposition |
| --- | --- | --- |
| Sidebar/page navigation, URL routing, mobile scrim | Working | Keep, but reduce the top-level IA to jobs rather than subsystem pages. |
| Ctrl+K palette and controller focus navigation | Working | Keep; palette actions must resolve to real navigation/actions only. |
| Bootstrap polling and runtime/broker status | Working | Keep and refresh on demand. |
| Response Spine | Mixed | Native events are real but the normal result is still a silent fixture. Move to a compact turn trace and label the exact integration mode. |
| Open profile menu, notifications, minimize-to-tray | Inert | Remove until commands exist. |
| Query-string demo state controls | Fixture-only | Keep outside product UI as developer-only visual QA. |

## Screen audit

### Onboarding

- Navigation is working locally.
- Hardware “scan,” microphone rehearsal, performance verdict, and 1.31-second
  result are timer/static fixtures.
- Game/provider/voice/device choices are not connected; several buttons are
  inert.
- Preferences are React-only and “Preferences saved” is false.
- Onboarding is opened only by `?onboarding=1`, not persisted first-run state.

Disposition: replace with a shorter readiness path that loads native bootstrap,
persists every completed step, exposes the synthetic target, selects Mara,
reviews the active provider/stock-voice route, and runs either a real qualified
turn or one explicit blocker. Do not retain simulated scans.

### Home

- Native bootstrap health and Run/Cancel deterministic simulation work.
- Debug synthetic capture control invokes real commands.
- Hero profile, memory, performance, timing, route, and readiness panels are
  fixture presentation.
- Choose game, open conversation, and open diagnostics are inert.

Disposition: replace the marketing hero with a compact Session Deck. First
viewport contains selected target, character, capture state, provider/voice,
PTT, subtitles, and one Start/Stop action. All CTAs navigate or execute.

### Games

- Search/filter over static seeds works.
- Scan is disabled; profile/menu/setup actions are inert.
- `runtime_profile_summaries` exists but returns only identifiers/names.
- No launch/discovery/manual target command exists.

Disposition: show the synthetic target first and runtime profile packages
second. A profile detail shows only runtime-returned package/safety/fallback
data. Static authored catalog entries are not selectable as detected games.

### Characters

- Selection swaps one hard-coded cross-game fixture list.
- Search is not wired; add/edit/preview/memory actions are inert.
- Runtime can validate a supplied character ID for authored profiles, but the
  UI cannot fetch full character data and the synthetic route is manual.

Disposition: game scope is mandatory. For the vertical slice, Eclipse Harbor
contains only Mara Venn with explicit-selection evidence, the exact fixture
prompt boundary, a stock-voice binding, and an empty/delivered memory state.

### Conversation

- The most recent native delivered fixture text can be retained in React.
- Trace visibility and ephemeral subtitle preference work.
- Older sessions/counts, memory proposals, quest/relationship language,
  send/export/filter/archive controls are fixture or inert.
- Runtime SQLite delivery exists but has no list/read/edit/delete command.

Disposition: show only current-turn delivered evidence until a ledger query is
available. Fresh state is empty. Remove invented history and unsupported facts.

### Presence

- The debug capture probe is real; the page-level WGC/identity/frame values are
  static.
- “Read selected game window” only toggles React state.
- Tracking, context, camera, confidence, and mouth-motion controls are inert or
  incorrectly coupled.

Disposition: merge into World/Session for capture and Settings for subtitle
preferences. Keep visual response visibly unavailable until actor tracking,
identity locking, frame ownership, and a qualified residual exist.

### Performance

- Every hardware/headroom/latency/admission value is illustrative.
- Mode choice is ephemeral and not enforced.
- Benchmark action is disabled.

Disposition: merge into Diagnostics as an unmeasured state. Do not show a
number until a timestamped bounded benchmark command produced it.

### Models

- Three visual candidates are static and correctly unavailable.
- Provider/loadout functionality elsewhere is materially real.
- Native model summaries are static inventory metadata and unused.

Disposition: make Models the provider/loadout/voice workflow. Put unqualified
local visual research in a collapsed developer evidence section, never beside
selectable routes.

### Diagnostics

- Runtime and broker status rows can come from bootstrap.
- Provider rows, retry timer, timeline, environment, and most measurements are
  fixture copy.
- Run checks, retry, row details, trace, and export are inert despite existing
  native doctor/diagnostic commands.

Disposition: wire bounded refresh/doctor/broker calls, timestamps, provenance,
and plain-language remediation. Export stays absent until the redacted bundle
command exists.

### Settings

- Section navigation and several React preferences work only for the session.
- “Offline mode” does not enforce an egress boundary.
- Audio/overlay/storage/update sections are placeholder toggles.
- Provider vault and provider loadout editor are working and should remain.

Disposition: retain only persisted onboarding preferences and provider/loadout
configuration. Replace placeholders with explicit capability states; remove
inert switches. Add rerun/reset onboarding.

### Help

- Search filters static titles.
- Guides, quick actions, bundle, and release notes are inert.

Disposition: provide a small local task guide whose actions navigate to real
surfaces. Plain text replaces unavailable buttons.

## Windows launch and capture audit

The installed review binaries currently report:

| Executable | PE subsystem | Expected behavior |
| --- | --- | --- |
| `interactive-npcs-control.exe` | Windows CUI | Defect: Explorer launch opens a console in Debug packages |
| `npc-runtime.exe` | Windows CUI | Acceptable only as a supervised `CREATE_NO_WINDOW` child |
| `npc-media-broker.exe` | Windows CUI | Acceptable only as a supervised hidden child |
| `interactive-npcs-synthetic-target.exe` | Windows GUI | Correct |

The control entry point applies the GUI subsystem only outside debug builds.
All recorded hands-on packages are Debug, so the console is deterministic.
Runtime and broker launches already use no-window creation flags, but neither
has a bounded app-owned structured stderr log path.

Synthetic target validation already checks metadata consistency, PID/HWND,
same user/session, visibility, protected-process/anti-cheat policy, executable
basename, and loaded modules. It does not check a canonical image path/hash,
metadata freshness, or a positive counter delta tied to a new selection
generation. Target restart requires manual clear.

## API qualification evidence

An explicitly authorized private ElevenLabs stock-voice request succeeded on
2026-08-29. Credential authentication returned HTTP 200. The bounded 13
character synthesis returned 24,703 MP3 bytes in 2735.2 ms. Decoded audio was
mono 24 kHz, 1.486 seconds, non-silent, unclipped, peak 0.758392, and RMS
0.100684. Temporary audio was deleted and no remotely addressable artifact was
created. This proves provider synthesis only; it does not prove runtime-host
wiring, speaker playback, subtitles, or lip-sync.

## Immediate release blockers

1. First-run state is not consumed by the frontend.
2. Synthetic launch/discovery/capture and turn start are separate debug paths.
3. Selected character and stock voice are not persisted end to end.
4. Provider loadouts are not consumed by runtime simulation.
5. “Completed” fixture turns are silent.
6. No normal runtime PCM-to-speaker or microphone/PTT-to-STT path exists.
7. No delivered-turn ledger read API exists.
8. Debug control packages open a console.
9. Real diagnostics/benchmark/help actions are not connected.

