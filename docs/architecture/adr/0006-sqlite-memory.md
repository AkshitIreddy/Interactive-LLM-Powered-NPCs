# ADR-0006: SQLite as the authoritative memory store

Status: Accepted  
Date: 2026-08-28

## Context

A local desktop app needs transactional persistence, migrations, backup, metadata filtering and low overhead without a server. Version 1 commits mutable JSON and Chroma indexes, mixes authority types and trusts pickle metadata.

## Decision

Use one per-user SQLite database in WAL mode with versioned migrations and STRICT authoritative tables. Use FTS5 for lexical retrieval and a pinned sqlite-vec extension for optional semantic retrieval. Embeddings/indexes are derived data namespaced by source revision, embedding model and dimension; they can be dropped/rebuilt without losing source content.

Keep separate tables/scopes for profiles/canon, characters, immutable turns/delivery segments, working sessions, episodic memories, facts, relationships, summaries and quest/save state. Every derived record references source rows and generator/model version. A transaction commits only the delivered part of a reply.

## Consequences

- Backup/migration/recovery use one authoritative database and deterministic content packs.
- WAL improves reader/writer coexistence but requires checkpoint, disk-full and copied-database tests.
- sqlite-vec must be pinned, built reproducibly and loadable only from an application-owned verified path.
- Chroma/pickle state is not migrated; accepted text is re-embedded.

## Rejected alternatives

- **Chroma:** unnecessary packaging/server/library surface and opaque legacy binary/pickle migration risk for this local scale.
- **Separate vector service:** adds lifecycle/ports/resource cost without an established scale need.
- **JSON files:** weak transactions, migrations, querying and concurrent access.

## Evidence

- SQLite WAL: <https://www.sqlite.org/wal.html>
- SQLite STRICT tables: <https://www.sqlite.org/stricttables.html>
- SQLite FTS5: <https://www.sqlite.org/fts5.html>
- sqlite-vec: <https://github.com/asg017/sqlite-vec>

