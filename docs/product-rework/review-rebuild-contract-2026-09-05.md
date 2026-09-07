# Independent review rebuild contract

Baseline: `1f8fa78`, branch `feat/2.0-overhaul`, clean at inspection on 2026-09-05.
This is an implementation and acceptance map, not a completion report.

Snapshot notice: this contract records the September 5 starting conditions.
Follow-up implementation and measurements are reconciled in
[the local review record](local-review-2026-09-05.md). The v17 test game is
verified; final source tests, optional-pack activation, and the v17 application
manifest remain open until the source freeze.

## Current source audit

The handoff's description of App/Pages is stale: App mounts ProductConsole.
Pages and the standalone Onboarding retain obsolete fixtures but are not the
ordinary entrypoint. Rebuilding those would not fix the installed experience.
ProductConsole has real native bootstrap, persisted onboarding, provider
loadouts, game-scoped inspection, audio endpoint selection, delivered events,
and diagnostics. Preserve those connections and verify their native consumers.

Concrete defects in the reachable source:

| Surface | Finding | Action and acceptance |
|---|---|---|
| Shell | Five-node signal rail repeats technical state on every page; numeric navigation encodes no meaningful sequence | Make persistent navigation recognizable and keep the session readiness rail on Session only; keyboard navigation names the current page |
| Session | Primary viewport spends multiple paragraphs on route authority and experimental visual machinery before Send; permanent MV seal is wrong for another character | Put character, input, Send, and capture/voice actions first; derive initials from selected identity; technical receipt panels expand on demand |
| Signal rail | TTS readiness is hard-coded to ElevenLabs credential presence even when a different route is configured; actor always ready | Use selected effective route for provider/voice status; distinguish selected identity from tracked identity |
| World | Identity enrollment button has no handler and can become enabled by a status flag | Remove the inert picker; expose only actual enrollment status until an implementation exists |
| Voice | The only actual route editor is hidden under Advanced below microphone, illustrative recommendation cards, accounts, and inventory | Route editor becomes first-class; accounts and audio have clear section navigation; installability remains evidence based |
| Onboarding | Provider step cannot edit loadouts; system language is developer-facing; modal has no focus containment; finish condition accepts any completed fixture | Expose same real editor during setup, focus containment and escape/return behavior, persist incomplete setup when deferred, require live audio delivery for successful finish |
| Settings | Saving the cloud preference calls nowSnapshot(true) and can falsely complete setup | Preserve actual onboarding completion and step when changing one preference |
| Diagnostics | Multiple repeating empty panels and raw check IDs dominate, while actual actions are present | One health view with measured results, optional event/provenance detail and actionable empty state |

Detailed per-control trace and native/legacy findings are recorded in the
independent audit companion documents. Existing tests are behavioral evidence;
screenshots must still be inspected after rework.

## Information architecture and visual direction

Five destinations: Session, Games & characters, Voice & models, Diagnostics,
Settings & help. Each has one main task and section navigation for large
workspaces. Session owns current delivered dialogue; Games owns target and
identity; Voice owns actual model choice; Diagnostics owns provenance.

Design subject: a compact communications console used beside a running game.
Palette: ink violet `#11101a`, graphite `#1c1a28`, signal mint `#7de3ca`,
electric lilac `#b8a1ff`, warm amber `#f1cb87`, readable ivory `#f2eff7`.
Use restrained Bahnschrift display, Segoe UI body, and Consolas utility with
Windows fallbacks; these are system fonts, not redistributed assets. Signature:
an asymmetric call panel with a clear character identity strip and a narrow
connection checklist. No flashing effects, animated backgrounds or sounds.

This retains the user's controlled-neon gaming direction while replacing the
all-cyan technical dashboard. Typography and structural color carry identity;
decorative hardware statistics, fake waveforms and giant hero copy do not.

## Acceptance before local review packaging

1. Source-backed audits and current alternatives ledger exist.
2. Native first-run state opens setup; provider configuration is reachable
   there; incomplete configuration remains incomplete across restart.
3. Synthetic target discovery and exact PID/HWND validation produce advancing
   captured frames or a concrete failure, without visible helper consoles.
4. Selected character and provider routes drive the actual turn; only delivered
   dialogue is shown. Qualify real provider synthesis, route consumption, and
   cancellation headlessly. A file/null sink must never manufacture a physical
   audio-drain receipt. Physical playback and visible WGC presentation remain
   separate gates while the user's display/audio restriction is active.
5. Every retained action has navigation, persistence, or a real native command.
6. Every screen has inspected full/close-up headless renders at normal/narrow
   and 150/200% scale; keyboard focus and no horizontal overflow are checked.
7. Moving-mouth methods compete on the same fresh moving frame sequence.
   Natural anatomy, temporal stability and identity preservation must pass
   visual inspection. Passing a geometric score alone does not qualify a path.
8. After source settles and executable headless checks pass, package a fresh
   source-identified local app and test game under E:\temp. Include the exact
   remaining native and visual gates with the artifact; unqualified optional
   renderers stay disabled. This follows the handoff's explicit fresh-review
   requirement and does not promote the artifact to production acceptance.
   No push, tag, release or updater activation.

## Primary design sources opened 2026-09-05

- [Microsoft settings guidance](https://learn.microsoft.com/en-us/windows/apps/design/app-settings/guidelines-for-app-settings): everyday workflow commands belong with the workflow; group less-used settings under expanders. Updated 2026-04-15.
- [Microsoft navigation basics](https://learn.microsoft.com/en-us/windows/apps/design/basics/navigation-basics): consistent location and hierarchy across destinations.
- [Windows app best practices](https://learn.microsoft.com/en-us/windows/apps/get-started/best-practices): verify adaptive layout at differing DPI/window dimensions.
- [WAI modal dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/): contain Tab focus, set meaningful initial focus, Escape closes, restore trigger focus.

These are design constraints, not evidence that this application meets them.
