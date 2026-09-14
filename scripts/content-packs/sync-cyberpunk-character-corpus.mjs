import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  CYBERPUNK_CHARACTER_CORPUS,
  backgroundResidentCharacter,
  buildKnowledgeRecord,
  buildProfileCharacter
} from "./cyberpunk-character-corpus.mjs";

const [profileArgument] = process.argv.slice(2);
if (!profileArgument) {
  throw new Error("usage: node sync-cyberpunk-character-corpus.mjs <profile.json>");
}

const profilePath = resolve(profileArgument);
const profile = JSON.parse(await readFile(profilePath, "utf8"));
if (profile.id !== "cyberpunk-2077" || profile.schema_version !== "2.0.0") {
  throw new Error("the target must be the Cyberpunk 2077 GameProfileV2");
}

const provenance = new Map(
  profile.content.provenance.map((record) => [record.id, record])
);
for (const record of [
  {
    id: "corpus-original-summaries",
    title: "Original Cyberpunk character summaries",
    kind: "original",
    license: "MIT",
    notes: "Transformative biographies, relationship context, knowledge boundaries, and speaking guidance authored for this repository after source review; no source prose is reproduced.",
    source_path: "docs/research/cyberpunk-character-corpus-2026-09-14.md",
    review_status: "approved",
    transform_version: "cyberpunk-character-corpus-v1"
  },
  {
    id: "corpus-original-dialogue",
    title: "Original illustrative Cyberpunk dialogue",
    kind: "original",
    license: "MIT",
    notes: "Fresh non-canonical examples written to demonstrate cadence and reasoning style. They are neither game quotations nor performer transcripts.",
    source_path: "docs/research/cyberpunk-character-corpus-2026-09-14.md",
    review_status: "approved",
    transform_version: "cyberpunk-character-corpus-v1"
  },
  {
    id: "official-ultimate-edition-booklet",
    title: "Cyberpunk 2077 Ultimate Edition booklet",
    kind: "official",
    source_url: "https://cdn-s-cyberpunk.cdprojektred.com/CP2077-UE-Booklet-EN-1.pdf",
    notes: "Consulted for the official setting overview and principal-character framing; no images or text are redistributed.",
    review_status: "approved"
  },
  {
    id: "official-phantom-liberty-dossiers",
    title: "Official Phantom Liberty dossiers",
    kind: "official",
    source_url: "https://www.cyberpunk.net/en/phantom-liberty",
    notes: "Consulted for the roles of Myers, Songbird, Reed, Alex, Hansen, and Dogtown; no official assets or prose are redistributed.",
    review_status: "approved"
  },
  {
    id: "community-cyberpunk-character-index",
    title: "Cyberpunk Wiki character index",
    kind: "licensed",
    source_url: "https://cyberpunk.fandom.com/wiki/Category:Cyberpunk_2077_Characters",
    license: "CC BY-SA 3.0",
    notes: "Used as a cross-check and pointer to in-game database citations. Profile prose remains independently authored and is attributed to the original corpus records.",
    review_status: "approved"
  }
]) {
  provenance.set(record.id, record);
}

profile.content.provenance = [...provenance.values()];
profile.content.knowledge = CYBERPUNK_CHARACTER_CORPUS.map(buildKnowledgeRecord);
profile.content.retrieval = {
  query_window_delivered_turns: 8,
  max_core_records: 12,
  max_public_records: 8,
  max_character_records: 8,
  max_memory_records: 12,
  authority_order: [
    "core_canon",
    "character_profile",
    "character_authored",
    "game_public",
    "session_summary",
    "retrieved_memory",
    "recent_delivered_turns"
  ]
};
profile.content.character_data_readiness = "curated";
profile.content.character_data_readiness_notes =
  "Thirty-six named records and one encounter-scoped resident have stable IDs, detailed original biographies, relationship-aware speaking guidance, provenance-linked illustrative dialogue, bounded knowledge, and player-overridable voice intent. Visual identity and mouth references are separate optional assets and are not certified by this content corpus.";
profile.characters = [
  ...CYBERPUNK_CHARACTER_CORPUS.map(buildProfileCharacter),
  backgroundResidentCharacter()
];

await writeFile(profilePath, `${JSON.stringify(profile, null, 2)}\n`, "utf8");
console.log(profilePath);
