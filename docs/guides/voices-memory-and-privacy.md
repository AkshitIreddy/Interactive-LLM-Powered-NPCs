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

The current implementation is in `crates/memory`. Schema v3 retains the legacy STRICT `memory_items` namespaces, adds immutable delivered turns and provenance-linked typed memories, and permits one narrowly authorized native character-erasure transaction with a content-free hashed audit receipt. It also uses an external-content FTS5 index, versioned derived embeddings, and a leased/deduplicated outbox. One bounded writer thread and query-only WAL readers keep writes serialized without blocking retrieval.

Only dialogue actually delivered/heard is committed. Proposed memories are validated, deduplicated, and written asynchronously. Changing embedding model rebuilds derived vectors; it does not rewrite source memories.

Lexical and semantic results use deterministic reciprocal-rank fusion. Exact cosine is the active safe semantic backend; sqlite-vec is capability-gated for future large-corpus acceleration. Callers must provide authorization-aware scope/visibility filters and embeddings.

## Privacy boundaries

- **API-powered:** selected hosted-provider inputs may include microphone audio, transcript, retrieved lore/memory, read-only screen evidence, and generated response according to the configured stage.
- **API + optional local lip-sync:** conversation egress is unchanged; captured frames stay within the local visual worker.
- **Offline:** provider calls and new AI conversation are disabled.

The route preview should state what leaves the PC before a session. There is no silent fallback.

## Presence/webcam

Webcam presence is separate from game-screen vision and NPC output emotion. It is off by default for every preset; legacy screen-presence preferences do not opt the user in. The current build records an explicit `webcamPresence` preference but registers no live webcam capture command, so saving the preference does not open a device. Any future qualified producer must remain local, ephemeral, visibly active, previewable, device-selectable, and revocable during a session. It may expose authored observation signals such as face present or approximate blendshape/action-unit activity; it must not claim to infer inner emotion, race, gender, or age. Frames are not retained by default.

## Deleting data

The native memory workspace can inspect an exact authored game/character scope, create/list/delete app-owned local SQLite backups, and physically erase an exact character scope across encounters/sessions/saves after explicit confirmation. Arbitrary paths and WebView-supplied user/session principals are not accepted. Physical erasure removes delivered turns, provenance-linked typed memories, legacy scoped items, and dependent outbox work in one transaction; a content-free scope hash/count receipt remains. Direct SQL deletion stays blocked outside that transaction.

A pre-erasure backup contains all local memory, including the character history subsequently erased from the live store. The UI must disclose that fact and let the user delete the backup by its opaque app-owned ID. Deleting local data cannot retract data already processed or retained by a hosted provider; use that provider's controls too. Complete removal of every application artifact remains a separate uninstall/local-data workflow and must not be inferred from character erasure.

Restoring accepts only an app-owned backup UUID after explicit confirmation. The native command stops the runtime writer, validates the source and copied database, quarantines the prior live database and SQLite sidecars, atomically installs the backup, then reopens it and verifies integrity. The selected backup remains available; the runtime starts again only when next used.

“Remove all local memory” is narrower than uninstall: it removes the live memory database/WAL/SHM, recognized restore temporaries, and prior-store quarantine directories, then creates and verifies a new empty store. App-owned backups are preserved by default. Removing those too requires the separate `includeBackups` choice in the confirmed request. Both restore and complete removal append a content-free local lifecycle receipt containing only operation identity, time, counts and integrity outcome; dialogue, character IDs, paths and backup contents are excluded.
