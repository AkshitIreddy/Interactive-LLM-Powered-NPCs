# Cyberpunk experience revision

Owner request: original game-like UI inspired by their Cyberpunk screenshots,
visual model selection, simpler language, Cyberpunk-only focus, private provider
setup, and scalable handling of characters without prepared mouth packs.

## Design and navigation

Four primary destinations: Channel (talk), Night City (game and character),
Loadout (models, voices and accounts), Settings (preferences and help).
Diagnostics moves under help/troubleshooting rather than occupying a primary tab.
The test game remains accessible as a separate practice environment.

Palette: ink #100b10, oxblood #29131c, signal red #ff626a, cyan #65e1e2,
paper #f1dfe0, muted #b799a4. Bahnschrift for display and control labels,
Segoe UI for reading, Cascadia Mono for small operational values. Fine angular
frames and a horizontal game-menu navigation replace the purple app sidebar.
No flashing, audio effects or disruptive screen-wide animation.

Signature: original anatomical illustration with selectable intelligence,
listening, voice, memory, vision and mouth components. Selection reveals one
configuration panel; advanced parameters and diagnostic provenance stay in
disclosures. All controls retain real persistence or explicit unavailable states.
Illustrations are generated assets; labels/buttons are accessible DOM elements.

## Current functional audit and changes

- App.tsx routes to ProductConsole; old Pages/Onboarding screens are legacy,
  not the live shell. Existing native bridges should be retained.
- Primary diagnostics tab exposes engineering detail too early: remove from
  main NAV, retain actionable support entry and deep-link compatibility.
- VoicePage/ProviderLoadoutEditor has real scope, native save/review and account
  actions, but stacked panels obscure primary decisions: replace its layout.
- WorldPage promotes the synthetic harbor ahead of the actual target and lists
  profiles without equivalent visual support: make Cyberpunk primary, practice
  environment secondary, retain other backend profile data without advertising it.
- SessionPage exposes receipt terminology in primary controls: shorten user-facing
  labels and preserve exact evidence in existing expandable details.
- Public package defaults must not contain owner credentials; private setup uses
  the existing native credential boundary and separate local state.

## Acceptance

Verify selection/save/reload, scope switching, provider/account navigation,
keyboard focus, narrow and scaled layout, no horizontal overflow, loading and
unavailable states, support access and Cyberpunk character selection. Inspect
headless screenshots of real rendered UI. Native provider setup and mouth fallback
need their own receipts; rendered browser state alone cannot qualify them.

Reference folder moved from Downloads to
`E:\temp\InteractiveNPCs\design-references\cyberpunk-20260908`.
The four JPEGs and AVIF are user-supplied inspiration, not redistributable UI assets.
Generated originals are in `apps/control/public/art`; provenance recorded alongside.

## Implemented review experience

- Neural loadout uses six selectable anatomical modules. Provider, model and
  voice remain ordinary keyboard-accessible controls, with custom voice IDs and
  route parameters behind disclosures. Existing native persistence is retained.
- Night City lists Cyberpunk characters, their dialogue/voice configuration and
  separate installed/active mouth-pack states. The practice game is collapsed.
- Settings uses compact label/value rows. Help still reaches troubleshooting;
  diagnostics URLs remain compatible without a primary navigation destination.
- Repeated per-attempt AssemblyAI approval was removed. Enable push-to-talk is
  the action, with the provider named beside it; native readiness and persisted
  routing preferences remain authoritative.
- The owner's Cyberpunk provider configuration is stored outside the package in
  the review application profile. Groq, AssemblyAI and Cartesia are active;
  Mistral and Gemini presets are also saved. Review credentials use the separate
  `interactive-npcs/v2/review` Windows vault namespace. The package contains no
  key values. Provisioning verifies configuration and credential presence, not
  a new paid provider request.

Headless frontend acceptance: 163 tests passed before the final Cyberpunk-first
startup refinement; production build passed. Rendered browser surfaces cover
Channel, Night City, Loadout, Settings and Setup, including 720px layouts. The
injected native World layout fixture additionally covers three viewport sizes
and active/installed/no-pack character states. These fixtures prove layout and
interaction contracts, not a live game connection. Evidence is under
`E:\temp\InteractiveNPCs\cyberpunk-ui-20260908`.

The source-only mouth renderer has separate Misty and Claire recorded-frame
comparisons. It preserves current-frame appearance and changes only mouth
support pixels. Its motion is intentionally modest; closed source lips cannot
provide unseen teeth/tongue appearance. The native activation integration and
final package acceptance are recorded separately when complete.
