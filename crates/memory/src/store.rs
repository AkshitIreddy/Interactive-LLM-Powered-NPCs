use crate::domain::*;
use crate::retrieval::{cosine_similarity, rank_candidates, rank_map, CandidateRanks};
use crate::schema;
use rusqlite::types::Type;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row, Transaction};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StoreOptions {
    pub busy_timeout: Duration,
    pub create_parent_directories: bool,
    /// Bounded mutation queue. Saturation is reported to callers instead of
    /// consuming unbounded memory or blocking an async executor thread.
    pub writer_queue_capacity: usize,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(5),
            create_parent_directories: true,
            writer_queue_capacity: 1_024,
        }
    }
}

#[derive(Clone)]
pub struct MemoryStore {
    inner: std::sync::Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    options: StoreOptions,
    writer: std_mpsc::SyncSender<WriterCommand>,
}

enum WriterCommand {
    Upsert {
        input: Box<MemoryInput>,
        response: oneshot::Sender<Result<MemoryRecord, MemoryError>>,
    },
    Delete {
        id: String,
        deleted_at_ms: i64,
        response: oneshot::Sender<Result<bool, MemoryError>>,
    },
    PutEmbedding {
        embedding: EmbeddingInput,
        response: oneshot::Sender<Result<(), MemoryError>>,
    },
    Enqueue {
        job: OutboxJobInput,
        response: oneshot::Sender<Result<i64, MemoryError>>,
    },
    Claim {
        worker: String,
        now_ms: i64,
        lease_ms: i64,
        limit: usize,
        response: oneshot::Sender<Result<Vec<OutboxJob>, MemoryError>>,
    },
    Complete {
        id: i64,
        worker: String,
        now_ms: i64,
        response: oneshot::Sender<Result<bool, MemoryError>>,
    },
    Fail {
        id: i64,
        worker: String,
        now_ms: i64,
        retry_at_ms: i64,
        error: String,
        response: oneshot::Sender<Result<OutboxStatus, MemoryError>>,
    },
    Reindex {
        request: ReindexRequest,
        now_ms: i64,
        response: oneshot::Sender<Result<ReindexReport, MemoryError>>,
    },
    Import {
        records: Vec<MemoryInput>,
        policy: ImportConflictPolicy,
        now_ms: i64,
        response: oneshot::Sender<Result<ImportReport, MemoryError>>,
    },
    Backup {
        destination: PathBuf,
        response: oneshot::Sender<Result<BackupReport, MemoryError>>,
    },
    PurgeExpired {
        now_ms: i64,
        response: oneshot::Sender<Result<usize, MemoryError>>,
    },
}

impl MemoryStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, MemoryError> {
        Self::open_with_options(path, StoreOptions::default()).await
    }

    pub async fn open_with_options(
        path: impl AsRef<Path>,
        options: StoreOptions,
    ) -> Result<Self, MemoryError> {
        let path = path.as_ref().to_path_buf();
        if options.writer_queue_capacity == 0 {
            return Err(MemoryError::InvalidData(
                "writer_queue_capacity must be greater than zero".to_owned(),
            ));
        }
        if options.create_parent_directories {
            if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
        }

        let (writer, receiver) = std_mpsc::sync_channel(options.writer_queue_capacity);
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let writer_path = path.clone();
        let busy_timeout = options.busy_timeout;
        std::thread::Builder::new()
            .name("npc-memory-writer".to_owned())
            .spawn(move || writer_main(writer_path, busy_timeout, receiver, ready_tx))
            .map_err(MemoryError::Io)?;

        ready_rx
            .recv()
            .map_err(|_| MemoryError::WriterUnavailable)??;

        Ok(Self {
            inner: std::sync::Arc::new(Inner {
                path,
                options,
                writer,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub async fn upsert(&self, input: MemoryInput) -> Result<MemoryRecord, MemoryError> {
        input.validate()?;
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Upsert {
            input: Box::new(input),
            response,
        })?;
        receive(receiver).await
    }

    pub async fn soft_delete(
        &self,
        id: impl Into<String>,
        deleted_at_ms: i64,
    ) -> Result<bool, MemoryError> {
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Delete {
            id: id.into(),
            deleted_at_ms,
            response,
        })?;
        receive(receiver).await
    }

    pub async fn put_embedding(&self, embedding: EmbeddingInput) -> Result<(), MemoryError> {
        embedding.validate()?;
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::PutEmbedding {
            embedding,
            response,
        })?;
        receive(receiver).await
    }

    pub async fn get(&self, id: impl Into<String>) -> Result<Option<MemoryRecord>, MemoryError> {
        let id = id.into();
        self.read(move |connection| load_record(connection, &id))
            .await
    }

    pub async fn retrieve(&self, query: RetrievalQuery) -> Result<Vec<SearchHit>, MemoryError> {
        query.validate()?;
        self.read(move |connection| retrieve_on(connection, query))
            .await
    }

    pub async fn enqueue_job(&self, job: OutboxJobInput) -> Result<i64, MemoryError> {
        validate_job(&job)?;
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Enqueue { job, response })?;
        receive(receiver).await
    }

    /// Claims jobs in stable ID order. Expired leases are recovered before the
    /// claim and attempts are incremented atomically with lease acquisition.
    pub async fn claim_jobs(
        &self,
        worker: impl Into<String>,
        now_ms: i64,
        lease: Duration,
        limit: usize,
    ) -> Result<Vec<OutboxJob>, MemoryError> {
        let worker = worker.into();
        if worker.trim().is_empty() || limit == 0 || limit > 1_000 {
            return Err(MemoryError::InvalidData(
                "worker is required and claim limit must be in 1..=1000".to_owned(),
            ));
        }
        let lease_ms = i64::try_from(lease.as_millis())
            .map_err(|_| MemoryError::InvalidData("lease is too long".to_owned()))?;
        if lease_ms <= 0 {
            return Err(MemoryError::InvalidData(
                "lease must be greater than zero".to_owned(),
            ));
        }
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Claim {
            worker,
            now_ms,
            lease_ms,
            limit,
            response,
        })?;
        receive(receiver).await
    }

    pub async fn complete_job(
        &self,
        id: i64,
        worker: impl Into<String>,
        now_ms: i64,
    ) -> Result<bool, MemoryError> {
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Complete {
            id,
            worker: worker.into(),
            now_ms,
            response,
        })?;
        receive(receiver).await
    }

    pub async fn fail_job(
        &self,
        id: i64,
        worker: impl Into<String>,
        now_ms: i64,
        retry_at_ms: i64,
        error: impl Into<String>,
    ) -> Result<OutboxStatus, MemoryError> {
        let error = error.into();
        if error.trim().is_empty() || error.len() > 16_384 || retry_at_ms < now_ms {
            return Err(MemoryError::InvalidData(
                "failure requires a bounded error and a non-past retry time".to_owned(),
            ));
        }
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Fail {
            id,
            worker: worker.into(),
            now_ms,
            retry_at_ms,
            error,
            response,
        })?;
        receive(receiver).await
    }

    pub async fn reindex(&self, request: ReindexRequest) -> Result<ReindexReport, MemoryError> {
        if request.target_generation < 0
            || request.model_id.trim().is_empty()
            || request.batch_size == 0
            || request.batch_size > 10_000
        {
            return Err(MemoryError::InvalidData(
                "invalid reindex generation, model, or batch size".to_owned(),
            ));
        }
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Reindex {
            request,
            now_ms: unix_time_ms(),
            response,
        })?;
        receive(receiver).await
    }

    pub async fn import(
        &self,
        records: Vec<MemoryInput>,
        policy: ImportConflictPolicy,
    ) -> Result<ImportReport, MemoryError> {
        for record in &records {
            record.validate()?;
            if record.id.as_deref().unwrap_or_default().is_empty() {
                return Err(MemoryError::InvalidData(
                    "imports require a stable non-empty record ID".to_owned(),
                ));
            }
        }
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Import {
            records,
            policy,
            now_ms: unix_time_ms(),
            response,
        })?;
        receive(receiver).await
    }

    pub async fn backup(&self, destination: impl AsRef<Path>) -> Result<BackupReport, MemoryError> {
        let destination = destination.as_ref().to_path_buf();
        if destination == self.inner.path {
            return Err(MemoryError::InvalidData(
                "backup destination cannot be the live database".to_owned(),
            ));
        }
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Backup {
            destination,
            response,
        })?;
        receive(receiver).await
    }

    pub async fn purge_expired(&self, now_ms: i64) -> Result<usize, MemoryError> {
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::PurgeExpired { now_ms, response })?;
        receive(receiver).await
    }

    pub async fn vector_backend(&self) -> Result<VectorSearchBackend, MemoryError> {
        self.read(|connection| {
            #[cfg(feature = "sqlite-vec")]
            {
                let available = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM pragma_module_list WHERE name = 'vec0')",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                    .unwrap_or(false);
                if available {
                    return Ok(VectorSearchBackend::SqliteVec);
                }
            }
            let _ = connection;
            Ok(VectorSearchBackend::ExactCosine)
        })
        .await
    }

    fn send(&self, command: WriterCommand) -> Result<(), MemoryError> {
        match self.inner.writer.try_send(command) {
            Ok(()) => Ok(()),
            Err(std_mpsc::TrySendError::Full(_)) => Err(MemoryError::WriterBackpressure),
            Err(std_mpsc::TrySendError::Disconnected(_)) => Err(MemoryError::WriterUnavailable),
        }
    }

    async fn read<T, F>(&self, operation: F) -> Result<T, MemoryError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, MemoryError> + Send + 'static,
    {
        let path = self.inner.path.clone();
        let timeout = self.inner.options.busy_timeout;
        tokio::task::spawn_blocking(move || {
            let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI;
            let connection = Connection::open_with_flags(path, flags)?;
            connection.busy_timeout(timeout)?;
            connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
            operation(&connection)
        })
        .await
        .map_err(|error| MemoryError::BackgroundTask(error.to_string()))?
    }
}

async fn receive<T>(receiver: oneshot::Receiver<Result<T, MemoryError>>) -> Result<T, MemoryError> {
    receiver.await.map_err(|_| MemoryError::WriterUnavailable)?
}

fn writer_main(
    path: PathBuf,
    busy_timeout: Duration,
    receiver: std_mpsc::Receiver<WriterCommand>,
    ready: std_mpsc::SyncSender<Result<(), MemoryError>>,
) {
    let connection = Connection::open(&path).and_then(|mut connection| {
        connection.busy_timeout(busy_timeout)?;
        schema::migrate(&mut connection).map_err(|error| match error {
            MemoryError::Database(error) => error,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        })?;
        Ok(connection)
    });

    let mut connection = match connection {
        Ok(connection) => {
            let _ = ready.send(Ok(()));
            connection
        }
        Err(error) => {
            let _ = ready.send(Err(MemoryError::Database(error)));
            return;
        }
    };

    while let Ok(command) = receiver.recv() {
        match command {
            WriterCommand::Upsert { input, response } => {
                let _ = response.send(upsert_record(&mut connection, *input, unix_time_ms()));
            }
            WriterCommand::Delete {
                id,
                deleted_at_ms,
                response,
            } => {
                let result = connection
                    .execute(
                        "UPDATE memory_items SET deleted_at_ms=?2, updated_at_ms=?2, revision=revision+1 WHERE id=?1 AND deleted_at_ms IS NULL",
                        params![id, deleted_at_ms],
                    )
                    .map(|changed| changed > 0)
                    .map_err(MemoryError::from);
                let _ = response.send(result);
            }
            WriterCommand::PutEmbedding {
                embedding,
                response,
            } => {
                let _ = response.send(put_embedding(&mut connection, embedding, unix_time_ms()));
            }
            WriterCommand::Enqueue { job, response } => {
                let _ = response.send(enqueue_job(&mut connection, job, unix_time_ms()));
            }
            WriterCommand::Claim {
                worker,
                now_ms,
                lease_ms,
                limit,
                response,
            } => {
                let _ = response.send(claim_jobs(&mut connection, worker, now_ms, lease_ms, limit));
            }
            WriterCommand::Complete {
                id,
                worker,
                now_ms,
                response,
            } => {
                let result = connection
                    .execute(
                        "UPDATE memory_outbox SET status='completed', lease_owner=NULL, lease_expires_at_ms=NULL, updated_at_ms=?3 WHERE id=?1 AND status='processing' AND lease_owner=?2",
                        params![id, worker, now_ms],
                    )
                    .map(|changed| changed > 0)
                    .map_err(MemoryError::from);
                let _ = response.send(result);
            }
            WriterCommand::Fail {
                id,
                worker,
                now_ms,
                retry_at_ms,
                error,
                response,
            } => {
                let _ = response.send(fail_job(
                    &mut connection,
                    id,
                    worker,
                    now_ms,
                    retry_at_ms,
                    error,
                ));
            }
            WriterCommand::Reindex {
                request,
                now_ms,
                response,
            } => {
                let _ = response.send(reindex(&mut connection, request, now_ms));
            }
            WriterCommand::Import {
                records,
                policy,
                now_ms,
                response,
            } => {
                let _ = response.send(import_records(&mut connection, records, policy, now_ms));
            }
            WriterCommand::Backup {
                destination,
                response,
            } => {
                let _ = response.send(backup_database(&connection, destination));
            }
            WriterCommand::PurgeExpired { now_ms, response } => {
                let result = connection
                    .execute(
                        "UPDATE memory_items SET deleted_at_ms=?1, updated_at_ms=?1, revision=revision+1 WHERE deleted_at_ms IS NULL AND expires_at_ms IS NOT NULL AND expires_at_ms <= ?1",
                        [now_ms],
                    )
                    .map_err(MemoryError::from);
                let _ = response.send(result);
            }
        }
    }
}

fn upsert_record(
    connection: &mut Connection,
    mut input: MemoryInput,
    now_ms: i64,
) -> Result<MemoryRecord, MemoryError> {
    input.validate()?;
    let tx = connection.transaction()?;
    let record = upsert_in_transaction(&tx, &mut input, now_ms)?;
    tx.commit()?;
    Ok(record)
}

fn upsert_in_transaction(
    tx: &Transaction<'_>,
    input: &mut MemoryInput,
    now_ms: i64,
) -> Result<MemoryRecord, MemoryError> {
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.trim().is_empty() || id.len() > 512 {
        return Err(MemoryError::InvalidData(
            "record ID must be non-empty and at most 512 bytes".to_owned(),
        ));
    }
    let existing = tx
        .query_row(
            "SELECT revision, content_sha256 FROM memory_items WHERE id=?1",
            [&id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some(expected) = input.expected_revision {
        let actual = existing.as_ref().map(|value| value.0).unwrap_or(0);
        if actual != expected {
            return Err(MemoryError::RevisionConflict {
                id,
                expected,
                actual,
            });
        }
    }

    let content = input.content.trim().to_owned();
    let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
    let provenance = serde_json::to_string(&input.provenance)?;
    let content_changed = existing
        .as_ref()
        .map(|value| value.1 != content_hash)
        .unwrap_or(true);

    tx.execute(
        r#"INSERT INTO memory_items(
            id, namespace, profile_id, game_id, character_id, session_id, save_id,
            visibility, content, content_sha256, provenance_json, confidence,
            importance, observed_at_ms, created_at_ms, updated_at_ms,
            expires_at_ms, embedding_generation, revision, deleted_at_ms
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?15,?16,NULL,1,NULL)
        ON CONFLICT(id) DO UPDATE SET
            namespace=excluded.namespace, profile_id=excluded.profile_id,
            game_id=excluded.game_id, character_id=excluded.character_id,
            session_id=excluded.session_id, save_id=excluded.save_id,
            visibility=excluded.visibility, content=excluded.content,
            content_sha256=excluded.content_sha256,
            provenance_json=excluded.provenance_json, confidence=excluded.confidence,
            importance=excluded.importance, observed_at_ms=excluded.observed_at_ms,
            updated_at_ms=excluded.updated_at_ms, expires_at_ms=excluded.expires_at_ms,
            embedding_generation=CASE WHEN memory_items.content_sha256=excluded.content_sha256 THEN memory_items.embedding_generation ELSE NULL END,
            revision=memory_items.revision+1, deleted_at_ms=NULL"#,
        params![
            id,
            input.namespace.as_str(),
            input.scope.profile_id,
            input.scope.game_id,
            input.scope.character_id,
            input.scope.session_id,
            input.scope.save_id,
            input.visibility.as_str(),
            content,
            content_hash,
            provenance,
            input.confidence,
            input.importance,
            input.observed_at_ms,
            now_ms,
            input.expires_at_ms,
        ],
    )?;

    if content_changed {
        tx.execute("DELETE FROM memory_embeddings WHERE item_id=?1", [&id])?;
    }
    let record = load_record_tx(tx, &id)?.ok_or_else(|| MemoryError::NotFound(id.clone()))?;
    let payload = json!({
        "item_id": id,
        "revision": record.revision,
        "content_sha256": record.content_sha256,
    });
    enqueue_job_tx(
        tx,
        &OutboxJobInput {
            kind: "memory.embedding.requested".to_owned(),
            aggregate_id: Some(record.id.clone()),
            payload,
            available_at_ms: now_ms,
            max_attempts: 5,
            dedupe_key: Some(format!("embedding:{}:{}", record.id, record.revision)),
        },
        now_ms,
    )?;
    Ok(record)
}

fn put_embedding(
    connection: &mut Connection,
    embedding: EmbeddingInput,
    now_ms: i64,
) -> Result<(), MemoryError> {
    embedding.validate()?;
    let norm = embedding
        .values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    let bytes = encode_f32(&embedding.values);
    let tx = connection.transaction()?;
    let changed = tx.execute(
        "INSERT INTO memory_embeddings(item_id,generation,model_id,dimensions,vector,l2_norm,created_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(item_id,generation) DO UPDATE SET model_id=excluded.model_id,dimensions=excluded.dimensions,vector=excluded.vector,l2_norm=excluded.l2_norm,created_at_ms=excluded.created_at_ms",
        params![embedding.item_id, embedding.generation, embedding.model_id, embedding.values.len() as i64, bytes, norm, now_ms],
    );
    if let Err(rusqlite::Error::SqliteFailure(error, _)) = &changed {
        if error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY {
            return Err(MemoryError::NotFound(embedding.item_id));
        }
    }
    changed?;
    tx.execute(
        "UPDATE memory_items SET embedding_generation=?2 WHERE id=?1 AND deleted_at_ms IS NULL",
        params![embedding.item_id, embedding.generation],
    )?;
    tx.commit()?;
    Ok(())
}

fn enqueue_job(
    connection: &mut Connection,
    job: OutboxJobInput,
    now_ms: i64,
) -> Result<i64, MemoryError> {
    validate_job(&job)?;
    let tx = connection.transaction()?;
    let id = enqueue_job_tx(&tx, &job, now_ms)?;
    tx.commit()?;
    Ok(id)
}

fn enqueue_job_tx(
    tx: &Transaction<'_>,
    job: &OutboxJobInput,
    now_ms: i64,
) -> Result<i64, MemoryError> {
    let payload = serde_json::to_string(&job.payload)?;
    tx.execute(
        "INSERT INTO memory_outbox(kind,aggregate_id,payload_json,status,attempts,max_attempts,available_at_ms,dedupe_key,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,'pending',0,?4,?5,?6,?7,?7) ON CONFLICT(dedupe_key) DO NOTHING",
        params![job.kind, job.aggregate_id, payload, job.max_attempts, job.available_at_ms, job.dedupe_key, now_ms],
    )?;
    if let Some(key) = &job.dedupe_key {
        Ok(tx.query_row(
            "SELECT id FROM memory_outbox WHERE dedupe_key=?1",
            [key],
            |row| row.get(0),
        )?)
    } else {
        Ok(tx.last_insert_rowid())
    }
}

fn validate_job(job: &OutboxJobInput) -> Result<(), MemoryError> {
    if job.kind.trim().is_empty()
        || job.kind.len() > 256
        || job.max_attempts == 0
        || job.max_attempts > 1_000
    {
        return Err(MemoryError::InvalidData(
            "job kind and max_attempts are invalid".to_owned(),
        ));
    }
    let payload_len = serde_json::to_vec(&job.payload)?.len();
    if payload_len > 1_048_576 {
        return Err(MemoryError::InvalidData(
            "job payload exceeds 1 MiB".to_owned(),
        ));
    }
    Ok(())
}

fn claim_jobs(
    connection: &mut Connection,
    worker: String,
    now_ms: i64,
    lease_ms: i64,
    limit: usize,
) -> Result<Vec<OutboxJob>, MemoryError> {
    let tx = connection.transaction()?;
    tx.execute(
        "UPDATE memory_outbox SET status=CASE WHEN attempts>=max_attempts THEN 'dead' ELSE 'pending' END, lease_owner=NULL, lease_expires_at_ms=NULL, available_at_ms=?1, updated_at_ms=?1, last_error=COALESCE(last_error,'worker lease expired') WHERE status='processing' AND lease_expires_at_ms<=?1",
        [now_ms],
    )?;
    let ids = {
        let mut statement = tx.prepare(
            "SELECT id FROM memory_outbox WHERE status='pending' AND available_at_ms<=?1 AND attempts<max_attempts ORDER BY available_at_ms,id LIMIT ?2",
        )?;
        let ids = statement
            .query_map(params![now_ms, limit as i64], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    let lease_expires = now_ms.saturating_add(lease_ms);
    for id in &ids {
        tx.execute(
            "UPDATE memory_outbox SET status='processing',attempts=attempts+1,lease_owner=?2,lease_expires_at_ms=?3,updated_at_ms=?4 WHERE id=?1 AND status='pending'",
            params![id, worker, lease_expires, now_ms],
        )?;
    }
    let mut jobs = Vec::with_capacity(ids.len());
    for id in ids {
        jobs.push(load_job_tx(&tx, id)?);
    }
    tx.commit()?;
    Ok(jobs)
}

fn fail_job(
    connection: &mut Connection,
    id: i64,
    worker: String,
    now_ms: i64,
    retry_at_ms: i64,
    error: String,
) -> Result<OutboxStatus, MemoryError> {
    let tx = connection.transaction()?;
    let attempts = tx
        .query_row(
            "SELECT attempts,max_attempts FROM memory_outbox WHERE id=?1 AND status='processing' AND lease_owner=?2",
            params![id, worker],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?)),
        )
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("leased outbox job {id}")))?;
    let status = if attempts.0 >= attempts.1 {
        OutboxStatus::Dead
    } else {
        OutboxStatus::Pending
    };
    tx.execute(
        "UPDATE memory_outbox SET status=?2,available_at_ms=?3,lease_owner=NULL,lease_expires_at_ms=NULL,last_error=?4,updated_at_ms=?5 WHERE id=?1",
        params![id, status.as_str(), retry_at_ms, error, now_ms],
    )?;
    tx.commit()?;
    Ok(status)
}

fn reindex(
    connection: &mut Connection,
    request: ReindexRequest,
    now_ms: i64,
) -> Result<ReindexReport, MemoryError> {
    let tx = connection.transaction()?;
    tx.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", [])?;
    let ids = {
        let mut statement = tx.prepare(
            "SELECT id,revision,content_sha256 FROM memory_items WHERE deleted_at_ms IS NULL AND (expires_at_ms IS NULL OR expires_at_ms>?1) AND (embedding_generation IS NULL OR embedding_generation<>?2) ORDER BY id",
        )?;
        let ids = statement
            .query_map(params![now_ms, request.target_generation], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    for (id, revision, hash) in &ids {
        enqueue_job_tx(
            &tx,
            &OutboxJobInput {
                kind: "memory.embedding.requested".to_owned(),
                aggregate_id: Some(id.clone()),
                payload: json!({
                    "item_id": id,
                    "revision": revision,
                    "content_sha256": hash,
                    "generation": request.target_generation,
                    "model_id": request.model_id,
                    "batch_size": request.batch_size,
                }),
                available_at_ms: now_ms,
                max_attempts: 5,
                dedupe_key: Some(format!(
                    "embedding:{id}:{revision}:{}",
                    request.target_generation
                )),
            },
            now_ms,
        )?;
    }
    tx.commit()?;
    Ok(ReindexReport {
        fts_rebuilt: true,
        embedding_jobs_enqueued: ids.len(),
        target_generation: request.target_generation,
    })
}

fn import_records(
    connection: &mut Connection,
    records: Vec<MemoryInput>,
    policy: ImportConflictPolicy,
    now_ms: i64,
) -> Result<ImportReport, MemoryError> {
    let tx = connection.transaction()?;
    let mut report = ImportReport::default();
    for mut input in records {
        let id = input.id.as_deref().unwrap_or_default();
        let exists = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_items WHERE id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )?;
        match (exists, policy) {
            (true, ImportConflictPolicy::Reject) => {
                return Err(MemoryError::InvalidData(format!(
                    "import conflicts with existing record {id}"
                )));
            }
            (true, ImportConflictPolicy::KeepExisting) => {
                report.skipped += 1;
            }
            (true, ImportConflictPolicy::Replace) => {
                input.expected_revision = None;
                upsert_in_transaction(&tx, &mut input, now_ms)?;
                report.replaced += 1;
            }
            (false, _) => {
                input.expected_revision = Some(0);
                upsert_in_transaction(&tx, &mut input, now_ms)?;
                report.inserted += 1;
            }
        }
    }
    tx.commit()?;
    Ok(report)
}

fn backup_database(
    connection: &Connection,
    destination: PathBuf,
) -> Result<BackupReport, MemoryError> {
    if destination.exists() {
        return Err(MemoryError::InvalidData(format!(
            "backup destination already exists: {}",
            destination.display()
        )));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut target = Connection::open(&destination)?;
    let backup = rusqlite::backup::Backup::new(connection, &mut target)?;
    backup.run_to_completion(128, Duration::from_millis(5), None)?;
    let pages_copied = backup.progress().pagecount;
    drop(backup);
    target.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(BackupReport {
        destination,
        pages_copied,
    })
}

fn retrieve_on(
    connection: &Connection,
    query: RetrievalQuery,
) -> Result<Vec<SearchHit>, MemoryError> {
    let lexical_ids = if let Some(text) = query.text.as_deref() {
        lexical_candidates(connection, text, query.now_ms, query.candidate_limit)?
    } else {
        Vec::new()
    };
    let semantic_ids = if let Some(query_vector) = query.embedding.as_deref() {
        semantic_candidates(
            connection,
            query_vector,
            query.embedding_generation,
            query.now_ms,
            query.candidate_limit,
        )?
    } else {
        Vec::new()
    };
    let mut candidate_ids = BTreeSet::new();
    candidate_ids.extend(lexical_ids.iter().cloned());
    candidate_ids.extend(semantic_ids.iter().cloned());
    if candidate_ids.is_empty() && query.text.is_none() && query.embedding.is_none() {
        candidate_ids.extend(recent_candidates(
            connection,
            query.now_ms,
            query.candidate_limit,
        )?);
    }

    let mut records = HashMap::new();
    for id in candidate_ids {
        let Some(record) = load_record(connection, &id)? else {
            continue;
        };
        if record
            .expires_at_ms
            .is_some_and(|expiry| expiry <= query.now_ms)
            || !record_matches_filter(&record, &query.filter)
        {
            continue;
        }
        records.insert(id, record);
    }
    // Modality ranks are dense over records the caller is actually permitted to
    // see. An expired or out-of-scope memory must not alter visible ranking.
    let lexical = rank_map(
        lexical_ids
            .into_iter()
            .filter(|id| records.contains_key(id)),
    );
    let semantic = rank_map(
        semantic_ids
            .into_iter()
            .filter(|id| records.contains_key(id)),
    );
    let candidates = records
        .into_iter()
        .map(|(id, record)| {
            (
                record,
                CandidateRanks {
                    lexical: lexical.get(&id).copied(),
                    semantic: semantic.get(&id).copied(),
                },
            )
        })
        .collect();
    Ok(rank_candidates(candidates, query.now_ms)
        .into_iter()
        .take(query.limit)
        .map(|(record, score, ranks)| SearchHit {
            record,
            score,
            lexical_rank: ranks.lexical,
            semantic_rank: ranks.semantic,
        })
        .collect())
}

fn lexical_candidates(
    connection: &Connection,
    text: &str,
    now_ms: i64,
    limit: usize,
) -> Result<Vec<String>, MemoryError> {
    let Some(fts_query) = fts_query(text) else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(
        "SELECT m.id FROM memory_fts JOIN memory_items m ON m.rowid=memory_fts.rowid WHERE memory_fts MATCH ?1 AND m.deleted_at_ms IS NULL AND (m.expires_at_ms IS NULL OR m.expires_at_ms>?2) ORDER BY bm25(memory_fts),m.id LIMIT ?3",
    )?;
    let ids = statement
        .query_map(params![fts_query, now_ms, limit as i64], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn semantic_candidates(
    connection: &Connection,
    query: &[f32],
    generation: Option<i64>,
    now_ms: i64,
    limit: usize,
) -> Result<Vec<String>, MemoryError> {
    let mut statement = connection.prepare(
        "SELECT e.item_id,e.vector,e.dimensions FROM memory_embeddings e JOIN memory_items m ON m.id=e.item_id WHERE (?1 IS NULL OR e.generation=?1) AND m.deleted_at_ms IS NULL AND (m.expires_at_ms IS NULL OR m.expires_at_ms>?2) ORDER BY e.item_id",
    )?;
    let mut rows = statement.query(params![generation, now_ms])?;
    let mut scored = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        let dimensions: usize = row.get(2)?;
        let values = decode_f32(&bytes, dimensions)?;
        if let Some(score) = cosine_similarity(query, &values) {
            scored.push((id, score));
        }
    }
    scored.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    Ok(scored
        .into_iter()
        .take(limit)
        .map(|value| value.0)
        .collect())
}

fn recent_candidates(
    connection: &Connection,
    now_ms: i64,
    limit: usize,
) -> Result<Vec<String>, MemoryError> {
    let mut statement = connection.prepare(
        "SELECT id FROM memory_items WHERE deleted_at_ms IS NULL AND (expires_at_ms IS NULL OR expires_at_ms>?1) ORDER BY observed_at_ms DESC,id LIMIT ?2",
    )?;
    let ids = statement
        .query_map(params![now_ms, limit as i64], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn record_matches_filter(record: &MemoryRecord, filter: &RetrievalFilter) -> bool {
    if !filter.namespaces.contains(&record.namespace)
        || !filter.visibility.contains(&record.visibility)
    {
        return false;
    }
    scope_matches(
        record.scope.profile_id.as_deref(),
        filter.profile_id.as_deref(),
        filter.include_global_scope,
    ) && scope_matches(
        record.scope.game_id.as_deref(),
        filter.game_id.as_deref(),
        filter.include_global_scope,
    ) && scope_matches(
        record.scope.character_id.as_deref(),
        filter.character_id.as_deref(),
        filter.include_global_scope,
    ) && scope_matches(
        record.scope.session_id.as_deref(),
        filter.session_id.as_deref(),
        filter.include_global_scope,
    ) && scope_matches(
        record.scope.save_id.as_deref(),
        filter.save_id.as_deref(),
        filter.include_global_scope,
    )
}

fn scope_matches(actual: Option<&str>, wanted: Option<&str>, include_global: bool) -> bool {
    match wanted {
        None => true,
        Some(wanted) => actual == Some(wanted) || (include_global && actual.is_none()),
    }
}

fn fts_query(text: &str) -> Option<String> {
    let tokens = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .take(32)
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" OR "))
}

fn load_record(connection: &Connection, id: &str) -> Result<Option<MemoryRecord>, MemoryError> {
    connection
        .query_row(
            "SELECT id,namespace,profile_id,game_id,character_id,session_id,save_id,visibility,content,content_sha256,provenance_json,confidence,importance,observed_at_ms,created_at_ms,updated_at_ms,expires_at_ms,embedding_generation,revision FROM memory_items WHERE id=?1 AND deleted_at_ms IS NULL",
            [id],
            record_from_row,
        )
        .optional()
        .map_err(MemoryError::from)
}

fn load_record_tx(tx: &Transaction<'_>, id: &str) -> Result<Option<MemoryRecord>, MemoryError> {
    tx.query_row(
        "SELECT id,namespace,profile_id,game_id,character_id,session_id,save_id,visibility,content,content_sha256,provenance_json,confidence,importance,observed_at_ms,created_at_ms,updated_at_ms,expires_at_ms,embedding_generation,revision FROM memory_items WHERE id=?1 AND deleted_at_ms IS NULL",
        [id],
        record_from_row,
    )
    .optional()
    .map_err(MemoryError::from)
}

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let namespace: String = row.get(1)?;
    let visibility: String = row.get(7)?;
    let provenance: String = row.get(10)?;
    Ok(MemoryRecord {
        id: row.get(0)?,
        namespace: namespace.parse().map_err(sql_conversion_error)?,
        scope: MemoryScope {
            profile_id: row.get(2)?,
            game_id: row.get(3)?,
            character_id: row.get(4)?,
            session_id: row.get(5)?,
            save_id: row.get(6)?,
        },
        visibility: visibility.parse().map_err(sql_conversion_error)?,
        content: row.get(8)?,
        content_sha256: row.get(9)?,
        provenance: serde_json::from_str(&provenance).map_err(sql_conversion_error)?,
        confidence: row.get(11)?,
        importance: row.get(12)?,
        observed_at_ms: row.get(13)?,
        created_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
        expires_at_ms: row.get(16)?,
        embedding_generation: row.get(17)?,
        revision: row.get(18)?,
    })
}

fn load_job_tx(tx: &Transaction<'_>, id: i64) -> Result<OutboxJob, MemoryError> {
    Ok(tx.query_row(
        "SELECT id,kind,aggregate_id,payload_json,status,attempts,max_attempts,available_at_ms,lease_owner,lease_expires_at_ms,last_error,created_at_ms,updated_at_ms FROM memory_outbox WHERE id=?1",
        [id],
        |row| {
            let payload: String = row.get(3)?;
            let status: String = row.get(4)?;
            Ok(OutboxJob {
                id: row.get(0)?,
                kind: row.get(1)?,
                aggregate_id: row.get(2)?,
                payload: serde_json::from_str(&payload).map_err(sql_conversion_error)?,
                status: match status.as_str() {
                    "pending" => OutboxStatus::Pending,
                    "processing" => OutboxStatus::Processing,
                    "completed" => OutboxStatus::Completed,
                    "dead" => OutboxStatus::Dead,
                    _ => return Err(sql_conversion_error(MemoryError::InvalidData(status))),
                },
                attempts: row.get(5)?,
                max_attempts: row.get(6)?,
                available_at_ms: row.get(7)?,
                lease_owner: row.get(8)?,
                lease_expires_at_ms: row.get(9)?,
                last_error: row.get(10)?,
                created_at_ms: row.get(11)?,
                updated_at_ms: row.get(12)?,
            })
        },
    )?)
}

fn sql_conversion_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
}

fn encode_f32(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_f32(bytes: &[u8], dimensions: usize) -> Result<Vec<f32>, MemoryError> {
    if bytes.len() != dimensions.saturating_mul(4) {
        return Err(MemoryError::InvalidData(
            "stored embedding dimensions do not match its byte length".to_owned(),
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}
