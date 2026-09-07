# Local game and character content packs

The Windows app can review and activate one `npc.content-pack` schema-version 1 JSON file for an existing bundled game ID. This first local-only format carries a complete validated `GameProfileV2`, including world lore, character backstories, style examples, opening lines, and provenance-linked knowledge. It may also carry current provider/model/stock-voice recommendations.

Import a pack from **Games & characters → Game & character packs**. The app previews the exact file, validates it without network access, and binds activation to the reviewed SHA-256 digest. Activation changes only the authored profile layer. Saved character selection, delivered memory, credentials, and provider loadouts remain player-owned. Provider recommendations link to **Voice & models** and are never selected automatically.

Open a character and choose **Customize this character** to save a local player layer for its stable game and character IDs. Name, backstory/biography, and extra prompt context are editable. This layer is composed into the same validated profile snapshot used for ordinary runtime turns, remains above later pack replacements, and can be reset separately to reveal the newest active-pack values. Identity enrollment, delivered memory, voice selection, and provider routes stay in their own stores.

This initial format deliberately rejects archives, scripts, executable payloads, credentials, new game IDs, changed process detection, weaker safety rules, altered capability claims, removed stable character IDs, stale provider routes, files over 2 MiB, embedded profiles over the 768 KiB authenticated runtime-message budget, and voice defaults that disable player overrides. Remote catalogs, signatures, dependency composition, updates, removal, and rollback remain future work; the UI does not claim those lifecycle features.

The included local review pack is [cyberpunk-2077-authored-context-v1.pack.json](local-review/cyberpunk-2077-authored-context-v1.pack.json). Rebuild it from the bundled Cyberpunk profile with:

```powershell
node scripts/content-packs/build-cyberpunk-local-review-pack.mjs profiles/games/cyberpunk-2077/profile.json profiles/content-packs/local-review/cyberpunk-2077-authored-context-v1.pack.json
```

The example contains original private-review text and no publisher art, audio, fonts, screenshots, extracted dialogue, face images, identity embeddings, mouth atlases, or voice clones. Visual identity and mouth appearance require a separate rights-bound and runtime-qualified appearance-pack workflow.

## Canonical character joins

Profile selection, character content, provider overrides, and a reviewed mouth
atlas join on the exact pair `game_profile_id` + `character_id`. The reviewed
Cyberpunk character IDs currently needed by the local integration are:

| Character | Game profile ID | Canonical character ID |
| --- | --- | --- |
| Misty Olszewski | `cyberpunk-2077` | `misty-olszewski` |
| Claire Russell | `cyberpunk-2077` | `claire-russell` |
| Johnny Silverhand | `cyberpunk-2077` | `johnny-silverhand` |

Do not resolve `misty`, `claire`, or `johnny` as aliases at this boundary. The
unreviewed `integration-20260907/packs/*-v1` scratch atlases use those short IDs
and therefore cannot bind to a persisted canonical character selection. They
must be regenerated with the canonical IDs after visual review. Older immutable
artifacts may retain historical short IDs as evidence, but they do not become
selectable content by renaming them.

Two frontend-only content-override test fixtures still spell Misty's ID as
`misty-olzewski`. That misspelling is not present in the authored profile or
generated content pack and remains a separate test-fixture cleanup; runtime
selection must continue to use `misty-olszewski`.
