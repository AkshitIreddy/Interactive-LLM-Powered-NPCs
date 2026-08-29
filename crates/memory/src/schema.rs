use crate::MemoryError;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

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

pub fn configure(connection: &Connection) -> Result<(), MemoryError> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA foreign_keys=ON;\
         PRAGMA journal_mode=WAL;\
         PRAGMA synchronous=NORMAL;\
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
    Ok(())
}
