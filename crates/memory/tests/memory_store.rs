use npc_memory::{
    EmbeddingInput, ImportConflictPolicy, MemoryError, MemoryInput, MemoryNamespace, MemoryScope,
    MemoryStore, OutboxJobInput, OutboxStatus, Provenance, ReindexRequest, RetrievalFilter,
    RetrievalQuery, VectorSearchBackend, Visibility,
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
    assert_eq!(version, 1);
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
