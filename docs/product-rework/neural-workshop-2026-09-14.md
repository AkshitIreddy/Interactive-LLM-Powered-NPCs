# Neural workshop revision — 14 September 2026

The owner reported unstyled scrollbars/dropdowns, wasted space, an unsuitable skeleton map, an undiscoverable test game, and sparse character content/packs.

## Interface changes

The loadout now uses an original synthetic intelligence head. Cognition connects to brain circuitry, memory to rear modules, hearing to the acoustic receiver, sight to the optical sensor, and voice/mouth motion to the speaker and articulated mouth. Only the selected connection is drawn. All six capabilities remain keyboard-operable DOM buttons; the art does not encode interactive text.

Saved loadouts use a horizontal strip, repeated page chrome is smaller, and the model editor shares the main bay with the system map. Narrow windows stack the panels. The global/game/character inheritance controls, provider choices, account actions, and model configuration retain their existing persistence behavior.

The Games page exposes the included practice-game launch card directly. The older practice disclosure retains detailed capture inspection. Launch requires a user click; no game windows were opened during this work.

## Asset provenance

Asset: `apps/control/public/art/neural-interface-v2.png`.
Created with the built-in image-generation tool, not the API fallback, on 14 September 2026. Original art with no copied game UI, logos, or text. The source output was copied into the app's public assets.

Prompt: Create an original premium cyberpunk configuration illustration: a three-quarter synthetic intelligence head facing right, no body or skeleton; smoked-glass cranial shell with coral neural circuitry, cyan optical sensor, circular acoustic receiver, articulated speaker membrane, rear memory modules and short neck connector. Center the head on a portrait black background with burgundy falloff. Use precise industrial materials and restrained cinematic lighting, no labels, UI, logos, arrows or floating objects. The anatomy must make cognition, sight, hearing, voice, mouth sync and memory connections meaningful.

## Headless verification

TypeScript checking and all 178 frontend tests passed in the first integrated pass. Headless Edge screenshots at 1600px and 720px show the new system map and compact library; full-page captures and close-ups were inspected. The existing eight-screen harness reports no body/main horizontal overflow or page errors. More changes to character content, native game status and packaging are being verified separately; these are browser-level UI results, not a live game/capture claim.

Local evidence: `E:\temp\InteractiveNPCs\ui-refinement-20260914`.
