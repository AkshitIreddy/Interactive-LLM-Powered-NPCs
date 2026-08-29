# Game profiles

A 2.0 game profile is declarative, versioned data. It cannot execute Python, launch commands, load arbitrary DLLs, or embed credentials.

## Profile responsibilities

- identify store/install/process/executable/build candidates;
- define safe session modes and protected/ambiguous refusal rules;
- describe capture regions/exclusions and display fallbacks;
- declare identity/world-state evidence and confidence rules;
- provide original/provenanced lore, characters, biographies, personality/style, spoiler tiers, and prompts;
- select provider-neutral model/voice defaults and deterministic background-NPC rules;
- list external capture/selection requirements, diagnostics, troubleshooting, and certification evidence.

Every profile uses the same external, non-injecting boundary. It cannot require or reference a mod, hook, script extender, injected DLL, executable adapter, native rig, or game action. Profiles remain useful as authored lore/detection/troubleshooting data even when capture or automatic identity is unavailable.

## Authoring workflow

1. Start with the authoritative `schemas/game-profile-v2.schema.json` after its ABI is marked stable.
2. Add safe detection with manual executable fallback.
3. Author content from original writing or compatible sources and record field-level provenance.
4. Set every capability to the lowest truthful level and provide a fallback.
5. Add deterministic fixtures for detection, identity evidence, offscreen selection, dialogue/memory, diagnostics, and refusal.
6. Validate schema and replay behavior.
7. Live-test each advertised game/build capability; record it independently from replay status.

Validate all profiles from the repository root:

```powershell
$profiles = (Get-ChildItem profiles/games/*/profile.json).FullName
cargo run --manifest-path crates/game-profile/Cargo.toml --bin validate-profile -- $profiles
```

When the root Cargo workspace is available, `cargo run -p npc-game-profile --bin validate-profile -- $profiles` is equivalent.

## Identity and background NPCs

Named characters use stable profile IDs. Unknown/background NPCs receive session-scoped encounter IDs so unrelated characters never share a “default” persona or memory. Identity fuses profile-approved evidence over time; a single face match is insufficient. Low confidence asks for selection/name.

## Spoilers and canon

Separate immutable lore/canon from player dialogue, episodic memory, relationship state, and ephemeral quest/save state. User dialogue never becomes canon automatically. Profiles declare spoiler tiers and the user selects the allowed tier.

## Community submissions

Community profiles are data-only initially. See [CONTRIBUTING.md](../../CONTRIBUTING.md) and [game-content policy](../legal/game-content-policy.md). A game logo in the UI is not evidence of live compatibility.
