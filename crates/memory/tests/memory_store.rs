use npc_memory::{
    AuthorityScope, CharacterMemoryErasureRequest, ContextQuery, DeliveryDisposition,
    DerivedMemoryInput, EmbeddingInput, GeneratorProvenance, ImportConflictPolicy, KnowledgeClass,
    MemoryCommitBatch, MemoryError, MemoryInput, MemoryNamespace, MemoryScope, MemoryStore,
    OutboxJobInput, OutboxStatus, Provenance, ReindexRequest, RetrievalFilter, RetrievalQuery,
    SkippedTurnReason, SpoilerPolicy, SpoilerScope, StorageFailureKind, TurnCommitInput,
    TurnSpeaker, VectorSearchBackend, Visibility,
};
use rusqlite::Connection;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;

const NOW: i64 = 2_000_000_000_000;

async fn store() -> (TempDir, MemoryStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(directory.path().join("memory.sqlite"))
        .await
        .unwrap();
    (directory, store)
}

fn input(id: &str, namespace: MemoryNamespace, content: &str) -> MemoryInput {
    MemoryInput {
        id: Some(id.to_owned()),
        namespace,
        scope: MemoryScope {
            profile_id: Some("profile-a".to_owned()),
            game_id: Some("game-a".to_owned()),
            character_id: Some("npc-a".to_owned()),
            session_id: Some("session-a".to_owned()),
            save_id: Some("save-a".to_owned()),
        },
        visibility: Visibility::Character,
        content: content.to_owned(),
        provenance: Provenance {
            source_kind: "test".to_owned(),
            source_id: Some("fixture-1".to_owned()),
            ..Provenance::default()
        },
        confidence: 0.9,
        importance: 0.7,
        observed_at_ms: NOW - 1_000,
        expires_at_ms: None,
        expected_revision: None,
    }
}

fn query(text: Option<&str>, embedding: Option<Vec<f32>>) -> RetrievalQuery {
    RetrievalQuery {
        text: text.map(str::to_owned),
        embedding,
        embedding_generation: Some(3),
        filter: RetrievalFilter::default(),
        limit: 10,
        candidate_limit: 100,
        now_ms: NOW,
    }
}

fn authority_scope(user: &str, game: &str, character: &str) -> AuthorityScope {
    AuthorityScope {
        user_id: user.to_owned(),
        profile_id: format!("profile-{user}"),
        game_id: game.to_owned(),
        character_id: Some(character.to_owned()),
        encounter_id: Some("encounter-a".to_owned()),
        session_id: Some("session-a".to_owned()),
        save_id: Some("save-a".to_owned()),
    }
}

fn delivered_turn(
    turn_id: &str,
    scope: AuthorityScope,
    sequence: u64,
    text: &str,
) -> TurnCommitInput {
    TurnCommitInput {
        turn_id: turn_id.to_owned(),
        scope,
        speaker: if sequence.is_multiple_of(2) {
            TurnSpeaker::Npc
        } else {
            TurnSpeaker::Player
        },
        text: text.to_owned(),
        delivery: DeliveryDisposition::Delivered {
            delivered_at_ms: NOW + sequence as i64,
        },
        created_at_ms: NOW,
        sequence,
        cancellation_generation: 7,
        provider_id: Some("fixture-provider".to_owned()),
        delivery_receipt_id: Some(format!("receipt-{turn_id}")),
        provenance: Provenance {
            source_kind: "runtime_delivery_receipt".to_owned(),
            source_id: Some(format!("source-{turn_id}")),
            ..Provenance::default()
        },
    }
}

fn derived_memory(
    id: &str,
    scope: AuthorityScope,
    class: KnowledgeClass,
    spoiler_scope: SpoilerScope,
    content: &str,
    source_turn_ids: Vec<&str>,
) -> DerivedMemoryInput {
    let is_summary = class == KnowledgeClass::LongTermSummary;
    DerivedMemoryInput {
        id: Some(id.to_owned()),
        scope,
        class,
        spoiler_scope,
        content: content.to_owned(),
        provenance: Provenance {
            source_kind: if is_summary {
                "summary_derivation".to_owned()
            } else {
                "curated_character_database".to_owned()
            },
            ..Provenance::default()
        },
        confidence: 0.8,
        importance: 0.7,
        observed_at_ms: NOW,
        expires_at_ms: None,
        source_turn_ids: source_turn_ids.into_iter().map(str::to_owned).collect(),
        generator: is_summary.then(|| GeneratorProvenance {
            provider_id: "local".to_owned(),
            model_id: "summary-fixture".to_owned(),
            model_revision: "sha256:fixture".to_owned(),
            prompt_version: "summary.v1".to_owned(),
        }),
    }
}

#[tokio::test]
async fn creates_wal_strict_schema_and_supports_every_namespace() {
    let (directory, store) = store().await;
    for (index, namespace) in MemoryNamespace::ALL.into_iter().enumerate() {
        let record = store
            .upsert(input(
                &format!("item-{index}"),
                namespace,
                &format!("namespace {namespace} fixture"),
            ))
            .await
            .unwrap();
        assert_eq!(record.namespace, namespace);
        assert_eq!(record.revision, 1);
    }

    let connection = Connection::open(directory.path().join("memory.sqlite")).unwrap();
    let journal: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    let strict: i64 = connection
        .query_row(
            "SELECT strict FROM pragma_table_list WHERE name='memory_items'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal.to_ascii_lowercase(), "wal");
    assert_eq!(strict, 1);
    assert_eq!(version, 3);
}

#[tokio::test]
async fn explicit_character_erasure_is_atomic_scope_bounded_and_durable() {
    let (directory, store) = store().await;
    let target_scope = authority_scope("user-a", "game-a", "npc-a");
    let mut later_session_scope = target_scope.clone();
    later_session_scope.encounter_id = Some("encounter-b".to_owned());
    later_session_scope.session_id = Some("session-b".to_owned());
    later_session_scope.save_id = Some("save-b".to_owned());
    let other_character_scope = authority_scope("user-a", "game-a", "npc-b");

    store
        .commit_batch(MemoryCommitBatch {
            turns: vec![
                delivered_turn("erase-turn", target_scope.clone(), 1, "erase this turn"),
                delivered_turn(
                    "later-session-turn",
                    later_session_scope.clone(),
                    2,
                    "retain until broad character deletion",
                ),
                delivered_turn(
                    "other-character-turn",
                    other_character_scope.clone(),
                    3,
                    "never erase with npc-a",
                ),
            ],
            derived: vec![
                derived_memory(
                    "erase-derived",
                    target_scope.clone(),
                    KnowledgeClass::LongTermSummary,
                    SpoilerScope::UserPrivate,
                    "derived from the erased turn",
                    vec!["erase-turn"],
                ),
                derived_memory(
                    "later-session-derived",
                    later_session_scope.clone(),
                    KnowledgeClass::LongTermSummary,
                    SpoilerScope::UserPrivate,
                    "belongs to the later session",
                    vec!["later-session-turn"],
                ),
            ],
        })
        .await
        .unwrap();

    let mut target_legacy = input(
        "erase-legacy",
        MemoryNamespace::Fact,
        "legacy target record",
    );
    target_legacy.scope.profile_id = Some(target_scope.profile_id.clone());
    target_legacy.scope.game_id = Some(target_scope.game_id.clone());
    target_legacy.scope.character_id = target_scope.character_id.clone();
    target_legacy.scope.session_id = Some("session-a".to_owned());
    target_legacy.scope.save_id = Some("save-a".to_owned());
    store.upsert(target_legacy).await.unwrap();
    let mut other_legacy = input(
        "other-character-legacy",
        MemoryNamespace::Fact,
        "legacy other character record",
    );
    other_legacy.scope.profile_id = Some(target_scope.profile_id.clone());
    other_legacy.scope.game_id = Some(target_scope.game_id.clone());
    other_legacy.scope.character_id = Some("npc-b".to_owned());
    store.upsert(other_legacy).await.unwrap();
    store
        .enqueue_job(OutboxJobInput {
            kind: "derive".to_owned(),
            aggregate_id: Some("erase-turn".to_owned()),
            payload: json!({"turnId":"erase-turn"}),
            max_attempts: 3,
            available_at_ms: NOW,
            dedupe_key: Some("erase-turn-job".to_owned()),
        })
        .await
        .unwrap();

    let before = store
        .character_memory_status(target_scope.clone())
        .await
        .unwrap();
    assert_eq!(before.delivered_turns, 1);
    assert_eq!(before.structured_memories, 1);
    assert_eq!(before.legacy_items, 1);

    let scoped_report = store
        .erase_character_memory(CharacterMemoryErasureRequest {
            scope: target_scope.clone(),
            erased_at_ms: NOW + 100,
        })
        .await
        .unwrap();
    assert_eq!(scoped_report.delivered_turns_deleted, 1);
    assert_eq!(scoped_report.structured_memories_deleted, 1);
    assert_eq!(scoped_report.legacy_items_deleted, 1);
    // The explicit turn job and the legacy row's automatically queued
    // embedding job are both part of the erased aggregate set.
    assert_eq!(scoped_report.outbox_jobs_deleted, 2);
    assert_eq!(scoped_report.scope_sha256.len(), 64);
    assert!(!scoped_report.scope_sha256.contains("npc-a"));
    assert!(store
        .get_delivered_turn("erase-turn")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_derived_memory("erase-derived")
        .await
        .unwrap()
        .is_none());
    assert!(store.get("erase-legacy").await.unwrap().is_none());
    assert!(store
        .get_delivered_turn("later-session-turn")
        .await
        .unwrap()
        .is_some());
    let after_scoped = store
        .character_memory_status(target_scope.clone())
        .await
        .unwrap();
    assert_eq!(after_scoped.delivered_turns, 0);
    assert_eq!(after_scoped.structured_memories, 0);
    assert_eq!(after_scoped.legacy_items, 0);
    assert!(store
        .get_delivered_turn("other-character-turn")
        .await
        .unwrap()
        .is_some());
    assert!(store.get("other-character-legacy").await.unwrap().is_some());

    let mut broad_character_scope = target_scope;
    broad_character_scope.encounter_id = None;
    broad_character_scope.session_id = None;
    broad_character_scope.save_id = None;
    let broad_report = store
        .erase_character_memory(CharacterMemoryErasureRequest {
            scope: broad_character_scope,
            erased_at_ms: NOW + 200,
        })
        .await
        .unwrap();
    assert_eq!(broad_report.delivered_turns_deleted, 1);
    assert_eq!(broad_report.structured_memories_deleted, 1);
    assert!(store
        .get_delivered_turn("later-session-turn")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .get_delivered_turn("other-character-turn")
        .await
        .unwrap()
        .is_some());

    store.flush().await.unwrap();
    let database_path = directory.path().join("memory.sqlite");
    drop(store);
    let reopened = MemoryStore::open(&database_path).await.unwrap();
    assert!(reopened
        .get_delivered_turn("later-session-turn")
        .await
        .unwrap()
        .is_none());
    assert!(reopened.integrity_check().await.unwrap().ok);
    let connection = Connection::open(database_path).unwrap();
    let audit_rows: i64 = connection
        .query_row("SELECT count(*) FROM memory_erasure_audit", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(audit_rows, 2);
    let raw_scope_leaks: i64 = connection
        .query_row(
            "SELECT count(*) FROM memory_erasure_audit WHERE scope_sha256 LIKE '%npc-a%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw_scope_leaks, 0);
}

#[tokio::test]
async fn character_erasure_rejects_missing_character_without_mutating_memory() {
    let (_directory, store) = store().await;
    let scope = authority_scope("user-a", "game-a", "npc-a");
    store
        .commit_turn(delivered_turn(
            "preserved-turn",
            scope.clone(),
            1,
            "preserved",
        ))
        .await
        .unwrap();
    let mut invalid_scope = scope;
    invalid_scope.character_id = None;
    assert!(matches!(
        store
            .erase_character_memory(CharacterMemoryErasureRequest {
                scope: invalid_scope,
                erased_at_ms: NOW,
            })
            .await,
        Err(MemoryError::InvalidData(_))
    ));
    assert!(store
        .get_delivered_turn("preserved-turn")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn explicit_close_releases_sqlite_files_before_native_maintenance() {
    let (directory, store) = store().await;
    store
        .upsert(input(
            "close-fixture",
            MemoryNamespace::Fact,
            "close releases the writer connection",
        ))
        .await
        .unwrap();
    store.close().await.unwrap();
    assert!(matches!(store.get("close-fixture").await, Ok(Some(_))));
    assert!(matches!(
        store
            .upsert(input("after-close", MemoryNamespace::Fact, "must fail"))
            .await,
        Err(MemoryError::WriterUnavailable)
    ));
    let database = directory.path().join("memory.sqlite");
    std::fs::remove_file(&database).unwrap();
    assert!(!database.exists());
}

#[tokio::test]
async fn revisions_are_optimistic_and_content_changes_invalidate_embeddings() {
    let (_directory, store) = store().await;
    let first = store
        .upsert(input("revisioned", MemoryNamespace::Fact, "V knows Jackie"))
        .await
        .unwrap();
    store
        .put_embedding(EmbeddingInput {
            item_id: first.id.clone(),
            generation: 3,
            model_id: "fixture-embedding".to_owned(),
            values: vec![1.0, 0.0, 0.0],
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .get("revisioned")
            .await
            .unwrap()
            .unwrap()
            .embedding_generation,
        Some(3)
    );

    let mut update = input(
        "revisioned",
        MemoryNamespace::Fact,
        "V trusts Jackie completely",
    );
    update.expected_revision = Some(1);
    let second = store.upsert(update).await.unwrap();
    assert_eq!(second.revision, 2);
    assert_eq!(second.embedding_generation, None);

    let mut stale = input("revisioned", MemoryNamespace::Fact, "stale update");
    stale.expected_revision = Some(1);
    assert!(matches!(
        store.upsert(stale).await,
        Err(MemoryError::RevisionConflict {
            expected: 1,
            actual: 2,
            ..
        })
    ));
}

#[tokio::test]
async fn lexical_retrieval_honors_ttl_scope_visibility_and_soft_delete() {
    let (_directory, store) = store().await;
    store
        .upsert(input(
            "visible",
            MemoryNamespace::Lore,
            "The sapphire dragon guards the northern pass",
        ))
        .await
        .unwrap();

    let mut expired = input(
        "expired",
        MemoryNamespace::Lore,
        "The ancient dragon has vanished",
    );
    expired.observed_at_ms = NOW - 10_000;
    expired.expires_at_ms = Some(NOW - 1);
    // Insert while the record is not yet expired, then retrieve at NOW.
    expired.expires_at_ms = Some(NOW - 1);
    store.upsert(expired).await.unwrap();

    let mut private_other = input(
        "other-character",
        MemoryNamespace::Lore,
        "A crimson dragon lives below",
    );
    private_other.scope.character_id = Some("npc-b".to_owned());
    private_other.visibility = Visibility::Private;
    store.upsert(private_other).await.unwrap();

    let mut retrieval = query(Some("dragon"), None);
    retrieval.filter.character_id = Some("npc-a".to_owned());
    retrieval.filter.visibility = vec![Visibility::Character];
    let hits = store.retrieve(retrieval.clone()).await.unwrap();
    assert_eq!(
        hits.iter()
            .map(|hit| hit.record.id.as_str())
            .collect::<Vec<_>>(),
        ["visible"]
    );
    assert_eq!(hits[0].lexical_rank, Some(0));

    assert!(store
        .retrieve(query(Some("term-that-does-not-exist"), None))
        .await
        .unwrap()
        .is_empty());

    assert!(store.soft_delete("visible", NOW + 1).await.unwrap());
    assert!(store.retrieve(retrieval).await.unwrap().is_empty());
    assert_eq!(store.purge_expired(NOW).await.unwrap(), 1);
}

#[tokio::test]
async fn exact_vector_search_and_hybrid_ranking_are_deterministic() {
    let (_directory, store) = store().await;
    for (id, text, vector) in [
        ("alpha", "Jackie values loyalty", vec![1.0, 0.0]),
        ("beta", "A mercenary remembers the heist", vec![0.8, 0.2]),
        ("gamma", "The weather is rainy", vec![0.0, 1.0]),
    ] {
        store
            .upsert(input(id, MemoryNamespace::Episode, text))
            .await
            .unwrap();
        store
            .put_embedding(EmbeddingInput {
                item_id: id.to_owned(),
                generation: 3,
                model_id: "fixture".to_owned(),
                values: vector,
            })
            .await
            .unwrap();
    }
    assert_eq!(
        store.vector_backend().await.unwrap(),
        VectorSearchBackend::ExactCosine
    );

    let retrieval = query(Some("loyalty"), Some(vec![1.0, 0.0]));
    let first = store.retrieve(retrieval.clone()).await.unwrap();
    let second = store.retrieve(retrieval).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first[0].record.id, "alpha");
    assert_eq!(first[0].lexical_rank, Some(0));
    assert_eq!(first[0].semantic_rank, Some(0));
}

#[tokio::test]
async fn outbox_deduplicates_claims_recovers_leases_and_dead_letters() {
    let (_directory, store) = store().await;
    let job = OutboxJobInput {
        kind: "test.job".to_owned(),
        aggregate_id: Some("aggregate".to_owned()),
        payload: json!({"value": 42}),
        available_at_ms: NOW,
        max_attempts: 2,
        dedupe_key: Some("test-job-once".to_owned()),
    };
    let id = store.enqueue_job(job.clone()).await.unwrap();
    assert_eq!(store.enqueue_job(job).await.unwrap(), id);

    let claimed = store
        .claim_jobs("worker-a", NOW, Duration::from_millis(100), 10)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].status, OutboxStatus::Processing);
    assert_eq!(claimed[0].attempts, 1);
    assert!(!store
        .complete_job(id, "wrong-worker", NOW + 1)
        .await
        .unwrap());

    // The expired lease is recovered and can be claimed by another worker.
    let reclaimed = store
        .claim_jobs("worker-b", NOW + 101, Duration::from_secs(1), 10)
        .await
        .unwrap();
    assert_eq!(reclaimed[0].attempts, 2);
    assert_eq!(
        store
            .fail_job(id, "worker-b", NOW + 102, NOW + 200, "permanent failure")
            .await
            .unwrap(),
        OutboxStatus::Dead
    );
    assert!(store
        .claim_jobs("worker-c", NOW + 1_000, Duration::from_secs(1), 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn reindex_rebuilds_fts_and_schedules_stale_embedding_work() {
    let (_directory, store) = store().await;
    store
        .upsert(input("one", MemoryNamespace::Summary, "first summary"))
        .await
        .unwrap();
    store
        .upsert(input("two", MemoryNamespace::Summary, "second summary"))
        .await
        .unwrap();
    store
        .put_embedding(EmbeddingInput {
            item_id: "one".to_owned(),
            generation: 7,
            model_id: "new-model".to_owned(),
            values: vec![0.5, 0.5],
        })
        .await
        .unwrap();

    let report = store
        .reindex(ReindexRequest {
            target_generation: 7,
            model_id: "new-model".to_owned(),
            batch_size: 16,
        })
        .await
        .unwrap();
    assert!(report.fts_rebuilt);
    assert_eq!(report.embedding_jobs_enqueued, 1);
    assert_eq!(report.target_generation, 7);
    assert_eq!(
        store.retrieve(query(Some("second"), None)).await.unwrap()[0]
            .record
            .id,
        "two"
    );
}

#[tokio::test]
async fn import_is_atomic_and_backup_is_openable() {
    let (directory, store) = store().await;
    store
        .upsert(input(
            "existing",
            MemoryNamespace::Profile,
            "original profile",
        ))
        .await
        .unwrap();

    let rejected = store
        .import(
            vec![
                input("new-before-conflict", MemoryNamespace::Lore, "new lore"),
                input("existing", MemoryNamespace::Profile, "replacement"),
            ],
            ImportConflictPolicy::Reject,
        )
        .await;
    assert!(rejected.is_err());
    assert!(store.get("new-before-conflict").await.unwrap().is_none());

    let report = store
        .import(
            vec![
                input("new", MemoryNamespace::Lore, "imported lore"),
                input("existing", MemoryNamespace::Profile, "replacement profile"),
            ],
            ImportConflictPolicy::Replace,
        )
        .await
        .unwrap();
    assert_eq!(report.inserted, 1);
    assert_eq!(report.replaced, 1);

    let backup_path = directory.path().join("backups").join("snapshot.sqlite");
    let backup = store.backup(&backup_path).await.unwrap();
    assert_eq!(backup.destination, backup_path);
    assert!(backup.pages_copied > 0);
    let restored = MemoryStore::open(&backup_path).await.unwrap();
    assert_eq!(
        restored.get("existing").await.unwrap().unwrap().content,
        "replacement profile"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_callers_are_serialized_by_the_single_writer() {
    let (_directory, store) = store().await;
    let mut tasks = Vec::new();
    for index in 0..64 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store
                .upsert(input(
                    &format!("concurrent-{index:02}"),
                    MemoryNamespace::Event,
                    &format!("event number {index}"),
                ))
                .await
                .unwrap()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap().revision, 1);
    }
    let mut retrieval = query(None, None);
    retrieval.limit = 64;
    retrieval.candidate_limit = 64;
    let hits = store.retrieve(retrieval).await.unwrap();
    assert_eq!(hits.len(), 64);
}

#[test]
fn validates_untrusted_boundaries_before_sql() {
    let mut blank = input("blank", MemoryNamespace::Fact, "   ");
    assert!(blank.validate().is_err());
    blank.content = "valid".to_owned();
    blank.confidence = f64::NAN;
    assert!(blank.validate().is_err());

    let zero_embedding = EmbeddingInput {
        item_id: "item".to_owned(),
        generation: 1,
        model_id: "model".to_owned(),
        values: vec![0.0, 0.0],
    };
    assert!(zero_embedding.validate().is_err());
}

#[tokio::test]
async fn authoritative_commit_filters_undelivered_and_preserves_exact_partial_prefix() {
    let (directory, store) = store().await;
    let scope = authority_scope("user-a", "game-a", "npc-a");
    let mut partial = delivered_turn("partial", scope.clone(), 1, "héllo, unfinished tail");
    partial.delivery = DeliveryDisposition::PartiallyDelivered {
        delivered_bytes: "héllo".len(),
        delivered_at_ms: NOW + 1,
    };
    let mut cancelled = delivered_turn("cancelled", scope.clone(), 2, "false history");
    cancelled.delivery = DeliveryDisposition::Cancelled;
    let mut failed = delivered_turn("failed", scope.clone(), 3, "provider failed");
    failed.delivery = DeliveryDisposition::Failed;
    let mut queued = delivered_turn("queued", scope, 4, "not heard yet");
    queued.delivery = DeliveryDisposition::Queued;

    let report = store
        .commit_batch(MemoryCommitBatch {
            turns: vec![partial.clone(), cancelled, failed, queued],
            derived: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(report.stored_turns.len(), 1);
    assert_eq!(report.stored_turns[0].delivered_text, "héllo");
    assert_eq!(
        report
            .skipped_turns
            .iter()
            .map(|turn| turn.reason)
            .collect::<Vec<_>>(),
        [
            SkippedTurnReason::Cancelled,
            SkippedTurnReason::Failed,
            SkippedTurnReason::Queued
        ]
    );

    // Exact retries are idempotent, but a producer cannot rewrite heard history.
    assert_eq!(
        store
            .commit_turn(partial.clone())
            .await
            .unwrap()
            .stored_turns
            .len(),
        1
    );
    partial.text = "different delivered source".to_owned();
    partial.delivery = DeliveryDisposition::Delivered {
        delivered_at_ms: NOW + 1,
    };
    assert!(store.commit_turn(partial).await.is_err());

    let connection = Connection::open(directory.path().join("memory.sqlite")).unwrap();
    let count: i64 = connection
        .query_row("SELECT count(*) FROM delivered_turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert!(connection
        .execute("DELETE FROM delivered_turns WHERE turn_id='partial'", [])
        .is_err());
}

#[tokio::test]
async fn typed_context_is_bounded_deterministic_spoiler_aware_and_namespace_isolated() {
    let (_directory, store) = store().await;
    let scope = authority_scope("user-a", "game-a", "npc-a");
    let other_user_scope = authority_scope("user-b", "game-a", "npc-a");
    let turns = vec![
        delivered_turn("turn-1", scope.clone(), 1, "Player asked about the harbor."),
        delivered_turn(
            "turn-2",
            scope.clone(),
            2,
            "The harbor remembers every ship.",
        ),
        delivered_turn(
            "other-user-turn",
            other_user_scope.clone(),
            3,
            "private other user",
        ),
    ];
    let derived = vec![
        derived_memory(
            "lore",
            scope.clone(),
            KnowledgeClass::WorldLore,
            SpoilerScope::Game,
            "The harbor was founded in 1204.",
            vec![],
        ),
        derived_memory(
            "bio",
            scope.clone(),
            KnowledgeClass::Biography,
            SpoilerScope::CharacterPrivate,
            "Mara captains the night ferry.",
            vec![],
        ),
        derived_memory(
            "knowledge",
            scope.clone(),
            KnowledgeClass::CharacterKnowledge,
            SpoilerScope::Save,
            "Mara knows the player found the brass key.",
            vec!["turn-1"],
        ),
        derived_memory(
            "uncertain",
            scope.clone(),
            KnowledgeClass::UncertainPublicInfo,
            SpoilerScope::None,
            "A rumor says the lighthouse keeper vanished.",
            vec![],
        ),
        derived_memory(
            "summary",
            scope.clone(),
            KnowledgeClass::LongTermSummary,
            SpoilerScope::UserPrivate,
            "The player and Mara discussed the harbor.",
            vec!["turn-1", "turn-2"],
        ),
        derived_memory(
            "other-user-lore",
            other_user_scope,
            KnowledgeClass::WorldLore,
            SpoilerScope::None,
            "This must never cross the user boundary.",
            vec!["other-user-turn"],
        ),
    ];
    store
        .commit_batch(MemoryCommitBatch { turns, derived })
        .await
        .unwrap();

    let query = ContextQuery {
        scope: scope.clone(),
        text: Some("harbor".to_owned()),
        spoiler_policy: SpoilerPolicy::default(),
        recent_turn_limit: 1,
        per_class_limit: 1,
        now_ms: NOW + 1_000,
    };
    let first = store.retrieve_context(query.clone()).await.unwrap();
    let second = store.retrieve_context(query.clone()).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first.recent_dialogue.len(), 1);
    assert_eq!(first.recent_dialogue[0].turn_id, "turn-2");
    assert_eq!(first.world_lore[0].id, "lore");
    assert_eq!(first.biography[0].id, "bio");
    assert_eq!(first.character_knowledge[0].id, "knowledge");
    assert_eq!(first.uncertain_public_info[0].id, "uncertain");
    assert_eq!(
        first.long_term_summaries[0].source_turn_ids,
        ["turn-1", "turn-2"]
    );
    assert!(!format!("{first:?}").contains("other-user"));

    let restricted = store
        .retrieve_context(ContextQuery {
            spoiler_policy: SpoilerPolicy {
                allow_game: false,
                allow_save: false,
                allow_character_private: false,
                allow_user_private: false,
            },
            ..query
        })
        .await
        .unwrap();
    assert!(restricted.world_lore.is_empty());
    assert!(restricted.biography.is_empty());
    assert!(restricted.character_knowledge.is_empty());
    assert_eq!(restricted.uncertain_public_info.len(), 1);
    assert!(restricted.long_term_summaries.is_empty());
}

#[tokio::test]
async fn derived_failure_rolls_back_raw_turns_and_cross_scope_sources_are_rejected() {
    let (_directory, store) = store().await;
    let scope = authority_scope("user-a", "game-a", "npc-a");
    let invalid_summary = derived_memory(
        "bad-summary",
        scope.clone(),
        KnowledgeClass::LongTermSummary,
        SpoilerScope::UserPrivate,
        "This source does not exist.",
        vec!["missing-turn"],
    );
    assert!(store
        .commit_batch(MemoryCommitBatch {
            turns: vec![delivered_turn("rolled-back", scope.clone(), 1, "heard")],
            derived: vec![invalid_summary],
        })
        .await
        .is_err());
    let empty = store
        .retrieve_context(ContextQuery {
            scope: scope.clone(),
            text: None,
            spoiler_policy: SpoilerPolicy::default(),
            recent_turn_limit: 10,
            per_class_limit: 10,
            now_ms: NOW + 10,
        })
        .await
        .unwrap();
    assert!(empty.recent_dialogue.is_empty());

    let other_scope = authority_scope("other-user", "game-a", "npc-a");
    store
        .commit_turn(delivered_turn(
            "other-source",
            other_scope,
            1,
            "other user's turn",
        ))
        .await
        .unwrap();
    let cross_scope = derived_memory(
        "cross-scope",
        scope,
        KnowledgeClass::LongTermSummary,
        SpoilerScope::UserPrivate,
        "invalid cross-scope summary",
        vec!["other-source"],
    );
    assert!(store.derive_memory(cross_scope).await.is_err());
}

#[tokio::test]
async fn backup_integrity_flush_recovery_and_corruption_are_explicit() {
    let (directory, store) = store().await;
    let scope = authority_scope("user-a", "game-a", "npc-a");
    store
        .commit_turn(delivered_turn("durable", scope.clone(), 1, "durable turn"))
        .await
        .unwrap();
    let flush = store.flush().await.unwrap();
    assert!(!flush.busy);
    let integrity = store.integrity_check().await.unwrap();
    assert!(integrity.ok, "{:?}", integrity.messages);
    assert_eq!(integrity.delivered_turns, 1);

    let backup = directory.path().join("backup.sqlite");
    store.backup(&backup).await.unwrap();
    drop(store);
    let destination = directory.path().join("restored.sqlite");
    std::fs::write(&destination, b"not a sqlite database").unwrap();
    let corrupt = match MemoryStore::open(&destination).await {
        Ok(_) => panic!("corrupt database unexpectedly opened"),
        Err(error) => error,
    };
    assert_eq!(
        corrupt.storage_failure_kind(),
        Some(StorageFailureKind::Corrupt)
    );
    std::fs::write(format!("{}-wal", destination.display()), b"stale wal").unwrap();
    std::fs::write(format!("{}-shm", destination.display()), b"stale shm").unwrap();
    let recovery = MemoryStore::recover_from_backup(&backup, &destination, true)
        .await
        .unwrap();
    let quarantined = recovery.quarantined_database.unwrap();
    assert!(quarantined.exists());
    let quarantine_directory = quarantined.parent().unwrap();
    assert!(quarantine_directory.join("restored.sqlite-wal").exists());
    assert!(quarantine_directory.join("restored.sqlite-shm").exists());
    assert!(recovery.integrity.ok);
    let restored = MemoryStore::open(&destination).await.unwrap();
    let context = restored
        .retrieve_context(ContextQuery {
            scope,
            text: None,
            spoiler_policy: SpoilerPolicy::default(),
            recent_turn_limit: 10,
            per_class_limit: 10,
            now_ms: NOW + 10,
        })
        .await
        .unwrap();
    assert_eq!(context.recent_dialogue[0].turn_id, "durable");
}

#[tokio::test]
async fn integrity_check_detects_migration_ledger_tampering() {
    let (directory, store) = store().await;
    let connection = Connection::open(directory.path().join("memory.sqlite")).unwrap();
    connection
        .execute(
            "UPDATE memory_schema_migrations SET checksum=?1 WHERE version=2",
            ["0".repeat(64)],
        )
        .unwrap();
    drop(connection);
    let report = store.integrity_check().await.unwrap();
    assert!(!report.ok);
    assert!(report
        .messages
        .iter()
        .any(|message| message.contains("migration 2 checksum")));
}

#[tokio::test]
async fn schema_one_database_migrates_without_rewriting_legacy_authority() {
    let (directory, store) = store().await;
    store
        .upsert(input(
            "legacy-source",
            MemoryNamespace::Lore,
            "preserved legacy source",
        ))
        .await
        .unwrap();
    store.flush().await.unwrap();
    let path = directory.path().join("memory.sqlite");
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE structured_memory_sources;
             DROP TABLE structured_memories;
             DROP TABLE delivered_turns;
             DROP TABLE memory_schema_migrations;
             PRAGMA user_version=1;",
        )
        .unwrap();
    drop(connection);

    let migrated = MemoryStore::open(&path).await.unwrap();
    assert_eq!(
        migrated
            .get("legacy-source")
            .await
            .unwrap()
            .unwrap()
            .content,
        "preserved legacy source"
    );
    let integrity = migrated.integrity_check().await.unwrap();
    assert!(integrity.ok, "{:?}", integrity.messages);
    assert_eq!(integrity.schema_version, 3);
    migrated
        .commit_turn(delivered_turn(
            "post-migration",
            authority_scope("user-a", "game-a", "npc-a"),
            1,
            "new authority works",
        ))
        .await
        .unwrap();
}

#[test]
fn database_full_errors_have_a_stable_recovery_category() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("PRAGMA page_size=512;").unwrap();
    connection.pragma_update(None, "max_page_count", 1).unwrap();
    let error = connection
        .execute_batch(
            "CREATE TABLE too_large(value BLOB); INSERT INTO too_large VALUES(zeroblob(8192));",
        )
        .unwrap_err();
    assert_eq!(
        MemoryError::from(error).storage_failure_kind(),
        Some(StorageFailureKind::DiskFull)
    );
}
