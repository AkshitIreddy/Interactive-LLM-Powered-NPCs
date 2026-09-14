import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const profilePath = resolve(root, "profiles/games/cyberpunk-2077/profile.json");
const packPath = resolve(
  root,
  "profiles/content-packs/local-review/cyberpunk-2077-authored-context-v1.pack.json",
);
const builderPath = resolve(
  root,
  "scripts/content-packs/build-cyberpunk-local-review-pack.mjs",
);

const canonicalReviewedCharacters = new Map([
  ["misty-olszewski", "Misty Olszewski"],
  ["claire-russell", "Claire Russell"],
  ["johnny-silverhand", "Johnny Silverhand"],
]);

const expectedNamedCharacterIds = [
  "jackie-welles", "johnny-silverhand", "judy-alvarez", "panam-palmer",
  "viktor-vector", "misty-olszewski", "claire-russell", "rogue-amendiares",
  "kerry-eurodyne", "river-ward", "goro-takemura", "hanako-arasaka",
  "yorinobu-arasaka", "evelyn-parker", "dexter-deshawn", "t-bug",
  "mama-welles", "sebastian-ibarra", "regina-jones", "wakako-okada",
  "mr-hands", "elizabeth-peralez", "jefferson-peralez", "meredith-stout",
  "alt-cunningham", "anders-hellman", "saul-bright", "mitch-anderson",
  "placide", "brigitte", "delamain", "song-so-mi", "solomon-reed",
  "alex-xenakis", "rosalind-myers", "kurt-hansen",
];

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

test("bundled profile exposes Claire under the canonical runtime identity", async () => {
  const profile = await readJson(profilePath);
  const characters = new Map(
    profile.characters.map((character) => [character.id, character]),
  );

  for (const [id, displayName] of canonicalReviewedCharacters) {
    assert.equal(characters.get(id)?.display_name, displayName);
  }
  for (const shorthand of ["misty", "misty-olzewski", "claire", "johnny"]) {
    assert.equal(characters.has(shorthand), false);
  }

  const claire = characters.get("claire-russell");
  assert.equal(claire.background_npc, false);
  assert.equal(claire.identity.strategy, "explicit_selection");
  assert.equal(claire.identity.fallback, "explicit_selection");
  assert.equal(claire.voice.user_override_allowed, true);
  assert.match(claire.voice.description, /no performer resemblance/i);
  assert.ok(
    claire.prompt.constraints.some((constraint) =>
      /do not assume mission outcomes/i.test(constraint),
    ),
  );
  assert.ok(
    profile.content.provenance.some(
      (record) =>
        record.id === "official-claire-race-reference" &&
        record.kind === "official" &&
        record.source_url ===
          "https://www.cyberpunk.net/en/news/38612/patch-1-23",
    ),
  );
});

test("bundled profile ships a detailed, prompt-ready Cyberpunk roster", async () => {
  const profile = await readJson(profilePath);
  const characters = new Map(
    profile.characters.map((character) => [character.id, character]),
  );
  assert.equal(profile.characters.length, 37);
  assert.deepEqual(
    profile.characters.slice(0, expectedNamedCharacterIds.length).map(({ id }) => id),
    expectedNamedCharacterIds,
  );

  const knowledgeByOwner = new Map(
    profile.content.knowledge.map((record) => [record.owner_character_id, record]),
  );
  for (const id of expectedNamedCharacterIds) {
    const character = characters.get(id);
    assert.ok(character, `missing character ${id}`);
    const biographyWords = character.biography.trim().split(/\s+/u).length;
    assert.ok(biographyWords >= 150, `${id} biography has only ${biographyWords} words`);
    assert.ok(character.biography.includes("\n\n"), `${id} biography needs multiple paragraphs`);
    assert.ok(character.personality.length >= 140, `${id} personality is too short`);
    assert.ok(character.dialogue_style.length >= 125, `${id} dialogue style is too short`);
    assert.equal(character.opening_lines.length, 1);
    assert.equal(character.style_examples.length, 1);
    assert.equal(character.style_examples[0].provenance_id, "corpus-original-dialogue");
    assert.ok(character.style_examples[0].situation_tags.includes("illustrative-original"));
    assert.match(character.voice.description, /original/i);
    assert.equal(character.voice.user_override_allowed, true);

    const knowledge = knowledgeByOwner.get(id);
    assert.ok(knowledge, `missing bounded knowledge for ${id}`);
    assert.equal(knowledge.provenance_id, "corpus-original-summaries");
    assert.ok(character.prompt.knowledge_refs.includes(knowledge.id));
  }

  for (const id of [
    "song-so-mi", "solomon-reed", "alex-xenakis", "rosalind-myers", "kurt-hansen",
  ]) {
    assert.ok(characters.get(id).prompt.knowledge_refs.includes("dogtown"));
  }
  assert.equal(characters.get("night-city-resident").background_npc, true);
});

test("authored pack is deterministic and keeps canonical character joins", async () => {
  const generated = spawnSync(
    process.execPath,
    [builderPath, profilePath, "-"],
    { cwd: root, encoding: "utf8" },
  );
  assert.equal(generated.status, 0, generated.stderr);
  assert.equal(generated.stderr, "");

  const checkedIn = await readFile(packPath, "utf8");
  assert.equal(generated.stdout, checkedIn);
  const pack = JSON.parse(generated.stdout);
  assert.equal(pack.version, "1.2.0");

  const characterIds = new Set(
    pack.profile.characters.map((character) => character.id),
  );
  for (const id of canonicalReviewedCharacters.keys()) {
    assert.equal(characterIds.has(id), true);
  }
  assert.equal(
    pack.profile.content.knowledge.some(
      (record) => record.owner_character_id === "claire-russell",
    ),
    true,
  );
  assert.equal(
    pack.profile.characters.every(
      (character) => character.voice.user_override_allowed === true,
    ),
    true,
  );
  for (const recommendation of pack.provider_recommendations) {
    if (recommendation.character_id) {
      assert.equal(characterIds.has(recommendation.character_id), true);
    }
  }
});
