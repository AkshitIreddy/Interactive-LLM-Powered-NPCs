use crate::MemoryError;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 3;
pub(crate) const MIGRATION_CHECKSUMS: [(i64, &str); 3] = [
    (
        1,
        "c65447d8418099ea9498325381c8c0e85d03cd0c78baeba6ab4bbf63f58d8acf",
    ),
    (
        2,
        "3af1f9938f07aa7d996d12ca489584cfda433e870f829ca7414c9c238f77b25e",
    ),
    (
        3,
        "a2bcae6c5ca1e39e4d193c53f934be9490be10088821785f494235b2940d1630",
    ),
];

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS memory_items (
    id TEXT PRIMARY KEY NOT NULL,
    namespace TEXT NOT NULL CHECK(namespace IN ('event','episode','fact','relationship','summary','lore','profile')),
    profile_id TEXT,
    game_id TEXT,
    character_id TEXT,
    session_id TEXT,
    save_id TEXT,
    visibility TEXT NOT NULL CHECK(visibility IN ('private','character','game','profile','global')),
    content TEXT NOT NULL CHECK(length(trim(content)) > 0 AND length(content) <= 1048576),
    content_sha256 TEXT NOT NULL CHECK(length(content_sha256) = 64),
    provenance_json TEXT NOT NULL CHECK(json_valid(provenance_json)),
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    importance REAL NOT NULL CHECK(importance >= 0.0 AND importance <= 1.0),
    observed_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER CHECK(expires_at_ms IS NULL OR expires_at_ms > observed_at_ms),
    embedding_generation INTEGER CHECK(embedding_generation IS NULL OR embedding_generation >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    deleted_at_ms INTEGER
) STRICT;

CREATE INDEX IF NOT EXISTS idx_memory_active_namespace
    ON memory_items(namespace, updated_at_ms DESC, id) WHERE deleted_at_ms IS NULL;
CREATE INDEX IF NOT EXISTS idx_memory_character
    ON memory_items(character_id, namespace, updated_at_ms DESC) WHERE deleted_at_ms IS NULL;
CREATE INDEX IF NOT EXISTS idx_memory_game
    ON memory_items(game_id, namespace, updated_at_ms DESC) WHERE deleted_at_ms IS NULL;
CREATE INDEX IF NOT EXISTS idx_memory_profile
    ON memory_items(profile_id, namespace, updated_at_ms DESC) WHERE deleted_at_ms IS NULL;
CREATE INDEX IF NOT EXISTS idx_memory_expiry
    ON memory_items(expires_at_ms) WHERE expires_at_ms IS NOT NULL AND deleted_at_ms IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_fact_identity
    ON memory_items(namespace, profile_id, game_id, character_id, content_sha256)
    WHERE namespace IN ('fact','relationship','lore','profile') AND deleted_at_ms IS NULL;

CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    content,
    provenance_json,
    content='memory_items',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS memory_fts_insert AFTER INSERT ON memory_items
WHEN new.deleted_at_ms IS NULL BEGIN
    INSERT INTO memory_fts(rowid, content, provenance_json)
    VALUES (new.rowid, new.content, new.provenance_json);
END;
CREATE TRIGGER IF NOT EXISTS memory_fts_delete AFTER DELETE ON memory_items BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, content, provenance_json)
    VALUES ('delete', old.rowid, old.content, old.provenance_json);
END;
CREATE TRIGGER IF NOT EXISTS memory_fts_update AFTER UPDATE ON memory_items BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, content, provenance_json)
    SELECT 'delete', old.rowid, old.content, old.provenance_json
    WHERE old.deleted_at_ms IS NULL;
    INSERT INTO memory_fts(rowid, content, provenance_json)
    SELECT new.rowid, new.content, new.provenance_json
    WHERE new.deleted_at_ms IS NULL;
END;

CREATE TABLE IF NOT EXISTS memory_embeddings (
    item_id TEXT NOT NULL REFERENCES memory_items(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK(generation >= 0),
    model_id TEXT NOT NULL CHECK(length(trim(model_id)) > 0),
    dimensions INTEGER NOT NULL CHECK(dimensions > 0 AND dimensions <= 65536),
    vector BLOB NOT NULL,
    l2_norm REAL NOT NULL CHECK(l2_norm > 0.0),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY(item_id, generation)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_embeddings_generation
    ON memory_embeddings(generation, model_id, item_id);

CREATE TABLE IF NOT EXISTS memory_outbox (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK(length(trim(kind)) > 0),
    aggregate_id TEXT,
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
    status TEXT NOT NULL CHECK(status IN ('pending','processing','completed','dead')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    max_attempts INTEGER NOT NULL CHECK(max_attempts > 0),
    available_at_ms INTEGER NOT NULL,
    lease_owner TEXT,
    lease_expires_at_ms INTEGER,
    last_error TEXT,
    dedupe_key TEXT UNIQUE,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK((status = 'processing') = (lease_owner IS NOT NULL AND lease_expires_at_ms IS NOT NULL))
) STRICT;
CREATE INDEX IF NOT EXISTS idx_outbox_claim
    ON memory_outbox(status, available_at_ms, id);

CREATE TABLE IF NOT EXISTS memory_metadata (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL CHECK(json_valid(value_json)),
    updated_at_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
"#;

const MIGRATION_2: &str = r#"
CREATE TABLE IF NOT EXISTS memory_schema_migrations (
    version INTEGER PRIMARY KEY CHECK(version > 0),
    name TEXT NOT NULL UNIQUE CHECK(length(trim(name)) > 0),
    checksum TEXT NOT NULL CHECK(length(checksum) = 64),
    applied_at_ms INTEGER NOT NULL
) STRICT;

INSERT OR IGNORE INTO memory_schema_migrations(version,name,checksum,applied_at_ms)
VALUES(1,'initial_authoritative_memory','c65447d8418099ea9498325381c8c0e85d03cd0c78baeba6ab4bbf63f58d8acf',0);

CREATE TABLE IF NOT EXISTS delivered_turns (
    turn_id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(turn_id)) > 0 AND length(turn_id) <= 512),
    user_id TEXT NOT NULL CHECK(length(trim(user_id)) > 0 AND length(user_id) <= 512),
    profile_id TEXT NOT NULL CHECK(length(trim(profile_id)) > 0 AND length(profile_id) <= 512),
    game_id TEXT NOT NULL CHECK(length(trim(game_id)) > 0 AND length(game_id) <= 512),
    character_id TEXT CHECK(character_id IS NULL OR (length(trim(character_id)) > 0 AND length(character_id) <= 512)),
    encounter_id TEXT CHECK(encounter_id IS NULL OR (length(trim(encounter_id)) > 0 AND length(encounter_id) <= 512)),
    session_id TEXT CHECK(session_id IS NULL OR (length(trim(session_id)) > 0 AND length(session_id) <= 512)),
    save_id TEXT CHECK(save_id IS NULL OR (length(trim(save_id)) > 0 AND length(save_id) <= 512)),
    speaker TEXT NOT NULL CHECK(speaker IN ('player','npc')),
    delivered_text TEXT NOT NULL CHECK(length(trim(delivered_text)) > 0 AND length(delivered_text) <= 1048576),
    content_sha256 TEXT NOT NULL CHECK(length(content_sha256) = 64),
    created_at_ms INTEGER NOT NULL,
    delivered_at_ms INTEGER NOT NULL CHECK(delivered_at_ms >= created_at_ms),
    sequence_no INTEGER NOT NULL CHECK(sequence_no >= 0),
    cancellation_generation INTEGER NOT NULL CHECK(cancellation_generation >= 0),
    provider_id TEXT CHECK(provider_id IS NULL OR (length(trim(provider_id)) > 0 AND length(provider_id) <= 512)),
    delivery_receipt_id TEXT CHECK(delivery_receipt_id IS NULL OR (length(trim(delivery_receipt_id)) > 0 AND length(delivery_receipt_id) <= 512)),
    provenance_json TEXT NOT NULL CHECK(json_valid(provenance_json))
) STRICT, WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_delivered_turns_scope_recent
    ON delivered_turns(user_id,profile_id,game_id,character_id,encounter_id,session_id,delivered_at_ms DESC,sequence_no DESC,turn_id);
CREATE INDEX IF NOT EXISTS idx_delivered_turns_character_recent
    ON delivered_turns(user_id,game_id,character_id,delivered_at_ms DESC,turn_id);

CREATE TRIGGER IF NOT EXISTS delivered_turns_immutable_update
BEFORE UPDATE ON delivered_turns BEGIN
    SELECT RAISE(ABORT,'delivered turns are append-only');
END;
CREATE TRIGGER IF NOT EXISTS delivered_turns_immutable_delete
BEFORE DELETE ON delivered_turns BEGIN
    SELECT RAISE(ABORT,'delivered turns are append-only');
END;

CREATE TABLE IF NOT EXISTS structured_memories (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0 AND length(id) <= 512),
    user_id TEXT NOT NULL CHECK(length(trim(user_id)) > 0 AND length(user_id) <= 512),
    profile_id TEXT NOT NULL CHECK(length(trim(profile_id)) > 0 AND length(profile_id) <= 512),
    game_id TEXT NOT NULL CHECK(length(trim(game_id)) > 0 AND length(game_id) <= 512),
    character_id TEXT CHECK(character_id IS NULL OR (length(trim(character_id)) > 0 AND length(character_id) <= 512)),
    encounter_id TEXT CHECK(encounter_id IS NULL OR (length(trim(encounter_id)) > 0 AND length(encounter_id) <= 512)),
    session_id TEXT CHECK(session_id IS NULL OR (length(trim(session_id)) > 0 AND length(session_id) <= 512)),
    save_id TEXT CHECK(save_id IS NULL OR (length(trim(save_id)) > 0 AND length(save_id) <= 512)),
    knowledge_class TEXT NOT NULL CHECK(knowledge_class IN ('world_lore','biography','character_knowledge','uncertain_public_info','long_term_summary')),
    spoiler_scope TEXT NOT NULL CHECK(spoiler_scope IN ('none','game','save','character_private','user_private')),
    content TEXT NOT NULL CHECK(length(trim(content)) > 0 AND length(content) <= 1048576),
    content_sha256 TEXT NOT NULL CHECK(length(content_sha256) = 64),
    provenance_json TEXT NOT NULL CHECK(json_valid(provenance_json)),
    confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
    importance REAL NOT NULL CHECK(importance >= 0.0 AND importance <= 1.0),
    observed_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER CHECK(expires_at_ms IS NULL OR expires_at_ms > observed_at_ms),
    generator_json TEXT CHECK(generator_json IS NULL OR json_valid(generator_json)),
    CHECK(knowledge_class <> 'long_term_summary' OR generator_json IS NOT NULL)
) STRICT, WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_structured_memory_scope_class
    ON structured_memories(user_id,profile_id,game_id,character_id,encounter_id,knowledge_class,importance DESC,observed_at_ms DESC,id);
CREATE INDEX IF NOT EXISTS idx_structured_memory_expiry
    ON structured_memories(expires_at_ms) WHERE expires_at_ms IS NOT NULL;

CREATE TABLE IF NOT EXISTS structured_memory_sources (
    memory_id TEXT NOT NULL REFERENCES structured_memories(id) ON DELETE RESTRICT,
    turn_id TEXT NOT NULL REFERENCES delivered_turns(turn_id) ON DELETE RESTRICT,
    source_ordinal INTEGER NOT NULL CHECK(source_ordinal >= 0),
    PRIMARY KEY(memory_id,turn_id),
    UNIQUE(memory_id,source_ordinal)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER IF NOT EXISTS structured_memories_immutable_update
BEFORE UPDATE ON structured_memories BEGIN
    SELECT RAISE(ABORT,'structured memories are immutable; append a replacement');
END;
CREATE TRIGGER IF NOT EXISTS structured_memories_immutable_delete
BEFORE DELETE ON structured_memories BEGIN
    SELECT RAISE(ABORT,'structured memories are immutable; append a replacement');
END;
CREATE TRIGGER IF NOT EXISTS structured_sources_immutable_update
BEFORE UPDATE ON structured_memory_sources BEGIN
    SELECT RAISE(ABORT,'structured memory provenance is immutable');
END;
CREATE TRIGGER IF NOT EXISTS structured_sources_immutable_delete
BEFORE DELETE ON structured_memory_sources BEGIN
    SELECT RAISE(ABORT,'structured memory provenance is immutable');
END;

INSERT OR IGNORE INTO memory_schema_migrations(version,name,checksum,applied_at_ms)
VALUES(2,'immutable_delivered_turns_and_typed_context','3af1f9938f07aa7d996d12ca489584cfda433e870f829ca7414c9c238f77b25e',CAST(strftime('%s','now') AS INTEGER)*1000);
"#;

const MIGRATION_3: &str = r#"
DROP TRIGGER IF EXISTS delivered_turns_immutable_delete;
DROP TRIGGER IF EXISTS structured_memories_immutable_delete;
DROP TRIGGER IF EXISTS structured_sources_immutable_delete;

CREATE TRIGGER delivered_turns_immutable_delete
BEFORE DELETE ON delivered_turns
WHEN memory_erasure_authorized() <> 1 BEGIN
    SELECT RAISE(ABORT,'delivered turns are append-only outside explicit native erasure');
END;
CREATE TRIGGER structured_memories_immutable_delete
BEFORE DELETE ON structured_memories
WHEN memory_erasure_authorized() <> 1 BEGIN
    SELECT RAISE(ABORT,'structured memories are immutable outside explicit native erasure');
END;
CREATE TRIGGER structured_sources_immutable_delete
BEFORE DELETE ON structured_memory_sources
WHEN memory_erasure_authorized() <> 1 BEGIN
    SELECT RAISE(ABORT,'structured provenance is immutable outside explicit native erasure');
END;

CREATE TABLE IF NOT EXISTS memory_erasure_audit (
    erasure_id TEXT PRIMARY KEY NOT NULL CHECK(length(erasure_id) = 36),
    scope_sha256 TEXT NOT NULL CHECK(length(scope_sha256) = 64),
    delivered_turns_deleted INTEGER NOT NULL CHECK(delivered_turns_deleted >= 0),
    structured_memories_deleted INTEGER NOT NULL CHECK(structured_memories_deleted >= 0),
    legacy_items_deleted INTEGER NOT NULL CHECK(legacy_items_deleted >= 0),
    outbox_jobs_deleted INTEGER NOT NULL CHECK(outbox_jobs_deleted >= 0),
    erased_at_ms INTEGER NOT NULL CHECK(erased_at_ms >= 0)
) STRICT, WITHOUT ROWID;

INSERT OR IGNORE INTO memory_schema_migrations(version,name,checksum,applied_at_ms)
VALUES(3,'explicit_character_memory_erasure','a2bcae6c5ca1e39e4d193c53f934be9490be10088821785f494235b2940d1630',CAST(strftime('%s','now') AS INTEGER)*1000);
"#;

pub fn configure(connection: &Connection) -> Result<(), MemoryError> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA foreign_keys=ON;\
         PRAGMA journal_mode=WAL;\
         PRAGMA synchronous=FULL;\
         PRAGMA temp_store=MEMORY;\
         PRAGMA trusted_schema=OFF;",
    )?;
    Ok(())
}

pub fn migrate(connection: &mut Connection) -> Result<(), MemoryError> {
    configure(connection)?;
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(MemoryError::InvalidData(format!(
            "database schema {current} is newer than supported schema {SCHEMA_VERSION}"
        )));
    }
    if current < 1 {
        let tx = connection.transaction()?;
        tx.execute_batch(MIGRATION_1)?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    if current < 2 {
        let tx = connection.transaction()?;
        tx.execute_batch(MIGRATION_2)?;
        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
    }
    if current < 3 {
        let tx = connection.transaction()?;
        tx.execute_batch(MIGRATION_3)?;
        tx.pragma_update(None, "user_version", 3)?;
        tx.commit()?;
    }
    Ok(())
}
