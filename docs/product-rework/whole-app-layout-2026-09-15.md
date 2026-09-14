# Whole-app workspace revision — 15 September 2026

## Resulting interaction structure

The owner requested efficient use of space throughout the app, rather than smaller text or clipped scrolling pages. Main desktop workspaces now separate selection, editing and actions:

- **Channel:** compact status rail, conversation/composer workspace, connection summary and secondary evidence disclosures. Push-to-talk and sending remain in view.
- **Games:** a scrolling character roster beside a persistent inspector; direct included-game launch; connected-game actions for NPC selection, capture checking and disconnect. Story/voice, customization, mouth packs, memory, content packs and practice details open focused accessible dialogs.
- **Loadout:** six anatomical system choices beside provider/model controls; persistent activation footer. Loadout management and advanced routing are dialogs. Accounts, microphone and local models retain their own workspaces. Microphone device selection sits beside push-to-talk. Local models has Tracking, PC budget, Downloads, Benchmark and Advanced packs sections.
- **Settings:** one outer section list and a category bar. Conversation settings use Response, Interaction and Presence groups; save/reset/reload stay in the command row. Scope selection remains explicit. Subtitles and effective configuration have separate editors.

The original generated neural head and the provider/loadout work are documented in `neural-workshop-2026-09-14.md`. Dropdown popups, triggers and scrollbars use the app's shared control theme.

## Behavior preserved and defects corrected

Dialogs use React Aria focus containment, background hiding, Escape dismissal and focus restoration. Long biographies retain paragraph breaks. Character initials are decorative to screen readers; full names remain readable. Provider/native state and scope persistence continue through their existing typed bridges.

Testing identified issues beyond simple overflow counts: clipped status text, hidden settings collisions, truncated roster names, inaccessible Local models content, a capture error lost when a target became selected, and verification feedback behind the practice dialog. These were corrected in the layout and feedback ownership. Main notices are overlays so they do not shift fixed workspaces; practice feedback appears inside its dialog.

## Reproducible verification

With the headless Vite preview on port 1426:

- `node scripts/neural-workshop-fit-qa.cjs`: all six model roles at 1440x900 and 1280x720, editor/footer/button containment and modal keyboard behavior.
- `node scripts/secondary-loadout-fit-qa.cjs`: Accounts, Microphone and all local-model sections at both sizes, downloads handoff and execution-dialog focus.
- `node scripts/workspace-layout-fit-qa.cjs`: normal whole-app composition and dialog interactions.
- `node scripts/workspace-layout-populated-qa.cjs`: injected native-contract data using the actual 37-entry Cyberpunk profile, populated settings groups and biography dialog. These are explicit layout fixtures, not live native evidence.
- The frontend Vitest suite covers persistence, provider boundaries, restored selections, error feedback and capture-proof rejection. TypeScript checking verifies the integrated source.

Evidence is under `E:\temp\InteractiveNPCs\ui-refinement-20260914`, including `final-frontend-tests.json`. Narrow/200%-equivalent viewport and forced-colors checks are under `accessibility`.

Ordinary desktop controls fit without scrolling the main page. Long rosters, detailed model catalogs, advanced content and highly scaled/narrow windows retain accessible scrolling. No visible app, game, capture, audio or flashing-window test was run. Natural/live-game mouth quality remains a separate qualification from UI and private pack preparation.
