# Initial game-profile selection rationale

The initial 20 profiles cover different engine eras, dialogue modes, storefronts, capture conditions and safety risks. Selection is not a promise that every game supports automatic identity or face animation; every profile ships complete authored data and honest capability/fallback evidence. None requires a mod.

| Wave | Game/profile | Why it exercises the architecture | Minimum dependable fallback |
| --- | --- | --- | --- |
| A | Skyrim Special Edition | Many named/background NPCs and an older rendering/UI stack | Explicit target + audio/subtitles |
| A | Cyberpunk 2077 | Modern rendering, moving cinematic/ambient NPCs, preserves prototype lineage | Explicit target + audio/subtitles |
| A | Baldur’s Gate 3 | Cinematic dialogue and strong named-character context | Explicit target + audio/subtitles |
| B | Fallout 4 | NPC-dense world with different lore/UI and capture conditions | Explicit target + audio/subtitles |
| B | Fallout: New Vegas | Older engine/UI and lower-end hardware | Explicit target + audio/subtitles |
| B | The Witcher 3 | Third-person/cinematic dialogue and named-character corpus | Explicit target + audio/subtitles |
| B | Starfield | Modern Bethesda engine and dense world/state scope | Explicit target + audio/subtitles |
| B | Mount & Blade II: Bannerlord | Many procedural nobles/troops and encounter identity | Explicit target + audio/subtitles |
| B | Kingdom Come: Deliverance II | Realistic presentation, moving/occluded faces | Explicit target + audio/subtitles |
| B | Oblivion Remastered | Modernized presentation with classic NPC-heavy structure | Explicit target + audio/subtitles |
| C | The Sims 4 | Procedural characters, relationship/memory emphasis | Explicit target + audio/subtitles |
| C | Stardew Valley | 2D portraits/text dialogue proves non-face generic path | Explicit target + audio/subtitles |
| C | Minecraft Java Edition | Generated/entity identities and a non-cinematic presentation | Explicit target + audio/subtitles |
| C | Divinity: Original Sin 2 | Party/cinematic dialogue and lore scoping | Explicit target + audio/subtitles |
| C | Mass Effect Legendary Edition | Trilogy/save/spoiler scope and cinematic dialogue | Explicit target + audio/subtitles |
| C | Dragon Age: Inquisition | Party relationships and cinematic interactions | Explicit target + audio/subtitles |
| C | Kenshi | Many emergent/background NPCs and persistent encounters | Explicit target + audio/subtitles |
| Risk | Red Dead Redemption 2 Story Mode | High-fidelity capture/occlusion; strict story/offline boundary | Story-mode audio/subtitles only unless safe capability proven |
| Risk | Grand Theft Auto V Story Mode | Strong protected-online adjacency requiring refusal tests | Story-mode audio/subtitles only unless safe capability proven |
| Risk | Elden Ring offline | Anti-cheat/offline boundary and sparse dialogue | Offline audio/subtitles; capture/overlay fails closed on ambiguity |

## Profile completeness gate

Every profile must include detection, build/process rules, safety policy, capture/UI exclusions, world lore, biographies, personality/style, prompt policy, important characters, background rules, model/voice defaults, identity strategy, diagnostic checks, troubleshooting, provenance and deterministic validation. Placeholder prose, copied wiki text, game images/audio and performer clones fail the gate.

Replay verification and live-game certification are separate evidence labels. Replay can validate detection/tracking/composition transforms but cannot prove install discovery, live state, anti-cheat safety or frame impact beside the real game.
