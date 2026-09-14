import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const [profileArgument, outputArgument] = process.argv.slice(2);
if (!profileArgument || !outputArgument) {
  throw new Error(
    "usage: node build-cyberpunk-local-review-pack.mjs <profile.json> <output.pack.json>",
  );
}

const profilePath = resolve(profileArgument);
const profile = JSON.parse(await readFile(profilePath, "utf8"));
if (profile.id !== "cyberpunk-2077" || profile.schema_version !== "2.0.0") {
  throw new Error(
    "the source must be the validated Cyberpunk 2077 GameProfileV2",
  );
}

profile.defaults.voice.user_override_allowed = true;
profile.content.knowledge ??= [];
for (const character of profile.characters) {
  character.voice.user_override_allowed = true;
}
const namedCharacters = profile.characters.filter(
  (character) => !character.background_npc,
);
if (namedCharacters.length !== 36) {
  throw new Error(`expected 36 named authored characters, found ${namedCharacters.length}`);
}
for (const character of namedCharacters) {
  if (!character.opening_lines?.length || !character.style_examples?.length) {
    throw new Error(`character lacks authored conversation context: ${character.id}`);
  }
}

const pack = {
  format: "npc.content-pack",
  schema_version: 1,
  namespace: "io.github.akshitireddy.local-review",
  pack_id: "cyberpunk-2077-authored-context",
  version: "1.2.0",
  kind: "game_base",
  game_profile_id: "cyberpunk-2077",
  title: "Night City local review context",
  summary:
    "A private data-only review pack with 36 named Cyberpunk 2077 characters, original speaking examples, and provenance-linked bounded context.",
  rights: {
    license_name: "Local private review data",
    source_summary:
      "Original transformative summaries and dialogue guidance written for local interoperability testing. No publisher art, audio, fonts, screenshots, extracted game dialogue, face references, or voice clones.",
    redistributable: false,
    commercial_use: false,
    derivative_use: false,
    review_status: "approved",
  },
  provider_recommendations: [
    {
      role: "llm",
      provider_id: "groq",
      model_id: "qwen/qwen3.6-27b",
      rationale:
        "A current selectable low-latency structured-dialogue candidate. The player must intentionally adopt it in Voice & models.",
    },
    {
      role: "llm",
      provider_id: "mistral",
      model_id: "ministral-8b-2512",
      rationale:
        "A current selectable dialogue alternative. It remains a recommendation and never replaces a saved player route.",
    },
  ],
  profile,
};

const serialized = `${JSON.stringify(pack, null, 2)}\n`;
if (outputArgument === "-") {
  process.stdout.write(serialized);
} else {
  const outputPath = resolve(outputArgument);
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, serialized, "utf8");
  console.log(outputPath);
}
