# Mara Venn realistic capture fixture

These images are original, fictional test assets generated for Interactive NPCs 2.0 with OpenAI's built-in image generation tool on 2026-09-01. They do not depict a real person and were not copied from a game, film, stock library, or third-party repository.

The fixture deliberately uses a neutral, unobstructed mouth and a stable frontal face. The executable embeds `mara-venn-portrait-v1.png`; the two three-quarter views are a small identity-reference gallery for later multi-view enrollment tests.

## Generation prompts

Primary portrait:

> Create a photorealistic 16:9 cinematic game screenshot for a fictional narrative game called Eclipse Harbor. Show one original adult woman, Mara Venn, centered from the upper chest upward in a harbor control room at dusk. She has a realistic human face, natural skin texture, dark wavy shoulder-length hair, a calm neutral expression, eyes toward camera, lips gently closed, and a completely unobstructed chin and mouth. Use soft practical light, believable depth of field, detailed AAA-game realism, and enough contrast for face tracking. No text, logos, UI, weapons, masks, hands near the face, glasses, hair across the mouth, exaggerated expression, visible speech, open mouth, extra people, or resemblance to a real public figure.

Three-quarter-left reference:

> Preserve the exact fictional identity, age, hair, clothing, lighting, and harbor-control-room setting from the provided Mara Venn portrait. Render the same person at a mild three-quarter-left head angle, eyes near camera, lips gently closed, neutral expression, and mouth/chin fully visible. Photorealistic game-cinematic still; no text, UI, extra people, face occlusion, or identity change.

Three-quarter-right reference:

> Preserve the exact fictional identity, age, hair, clothing, lighting, and harbor-control-room setting from the provided Mara Venn portrait. Render the same person at a mild three-quarter-right head angle, eyes near camera, lips gently closed, neutral expression, and mouth/chin fully visible. Photorealistic game-cinematic still; no text, UI, extra people, face occlusion, or identity change.

## SHA-256

- `mara-venn-portrait-v1.png`: `0ae10605199bb831c34a3be29c31a06f8a1d5a7e5cd07b8a1456866f2bb7dc2d`
- `mara-venn-reference-gallery/front.png`: `0ae10605199bb831c34a3be29c31a06f8a1d5a7e5cd07b8a1456866f2bb7dc2d`
- `mara-venn-reference-gallery/three-quarter-left.png`: `900a5b1a99e501010f3930e81b9aae343cd51112f7a3426a30311db799f1574b`
- `mara-venn-reference-gallery/three-quarter-right.png`: `b5f371c7cac36317aeb9c614e529168f0ac56bb16f48473c20de851571098633`

The generated source mouth is intentionally static. Animated rain and status indicators are restricted to the frame periphery so the self-test can require a pixel-identical mouth guard region across source frames.
