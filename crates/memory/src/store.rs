use crate::domain::*;
use crate::retrieval::{cosine_similarity, rank_candidates, rank_map, CandidateRanks};
use crate::schema;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Type;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row, Transaction};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
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
    CommitBatch {
        batch: MemoryCommitBatch,
        response: oneshot::Sender<Result<MemoryCommitReport, MemoryError>>,
    },
    Upsert {
        input: Box<MemoryInput>,
        response: oneshot::Sender<Result<MemoryRecord, MemoryError>>,
    },
    Delete {
        id: String,
        deleted_at_ms: i64,
        response: oneshot::Sender<Result<bool, MemoryError>>,
    },
    EraseCharacter {
        request: CharacterMemoryErasureRequest,
        response: oneshot::Sender<Result<CharacterMemoryErasureReport, MemoryError>>,
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
    Flush {
        response: oneshot::Sender<Result<DurabilityReport, MemoryError>>,
    },
    Close {
        response: oneshot::Sender<Result<(), MemoryError>>,
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

    /// Atomically persists the delivered portion of raw turns, followed by any
    /// derived records that reference them. Queued, cancelled, and failed turns
    /// are reported as skipped and never reach an authoritative table.
    ///
    /// Completion of this future means the SQLite transaction committed under
    /// `synchronous=FULL`; queue admission alone is never reported as success.
    pub async fn commit_batch(
        &self,
        batch: MemoryCommitBatch,
    ) -> Result<MemoryCommitReport, MemoryError> {
        if batch.turns.len() > 10_000 || batch.derived.len() > 10_000 {
            return Err(MemoryError::InvalidData(
                "a memory commit batch may contain at most 10000 turns and 10000 derived records"
                    .to_owned(),
            ));
        }
        for turn in &batch.turns {
            turn.validate()?;
            checked_sql_integer("turn sequence", turn.sequence)?;
            checked_sql_integer("cancellation generation", turn.cancellation_generation)?;
        }
        for memory in &batch.derived {
            memory.validate()?;
        }
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::CommitBatch { batch, response })?;
        receive(receiver).await
    }

    pub async fn commit_turn(
        &self,
        turn: TurnCommitInput,
    ) -> Result<MemoryCommitReport, MemoryError> {
        self.commit_batch(MemoryCommitBatch {
            turns: vec![turn],
            derived: Vec::new(),
        })
        .await
    }

    pub async fn get_delivered_turn(
        &self,
        turn_id: impl Into<String>,
    ) -> Result<Option<DeliveredTurnRecord>, MemoryError> {
        let turn_id = turn_id.into();
        self.read(move |connection| load_delivered_turn(connection, &turn_id))
            .await
    }

    pub async fn derive_memory(
        &self,
        memory: DerivedMemoryInput,
    ) -> Result<DerivedMemoryRecord, MemoryError> {
        let mut report = self
            .commit_batch(MemoryCommitBatch {
                turns: Vec::new(),
                derived: vec![memory],
            })
            .await?;
        report
            .derived_memories
            .pop()
            .ok_or_else(|| MemoryError::BackgroundTask("derived write returned no record".into()))
    }

    pub async fn get_derived_memory(
        &self,
        id: impl Into<String>,
    ) -> Result<Option<DerivedMemoryRecord>, MemoryError> {
        let id = id.into();
        self.read(move |connection| load_derived_memory(connection, &id))
            .await
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

    /// Physically removes one exact character authority scope, including
    /// dependent derived records. This is the explicit user-erasure exception
    /// to append-only runtime history; the operation is atomic and leaves only
    /// a content-free hashed audit receipt.
    pub async fn erase_character_memory(
        &self,
        request: CharacterMemoryErasureRequest,
    ) -> Result<CharacterMemoryErasureReport, MemoryError> {
        request.validate()?;
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::EraseCharacter { request, response })?;
        receive(receiver).await
    }

    /// Counts records under one exact native authority scope without returning
    /// their content. Optional encounter/session/save values narrow the result;
    /// they never widen the mandatory character boundary.
    pub async fn character_memory_status(
        &self,
        scope: AuthorityScope,
    ) -> Result<CharacterMemoryStatusReport, MemoryError> {
        scope.validate()?;
        if scope.character_id.is_none() {
            return Err(MemoryError::InvalidData(
                "character memory status requires an exact character ID".to_owned(),
            ));
        }
        self.read(move |connection| character_memory_status(connection, &scope))
            .await
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

    /// Builds typed prompt context without collapsing source categories into an
    /// indistinguishable string. Every query is exact-user/profile/game scoped,
    /// bounded, and deterministically ordered.
    pub async fn retrieve_context(
        &self,
        query: ContextQuery,
    ) -> Result<MemoryContextBundle, MemoryError> {
        query.validate()?;
        self.read(move |connection| retrieve_context_on(connection, &query))
            .await
    }

    pub async fn integrity_check(&self) -> Result<IntegrityReport, MemoryError> {
        self.read(integrity_report).await
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

    /// Forces the WAL through a FULL checkpoint. This is useful before a user
    /// initiated shutdown/export; normal mutation futures already wait for a
    /// synchronous FULL transaction commit.
    pub async fn flush(&self) -> Result<DurabilityReport, MemoryError> {
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Flush { response })?;
        receive(receiver).await
    }

    /// Terminates this store's writer connection after a WAL checkpoint. This
    /// is a terminal operation for every clone of the handle and is required
    /// before native maintenance replaces or removes the SQLite files.
    pub async fn close(&self) -> Result<(), MemoryError> {
        let (response, receiver) = oneshot::channel();
        self.send(WriterCommand::Close { response })?;
        receive(receiver).await
    }

    /// Restores a validated SQLite backup into a destination that is not open.
    /// Existing data is never silently overwritten: replacement must be
    /// explicit and the previous file is atomically quarantined beside it.
    pub async fn recover_from_backup(
        backup: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        replace_existing: bool,
    ) -> Result<RecoveryReport, MemoryError> {
        let backup = backup.as_ref().to_path_buf();
        let destination = destination.as_ref().to_path_buf();
        tokio::task::spawn_blocking(move || recover_database(backup, destination, replace_existing))
            .await
            .map_err(|error| MemoryError::BackgroundTask(error.to_string()))?
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
    let erasure_authorized = Arc::new(AtomicBool::new(false));
    let erasure_authorized_for_sql = Arc::clone(&erasure_authorized);
    let connection = Connection::open(&path).and_then(|mut connection| {
        connection.busy_timeout(busy_timeout)?;
        schema::migrate(&mut connection).map_err(|error| match error {
            MemoryError::Database(error) => error,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        })?;
        connection.create_scalar_function(
            "memory_erasure_authorized",
            0,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_INNOCUOUS,
            move |_| Ok(erasure_authorized_for_sql.load(Ordering::Acquire)),
        )?;
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

    let mut close_response = None;
    while let Ok(command) = receiver.recv() {
        match command {
            WriterCommand::CommitBatch { batch, response } => {
                let _ = response.send(commit_authoritative_batch(&mut connection, batch));
            }
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
            WriterCommand::EraseCharacter { request, response } => {
                let _ = response.send(erase_character_memory(
                    &mut connection,
                    &erasure_authorized,
                    request,
                ));
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
            WriterCommand::Flush { response } => {
                let result = connection
                    .query_row("PRAGMA wal_checkpoint(FULL)", [], |row| {
                        Ok(DurabilityReport {
                            busy: row.get::<_, i64>(0)? != 0,
                            wal_frames: row.get(1)?,
                            checkpointed_frames: row.get(2)?,
                        })
                    })
                    .map_err(MemoryError::from);
                let _ = response.send(result);
            }
            WriterCommand::Close { response } => {
                let result = connection
                    .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                    .map_err(MemoryError::from);
                close_response = Some((response, result));
                break;
            }
        }
    }
    drop(connection);
    if let Some((response, result)) = close_response {
        let _ = response.send(result);
    }
}

fn checked_sql_integer(name: &str, value: u64) -> Result<i64, MemoryError> {
    i64::try_from(value).map_err(|_| {
        MemoryError::InvalidData(format!("{name} exceeds SQLite's signed integer range"))
    })
}

fn character_memory_status(
    connection: &Connection,
    scope: &AuthorityScope,
) -> Result<CharacterMemoryStatusReport, MemoryError> {
    let character_id = scope
        .character_id
        .as_deref()
        .ok_or_else(|| MemoryError::InvalidData("character ID is required".to_owned()))?;
    let delivered_turns = connection.query_row(
        "SELECT count(*) FROM delivered_turns
         WHERE user_id=?1 AND profile_id=?2 AND game_id=?3 AND character_id IS ?4
           AND (?5 IS NULL OR encounter_id IS ?5)
           AND (?6 IS NULL OR session_id IS ?6)
           AND (?7 IS NULL OR save_id IS ?7)",
        params![
            &scope.user_id,
            &scope.profile_id,
            &scope.game_id,
            character_id,
            scope.encounter_id.as_deref(),
            scope.session_id.as_deref(),
            scope.save_id.as_deref(),
        ],
        |row| row.get(0),
    )?;
    let structured_memories = connection.query_row(
        "SELECT count(*) FROM structured_memories
         WHERE user_id=?1 AND profile_id=?2 AND game_id=?3 AND character_id IS ?4
           AND (?5 IS NULL OR encounter_id IS ?5)
           AND (?6 IS NULL OR session_id IS ?6)
           AND (?7 IS NULL OR save_id IS ?7)",
        params![
            &scope.user_id,
            &scope.profile_id,
            &scope.game_id,
            character_id,
            scope.encounter_id.as_deref(),
            scope.session_id.as_deref(),
            scope.save_id.as_deref(),
        ],
        |row| row.get(0),
    )?;
    // Legacy rows predate the mandatory user/encounter principals. The exact
    // profile/game/character scope is still enforced, with optional session and
    // save refinements when supplied.
    let legacy_items = connection.query_row(
        "SELECT count(*) FROM memory_items
         WHERE profile_id IS ?1 AND game_id IS ?2 AND character_id IS ?3
           AND (?4 IS NULL OR session_id IS ?4)
           AND (?5 IS NULL OR save_id IS ?5)",
        params![
            &scope.profile_id,
            &scope.game_id,
            character_id,
            scope.session_id.as_deref(),
            scope.save_id.as_deref(),
        ],
        |row| row.get(0),
    )?;
    Ok(CharacterMemoryStatusReport {
        delivered_turns,
        structured_memories,
        legacy_items,
    })
}

fn erase_character_memory(
    connection: &mut Connection,
    erasure_authorized: &AtomicBool,
    request: CharacterMemoryErasureRequest,
) -> Result<CharacterMemoryErasureReport, MemoryError> {
    request.validate()?;
    let character_id = request
        .scope
        .character_id
        .as_deref()
        .ok_or_else(|| MemoryError::InvalidData("character ID is required".to_owned()))?;
    let scope_bytes = serde_json::to_vec(&request.scope)?;
    let scope_sha256 = Sha256::digest(&scope_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let erasure_id = Uuid::new_v4().to_string();
    let _authorization = ErasureAuthorizationGuard::new(erasure_authorized);
    let tx = connection.transaction()?;
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS erase_turn_ids(id TEXT PRIMARY KEY) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS erase_memory_ids(id TEXT PRIMARY KEY) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS erase_legacy_ids(id TEXT PRIMARY KEY) WITHOUT ROWID;
         DELETE FROM erase_turn_ids;
         DELETE FROM erase_memory_ids;
         DELETE FROM erase_legacy_ids;",
    )?;
    tx.execute(
        "INSERT INTO erase_turn_ids(id)
         SELECT turn_id FROM delivered_turns
         WHERE user_id=?1 AND profile_id=?2 AND game_id=?3 AND character_id IS ?4
           AND (?5 IS NULL OR encounter_id IS ?5)
           AND (?6 IS NULL OR session_id IS ?6)
           AND (?7 IS NULL OR save_id IS ?7)",
        params![
            &request.scope.user_id,
            &request.scope.profile_id,
            &request.scope.game_id,
            character_id,
            request.scope.encounter_id.as_deref(),
            request.scope.session_id.as_deref(),
            request.scope.save_id.as_deref(),
        ],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO erase_memory_ids(id)
         SELECT id FROM structured_memories
         WHERE user_id=?1 AND profile_id=?2 AND game_id=?3 AND character_id IS ?4
           AND (?5 IS NULL OR encounter_id IS ?5)
           AND (?6 IS NULL OR session_id IS ?6)
           AND (?7 IS NULL OR save_id IS ?7)",
        params![
            &request.scope.user_id,
            &request.scope.profile_id,
            &request.scope.game_id,
            character_id,
            request.scope.encounter_id.as_deref(),
            request.scope.session_id.as_deref(),
            request.scope.save_id.as_deref(),
        ],
    )?;
    tx.execute_batch(
        "INSERT OR IGNORE INTO erase_memory_ids(id)
         SELECT DISTINCT memory_id FROM structured_memory_sources
         WHERE turn_id IN (SELECT id FROM erase_turn_ids);",
    )?;
    // Legacy rows predate mandatory user and encounter principals. They are
    // erased only under the same exact profile/game/character and any supplied
    // session/save refinements.
    tx.execute(
        "INSERT OR IGNORE INTO erase_legacy_ids(id)
         SELECT id FROM memory_items
         WHERE profile_id IS ?1 AND game_id IS ?2 AND character_id IS ?3
           AND (?4 IS NULL OR session_id IS ?4)
           AND (?5 IS NULL OR save_id IS ?5)",
        params![
            &request.scope.profile_id,
            &request.scope.game_id,
            character_id,
            request.scope.session_id.as_deref(),
            request.scope.save_id.as_deref(),
        ],
    )?;
    let delivered_turns_deleted: usize =
        tx.query_row("SELECT count(*) FROM erase_turn_ids", [], |row| row.get(0))?;
    let structured_memories_deleted: usize =
        tx.query_row("SELECT count(*) FROM erase_memory_ids", [], |row| {
            row.get(0)
        })?;
    let legacy_items_deleted: usize =
        tx.query_row("SELECT count(*) FROM erase_legacy_ids", [], |row| {
            row.get(0)
        })?;
    let outbox_jobs_deleted = tx.execute(
        "DELETE FROM memory_outbox
         WHERE aggregate_id IN (SELECT id FROM erase_turn_ids)
            OR aggregate_id IN (SELECT id FROM erase_memory_ids)
            OR aggregate_id IN (SELECT id FROM erase_legacy_ids)",
        [],
    )?;
    tx.execute(
        "DELETE FROM structured_memory_sources
         WHERE memory_id IN (SELECT id FROM erase_memory_ids)",
        [],
    )?;
    tx.execute(
        "DELETE FROM structured_memories WHERE id IN (SELECT id FROM erase_memory_ids)",
        [],
    )?;
    tx.execute(
        "DELETE FROM delivered_turns WHERE turn_id IN (SELECT id FROM erase_turn_ids)",
        [],
    )?;
    tx.execute(
        "DELETE FROM memory_items WHERE id IN (SELECT id FROM erase_legacy_ids)",
        [],
    )?;
    tx.execute(
        "INSERT INTO memory_erasure_audit(
             erasure_id,scope_sha256,delivered_turns_deleted,
             structured_memories_deleted,legacy_items_deleted,
             outbox_jobs_deleted,erased_at_ms
         ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            erasure_id,
            scope_sha256,
            delivered_turns_deleted,
            structured_memories_deleted,
            legacy_items_deleted,
            outbox_jobs_deleted,
            request.erased_at_ms,
        ],
    )?;
    tx.commit()?;
    Ok(CharacterMemoryErasureReport {
        erasure_id,
        scope_sha256,
        delivered_turns_deleted,
        structured_memories_deleted,
        legacy_items_deleted,
        outbox_jobs_deleted,
        erased_at_ms: request.erased_at_ms,
    })
}

struct ErasureAuthorizationGuard<'a> {
    authorized: &'a AtomicBool,
}

impl<'a> ErasureAuthorizationGuard<'a> {
    fn new(authorized: &'a AtomicBool) -> Self {
        authorized.store(true, Ordering::Release);
        Self { authorized }
    }
}

impl Drop for ErasureAuthorizationGuard<'_> {
    fn drop(&mut self) {
        self.authorized.store(false, Ordering::Release);
    }
}

fn commit_authoritative_batch(
    connection: &mut Connection,
    batch: MemoryCommitBatch,
) -> Result<MemoryCommitReport, MemoryError> {
    let tx = connection.transaction()?;
    let mut report = MemoryCommitReport::default();

    // Source authority is inserted first. A derived record in this same batch
    // can therefore reference a just-delivered turn through a real FK.
    for turn in batch.turns {
        let eligible = turn.delivery.eligible_text(&turn.text)?;
        let Some((delivered_text, delivered_at_ms)) = eligible else {
            let reason = match turn.delivery {
                DeliveryDisposition::Queued => SkippedTurnReason::Queued,
                DeliveryDisposition::Cancelled => SkippedTurnReason::Cancelled,
                DeliveryDisposition::Failed => SkippedTurnReason::Failed,
                DeliveryDisposition::Delivered { .. }
                | DeliveryDisposition::PartiallyDelivered { .. } => unreachable!(),
            };
            report.skipped_turns.push(SkippedTurn {
                turn_id: turn.turn_id,
                reason,
            });
            continue;
        };
        let content = delivered_text.to_owned();
        let content_sha256 = sha256(&content);
        let expected = DeliveredTurnRecord {
            turn_id: turn.turn_id.clone(),
            scope: turn.scope.clone(),
            speaker: turn.speaker,
            delivered_text: content.clone(),
            content_sha256: content_sha256.clone(),
            created_at_ms: turn.created_at_ms,
            delivered_at_ms,
            sequence: turn.sequence,
            cancellation_generation: turn.cancellation_generation,
            provider_id: turn.provider_id.clone(),
            delivery_receipt_id: turn.delivery_receipt_id.clone(),
            provenance: turn.provenance.clone(),
        };
        if let Some(existing) = load_delivered_turn(&tx, &turn.turn_id)? {
            if existing != expected {
                return Err(MemoryError::InvalidData(format!(
                    "delivered turn {} is immutable and conflicts with the existing source row",
                    turn.turn_id
                )));
            }
            report.stored_turns.push(existing);
            continue;
        }
        tx.execute(
            r#"INSERT INTO delivered_turns(
                turn_id,user_id,profile_id,game_id,character_id,encounter_id,
                session_id,save_id,speaker,delivered_text,content_sha256,
                created_at_ms,delivered_at_ms,sequence_no,cancellation_generation,
                provider_id,delivery_receipt_id,provenance_json
            ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)"#,
            params![
                turn.turn_id,
                turn.scope.user_id,
                turn.scope.profile_id,
                turn.scope.game_id,
                turn.scope.character_id,
                turn.scope.encounter_id,
                turn.scope.session_id,
                turn.scope.save_id,
                turn.speaker.as_str(),
                content,
                content_sha256,
                turn.created_at_ms,
                delivered_at_ms,
                checked_sql_integer("turn sequence", turn.sequence)?,
                checked_sql_integer("cancellation generation", turn.cancellation_generation)?,
                turn.provider_id,
                turn.delivery_receipt_id,
                serde_json::to_string(&turn.provenance)?,
            ],
        )?;
        report.stored_turns.push(expected);
    }

    for memory in batch.derived {
        report
            .derived_memories
            .push(insert_derived_memory(&tx, memory)?);
    }
    tx.commit()?;
    Ok(report)
}

fn insert_derived_memory(
    tx: &Transaction<'_>,
    memory: DerivedMemoryInput,
) -> Result<DerivedMemoryRecord, MemoryError> {
    let id = memory
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let content = memory.content.trim().to_owned();
    let content_sha256 = sha256(&content);
    let created_at_ms = unix_time_ms();
    let generator_json = memory
        .generator
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    for source_id in &memory.source_turn_ids {
        let source = load_delivered_turn(tx, source_id)?
            .ok_or_else(|| MemoryError::NotFound(format!("source turn {source_id}")))?;
        if !source_scope_compatible(&memory.scope, &source.scope) {
            return Err(MemoryError::InvalidData(format!(
                "source turn {source_id} crosses a user, game, character, encounter, or save boundary"
            )));
        }
    }

    if let Some(existing) = load_derived_memory(tx, &id)? {
        let same = existing.scope == memory.scope
            && existing.class == memory.class
            && existing.spoiler_scope == memory.spoiler_scope
            && existing.content == content
            && existing.provenance == memory.provenance
            && existing.confidence == memory.confidence
            && existing.importance == memory.importance
            && existing.observed_at_ms == memory.observed_at_ms
            && existing.expires_at_ms == memory.expires_at_ms
            && existing.source_turn_ids == memory.source_turn_ids
            && existing.generator == memory.generator;
        if !same {
            return Err(MemoryError::InvalidData(format!(
                "structured memory {id} is immutable and conflicts with the existing record"
            )));
        }
        return Ok(existing);
    }

    tx.execute(
        r#"INSERT INTO structured_memories(
            id,user_id,profile_id,game_id,character_id,encounter_id,session_id,
            save_id,knowledge_class,spoiler_scope,content,content_sha256,
            provenance_json,confidence,importance,observed_at_ms,created_at_ms,
            expires_at_ms,generator_json
        ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"#,
        params![
            id,
            memory.scope.user_id,
            memory.scope.profile_id,
            memory.scope.game_id,
            memory.scope.character_id,
            memory.scope.encounter_id,
            memory.scope.session_id,
            memory.scope.save_id,
            memory.class.as_str(),
            memory.spoiler_scope.as_str(),
            content,
            content_sha256,
            serde_json::to_string(&memory.provenance)?,
            memory.confidence,
            memory.importance,
            memory.observed_at_ms,
            created_at_ms,
            memory.expires_at_ms,
            generator_json,
        ],
    )?;
    for (ordinal, source_id) in memory.source_turn_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO structured_memory_sources(memory_id,turn_id,source_ordinal) VALUES(?1,?2,?3)",
            params![id, source_id, ordinal as i64],
        )?;
    }
    load_derived_memory(tx, &id)?.ok_or_else(|| MemoryError::NotFound(id))
}

fn source_scope_compatible(memory: &AuthorityScope, source: &AuthorityScope) -> bool {
    memory.user_id == source.user_id
        && memory.profile_id == source.profile_id
        && memory.game_id == source.game_id
        && memory.character_id == source.character_id
        && memory.encounter_id == source.encounter_id
        && memory.save_id == source.save_id
}

fn sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
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
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("memory-backup.sqlite");
    let temporary = parent.join(format!(".{file_name}.backup-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut target = Connection::open(&temporary)?;
        let backup = rusqlite::backup::Backup::new(connection, &mut target)?;
        backup.run_to_completion(128, Duration::from_millis(5), None)?;
        let pages_copied = backup.progress().pagecount;
        drop(backup);
        let integrity = integrity_report(&target)?;
        drop(target);
        if !integrity.ok {
            return Err(MemoryError::InvalidData(format!(
                "new backup failed integrity validation: {}",
                integrity.messages.join("; ")
            )));
        }
        std::fs::rename(&temporary, &destination)?;
        Ok(BackupReport {
            destination: destination.clone(),
            pages_copied,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn load_delivered_turn(
    connection: &Connection,
    turn_id: &str,
) -> Result<Option<DeliveredTurnRecord>, MemoryError> {
    connection
        .query_row(
            r#"SELECT turn_id,user_id,profile_id,game_id,character_id,encounter_id,
                session_id,save_id,speaker,delivered_text,content_sha256,
                created_at_ms,delivered_at_ms,sequence_no,cancellation_generation,
                provider_id,delivery_receipt_id,provenance_json
               FROM delivered_turns WHERE turn_id=?1"#,
            [turn_id],
            delivered_turn_from_row,
        )
        .optional()
        .map_err(MemoryError::from)
}

fn delivered_turn_from_row(row: &Row<'_>) -> rusqlite::Result<DeliveredTurnRecord> {
    let speaker: String = row.get(8)?;
    let sequence: i64 = row.get(13)?;
    let cancellation_generation: i64 = row.get(14)?;
    let provenance: String = row.get(17)?;
    Ok(DeliveredTurnRecord {
        turn_id: row.get(0)?,
        scope: AuthorityScope {
            user_id: row.get(1)?,
            profile_id: row.get(2)?,
            game_id: row.get(3)?,
            character_id: row.get(4)?,
            encounter_id: row.get(5)?,
            session_id: row.get(6)?,
            save_id: row.get(7)?,
        },
        speaker: speaker.parse().map_err(sql_conversion_error)?,
        delivered_text: row.get(9)?,
        content_sha256: row.get(10)?,
        created_at_ms: row.get(11)?,
        delivered_at_ms: row.get(12)?,
        sequence: u64::try_from(sequence).map_err(sql_conversion_error)?,
        cancellation_generation: u64::try_from(cancellation_generation)
            .map_err(sql_conversion_error)?,
        provider_id: row.get(15)?,
        delivery_receipt_id: row.get(16)?,
        provenance: serde_json::from_str(&provenance).map_err(sql_conversion_error)?,
    })
}

fn load_derived_memory(
    connection: &Connection,
    id: &str,
) -> Result<Option<DerivedMemoryRecord>, MemoryError> {
    let core = connection
        .query_row(
            r#"SELECT id,user_id,profile_id,game_id,character_id,encounter_id,
                session_id,save_id,knowledge_class,spoiler_scope,content,
                content_sha256,provenance_json,confidence,importance,
                observed_at_ms,created_at_ms,expires_at_ms,generator_json
               FROM structured_memories WHERE id=?1"#,
            [id],
            derived_memory_from_row,
        )
        .optional()?;
    let Some(mut record) = core else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT turn_id FROM structured_memory_sources WHERE memory_id=?1 ORDER BY source_ordinal",
    )?;
    record.source_turn_ids = statement
        .query_map([id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(record))
}

fn derived_memory_from_row(row: &Row<'_>) -> rusqlite::Result<DerivedMemoryRecord> {
    let class: String = row.get(8)?;
    let spoiler_scope: String = row.get(9)?;
    let provenance: String = row.get(12)?;
    let generator: Option<String> = row.get(18)?;
    Ok(DerivedMemoryRecord {
        id: row.get(0)?,
        scope: AuthorityScope {
            user_id: row.get(1)?,
            profile_id: row.get(2)?,
            game_id: row.get(3)?,
            character_id: row.get(4)?,
            encounter_id: row.get(5)?,
            session_id: row.get(6)?,
            save_id: row.get(7)?,
        },
        class: class.parse().map_err(sql_conversion_error)?,
        spoiler_scope: spoiler_scope.parse().map_err(sql_conversion_error)?,
        content: row.get(10)?,
        content_sha256: row.get(11)?,
        provenance: serde_json::from_str(&provenance).map_err(sql_conversion_error)?,
        confidence: row.get(13)?,
        importance: row.get(14)?,
        observed_at_ms: row.get(15)?,
        created_at_ms: row.get(16)?,
        expires_at_ms: row.get(17)?,
        source_turn_ids: Vec::new(),
        generator: generator
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(sql_conversion_error)?,
    })
}

fn retrieve_context_on(
    connection: &Connection,
    query: &ContextQuery,
) -> Result<MemoryContextBundle, MemoryError> {
    let mut context = MemoryContextBundle::default();
    if query.recent_turn_limit > 0 {
        let mut statement = connection.prepare(
            r#"SELECT turn_id,user_id,profile_id,game_id,character_id,encounter_id,
                      session_id,save_id,speaker,delivered_text,content_sha256,
                      created_at_ms,delivered_at_ms,sequence_no,cancellation_generation,
                      provider_id,delivery_receipt_id,provenance_json
               FROM (
                   SELECT * FROM delivered_turns
                   WHERE user_id=?1 AND profile_id=?2 AND game_id=?3
                     AND character_id IS ?4
                     AND encounter_id IS ?5
                     AND session_id IS ?6
                     AND save_id IS ?7
                   ORDER BY delivered_at_ms DESC,sequence_no DESC,turn_id DESC
                   LIMIT ?8
               )
               ORDER BY delivered_at_ms,sequence_no,turn_id"#,
        )?;
        context.recent_dialogue = statement
            .query_map(
                params![
                    query.scope.user_id,
                    query.scope.profile_id,
                    query.scope.game_id,
                    query.scope.character_id,
                    query.scope.encounter_id,
                    query.scope.session_id,
                    query.scope.save_id,
                    query.recent_turn_limit as i64,
                ],
                delivered_turn_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
    }

    for class in KnowledgeClass::ALL {
        let records = retrieve_class(connection, query, class)?;
        match class {
            KnowledgeClass::WorldLore => context.world_lore = records,
            KnowledgeClass::Biography => context.biography = records,
            KnowledgeClass::CharacterKnowledge => context.character_knowledge = records,
            KnowledgeClass::UncertainPublicInfo => context.uncertain_public_info = records,
            KnowledgeClass::LongTermSummary => context.long_term_summaries = records,
        }
    }
    Ok(context)
}

fn retrieve_class(
    connection: &Connection,
    query: &ContextQuery,
    class: KnowledgeClass,
) -> Result<Vec<DerivedMemoryRecord>, MemoryError> {
    if query.per_class_limit == 0 {
        return Ok(Vec::new());
    }
    let candidate_limit = query.per_class_limit.saturating_mul(8).min(8_000) as i64;
    let normalized_text = query
        .text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_lowercase);
    let mut statement = connection.prepare(
        r#"SELECT id,user_id,profile_id,game_id,character_id,encounter_id,
                  session_id,save_id,knowledge_class,spoiler_scope,content,
                  content_sha256,provenance_json,confidence,importance,
                  observed_at_ms,created_at_ms,expires_at_ms,generator_json
           FROM structured_memories
           WHERE user_id=?1 AND profile_id=?2 AND game_id=?3
             AND knowledge_class=?4
             AND (character_id IS NULL OR character_id IS ?5)
             AND (encounter_id IS NULL OR encounter_id IS ?6)
             AND (session_id IS NULL OR session_id IS ?7)
             AND (save_id IS NULL OR save_id IS ?8)
             AND (expires_at_ms IS NULL OR expires_at_ms>?9)
           ORDER BY CASE WHEN ?10 IS NOT NULL AND instr(lower(content),?10)>0 THEN 0 ELSE 1 END,
                    importance DESC,confidence DESC,observed_at_ms DESC,id
           LIMIT ?11"#,
    )?;
    let candidates = statement
        .query_map(
            params![
                query.scope.user_id,
                query.scope.profile_id,
                query.scope.game_id,
                class.as_str(),
                query.scope.character_id,
                query.scope.encounter_id,
                query.scope.session_id,
                query.scope.save_id,
                query.now_ms,
                normalized_text,
                candidate_limit,
            ],
            derived_memory_from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    let mut visible = Vec::with_capacity(query.per_class_limit);
    for mut record in candidates {
        if !query.spoiler_policy.allows(record.spoiler_scope)
            || !spoiler_scope_matches(&record, query)
        {
            continue;
        }
        let mut sources = connection.prepare(
            "SELECT turn_id FROM structured_memory_sources WHERE memory_id=?1 ORDER BY source_ordinal",
        )?;
        record.source_turn_ids = sources
            .query_map([&record.id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        visible.push(record);
        if visible.len() == query.per_class_limit {
            break;
        }
    }
    Ok(visible)
}

fn spoiler_scope_matches(record: &DerivedMemoryRecord, query: &ContextQuery) -> bool {
    match record.spoiler_scope {
        SpoilerScope::None | SpoilerScope::Game | SpoilerScope::UserPrivate => true,
        SpoilerScope::Save => {
            query.scope.save_id.is_some() && record.scope.save_id == query.scope.save_id
        }
        SpoilerScope::CharacterPrivate => {
            query.scope.character_id.is_some()
                && record.scope.character_id == query.scope.character_id
        }
    }
}

fn integrity_report(connection: &Connection) -> Result<IntegrityReport, MemoryError> {
    let schema_version = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let mut messages = Vec::new();
    let mut quick_check = connection.prepare("PRAGMA quick_check")?;
    for message in quick_check
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
    {
        if message != "ok" {
            messages.push(message);
        }
    }
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    let violations = foreign_keys
        .query_map([], |row| {
            let table: String = row.get(0)?;
            let parent: String = row.get(2)?;
            Ok(format!(
                "foreign key violation in {table} referencing {parent}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    messages.extend(violations);
    if schema_version != schema::SCHEMA_VERSION {
        messages.push(format!(
            "schema version {schema_version} does not match supported version {}",
            schema::SCHEMA_VERSION
        ));
    }
    for (version, expected_checksum) in schema::MIGRATION_CHECKSUMS {
        let actual = connection
            .query_row(
                "SELECT checksum FROM memory_schema_migrations WHERE version=?1",
                [version],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match actual {
            None => messages.push(format!("migration ledger is missing version {version}")),
            Some(actual) if actual != expected_checksum => messages.push(format!(
                "migration {version} checksum differs from the application-owned ledger"
            )),
            Some(_) => {}
        }
    }
    let unexpected_migrations = connection.query_row(
        "SELECT count(*) FROM memory_schema_migrations WHERE version>?1",
        [schema::SCHEMA_VERSION],
        |row| row.get::<_, i64>(0),
    )?;
    if unexpected_migrations > 0 {
        messages.push("migration ledger contains unsupported future versions".to_owned());
    }
    let delivered_turns =
        connection.query_row("SELECT count(*) FROM delivered_turns", [], |row| {
            row.get::<_, u64>(0)
        })?;
    let derived_memories =
        connection.query_row("SELECT count(*) FROM structured_memories", [], |row| {
            row.get::<_, u64>(0)
        })?;
    Ok(IntegrityReport {
        ok: messages.is_empty(),
        messages,
        schema_version,
        delivered_turns,
        derived_memories,
    })
}

fn recover_database(
    backup: PathBuf,
    destination: PathBuf,
    replace_existing: bool,
) -> Result<RecoveryReport, MemoryError> {
    if backup == destination {
        return Err(MemoryError::InvalidData(
            "recovery source and destination must differ".to_owned(),
        ));
    }
    let source = Connection::open_with_flags(
        &backup,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    source.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
    let source_integrity = integrity_report(&source)?;
    if !source_integrity.ok {
        return Err(MemoryError::InvalidData(format!(
            "backup failed integrity validation: {}",
            source_integrity.messages.join("; ")
        )));
    }
    drop(source);

    let sidecars = sqlite_sidecar_paths(&destination);
    let destination_artifacts_exist =
        destination.exists() || sidecars.iter().any(|path| path.exists());
    if destination_artifacts_exist && !replace_existing {
        return Err(MemoryError::InvalidData(format!(
            "recovery destination or its SQLite sidecars already exist: {}",
            destination.display()
        )));
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("memory.sqlite");
    let temporary = parent.join(format!(".{file_name}.restore-{}.tmp", Uuid::new_v4()));
    std::fs::copy(&backup, &temporary)?;
    let copied = Connection::open_with_flags(
        &temporary,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    copied.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
    let copied_integrity = integrity_report(&copied)?;
    drop(copied);
    if !copied_integrity.ok {
        let _ = std::fs::remove_file(&temporary);
        return Err(MemoryError::InvalidData(
            "copied backup failed integrity validation".to_owned(),
        ));
    }

    let quarantine = if destination_artifacts_exist {
        let directory = parent.join(format!("{file_name}.quarantine-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        let quarantined_database = directory.join(file_name);
        let main_existed = destination.exists();
        if main_existed {
            std::fs::rename(&destination, &quarantined_database)?;
        }
        for sidecar in &sidecars {
            if sidecar.exists() {
                let name = sidecar.file_name().ok_or_else(|| {
                    MemoryError::InvalidData("SQLite sidecar has no file name".to_owned())
                })?;
                std::fs::rename(sidecar, directory.join(name))?;
            }
        }
        let report_path = if main_existed {
            quarantined_database.clone()
        } else {
            directory.clone()
        };
        Some((report_path, directory, quarantined_database, main_existed))
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        if let Some((_report_path, directory, quarantined_database, main_existed)) = &quarantine {
            if *main_existed && quarantined_database.exists() {
                let _ = std::fs::rename(quarantined_database, &destination);
            }
            for sidecar in &sidecars {
                if let Some(name) = sidecar.file_name() {
                    let quarantined_sidecar = directory.join(name);
                    if quarantined_sidecar.exists() {
                        let _ = std::fs::rename(quarantined_sidecar, sidecar);
                    }
                }
            }
            let _ = std::fs::remove_dir(directory);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(MemoryError::Io(error));
    }
    Ok(RecoveryReport {
        destination,
        quarantined_database: quarantine.map(|value| value.0),
        integrity: copied_integrity,
    })
}

fn sqlite_sidecar_paths(database: &Path) -> [PathBuf; 2] {
    let mut wal = database.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shared_memory = database.as_os_str().to_os_string();
    shared_memory.push("-shm");
    [PathBuf::from(wal), PathBuf::from(shared_memory)]
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
