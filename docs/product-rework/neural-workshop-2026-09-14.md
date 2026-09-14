# Neural workshop revision — 14 September 2026

The owner requested a structural whole-app UX rework with readable controls, styled menus, an immediately discoverable test game, substantial character content, and an original threatening machine-intelligence illustration.

## Loadout structure

The primary workspace contains a loadout toolbar, global/game/character inheritance selection, six system buttons, a provider/model editor, and the activation action. Loadout creation, naming, cloning and deletion live in the Manage loadouts dialog. Route notes and manual retry preferences live in Advanced routing. Both are accessible modal dialogs with keyboard focus containment and Escape restoration. Existing scope persistence, provider account actions, stock/custom voice selection, and protected native activation behavior remain exercised by interaction tests.

The head's anatomy connects cognition to cranial circuitry, memory to rear modules, hearing to an acoustic receiver, sight to the optic, and voice/mouth motion to the articulated speaker. A ResizeObserver calculates the active connection from the real button and contained-image bounds. The bitmap keeps its aspect ratio; labels are DOM buttons, not text baked into art.

Ordinary setup fits at 1440x900 and 1280x720 across all six roles without scrolling the main workspace or provider editor. Narrow or highly scaled layouts retain natural scrolling and readable controls. Long advanced detail can scroll inside its modal; it is not clipped to manufacture a fit result.

## Asset provenance

Asset: `apps/control/public/art/neural-interface-v2.png`.
Created and revised with the built-in image-generation tool on 14 September 2026. Original artwork, with no copied game UI, logos, labels, or character likeness. The final generated output was copied into the public asset path.

Initial prompt: Create an original premium cyberpunk configuration illustration: a three-quarter synthetic intelligence head facing right, no body or skeleton; smoked-glass cranial shell with coral neural circuitry, cyan optical sensor, circular acoustic receiver, articulated speaker membrane, rear memory modules and short neck connector. Center the head on a portrait black background with burgundy falloff. Use precise industrial materials and restrained cinematic lighting, no labels, UI, logos, arrows or floating objects. The anatomy must make cognition, sight, hearing, voice, mouth sync and memory connections meaningful.

Final edit direction: Preserve the composition and anatomical connection locations while making the machine intelligence cold and threatening: angular asymmetrical gunmetal armor, a narrowed red optic, predatory silhouette, respirator-like articulated speaker seam, glass cranial circuitry, and restrained coral lighting. Create an original design rather than reproducing Ultron or another existing robot character.

## Verification

- TypeScript build check passed.
- All 16 provider/loadout interaction tests passed after the structural changes, including exact provider routes, scoped creation/cloning, voice persistence, manual retry controls, and native activation rejection.
- `scripts/neural-workshop-fit-qa.cjs` passes 12 role/viewport cases. It fails on main/editor overflow, clipped role buttons, offscreen activation footer, or browser errors. Manage loadouts opens and restores keyboard focus after Escape; Advanced routing opens correctly.
- Inspected the integrated head and connection, voice editor, loadout manager and advanced dialog screenshots. Readable secondary-dialog typography and three-column route facts corrected the initial cramped presentation.

Evidence: `E:\temp\InteractiveNPCs\ui-refinement-20260914\fit`. These are headless browser interaction/layout checks; no native game or capture window was launched.
