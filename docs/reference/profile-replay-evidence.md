# Profile replay evidence

The authored game count is locked at 20. Each `profiles/games/*/profile.json` file has one
versioned replay under `fixtures/profile-replays/v1/` and one SHA-256 entry in
`integrity-ledger.json`. `npc-runtime validate-profile-replays` verifies the profile and replay
hashes before executing the corpus.

Each deterministic replay checks:

- strict `GameProfileV2` validation and the absence of `live_certified` claims;
- one declared store/process detection match;
- explicit character selection while the character is marked offscreen;
- deterministic conversation, subtitle, silent-fixture audio, and scoped SQLite memory routes;
- protected-online and anti-cheat refusal for the three risk-gated offline profiles.

Replay evidence is not live-game evidence. Authored-profile capture and generic screen-space
lip-sync are therefore `unsupported` in this corpus until separately certified with
real game/build/display evidence. The silent fixture audio validates ordering and delivery, not
speech quality.

Generic Game is a separate built-in experimental contract (`generic-game`) and is not included
in the 20 authored profiles. It requires manual game, executable-leaf, and character-name
selection; has no executable adapters or game-action proposals; blocks protected online or detected
anti-cheat states; and continues through conversation, memory, audio, and subtitles. Identity and
screen-space lip-sync remain explicitly experimental.
