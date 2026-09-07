# Independent functional and visual audit — pre-redesign UI

Date: 2026-09-05
Baseline branch: `feat/2.0-overhaul`
Baseline commit: `1f8fa78` (`docs(handoff): transfer 2.0 overhaul context`)
Audited entry point: `apps/control/src/App.tsx` → `ProductConsole`
Evidence root: `E:\temp\InteractiveNPCs\audit-20260905\baseline-before-redesign`

Snapshot notice: the verdict below describes the frozen pre-redesign build. The
reachable product has since been rebuilt and the later sections in this file
record its headless browser verification. The current visual route and native
activation remain separate gates; browser fixtures do not prove installed GUI,
capture, playback, or live-game behavior. See
[the reconciled review record](local-review-2026-09-05.md).

## Verdict

The pre-redesign control app is unusually careful about not presenting browser fixtures as native truth, and several native workspaces already have sound ownership boundaries. It is not yet a coherent first-run product. The default Session page leads with an invented character and a disabled primary action; World, Voice, Diagnostics, and Settings are dominated by native-unavailable panels; the one large browser-operable editor is hidden inside a six-thousand-pixel advanced disclosure; and first-run setup cannot advance at all in browser preview. In the native path, setup advances without requiring a selected synthetic target or provider readiness, and its final completion gate accepts any completed turn rather than specifically requiring live speech, subtitle, and delivery receipts.

The architecture contains two UI generations. `App.tsx` mounts `ProductConsole`; `Pages.tsx` and `Onboarding.tsx` are unreachable from the application entry point. Those orphaned files contain most of the fabricated metrics and inert controls previously identified in the handoff. They must be removed from the product build or placed behind an explicit specimen/demo boundary. Keeping them in the main source tree makes functional review ambiguous and invites accidental resurrection.

## Evidence and method

The baseline was frozen before redesign work began. The frontend production build passed, and its exact `dist` directory was copied to:

`E:\temp\InteractiveNPCs\audit-20260905\baseline-before-redesign\dist`

`asset-sha256.txt`, `git-head.txt`, and `git-status.txt` bind the capture to the baseline. Screens were served from that frozen copy on `http://127.0.0.1:1421`; later source edits cannot change the captured UI.

The audit used headless Microsoft Edge at 1440 × 900 with reduced motion. Each active destination has a full viewport capture plus five or six targeted closeups. `voice-advanced` opens the editor disclosure before capture. The harness waits for fonts and two animation frames, captures twice until pixels stabilize, records SHA-256 hashes, inventories controls, detects closed-disclosure descendants and modal-obscured controls, measures document/main overflow, and records browser console/page errors.

Artifacts:

- Render inventory and screenshot hashes: `E:\temp\InteractiveNPCs\audit-20260905\baseline-before-redesign\screens\report.json`
- Interaction report: `E:\temp\InteractiveNPCs\audit-20260905\baseline-before-redesign\interaction-report.json`
- Screens: `screens\session`, `screens\world`, `screens\voice`, `screens\voice-advanced`, `screens\diagnostics`, `screens\settings`, and `screens\setup-system`
- Repeatable render harness: `scripts/audit-current-ui.cjs`
- Repeatable browser interaction harness: `scripts/audit-current-ui-interactions.cjs`

The render pass produced zero console errors, zero uncaught page errors, and zero page/main horizontal-overflow flags. The interaction pass executed 27 checks with zero failures and two legitimate native-only blockers. The interaction pass used isolated browser contexts and did not invoke native mutation commands.

This is browser-preview evidence. Native command behavior was traced through the React handlers and `tauriBridge.ts`; it was not treated as live installed-app proof.

## The application that actually renders

`App.tsx` has one production path. The `specimen=theme` query opens `ThemeSpecimen`; every normal URL opens `ProductConsole`.

| Destination | URL | Active owner | Main child workspaces |
| --- | --- | --- | --- |
| Session | `?page=session` | `ProductConsole.SessionPage` | selected STT, turn execution, delivery ledger |
| World | `?page=world` | `ProductConsole.WorldPage` | `GameTargetWorkspace`, `CharacterDatabase`, identity status |
| Voice & models | `?page=voice` | `ProductConsole.VoicePage` | audio input, selected STT, provider credentials, `ProviderLoadoutEditor`, `LocalResourcePlanner`, `ThisPcBenchmark` |
| Diagnostics | `?page=diagnostics` | `ProductConsole.DiagnosticsPage` | diagnostics v1/v2, recovery matrix, export/settings |
| Settings & guide | `?page=settings` | `ProductConsole.SettingsPage` | `ProductPreferencesWorkspace`, audio output/input, guide |
| Guided setup | overlay opened by Setup or `onboarding=1` | `ProductConsole.OnboardingOverlay` | four-step system/world/voice/test flow |

Legacy query names map to these five destinations through `PAGE_ALIASES`; they do not revive the old pages. The map is useful for old links, but it hides how much the old information architecture has already been collapsed.

## First-run vertical slice

The setup overlay has four steps: System, World, Voice, Test. Its state comes from `bootstrap_snapshot` and the persisted onboarding snapshot returned through `loadNativeBootstrapHealth`. Each transition calls `save_onboarding`. Browser preview correctly refuses to fake that persistence; Continue stays on System and reports, “Open the native desktop app to save and continue guided setup.”

The native path still has four contract gaps:

1. **World selection is not a transition gate.** The World step always presents Eclipse Harbor as “Configured.” Continue does not require `captureProof`, a selected PID/HWND, a verified synthetic target, or a persisted character selection.
2. **Provider readiness is displayed but not a transition gate.** Role readiness is calculated for the grid, but the Voice step only blocks on persisted audio output and, when PTT is selected, audio input. It can continue with missing hosted credentials.
3. **The stock voice is not selected in setup.** The active loadout name is shown, but the user does not choose or verify a voice identity before proceeding.
4. **Completion is too broad.** The final gate is `deliveredTurn != null`. A deterministic fixture-only completion or a text-only completion can finish setup. The UI itself distinguishes receipt-backed audio and subtitle evidence, but the gate does not require either.

The desired first-run gate should bind a persisted synthetic target and character, a reviewed active route, credential and stock-voice readiness, a selected output, then one live turn whose generation carries audio submission/drain, subtitle, and delivery receipts. Typed input can remain an accessibility/recovery path, but “spoken turn complete” must mean audible receipt-backed output rather than any terminal event.

## Control-to-owner map

### Shared shell

| Control | UI state source | Persistence | Runtime effect | Classification |
| --- | --- | --- | --- | --- |
| Brand and five rail buttons | `ProductConsole.page` | `history.replaceState` query only | swaps active destination | Real navigation |
| Setup | `setupOpen`, `setupStep` | onboarding snapshot only after each native save | opens overlay | Real local action; native persistence on progression |
| Runtime chip | bootstrap snapshot or browser fallback | native bootstrap store | display only | Real status |
| Signal rail | turn/capture/character/provider state | mixed native/session | display only | Real status, visually over-prominent |

### Session

| Control | Owner and persistence | Runtime behavior | Classification |
| --- | --- | --- | --- |
| Synthetic review game / selected native character | local `turnWorldMode` | chooses next-turn world source; selected mode disabled until target and character exist | Real session choice |
| Typed message / PTT rehearsal | local `turnInputMode` | switches input contract | Real session choice |
| Transcript textarea | local `turnPrompt` | submitted to `start_simulation` only in native shell | Real input; blocked in default PTT mode |
| Egress consent | selected STT state | authorizes one microphone egress session | Real native consent; disabled in browser |
| Arm/cancel/retry PTT | `selectedStt*` bridge calls | start/status/cancel selected STT capture | Real native action |
| Send/stop turn | `start_simulation` / `cancel_simulation` | consumes typed text or a one-time STT receipt | Real native action |
| Show delivered subtitles | React app preference only | affects current presentation; not saved by this control | Real but ephemeral |
| Inspect world selection | page navigation | opens World | Real navigation |

Browser interaction proved that Typed message enables the transcript and accepts input, but Send stays disabled with an explicit desktop-runtime reason. Subtitle toggling and Inspect world navigation both work.

### World

| Control | Owner and persistence | Runtime behavior | Classification |
| --- | --- | --- | --- |
| Game profile selector | bootstrap game profiles + local selected ID | selection itself is session state | scopes target and character operations | Real; only one synthetic profile in preview |
| Discover running windows | `discover_game_targets` | no mutation | lists eligible native candidates | Real native action |
| Candidate selection + acknowledgement | React confirmation + `select_game_target` | persists exact target | binds exact PID/HWND after eligibility checks | Real native action |
| Clear binding | `clear_game_target` | clears persisted target | disables selected-world turns | Real native action |
| Start/poll/cancel actor picker | manual actor picker bridge | picker state native | selects the in-game actor | Real native action |
| Debug target capture / verification | task-owned synthetic capture commands | capture proof in session | establishes a review-only target | Real native action |
| Character inspect/select | character database commands | selected character persists natively | scopes turns and preferences | Real native action |
| Encounter correct/merge | encounter commands | native, explicit acknowledgement | mutates unknown-encounter records | Real native destructive action with confirmation |
| Character memory backup/list/erase/restore/remove | memory commands | native database/backups | mutates local memory only | Real native destructive action with two-step confirmation |
| Choose identity reference | no active picker callback in baseline | none | none | Visible future control; remove until functional |

Browser preview exposes no fake target or character mutation. That honesty is correct, but it leaves World as 2,431 pixels of explanation and disabled controls. The page needs a concise unavailable state plus one clear installed-app action, with advanced/native details progressively disclosed.

### Voice & models

| Control | Owner and persistence | Runtime behavior | Classification |
| --- | --- | --- | --- |
| Audio input selector/refresh | native audio input commands | selected endpoint persisted natively | binds PTT endpoint generation | Real native action |
| PTT controls | selected STT bridge | one capture session/receipt | real provider capture | Real native action |
| Add/validate provider credential | native credential bridge | Windows Credential Manager reference | enables hosted route readiness | Real native action |
| Save API-first preference | `save_onboarding` | onboarding preferences | affects later route preference | Real native action, oddly located |
| Advanced profiles disclosure | HTML details | none | reveals full editor | Real local action |
| Scope trace, create, clone, rename, delete | `ProviderLoadoutEditor` | native provider store when present; otherwise localStorage | edits inactive loadouts | Real, but browser persistence is explicitly non-authoritative |
| Six provider/model selectors + TTS voice ID | loadout draft | same split owner | changes next-turn route only after activation | Real |
| Manual fallback authorization/selectors | loadout draft | same split owner | exposes manual retry; automatic fallback stays off | Real |
| Review/offline/deactivate | protected provider bridge | native | resolves active inheritance without contacting providers | Real native action |
| Activate for next turn | provider bridge or browser store | native/browser preview | swaps at next turn boundary | Real; browser version must never be mistaken for native authority |
| Resource reserve/governor inputs | `LocalResourcePlanner` | native app data | changes soft admission policy | Real native action |
| Selected-loadout admission | native planner | native admission receipt | verifies exact pack revisions and current-device envelopes | Real native action |
| Trusted optional packs | signed native catalog + lifecycle | native | explicit install/repair/remove/cancel | Real native action with acknowledgement/license gates |
| Experimental visual pack | local-review native state | native | explicit debug install/repair/activate/remove | Real review-only action; should not occupy primary product flow |
| This PC benchmark inputs/start/cancel | benchmark commands | report stored natively | measures exact target/loadout | Real native action |

The headless interaction pass opened Advanced profiles, changed scopes, cloned and renamed a global loadout, changed and restored a provider, authorized a manual fallback, activated the clone in isolated browser storage, and confirmed that protected route review remains disabled outside the installed app. These controls work, but the UI does not give their ownership hierarchy a usable shape.

### Diagnostics

| Control | Owner and persistence | Runtime behavior | Classification |
| --- | --- | --- | --- |
| Refresh | diagnostic summary/v2/matrix/settings commands | read only | re-reads native evidence | Real native action |
| Export | `export_diagnostics_v2` | native local artifact | exports bounded diagnostics | Real native action |
| Verbosity | diagnostics v2 settings | native settings | changes future diagnostic detail | Real native preference |
| Matrix recovery actions | action-specific page navigation or native recovery handler | depends on action | attempts named repair path | Real when supplied by native matrix |

In browser preview all three main controls are disabled and the page renders large empty matrices. The page should lead with a compact health summary and a clear reason/action, then reveal raw receipts and matrices on demand.

### Settings & guide

| Control | Owner and persistence | Runtime behavior | Classification |
| --- | --- | --- | --- |
| Global/game/character preference scope | local scope selector | determines subsequent native document | changes inheritance target | Real local choice |
| Execution/performance presets and explicit overrides | native product preference snapshot | revision-locked atomic native save/reset | records intent; does not activate routes or packs | Real native preference |
| Subtitle style, safe area, scale, opacity, backplate | native subtitle manager | revision-locked native save/reset | changes validated renderer parameters | Real native preference |
| Effective configuration inspector | native projection | read only | explains winning owner/scope/default | Real native inspection |
| Audio output/input | native endpoint commands | native selected endpoint | binds output/input generation | Real native action |
| Guide search and six guide choices | React local state | none | filters/selects static help | Real local action |
| Guide CTA | page/setup navigation | none | opens relevant destination | Real navigation |

Browser interaction proved scope switching, guide filtering, guide selection, and the Diagnostics guide action. Native preference and endpoint controls correctly remain unavailable rather than fabricating state.

### Guided setup

| Control | Owner and persistence | Runtime behavior | Classification |
| --- | --- | --- | --- |
| Close | local overlay state | none | closes only when explicit reopen/completed state permits it | Real local action |
| Continue/Back | local step + `save_onboarding` | native onboarding store | advances only after persistence succeeds | Real native-persisted action |
| Synthetic target action | native capture command | session proof | selects/reselects review target | Real native action |
| Credential/audio actions | same native owners as Voice | native | establishes readiness | Real native action |
| Use typed setup turn | local preferences/input mode | saved on later transition | selects typed recovery path | Real local action |
| Run turn | simulation bridge | native runtime | executes selected route | Real native action; final gate is currently insufficient |

The explicit `onboarding=1` preview correctly offers Close. Continue produces a truthful native-required message and does not change steps.

## Visual findings from the frozen baseline

### Session

The initial viewport spends its strongest type and color on “Mara Venn,” even though this is only a review character and no target is bound. The default PTT path makes the main teal action disabled, but its filled styling still reads as primary and available. The first action a new user can actually complete sits below the fold. The left rail has tiny low-contrast copy and substantial unused space; the right column repeats status in large, mostly empty cards. The delivery ledger is a legitimate empty state, but its terminology and sparse asymmetry make the page feel like an internal instrument panel.

### World

The page begins with multiple paragraphs and unavailable controls before the useful explanation becomes clear. The synthetic scene reads as a generic placeholder. The facts grid has visible text collisions: loaded-content copy runs into `PERSONALITY` and `IDENTITY` labels, and memory-scope copy collides with adjacent cell labels. The memory panel is much larger than its current value. A “future native picker” control appears in the product UI despite having no handler.

### Voice & models

The collapsed page is 3,307 pixels high and mixes microphone capture, route summary, credentials, local hardware policy, model-pack lifecycle, and benchmarking. Large unavailable panels are more prominent than the decisions the user can make. Opening Advanced profiles expands the page to 6,182 pixels. The editor has a distinctive magenta/black system but visually belongs to a different application; six role cards and all manual fallback rows appear at once; small uppercase labels carry important meaning; “VIS” labels optional lip-sync and is opaque to a new user. The active route, credential readiness, stock voice, and test action should be one staged flow. Resource/pack administration and benchmarking belong behind separate advanced destinations.

### Diagnostics

At 1,296 pixels, the page is shorter than Voice but still dominated by empty matrices and receipt areas. Empty states are tiny islands inside oversized panels. The first useful action is disabled in browser preview and no concise next step anchors the screen. Health, failure, and repair should be scanned in seconds; evidence tables should support that summary rather than become the page.

### Settings & guide

Settings combines native preferences, audio routing, and a full guide in one 1,940-pixel stream. In the baseline capture near scroll position 1,040, audio input/output selectors and refresh controls visibly extend underneath the Guide column, and microphone metric cells are clipped by the column boundary. The global page has no horizontal overflow, so a width-only overflow assertion misses this internal overlap. “Rerun guided setup” breaks awkwardly across a large button. The guide search and actions work, but pairing them with dense preference inheritance creates two competing page purposes.

### Guided setup

The overlay is the cleanest surface. It has a stable dialog frame and a visible four-step sequence, but the System page is sparse and leads with implementation language such as “native boundary.” The small progress labels are hard to scan. In browser preview Continue looks available even though it can only return a blocker. The subsequent gates do not yet enforce the end-to-end outcome promised by the setup copy.

## Orphaned UI audit

### `Pages.tsx`

`Pages.tsx` is not imported by the active app. It imports `GAME_PROFILES`, `CHARACTERS`, `MODEL_PACKS`, and `RESPONSE_STAGES` from `data.ts`, so almost every confident metric or record in it is fixture data.

- Home: marketing hero, invented usage/coverage metrics, and inert banner buttons.
- Games: twenty static cards. Search and filter are local and real; add/edit/open actions have no handler.
- Characters: static cross-game cast and invented relationship/memory content. Local selection works; add/edit/preview/review actions are inert.
- Conversation: invented sessions, quest, relationship, and memory events. Trace/subtitle disclosures toggle locally; export/filter/session/archive/keep/dismiss/send controls are inert.
- Presence: fixed “96%” and “18 ms” claims; multiple toggles and the confidence slider have no state owner.
- Performance: fixture timings and resource values; only the mode preference changes local state.
- Models: static candidate cards, with no trusted native lifecycle ownership.
- Diagnostics: bootstrap data is mixed with fixture checks and a fabricated timeline; run/export/details controls are inert.
- Settings: a few local preferences change, but audio/overlay/storage/update/accessibility controls are placeholders and no native persistence is wired.
- Help: search works locally; topic actions are inert.
- Loading and empty pages: retry/add actions are inert.

### `Onboarding.tsx`

This separate ten-step onboarding is also unreachable. It keeps all state in React and never calls `save_onboarding`. Hardware, games, providers, microphone, presence, and performance are simulated or marked unavailable. The simulation step receives no setter capable of marking the run complete, so step eight cannot satisfy its own Continue gate. The final page nevertheless says preferences were saved, which is false. This component should be deleted or moved into a clearly named visual specimen package.

### `ProviderSettings.tsx`

`ProviderSettings.tsx` is imported only by orphaned `Pages.tsx`. Its credential prompt/test/delete operations are backed by the native bridge and its external links are real, but the surface is unreachable. Its provider definitions also include dated promotional and experimental offer text checked on 2026-08-28; that content should not be a durable source of product truth. Credential management should have one active owner in the redesigned Voice flow.

### `data.ts`

`data.ts` owns the static navigation, response-stage timings, twenty game cards, five characters, four model packs, and ten legacy onboarding steps. None of these records should feed production state. If kept for Storybook/specimens, rename the module and put a hard boundary around it.

## What to retain, replace, and remove

Retain:

- `ProductConsole` as the sole product entry point.
- Exact-target, character, memory, provider, audio, preference, diagnostics, benchmark, and resource native owners in `tauriBridge.ts`.
- Revision-locked preference/loadout persistence and two-step destructive confirmations.
- Explicit browser-preview disclosures and fail-closed native buttons.
- Turn-boundary route activation and manual-only provider fallback.

Replace:

- The five instrument-like pages with task-led screens centered on Connect world, Configure voice, Test character, and Repair.
- The monolithic Voice page with a short active-route summary and staged provider/voice selection; move packs/resources/benchmarking to advanced administration.
- The enormous Settings/Guide page with separate preferences and help surfaces or a compact contextual help drawer.
- The setup transition rules with receipt-backed gates for the whole vertical slice.
- Disabled filled primary buttons with visually unmistakable unavailable states and a direct remedy.
- The generic synthetic scene with a verified capture/character state that uses real thumbnail evidence when available.

Remove or quarantine:

- `Pages.tsx`, legacy `Onboarding.tsx`, and production imports of `data.ts`.
- Inert “future” controls and dated provider promotions.
- Fixture character/relationship/memory claims outside a clearly labelled synthetic review target.
- Repeated implementation prose when one concise status and an expandable evidence section suffice.

## Acceptance targets for the rebuild

Each active screen needs explicit loading, empty, error, blocked, ready, and in-progress states. Browser preview must remain truthful, but should still support navigation, layout review, local guide behavior, and clearly labelled non-authoritative route drafting.

The rebuilt UI should pass:

1. Every visible control is keyboard reachable, has a readable name, and maps to one documented state owner.
2. Every enabled control changes state, navigates, or invokes a bridge command; no inert buttons remain.
3. Native-only controls explain the missing prerequisite without resembling enabled primary actions.
4. Setup cannot finish until target, character, active provider/voice route, audio output, spoken delivery, and subtitle receipts belong to the same completed turn generation.
5. Browser preview never creates native-success claims; browser-local loadouts identify their storage and authority.
6. Wide, 720-pixel, 150%, and 200% zoom captures show no clipping, internal overlap, hidden primary action, or off-canvas dialog.
7. Every active destination receives one full screenshot and four to six inspected closeups per required viewport/zoom state.
8. Automated geometry checks include element-to-element overlap and clipped text, not only document `scrollWidth`.
9. The installed-app pass repeats control coverage against real native loading/error/ready states and records provider, audio, turn, subtitle, and delivery receipts.
10. The legacy fixture UI is absent from the production bundle.

## Staged redesign verification

The first revised Session and Voice build was checked from the live headless development server on port 1423 after the baseline was frozen. Evidence is under:

`E:\temp\InteractiveNPCs\audit-20260905\revised-session-voice`

The `wide` and `narrow-720` directories each contain full captures and five inspected closeups for Session PTT, Session typed input, and the Voice model-loadout section. Both passes reported zero console/page errors and zero document/main horizontal-overflow flags.

The redesign materially improves the baseline. The rail names are plain-language, disabled turn actions now look disabled, Session separates the conversation composer from its checklist, the 720-pixel layout reflows without horizontal clipping, and Voice replaces the six-thousand-pixel all-at-once editor with four clear workspace sections and a one-role-at-a-time editor. Optional visual routes, manual retry policy, native validation, and provider detail are appropriately collapsed.

Three issues remained in this staged build:

- At wide widths, the sticky `.product-topbar` background is translucent. When the main pane scrolls, underlying headings and controls ghost through it. This is visible in `wide\session\closeup-4-main-109.png` and `wide\voice\closeup-4-main-607.png`. Use an opaque background for the sticky layer.
- At 720 pixels, the two Connection checklist navigation actions concatenate as `Select game & character →Configure voice & models →`. Give the actions a block/flex layout with an explicit gap. This is visible in `narrow-720\session-typed\closeup-5.png`.
- Session says `Eclipse Harbor · not selected` while presenting Mara Venn as the active Actor and the main “Conversation channel.” The copy should mark Mara as the synthetic review default until a target/character binding is persisted, so fixture identity cannot read as detected native state.

## Final redesigned browser verification

The staged issues above were corrected and rechecked against the current live development build on port 1423. The sticky top bar is now opaque, the Connection checklist links have distinct spacing, and the Session status language reads `Mara Venn · preview` and `Stock voice configured`. The navigation now exposes full labels at 720 pixels, 150%, and 200% after aligning the responsive breakpoint and removing the inherited fixed-height/clipped shape. Original-resolution evidence is stored under:

- `E:\temp\InteractiveNPCs\audit-20260905\revised-full\final-nav-720`
- `E:\temp\InteractiveNPCs\audit-20260905\revised-full\final-nav-150`
- `E:\temp\InteractiveNPCs\audit-20260905\revised-full\final-nav-200`

The final wide Voice screen moves connection/route metadata into a closed disclosure and places Reply model, Speech recognition, Character voice, and Memory embeddings in the first viewport. The single loadout no longer stretches to an artificial page height. The character-voice stock-discovery disclosure has readable padding and states the native/private-evaluation boundary directly. Final Voice evidence, including five distinct closeups and the stock-voice disclosure, is under:

- `E:\temp\InteractiveNPCs\audit-20260905\revised-full\voice-latest-wide`
- `E:\temp\InteractiveNPCs\audit-20260905\revised-full\voice-latest-320`

The Settings section-navigation labels and headings were rechecked after their final typography adjustment. Preferences, Audio devices, and Help remain distinct and readable at wide and 200% layouts, with no internal overlap. Evidence is under `E:\temp\InteractiveNPCs\audit-20260905\revised-full\settings-latest-wide` and `settings-latest-scaled`.

The complete final matrix retained unchanged evidence for all five main pages plus Setup at wide, 720 pixels, 150%, and 200%, with every recorded surface containing one full screenshot plus five closeups. Across 198 screenshots the harness recorded zero page errors, zero console errors, and zero document/main horizontal-overflow failures. Changed screens were recaptured separately after each final CSS/content adjustment, so the final evidence roots above supersede the corresponding earlier matrix images.

The actual Tauri minimum-width boundary was also tested at 200% scaling: a 640-pixel physical viewport yielding 320 CSS pixels. Session PTT, Session typed input, Voice model loadout, and Settings produced 25 screenshots under `E:\temp\InteractiveNPCs\audit-20260905\revised-full\logical-320-final`, with no horizontal overflow or browser errors. Voice role cards, provider/model selects, action buttons, and Settings scope tabs reflow into usable stacked layouts. An initial pass found that the Session top status and signal values truncated meaningful suffixes. The final below-380-pixel wrapping rule was then checked at `E:\temp\InteractiveNPCs\audit-20260905\revised-full\logical-320-session-final`: `native actions unavailable`, `not selected`, `preview`, and `configured` now remain fully visible, with no overflow or browser errors.

The final interaction audit is `E:\temp\InteractiveNPCs\audit-20260905\revised-full\interaction-report-final.json`: 36 checks passed with no failures. Three controls were recorded as blocked by design because browser preview cannot supply the native runtime: sending a typed turn, persisting character-scoped subtitle state, and connecting a credential. The audit exercised all five primary routes, the Session source/mode/disclosures/setup dialog, all four Voice sections, all three loadout scopes, clone/rename, each essential role, advanced disclosures, and the provider-account handoff.

Keyboard verification is recorded in `keyboard-wide-final.json` and `keyboard-logical-320-final.json` in the same evidence root. On every main page and Setup, every visible enabled focusable control appeared in the Tab sequence, each focused control scrolled into view and retained a visible focus indicator, and Setup focus stayed inside its modal. Both runs completed without page or console errors.

Forced-colors rendering was captured for Session, Voice, and Settings under `E:\temp\InteractiveNPCs\audit-20260905\revised-full\forced-colors-final`. System colors preserve borders, hierarchy, focus/selection state, and readable enabled-versus-disabled controls, with no horizontal overflow or browser errors. Native loading, degraded, and recovery states cannot be produced honestly by browser preview; they remain an installed-app verification gate rather than a mocked browser claim.

## Final provider catalog and setup verification

The final provider/default changes were checked separately without replacing the 198-screen matrix above. Fresh evidence is under `E:\temp\InteractiveNPCs\audit-20260905\provider-final`, covering Voice & models, Accounts, Session typed input, and Guided setup at 1440 × 900, 720 × 900, and a 320 × 450 CSS-pixel viewport rendered at 200% device scale.

The focused audit passed 104 of 104 checks with zero browser/page errors and zero horizontal overflow. It proved the fresh Groq Qwen 3.6 non-reasoning, AssemblyAI Universal-3 Pro, Cartesia Sonic 3.6/Greg, local SQLite FTS5, and visual-off defaults. It exercised every qualified Gemini, Groq, Mistral, OpenRouter, and Cohere reply route; the exact Cartesia Greg, Deepgram Arcas, and Inworld Dennis voice routes; all 13 operable account families; and the deliberate absence of Cloudflare and generic OpenAI-compatible reply routes.

A seeded existing browser loadout retained its ID, name, revision, and prior OpenAI, ElevenLabs, AssemblyAI, FTS, and visual-off choices when the expanded catalogs loaded. An explicit later switch to Mistral persisted across reload, incremented the revision, and left the other saved routes unchanged. This verifies that new providers supply fresh-profile defaults and choices without resetting an existing user's loadout.

Keyboard verification covered 15 affected states and 264 visible enabled controls. Every control appeared in the Tab sequence, entered the viewport, and retained a visible focus indicator; modal focus never escaped. The general interaction audit still passes 36 of 36 checks with the same three truthful native-only blockers. The frozen frontend suite passes 143 of 143 tests, TypeScript type checking, and the Prettier format check.

The first focused pass found two real setup-only responsive defects: the wide modal retained a two-column role picker inside a 620-pixel content region, and the 320-pixel account card overflowed by 11 pixels. Setup-specific single-column rules and bounded wrapping corrected both. The final inspected captures are `wide\setup-models-scrolled.png` and `logical320\setup-accounts-scrolled.png`; both now fit cleanly.

Browser preview intentionally cannot persist onboarding or advance through native-gated setup transitions. For visual and keyboard inspection only, Setup step 3 was rendered through a response-local initialization hook in the headless browser. Product source, native persistence, credentials, provider calls, and audio were not modified or simulated. Native onboarding transition and receipt behavior remains covered by the passing native-evidence frontend tests. The full focused report is `E:\temp\InteractiveNPCs\audit-20260905\provider-final\provider-final-report.json`, with a concise evidence index in `provider-final-summary.md`.
