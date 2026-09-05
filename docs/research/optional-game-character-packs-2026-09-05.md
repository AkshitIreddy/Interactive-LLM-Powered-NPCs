# Optional game and character packs: architecture audit and minimal contract

**Date:** 2026-09-05
**Scope:** data-only optional Cyberpunk 2077 content, character identity references, provider voice/model suggestions, and user-precedence rules
**Status:** architecture proposal; no pack installer, provider recommender, or distributable Cyberpunk reference pack is implemented by this document

## Decision

Support composable **base game packs**, **character packs**, and **appearance packs**. Do not duplicate the existing character, lore, provider-route, or atlas structures inside a second incompatible data model.

The missing piece is a thin signed `npc.content-pack/v1` envelope. It supplies namespace, version, target game, dependencies, conflicts, payload hashes, rights metadata and composition operations. Its payload reuses existing typed records:

- a base pack supplies one complete `GameProfileV2`;
- a character pack supplies exact `CharacterProfile`, `KnowledgeRecord`, provenance and optional prompt records to add to or deliberately replace in that base;
- provider recommendations reuse exact `ProviderLoadoutV1` role/route shapes as templates, including provider/model/stock-voice IDs;
- an appearance pack supplies rights-cleared reference images, derived identity tensors, mouth-atlas states or related metadata bound to stable character IDs; and
- deterministic composition materializes one complete `GameProfileV2` for the current parser and one validated pack-default provider layer for the existing resolver.

Requiring a whole profile is a constraint of today's `ResourceCatalog`, not the product contract. The first implementation may ship a base Cyberpunk pack alone, but the composer extension must be the planned path for independently versioned character packs.

Pack authors may recommend exact provider models and exact stock voices. At install/use time the app validates every recommendation against the current provider catalog and exposes unavailable or stale entries honestly. One **Use pack setup** action can accept the disclosure, create the existing loadout records, and apply the pack's game and character defaults transactionally. A saved player route always resolves above an accepted pack default, and a later pack update never overwrites that saved choice.

Public appearance references are allowed when the pack author documents distribution rights, likeness/subject permissions where applicable, source provenance, hashes and compatible licenses. The current downloaded test footage and private experiments are not cleared for redistribution. The existing `QualifiedIdentityGalleryV1` supports only local private/original enrollment today; that is an implementation limitation to extend, not a permanent ban on rights-cleared fictional-character reference packs.

## Current architecture audit

| Concern | Current owner and behavior | Implemented state | Pack consequence |
| --- | --- | --- | --- |
| Game, detection, safety and capabilities | `GameProfileV2` in `crates/game-profile/src/model.rs`; strict `deny_unknown_fields`; JSON schema in `schemas/game-profile-v2.schema.json` | Implemented | Reuse it as the base payload and as the materialized composition result. |
| Lore and backstory | `ContentBundle.world_lore`, spoiler tiers, background-NPC rules, provenance, and structured `KnowledgeRecord`s | Implemented | Author pack lore here; do not add a second RAG or lore-file schema. |
| Named characters | `CharacterProfile` contains stable ID/aliases, biography, personality, dialogue style, prompt, style examples, opening lines, voice defaults, identity contract and model defaults | Implemented | Reuse this record in base and character-extension packs; do not invent a reduced character schema. |
| Prompt assembly | `crates/character-db` builds prompts from the active profile, provenance-filtered knowledge, style examples, scoped memory and delivered turns | Implemented | Pack content becomes useful through the existing prompt path. |
| Character selection | `character_workspace.rs` resolves per-turn requested character, then persisted per-game choice, then profile default | Implemented | A pack default is a fallback; it cannot displace a player selection. |
| Provider/model/voice routes | `ProviderLoadoutDocumentV1` and `ProviderLoadoutV1` in `crates/provider-loadouts`; global, game and character scopes; explicit activation | Implemented | Reuse route/loadout shapes for exact pack defaults; add source-layer precedence so saved player routes always win. |
| Provider and model availability | Signed-capable catalog in `crates/provider-catalog` and `catalog/v1/catalog.json`; routes carry lifecycle, qualification and disclosure | Implemented | Resolve exact suggestions against the current catalog, never stale IDs embedded in the profile. |
| Semantic voice intent | Provider catalog has eight provider-neutral `VoiceIntent`s and deterministic tag selection | Partially implemented | Profile voice tags can express intent, but the catalog has no exact stock-voice-to-intent binding yet. |
| Face-reference enrollment | `QualifiedIdentityGalleryV1` in `crates/identity-engine`; private atomic store in `identity_runtime.rs` | Implemented but unavailable in the product | Reuse its qualification/provenance/tensor rules; extend its source-rights model for signed rights-cleared pack assets. |
| Profile discovery | `ResourceCatalog` has a compile-time `GAMES` allowlist and reads one `profiles/games/<id>/profile.json` from installed or repository roots | Bundled-only | Cyberpunk is allowlisted, but pack discovery, dependency resolution and deterministic composition are missing. |
| Signed game-content lifecycle | Architecture docs describe signed data, bounded extraction, atomic activation and rollback | Design only for game profiles | Implement this before describing community game packs as installable. |

### Cyberpunk profile as shipped

`profiles/games/cyberpunk-2077/profile.json` is already a valid `GameProfileV2` rather than a fixture-only character array. It currently supplies:

- a substantial original world-lore summary;
- four spoiler tiers and five background-NPC rules;
- six named characters: Jackie Welles, Johnny Silverhand, Judy Alvarez, Panam Palmer, Viktor Vector and Misty Olszewski;
- one generic `night-city-resident` background profile;
- per-character biography, personality, dialogue style, prompt objectives and constraints;
- provider-neutral voice descriptions, locales and style tags;
- `fast`, `balanced` or `quality` model intent plus an 8,192–24,576 token context budget; and
- two provenance records that distinguish original transformative writing from the official product reference.

The profile is not yet a rich character pack. It has zero structured `KnowledgeRecord`s, zero `StyleExample`s, zero opening lines and no face-reference enrollment. It carries no exact stock voice or provider recommendation. All character and default `user_override_allowed` fields are omitted, which deserializes to `false`; that conflicts with the requested player-override behavior and should be corrected in the profile content before this pack is presented as complete.

## Reuse existing types inside a composable envelope

| Data | Canonical typed owner | What a content pack may carry |
| --- | --- | --- |
| Game ID, detection, safety and capture policy | `GameProfileV2` | A base pack carries the complete record. Extensions may require it but may not weaken safety. |
| World lore, spoilers, knowledge and background rules | `GameProfileV2.content` | Base content or typed add/replace operations with provenance. |
| Character biography, style, prompt and model intent | `CharacterProfile` | Complete typed character records in namespaced character packs. |
| Semantic voice traits | `VoiceDefaults` | Yes, as author intent and fallback matching data. |
| Exact provider/model/stock voice | `ProviderLoadoutV1` route shapes plus current provider catalog | Yes, as pack-authored defaults/templates; never credentials. |
| Player provider choice | `ProviderLoadoutDocumentV1` | No direct writes by a pack. The app creates a player-owned override only through the normal UI. |
| Identity embeddings and provenance | `QualifiedIdentityGalleryV1` | Yes after extending source classes for rights-cleared signed pack assets; current implementation accepts local-only enrollment. |
| Mouth appearance states | Native mouth-worker atlas contract | Yes when identity, format, source hashes and distribution rights are declared. |
| Reference images | Content-pack asset record plus character binding | Yes when original/licensed and distributable; private/unlicensed footage stays local. |
| Optional model binaries | Model Manager pack | Declare a dependency by immutable pack ID/revision; do not hide a model inside content data. |
| User-selected character | `selected-characters-v1.json` | Never overwritten by install, update or composition. |

The envelope coordinates these owners. It does not redefine their internal records, and it never carries credential values, executable hooks or arbitrary code.

## Minimal `npc.content-pack/v1` contract

### Envelope fields

The new signed envelope needs a small amount of composition metadata that none of the existing schemas owns:

- `namespace`: reverse-DNS or catalog-assigned author namespace;
- `pack_id`: stable name unique inside that namespace;
- `version`: immutable semantic version plus content digest;
- `kind`: `game_base`, `character_extension`, `appearance_extension`, or `bundle`;
- `game_profile_id`: exact target game;
- `requires`: pack ID plus accepted version range and required base profile schema;
- `conflicts`: incompatible pack/version or claimed record IDs;
- `provides`: exact game, character, knowledge, provider-template and appearance IDs;
- `operations`: typed `add` or explicit `replace` operations over existing record types;
- `artifacts`: closed file list with media type, length, SHA-256, character binding and license-component reference; and
- `license_components` and provenance: author, source, rights holder, permitted redistribution/derivatives/commercial use, notice and review state.

This is a pack manifest, not a second character schema. A character payload remains a complete `CharacterProfile`; lore remains `KnowledgeRecord`; an exact route remains the existing `ProviderLoadoutV1` route shape; an atlas remains the native atlas format.

### Logical layouts

A base game pack can remain simple:

```text
pack.json
content/base-profile.json                 # complete GameProfileV2
provider-defaults/game-loadouts.json      # ProviderLoadoutV1 templates
NOTICE.txt
LICENSE.txt
```

A separately installable character/appearance pack can add material without copying the base profile:

```text
pack.json
content/characters/judy-alvarez.json      # complete CharacterProfile
content/knowledge/judy/*.json             # KnowledgeRecord values
provider-defaults/judy-loadouts.json      # ProviderLoadoutV1 templates
appearance/judy/references/*.webp          # rights-cleared reference images
appearance/judy/mouth-atlas/*              # typed atlas states and metadata
NOTICE.txt
LICENSE.txt
```

Each payload path and digest is declared in `pack.json`. The archive is closed-world: undeclared members fail validation.

### Composition

1. Select exactly one active base pack for a `game_profile_id`.
2. Resolve the dependency graph by immutable pack identity and compatible version range. Reject cycles and missing dependencies.
3. Order extensions deterministically by declared dependency edges, then namespace/pack ID. Order is reproducibility, not permission to overwrite.
4. `add` requires the target stable ID to be absent. `replace` requires an explicit replaced pack/record identity and expected prior digest. Two undeclared claims on one ID are a hard conflict.
5. Safety/detection restrictions are base-owned. An extension may add character/content capability requirements but cannot enable online/in-process behavior or weaken the base fallback.
6. Validate every knowledge, prompt, provenance, appearance and provider-template reference across the composed graph.
7. Materialize a complete `GameProfileV2` in a versioned cache and pass it through the existing strict parser/validator before activation.
8. Bind the result digest to the ordered input pack IDs, versions and hashes. Atomic activation points to that immutable composition and retains the prior composition for rollback.

Today steps 1–2 stop at a fixed built-in profile because `ResourceCatalog` loads one whole file from a compile-time allowlist. The pack envelope, dependency solver, composer and materialized-profile cache are required extensions. The full-profile-only implementation is an acceptable first base-pack slice, not the final modular architecture.

### Install, apply and activation

1. Fetch only a signed catalog-selected target with declared length and SHA-256.
2. Extract into a new version directory with file-count and byte caps. Reject traversal, absolute paths, device names, alternate data streams, links/reparse points, duplicates and executable file types.
3. Validate the envelope, closed artifact list, every reused typed payload, licenses, dependencies and conflicts.
4. Compose and strictly validate the materialized profile and pack-default loadout layer.
5. Show one review surface with content, provider/voice egress and license disclosures.
6. A single **Use pack setup** action may accept the disclosed configuration, atomically activate the content composition, and apply its currently valid game/character provider defaults. Do not require a second confirmation for each role or character.
7. Missing credentials or catalog-invalid routes remain clearly unavailable within that same flow; the rest of the pack can still install if its dependency policy permits.
8. On any atomic-apply failure, retain the prior content and loadout state.

No provider is contacted merely because the archive was downloaded or inspected. The one apply action is sufficient authorization for the disclosed valid setup; later saved player changes win without another pack decision.

### Update and removal

- Update one pack version, resolve the full graph again, and activate only a fully validated new composition.
- Stable character IDs preserve selection, memory, identity and loadout scope. A rename/removal uses an explicit migration or retirement record, never fuzzy matching.
- Updated pack defaults may fill roles the player has never overridden. They cannot rewrite or outrank any saved player route.
- Removal checks reverse dependencies and active references. It either removes dependants together in one reviewed action or explains the blocking dependency.
- Delete only the selected pack's manifest-owned version directory, then rematerialize the remaining composition.
- Keep player memory, selected-character preferences and saved provider overrides as detached/rebindable data unless the player explicitly removes them.
- Remove derived pack-owned gallery/atlas entries when their source pack disappears unless a player has made a separately rights-valid private copy; never delete unrelated private enrollment.

## Recommendation pipeline

Pack authors can express both semantic intent and exact provider choices. The current provider catalog remains the authority on whether each exact route is presently selectable, qualified and compatible.

```text
content pack
  exact ProviderLoadoutV1 templates by game/character
  semantic voice/model intent as alternatives and author rationale
              |
              v
current provider catalog validation
  exact provider/model/voice exists, is selectable, qualified,
  stock-only where required, compatible, and fully disclosed
              |
              v
one "Use pack setup" action
  accepts disclosure + installs validated pack-default layer
              |
              v
effective role: saved player override > accepted pack default > app default
```

The pack-default layer is inheritable: a game template can supply LLM/STT/TTS/embeddings while a character template overrides only TTS/voice. A pack bundle can also offer named alternatives such as “fast/free” and “best dialogue.” The selected alternative is applied in one transaction.

The current `ProviderLoadoutDocumentV1` has scope inheritance but no source layer. The planned compatible evolution should tag loadout templates/activations as `pack_default` or `player_saved`, or maintain a separate accepted-pack activation map inside the same versioned document. Resolution compares source first, then scope inside that source. This prevents a character-specific pack default from outranking a player's saved global provider choice.

Templates carry exact provider/model/voice identity but no secret value. On apply, the app resolves the player's existing opaque credential reference and rebuilds catalog disclosure from the current catalog revision; it does not trust a stale disclosure copied from the archive.

### LLM suggestions

The current catalog can validate and enumerate selectable, live-qualified LLM routes. A pack author may name any exact route below in a `ProviderLoadoutV1` template and explain why it is the preferred default. The app still cannot independently rank them from `ModelDefaults` because the catalog lacks normalized context-window, dialogue-quality and measured-latency fields. Current exact candidates are:

| Provider | Current exact route | Honest pack treatment |
| --- | --- | --- |
| Google Gemini | `gemini-3.1-flash-lite` | Show as an available candidate after credential/terms checks; do not claim it is best for Cyberpunk dialogue without a common benchmark. |
| Groq | `openai/gpt-oss-20b` | Available candidate; compare exact structured dialogue and latency behavior. |
| Groq | `qwen/qwen3.6-27b` with the qualified non-reasoning request profile | Available candidate; the generic reasoning profile is not the qualified path. |
| Mistral | `ministral-8b-2512` | Available candidate; useful quality/latency comparator. |
| Mistral | `ministral-3b-2512` | Available fast candidate; do not equate speed with character quality. |
| OpenRouter | `liquid/lfm-2.5-2.6b:free` | Available only while the exact free route remains listed and supports required parameters; keep user-requested no-fallback behavior. |
| Cohere | `command-a-plus-05-2026` | Available candidate; trial terms are evaluation-oriented and must remain disclosed. |

OpenAI and Anthropic are user-selected catalog routes rather than fixed pack-safe model recommendations. NVIDIA NIM remains private-evaluation-gated. Cloudflare is not present as an implemented/selectable LLM route in the current catalog, so a Cyberpunk pack must not advertise it merely because a key exists.

The minimum provider-catalog extension for automatic alternatives is model recommendation metadata such as usable context window, structured-output status, locale/language coverage, qualification state and measured conversation latency under a named fixture. This lets the resolver filter hard requirements and explain trade-offs without erasing the pack author's exact preferred route.

### Stock-voice suggestions

The current Cyberpunk profile has only semantic traits, while the optional pack design may include exact provider stock-voice defaults. Each exact suggestion is checked against the current catalog or a bounded provider stock-voice discovery response before it becomes applicable. The fixed live-qualified combinations currently known to the app are:

| Provider | Exact current stock route | Timing support | Limitation for a cast pack |
| --- | --- | --- | --- |
| Cartesia | Sonic 3.6 + public stock voice Greg (`a0e99841-438c-4a64-b679-ae501e7d6091`) | Provider phoneme timing | One qualified masculine voice is not a credible fit for every character. |
| Deepgram | Aura 2 Arcas (`aura-2-arcas-en`) | No phoneme/viseme timing | Exact qualified English stock route, but limited articulation cues and cast variety. |
| Inworld | TTS 2 Flash + Dennis | 44 provider visemes | Exact qualified English stock route, but still only one qualified masculine voice. |
| ElevenLabs | User-selected model and voice | Alignment available | A pack may name an exact stock voice, but the app must rediscover/validate that voice for the player's account and reject cloning/private voices. |

NVIDIA Magpie has a larger discovered stock set in private evaluation, but it requires a key, acknowledgement and exact live discovery. A pack may carry a private-evaluation suggestion, clearly gated as unavailable outside that mode; it cannot present the route as public production entitlement.

The three fixed routes do not provide enough variety for a convincing seven-character default cast. A pack author may still choose one where it fits the authored intent. For other characters, an exact ElevenLabs/account-discovered voice or future provider-catalog binding can be suggested. Unavailable recommendations stay visible with a reason and alternatives; they are never replaced silently. No suggestion should imply resemblance to the game's performers unless the rights and claim are documented.

The minimum provider-catalog extension is a versioned stock-voice binding:

- exact provider/model/voice identity;
- provider-owned stock-voice status and discovery timestamp;
- locale/language;
- semantic intent IDs or normalized traits;
- preview availability and terms disclosure;
- live qualification state, including timing capability; and
- catalog revision used when the suggestion was created.

Provider inventory metadata belongs in the provider catalog because it can change independently of a Cyberpunk content release. Author preference belongs in the content pack's existing loadout template. The profile can continue to carry semantic traits for fallback matching. If deterministic intent selection proves too ambiguous, add an optional `voice_intent_id` to the next compatible revision of `VoiceDefaults`; exact recommendations still reuse `ProviderLoadoutV1` rather than a new route schema.

### Initial Cyberpunk authoring intents

These are starting traits for the current profile. A real optional pack may attach one or more exact provider/model/stock-voice templates to each row after listening review and current-catalog validation. The audit did not establish performer-like voice matches:

| Character | Existing model intent | Initial semantic voice intent | Required content follow-up |
| --- | --- | --- | --- |
| Jackie Welles | `balanced`, 16,384 | `voice.warm-companion` | Add `grounded`/`friendly` semantic tags only if the authored profile supports them; preserve energetic delivery in the description. |
| Johnny Silverhand | `quality`, 24,576 | `voice.weathered-dry` with `voice.quick-wry` as a preview alternative | Let the player choose between slower weathered and quicker sarcastic delivery. |
| Judy Alvarez | `quality`, 24,576 | New focused/empathetic intent needed | Do not force the current bright/soft intents onto a precise, guarded characterization. |
| Panam Palmer | `quality`, 24,576 | `voice.intense-commanding` | Treat this as energy/style, not gender or actor imitation. |
| Viktor Vector | `balanced`, 16,384 | `voice.calm-grounded` | Existing tags already support calm/grounded intent. |
| Misty Olszewski | `balanced`, 16,384 | `voice.soft-enigmatic` | Retain gentle/reflective delivery and let preview override the initial mapping. |
| Night City resident | `fast`, 8,192 | Deterministic pool filtered by authored locale/archetype tags | Never use one global voice; seed from the stable encounter ID and preserve manual correction. |

The content author may recommend an exact provider route, a semantic intent, or both. The current provider catalog decides whether the exact route is usable now; the player can preview alternatives and apply the disclosed pack setup in one action.

## Player precedence: hard contract

The following order must remain testable and visible:

1. A direct per-turn character request wins.
2. Otherwise the player's persisted character choice for the game wins.
3. Otherwise the profile default character is used.
4. For each provider role, an explicit session route wins first.
5. Any saved player route wins next: player character scope, then player game scope, then player global scope.
6. Accepted pack defaults resolve only after the player layer: pack character scope, then pack game scope, then application default.
7. **Use pack setup** accepts and activates the currently valid pack-default graph in one transaction. It does not require a second activation confirmation.
8. A later player edit becomes a saved override and immediately wins. Pack updates cannot rewrite, reactivate or delete it.
9. Every fallback remains manual-only and user-authorized. An unavailable voice/model fails clearly and offers choices; it never silently changes provider, voice or data-egress destination.

`VoiceDefaults.user_override_allowed` should be `true` for every Cyberpunk character and for the profile default. Runtime resolution should also enforce player precedence independently of this descriptive field so malformed or old content cannot take control away from the user.

## Character references and identity

The phrase “character references” covers three related but differently validated data classes:

1. **Authored character reference** means biography, personality, aliases, dialogue guidance, lore records and semantic voice intent. These belong in `GameProfileV2` and are distributable when original or properly licensed.
2. **Visual identity reference** means a rights-cleared source image and/or its image-derived embedding used to match a captured actor. The existing `QualifiedIdentityGalleryV1` supplies the game/character/qualification binding, while the content-pack envelope supplies distributable asset rights and source-pack identity.
3. **Oral appearance atlas** means identity-bound lip/oral texture states used by the mouth worker. It uses the existing native atlas format plus pack asset provenance, character binding, pose/coverage metadata and exact source hashes.

The current identity contract permits only `user_private` and `original_synthetic` sources, and both are marked local-only. It records source and normalized-content hashes, consent/license fields and import time; it stores bounded raw `f32le` tensors rather than pickle. Reuse those safety and qualification properties, but extend the provenance enum with a signed pack source such as `licensed_distributable`. That source must bind pack ID/version, artifact digest, rights record and subject/likeness permission where applicable.

For Cyberpunk 2077:

- the currently downloaded footage and experimental references remain private because redistribution rights have not been established;
- a new pack may ship original or licensed reference images and oral atlases when the manifest documents distribution, derivative and commercial-use permissions plus any required likeness consent;
- imported/derived data binds to the exact pack version, `game_profile_id`, stable `character_id`, model/atlas revision and source digest;
- the app validates reference media dimensions/types and recomputes embeddings locally with an admitted identity model rather than trusting an opaque pickle;
- identity and oral assets never go to a hosted provider unless a separate feature and disclosure explicitly allows it; and
- pack-owned assets and derived entries are lifecycle-managed separately from unrelated player-private enrollment.

Rights-cleared appearance packs therefore need an explicit new `ReferenceSourceClassV1`, signed tensor/source provenance, revocation and migration rules. This is planned product work. The current local-only gallery is evidence of a useful safety base, not evidence that public fictional-character references should remain unsupported.

## Lore, dialogue and provenance rules

Every substantial pack-authored fact should be a structured `KnowledgeRecord` with:

- a stable ID;
- `core_canon`, `game_public` or `character_authored` authority;
- optional owning character;
- topic tags;
- a valid spoiler tier; and
- a `provenance_id` that resolves to an approved profile provenance record.

Style examples should use original, transformative lines rather than copied game dialogue. Their `provenance_id` must resolve and their purpose should be style guidance, not a quotation corpus. Opening lines are profile-authored greetings, not delivered conversation history. DLC/endgame records should use a disabled-by-default spoiler tier until the player opts in.

Distribution policy should distinguish unlicensed material from rights-cleared pack assets:

- use game/provider names only as nominative compatibility references;
- exclude publisher art, UI, fonts, screenshots, textures, dialogue, game audio, save data, actor likenesses and cloned voices unless the pack carries documented rights that specifically permit the intended distribution and use;
- allow original or properly licensed fictional-character reference images and oral atlases with per-artifact license components, source hashes and attribution;
- do not scrape or reproduce wiki prose;
- record source URL/revision/hash when permitted, author/reviewer status and transform version;
- keep official sources as high-level terminology/canon references while shipping original summaries; and
- require human legal review before any public catalog promotion.

## Current gaps versus future work

| Gap | Minimal future change | Acceptance evidence |
| --- | --- | --- |
| Cyberpunk content is bundled rather than optional | Add the signed `npc.content-pack/v1` envelope and lifecycle around existing typed payloads | A clean app installs two pack revisions, rejects a corrupt target, rolls back atomically and removes only owned files. |
| Runtime accepts only one whole profile | Add dependency/conflict resolution and a deterministic composer that materializes a complete `GameProfileV2` | A base plus two independently versioned character packs compose identically regardless of discovery order; undeclared duplicate IDs fail. |
| Compile-time game allowlist blocks independently introduced IDs | Replace fixed discovery with a signed catalog index while retaining explicit safety classification | An unknown unsigned ID fails; a signed approved ID appears without a code rebuild. |
| Profile lacks structured lore and style examples | Author original provenance-linked `KnowledgeRecord`s and `StyleExample`s | Parser/character DB tests prove spoiler and character authority filtering. |
| Player override flag defaults false | Set it true in Cyberpunk character/default voice records and enforce precedence in runtime | Character- and game-scoped user choices survive pack update/restart. |
| No pack-authored exact provider defaults | Allow `ProviderLoadoutV1` templates in the content envelope and add an accepted pack-default source layer below saved player routes | One action applies game plus character defaults; any saved global/game/character player route wins before and after update. |
| No exact provider voice inventory mapping | Extend provider catalog with qualified stock-voice inventory/bindings and bounded provider discovery | Stale/unqualified voices are excluded; previews disclose provider, terms and catalog revision. |
| Model defaults cannot rank exact LLM routes | Add comparable provider-catalog requirements/measurements | The same profile yields a disclosed candidate list from the current catalog, with no hidden winner or fallback. |
| Ordinary provider voice discovery/preview is incomplete outside NVIDIA | Add bounded stock-only discovery and preview per provider | Player can preview current account-visible stock voices without entering arbitrary voice-clone IDs. |
| Identity enrollment is blocked and source rights are local-only | Admit a signed measured identity runtime, connect private enrollment, and add a signed distributable-pack provenance class | Private and rights-cleared pack references import, restart, match, update and remove with distinct ownership and no unintended egress. |
| No composable oral-appearance asset lifecycle | Bind existing atlas states to pack/character/source/license metadata and validate them through the content lifecycle | A rights-cleared atlas activates only for its exact character/revision and rolls back with its source pack. |
| Pack update could orphan stable IDs | Add explicit lifecycle migration/retirement metadata within the existing profile lifecycle | Removed IDs are shown as detached; no fuzzy reassignment of memory, loadout or identity data occurs. |

## Implementation order

1. Define and validate the thin content-pack envelope using existing `GameProfileV2`, `CharacterProfile`, `KnowledgeRecord`, `ProviderLoadoutV1` and atlas payload types.
2. Add a signed pack catalog, dependency/conflict resolver, deterministic composer and atomic lifecycle around `ResourceCatalog`; keep the fixed allowlist as the fail-closed baseline while this lands.
3. Complete the Cyberpunk base content: structured lore, original style examples, opening lines, explicit override flags and reviewed provenance.
4. Add one independent Cyberpunk character pack to prove namespace/version/dependency and conflict behavior rather than baking modularity into prose only.
5. Add exact pack provider templates, current-catalog validation, the pack-default source layer and one-click **Use pack setup** in the existing loadout UI.
6. Add provider stock-voice discovery/bindings and model requirement metadata so unavailable authored choices get useful alternatives.
7. Extend rights provenance and admit the identity runtime before accepting private or distributable reference images; bind rights-cleared oral atlases through the same pack lifecycle.
8. Test every precedence, composition, update, rollback, conflict, rights and removal rule above.

The first reviewable slice does not need a general community marketplace. It needs one locally installable Cyberpunk base pack, one independently versioned character extension, one verified update/rollback/conflict path, one character whose original lore/style records reach the prompt builder, and one exact provider setup that validates and applies in one action while an existing saved player override still wins.

## Evidence ledger

| Evidence | What it establishes |
| --- | --- |
| `crates/game-profile/src/model.rs` | `GameProfileV2` already owns all game/character/lore/default fields and explicitly keeps provider/model IDs in independently swappable catalogs. |
| `schemas/game-profile-v2.schema.json` | A strict machine-readable schema already exists; a new character-pack schema would duplicate it. |
| `profiles/games/cyberpunk-2077/profile.json` | Current Cyberpunk content, safety, provenance, characters, empty knowledge/style/example gaps and omitted override fields. |
| `crates/game-profile/src/validation.rs` | Existing cross-reference and profile validation boundary. |
| `crates/character-db` | Existing profile-to-prompt, alias, memory and background-encounter behavior. |
| `apps/control/src-tauri/src/character_workspace.rs` | Persisted per-game character selection and direct-request precedence. |
| `apps/control/src-tauri/src/catalog.rs` | Fixed 20-game allowlist, installed/repository profile lookup, size/link checks and missing optional-pack discovery. |
| `crates/provider-loadouts/src/lib.rs` | Exact user-owned loadout format, global/game/character inheritance, explicit activations and manual-only authorized fallbacks. |
| `apps/control/src-tauri/src/provider_loadouts.rs` | Native persistence, catalog validation and provider route mutation path used by the app. |
| `crates/provider-catalog/src/lib.rs` and `catalog/v1/catalog.json` | Current route eligibility, qualification, voice intents, no-automatic-fallback policy and exact currently qualified provider candidates. |
| `crates/identity-engine/src/qualified.rs` | Existing local-only, provenance-bearing, qualification-pinned identity gallery and safe tensor import. |
| `apps/control/src-tauri/src/identity_runtime.rs` | Private per-game atomic gallery persistence and the current admission gate. |
| `crates/model-manager/src/manifest.rs`, `manifest_v2.rs` and lifecycle code | Existing immutable pack IDs/revisions, artifact hashes, license components, closed extraction, atomic activation and rollback primitives that the content-pack lifecycle can reuse, although they do not provide content dependencies/composition. |
| `native/mouth-worker/include/npc/mouth_worker/atlas.hpp` | Existing typed identity-observed atlas-state representation that appearance packs should carry rather than replacing with a generic image list. |
| `docs/architecture/profile-and-model-manager.md` | Intended signed-download, bounded extraction, atomic activation, rollback and exact-file removal model. |
| `docs/legal/game-content-policy.md` and `docs/legal/third-party-notices.md` | Current base-distribution boundary excludes publisher game assets and likeness/audio material; a public optional asset pack therefore needs explicit documented rights and legal review rather than assuming compatibility naming grants redistribution. |
