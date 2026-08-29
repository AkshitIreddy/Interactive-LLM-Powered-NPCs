# Supported game profiles

The current checkout contains **20 fully authored, schema-valid profiles**, not 20 identical integrations. “Authored” means complete declarative content, detection, characters, prompts, defaults, identity strategy, diagnostics, troubleshooting, safety, and provenance. “Replay-verified” means deterministic fixtures pass. “Live-game-certified” applies only to capabilities tested against the declared build. No profile capability is being represented here as live-certified.

## Capability semantics

- **C0 Conversation:** detection/manual target, PTT, audio/subtitles, explicit offscreen selection.
- **C1 Profile-aware:** authored lore, characters, prompts, voices, detection, and troubleshooting without modifying the game.
- **C2 External identity:** manual selection plus independently qualified read-only OCR/screen evidence; no game-memory or injected state access.
- **X Visual:** experimental screen-space identity/lip-sync; immediately reversible and never required for dialogue.

An authored profile is not automatically C1/C2/X certified. The runtime displays evidence and fallback for each feature.

In the machine-readable profile, each capability has one evidence status: `unsupported`, `experimental`, `replay_verified`, or `live_certified`. These statuses apply per capability, not to the game logo or profile as a whole.

## Profile waves

| Wave | Profiles | Integration purpose |
| --- | --- | --- |
| A | Skyrim SE, Cyberpunk 2077, Baldur's Gate 3 | Stabilize external capture across NPC-heavy, modern real-time, and cinematic-dialogue slices. |
| B | Fallout 4, Fallout: New Vegas, The Witcher 3, Starfield, Bannerlord, Kingdom Come: Deliverance II, Oblivion Remastered | Expand against the stable data-profile and external-media contracts. |
| C | The Sims 4, Stardew Valley, Minecraft Java, Divinity: Original Sin 2, Mass Effect Legendary Edition, Dragon Age: Inquisition, Kenshi | Cover simulation, sandbox, party RPG, and systemic worlds. |
| Risk-gated | RDR2 Story Mode, GTA V Story Mode, Elden Ring offline | Offline-only; refuse anti-cheat, protected, online, or ambiguous configurations. |

## Generic Game

Generic Game is experimental and data-light. It supports a capturable single-player window, manual target/name, conversation, memory, audio, and subtitles. Automatic identity, lore, state, and screen-space animation require explicit configuration/evidence and are not universal. It never provides native game actions.

## Store and build detection

Profiles may inspect Steam/Epic/GOG manifests, registry data, common install directories, processes, executables, and manually selected paths. Detection does not grant permission to inject or modify a game. No profile requires a mod or executable adapter.

## Games not listed

Use Generic Game only where capture and game policy permit it. Do not create an executable community profile, inject into a game, bypass protections, or represent an untested title as supported. Data-only community profiles can be proposed under the [content policy](../legal/game-content-policy.md).
