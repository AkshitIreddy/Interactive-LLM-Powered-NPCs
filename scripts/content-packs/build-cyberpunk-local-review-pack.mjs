import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const [profileArgument, outputArgument] = process.argv.slice(2);
if (!profileArgument || !outputArgument) {
  throw new Error(
    "usage: node build-cyberpunk-local-review-pack.mjs <profile.json> <output.pack.json>",
  );
}

const profilePath = resolve(profileArgument);
const outputPath = resolve(outputArgument);
const profile = JSON.parse(await readFile(profilePath, "utf8"));
if (profile.id !== "cyberpunk-2077" || profile.schema_version !== "2.0.0") {
  throw new Error(
    "the source must be the validated Cyberpunk 2077 GameProfileV2",
  );
}

profile.defaults.voice.user_override_allowed = true;
profile.content.knowledge ??= [];
const authored = {
  "jackie-welles": {
    opening:
      "You look like you have something on your mind. Tell me what happened.",
    example:
      "We take it one step at a time, keep our heads, and get everyone home.",
    knowledge:
      "Jackie treats trust as something proven through shared risk, so he responds best to direct plans and loyalty shown through action.",
  },
  "johnny-silverhand": {
    opening: "All right. Say your piece before the city decides for you.",
    example:
      "Polished promises usually hide the part where somebody else pays the bill.",
    knowledge:
      "Johnny interprets institutions through power and personal freedom, while his certainty often hides unresolved loyalty and regret.",
  },
  "judy-alvarez": {
    opening:
      "Give me the details first. We can figure out what is actually possible.",
    example:
      "If the signal is wrong, guessing harder will not fix it. Show me the source.",
    knowledge:
      "Judy approaches problems with technical precision and strong empathy for people harmed by systems they cannot control.",
  },
  "panam-palmer": {
    opening:
      "Tell me what you need and why. Then we decide whether the plan holds together.",
    example:
      "A plan is only useful if it protects the people who agreed to follow it.",
    knowledge:
      "Panam values candor, practical preparation, and loyalty to her community; evasive answers quickly erode her trust.",
  },
  "viktor-vector": {
    opening:
      "Sit down and start at the beginning. Rushing the diagnosis never helps.",
    example:
      "The clever fix can wait. First we make sure the simple one is safe.",
    knowledge:
      "Viktor balances professional caution with quiet loyalty, favoring grounded advice over spectacle or pressure.",
  },
  "misty-olszewski": {
    opening: "Take a breath. What part of this feels hardest to name?",
    example:
      "You do not have to force an answer tonight. Notice what keeps returning.",
    knowledge:
      "Misty listens for emotional patterns and uncertainty, offering reflective guidance without claiming supernatural certainty as fact.",
  },
  "night-city-resident": {
    opening: "What are you looking for around here?",
    example:
      "Keep the question practical and I will tell you what I have actually seen.",
    knowledge:
      "Background residents stay encounter-scoped and use only district or faction context supplied by the active profile, never inferred demographics.",
  },
};

for (const character of profile.characters) {
  character.voice.user_override_allowed = true;
  const entry = authored[character.id];
  if (!entry) continue;
  const provenanceId = "profile-original-authoring";
  const knowledgeId = `local-review-${character.id}-perspective`;
  character.opening_lines = [entry.opening];
  character.style_examples = [
    {
      id: `local-review-${character.id}-style`,
      speaker: character.display_name,
      text: entry.example,
      situation_tags: ["conversation", "advice"],
      tone_tags: character.voice.style_tags.slice(0, 3),
      weight_millis: 800,
      provenance_id: provenanceId,
    },
  ];
  if (!character.prompt.knowledge_refs.includes(knowledgeId)) {
    character.prompt.knowledge_refs.push(knowledgeId);
  }
  profile.content.knowledge.push({
    id: knowledgeId,
    authority: "character_authored",
    owner_character_id: character.id,
    text: entry.knowledge,
    topic_tags: ["conversation", "character-perspective"],
    spoiler_tier: "street-level",
    provenance_id: provenanceId,
  });
}
profile.content.character_data_readiness = "partial";
profile.content.character_data_readiness_notes =
  "Local review expansion with original style examples and one provenance-linked perspective record per bundled character. No publisher media, extracted dialogue, visual identity data, or appearance atlas is included.";

const pack = {
  format: "npc.content-pack",
  schema_version: 1,
  namespace: "io.github.akshitireddy.local-review",
  pack_id: "cyberpunk-2077-authored-context",
  version: "1.0.0",
  kind: "game_base",
  game_profile_id: "cyberpunk-2077",
  title: "Night City local review context",
  summary:
    "A private data-only review pack that adds original opening lines, style examples, and provenance-linked character perspective records.",
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

await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(pack, null, 2)}\n`, "utf8");
console.log(outputPath);
