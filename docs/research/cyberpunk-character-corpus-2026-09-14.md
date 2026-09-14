# Cyberpunk 2077 character corpus research

Date reviewed: 2026-09-14

This research supports the game profile's conversation context. It does not
ship publisher art, facial references, audio, transcripts, lyrics, or copied
wiki prose. Every biography, personality description, speaking rule, opening
line, and style example in the profile was written for this repository.

## Sources and how they were used

| Source | Use | Boundary |
| --- | --- | --- |
| [Cyberpunk 2077 Ultimate Edition booklet](https://cdn-s-cyberpunk.cdprojektred.com/CP2077-UE-Booklet-EN-1.pdf) | Official setting and principal-character framing for V's major allies and Arasaka relationships. | Facts were paraphrased; art and official copy are not included. |
| [Official Phantom Liberty dossiers](https://www.cyberpunk.net/en/phantom-liberty) | Official roles for Myers, Songbird, Reed, Alex, Hansen, and the Dogtown power structure. | The profile adds original interpretation and speaking guidance; it does not reproduce dossier prose. |
| [Cyberpunk 2077 plot overview](https://en.wikipedia.org/wiki/Cyberpunk_2077) | Cross-check for the principal cast, Relic dependencies, and which facts require late-game gating. | Used as an index; it is not the provenance of the original profile text. |
| [Phantom Liberty plot overview](https://en.wikipedia.org/wiki/Cyberpunk_2077:_Phantom_Liberty) | Cross-check for expansion cast and spoiler ordering. | Outcome-dependent facts remain behind the `dogtown` or `relic-and-endings` tiers. |
| [Cyberpunk Wiki character index](https://cyberpunk.fandom.com/wiki/Category:Cyberpunk_2077_Characters) | Character-by-character cross-check against in-game database citations, especially supporting fixers and faction figures. | No dialogue or descriptive prose was copied. Community pages can contain mistakes, so conflicts defer to official material or are stated as character belief. |

Representative character pages checked during authoring include
[Jackie Welles](https://cyberpunk.fandom.com/wiki/Jackie_Welles),
[Evelyn Parker](https://cyberpunk.fandom.com/wiki/Evelyn_Parker),
[Rogue Amendiares](https://cyberpunk.fandom.com/wiki/Rogue_Amendiares),
[Kerry Eurodyne](https://cyberpunk.fandom.com/wiki/Kerry_Eurodyne),
[River Ward](https://cyberpunk.fandom.com/wiki/River_Ward),
[Goro Takemura](https://cyberpunk.fandom.com/wiki/Goro_Takemura),
[Yorinobu Arasaka](https://cyberpunk.fandom.com/wiki/Yorinobu_Arasaka),
[Hanako Arasaka](https://cyberpunk.fandom.com/wiki/Hanako_Arasaka),
[Anders Hellman](https://cyberpunk.fandom.com/wiki/Anders_Hellman),
[Saul Bright](https://cyberpunk.fandom.com/wiki/Saul_Bright), and
[Placide](https://cyberpunk.fandom.com/wiki/Placide).

## Authoring decisions

- The roster favors characters a player can plausibly face and speak with in
  the base game or Phantom Liberty. V is excluded because the player defines V.
- Records distinguish public role, personal relationship, professional
  competence, and unavailable private information. A character does not become
  an omniscient wiki narrator.
- `street-level` is the only default spoiler tier. Relationship arcs, Relic and
  ending information, and Dogtown information require explicit activation.
- Relationship states, romance, survival, quest results, and endings are never
  assumed. The prompt uses only trusted progress and delivered conversation.
- Every style example is labeled `illustrative-original` and points to
  `corpus-original-dialogue`. These lines are non-canonical examples, not game
  quotations and not material for performer imitation.
- Voice descriptions specify delivery traits only. They never request a clone
  or resemblance to a game's performer.

## Roster coverage

The corpus contains 36 named characters plus one encounter-scoped background
resident. The named roster covers V's early crew; close allies and romance
paths; Afterlife, Aldecaldo, NCPD, Arasaka, Militech, Voodoo Boys, fixer, and
Peralez contexts; Delamain; and the six central Phantom Liberty figures.

The canonical order and IDs live in
`scripts/content-packs/cyberpunk-character-corpus.mjs`. Mouth-reference packs
use those same IDs but remain optional, separately licensed visual assets.
