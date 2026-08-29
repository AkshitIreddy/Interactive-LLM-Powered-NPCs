# Voices, memory, and privacy

## Voices

Voice bindings are provider-neutral characteristics (language, range, pacing, timbre/style tags), not executable scripts. Background NPC assignment is deterministic within an encounter. Do not imitate a performer or clone game audio without documented rights and consent.

TTS timing/viseme data may drive an optional generic local screen-space lip-sync worker. The product does not use native rigs or game mods. Animation is experimental and immediately removed when confidence or freshness fails. Audio/subtitles continue.

## Memory model

SQLite is authoritative and separates:

- immutable profile lore and character canon;
- delivered session turns;
- episodic memories and facts with provenance/confidence/salience;
- relationship state;
- summaries;
- ephemeral quest/save/world state;
- rebuildable embedding indexes namespaced by model/version.

The current implementation is in `crates/memory`. Schema v1 uses a STRICT `memory_items` table with `event`, `episode`, `fact`, `relationship`, `summary`, `lore`, and `profile` namespaces; an external-content FTS5 index; versioned derived embeddings; and a leased/deduplicated outbox. One bounded writer thread and query-only WAL readers keep writes serialized without blocking retrieval.

Only dialogue actually delivered/heard is committed. Proposed memories are validated, deduplicated, and written asynchronously. Changing embedding model rebuilds derived vectors; it does not rewrite source memories.

Lexical and semantic results use deterministic reciprocal-rank fusion. Exact cosine is the active safe semantic backend; sqlite-vec is capability-gated for future large-corpus acceleration. Callers must provide authorization-aware scope/visibility filters and embeddings.

## Privacy boundaries

- **API-powered:** selected hosted-provider inputs may include microphone audio, transcript, retrieved lore/memory, read-only screen evidence, and generated response according to the configured stage.
- **API + optional local lip-sync:** conversation egress is unchanged; captured frames stay within the local visual worker.
- **Offline:** provider calls and new AI conversation are disabled.

The route preview should state what leaves the PC before a session. There is no silent fallback.

## Presence/webcam

Presence is off by default, local, ephemeral, visibly active, and revocable during a session. It may expose authored observation signals such as face present or approximate blendshape/action-unit activity; it must not claim to infer inner emotion, race, gender, or age. Frames are not retained by default.

## Deleting data

The product must provide character/session memory inspection and targeted deletion, plus complete local-data removal. Schema v1 retains soft-deleted rows until a future physical-erasure/compaction operation, so the UI must not claim secure erasure yet. Deleting local data does not delete data already processed/retained by a hosted provider; use that provider's controls too. Back up only if you accept the sensitivity of transcripts and relationship state.
