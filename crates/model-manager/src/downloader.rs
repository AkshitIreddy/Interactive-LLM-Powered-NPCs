use crate::secure_path::secure_open_rw_file;
use crate::{
    verify_artifact, ArtifactEvidence, ArtifactV1, DownloadJournal, FileDownloadJournalStore,
    JournalError, PackRevision, PackSelectionAuthorizationV1, PackSelectionError, ResumeError,
    ResumeResponse, VerificationError,
};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, RANGE};
use reqwest::{Client, StatusCode, Url};
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Debug)]
pub struct DownloadPolicy {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub idle_timeout: Duration,
    pub checkpoint_bytes: u64,
    pub maximum_artifact_bytes: u64,
    /// Only permits plain HTTP to IP loopback hosts. Intended for deterministic local tests.
    pub allow_http_loopback: bool,
}

impl Default for DownloadPolicy {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(6 * 60 * 60),
            idle_timeout: Duration::from_secs(60),
            checkpoint_bytes: 1024 * 1024,
            maximum_artifact_bytes: crate::MAX_PACK_BYTES,
            allow_http_loopback: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpsArtifactDownloader {
    client: Client,
    policy: DownloadPolicy,
}

impl HttpsArtifactDownloader {
    pub fn new(policy: DownloadPolicy) -> Result<Self, DownloadError> {
        if policy.checkpoint_bytes == 0 || policy.maximum_artifact_bytes == 0 {
            return Err(DownloadError::InvalidPolicy);
        }
        let allow_loopback = policy.allow_http_loopback;
        let redirect = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("redirect limit exceeded");
            }
            if allowed_url(attempt.url(), allow_loopback) {
                attempt.follow()
            } else {
                attempt.error("redirect target violates HTTPS policy")
            }
        });
        let client = Client::builder()
            .connect_timeout(policy.connect_timeout)
            .timeout(policy.request_timeout)
            .redirect(redirect)
            .user_agent("Interactive-LLM-NPCs-ModelManager/2")
            .build()
            .map_err(DownloadError::Client)?;
        Ok(Self { client, policy })
    }

    pub async fn download(
        &self,
        identity: &PackRevision,
        artifact: &ArtifactV1,
        selection: &PackSelectionAuthorizationV1,
        destination: impl Into<PathBuf>,
        journals: &FileDownloadJournalStore,
        cancellation: &CancellationToken,
    ) -> Result<ArtifactEvidence, DownloadError> {
        if artifact.size_bytes == 0 || artifact.size_bytes > self.policy.maximum_artifact_bytes {
            return Err(DownloadError::DeclaredSizeOutsidePolicy(
                artifact.size_bytes,
            ));
        }
        selection.validate_download_artifact(identity, artifact)?;
        let destination = destination.into();
        validate_download_destination(&destination)?;
        let mut last_error = None;
        for source in &artifact.source_urls {
            let url = Url::parse(source).map_err(|_| DownloadError::InvalidUrl(source.clone()))?;
            if !allowed_url(&url, self.policy.allow_http_loopback) {
                return Err(DownloadError::InsecureUrl(source.clone()));
            }
            match self
                .download_from_url(
                    identity,
                    artifact,
                    &url,
                    &destination,
                    journals,
                    cancellation,
                )
                .await
            {
                Ok(evidence) => return Ok(evidence),
                Err(error @ (DownloadError::Cancelled | DownloadError::Journal(_))) => {
                    return Err(error);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(DownloadError::NoSources))
    }

    async fn download_from_url(
        &self,
        identity: &PackRevision,
        artifact: &ArtifactV1,
        url: &Url,
        destination: &Path,
        journals: &FileDownloadJournalStore,
        cancellation: &CancellationToken,
    ) -> Result<ArtifactEvidence, DownloadError> {
        if let Some(parent) = destination.parent() {
            if !parent.is_dir() {
                return Err(DownloadError::UnpreparedDestination(parent.to_owned()));
            }
        }
        let mut journal = journals
            .load(identity, &artifact.id)
            .await?
            .unwrap_or_else(|| {
                DownloadJournal::new(
                    artifact.id.clone(),
                    artifact.size_bytes,
                    artifact.sha256.clone(),
                )
            });
        if journal.expected_size != artifact.size_bytes
            || journal.expected_sha256 != artifact.sha256
            || journal.artifact_id != artifact.id
        {
            return Err(DownloadError::JournalManifestMismatch);
        }
        reconcile_file_and_journal(destination, &mut journal).await?;
        journals.save(identity, &journal).await?;

        if journal.received_bytes == journal.expected_size {
            return verify_completed_file(identity, artifact, destination, journals, &mut journal)
                .await;
        }

        // One automatic restart is permitted when a server stops honoring resume metadata.
        for restart_attempt in 0..=1 {
            if cancellation.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            if journal.received_bytes > 0 && journal.validator.is_none() {
                restart_download(destination, &mut journal, identity, journals).await?;
            }
            let resume = journal.resume_request();
            let mut request = self.client.get(url.clone());
            if resume.offset > 0 {
                request = request.header(RANGE, format!("bytes={}-", resume.offset));
                if let Some(validator) = &resume.if_range {
                    request = request.header(IF_RANGE, validator);
                }
            }
            let response = tokio::select! {
                () = cancellation.cancelled() => return Err(DownloadError::Cancelled),
                result = request.send() => result.map_err(DownloadError::Client)?,
            };

            if !response.status().is_success() {
                return Err(DownloadError::HttpStatus(response.status().as_u16()));
            }
            let response_status = response.status();
            let validator = strong_etag(response.headers().get(ETAG));
            let (range_start, total_size) = response_extent(&response, artifact.size_bytes)?;

            if resume.offset > 0
                && (response_status != StatusCode::PARTIAL_CONTENT
                    || range_start != Some(resume.offset)
                    || validator != journal.validator)
            {
                if restart_attempt == 0 {
                    restart_download(destination, &mut journal, identity, journals).await?;
                    continue;
                }
                return Err(DownloadError::ResumeNotHonored);
            }
            journal.accept_response(ResumeResponse {
                status: response_status.as_u16(),
                range_start,
                total_size,
                validator,
            })?;

            let protected_root = destination
                .parent()
                .ok_or_else(|| DownloadError::UnsafeDestination(destination.to_owned()))?;
            let mut secured_file = secure_open_rw_file(protected_root, destination)
                .map_err(DownloadError::Io)?
                .into_tokio();
            secured_file
                .file_mut()
                .seek(std::io::SeekFrom::Start(journal.received_bytes))
                .await
                .map_err(DownloadError::Io)?;
            let mut stream = response.bytes_stream();
            let mut uncommitted = 0_u64;
            loop {
                let next = tokio::select! {
                    () = cancellation.cancelled() => {
                        checkpoint(secured_file.file_mut(), &mut journal, uncommitted, identity, journals).await?;
                        return Err(DownloadError::Cancelled);
                    }
                    result = tokio::time::timeout(self.policy.idle_timeout, stream.next()) => {
                        result.map_err(|_| DownloadError::IdleTimeout)?
                    }
                };
                let Some(chunk) = next else { break };
                let chunk = chunk.map_err(DownloadError::Client)?;
                let projected = journal
                    .received_bytes
                    .checked_add(uncommitted)
                    .and_then(|value| value.checked_add(chunk.len() as u64))
                    .ok_or(DownloadError::SizeOverflow)?;
                if projected > artifact.size_bytes {
                    return Err(DownloadError::BodyTooLarge);
                }
                secured_file
                    .file_mut()
                    .write_all(&chunk)
                    .await
                    .map_err(DownloadError::Io)?;
                uncommitted += chunk.len() as u64;
                if uncommitted >= self.policy.checkpoint_bytes {
                    checkpoint(
                        secured_file.file_mut(),
                        &mut journal,
                        uncommitted,
                        identity,
                        journals,
                    )
                    .await?;
                    uncommitted = 0;
                }
            }
            checkpoint(
                secured_file.file_mut(),
                &mut journal,
                uncommitted,
                identity,
                journals,
            )
            .await?;
            drop(secured_file);
            return verify_completed_file(identity, artifact, destination, journals, &mut journal)
                .await;
        }
        Err(DownloadError::ResumeNotHonored)
    }
}

async fn checkpoint(
    file: &mut tokio::fs::File,
    journal: &mut DownloadJournal,
    uncommitted: u64,
    identity: &PackRevision,
    journals: &FileDownloadJournalStore,
) -> Result<(), DownloadError> {
    file.flush().await.map_err(DownloadError::Io)?;
    file.sync_data().await.map_err(DownloadError::Io)?;
    journal.record_bytes(uncommitted)?;
    journals.save(identity, journal).await?;
    Ok(())
}

async fn reconcile_file_and_journal(
    path: &Path,
    journal: &mut DownloadJournal,
) -> Result<(), DownloadError> {
    let file_size = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(DownloadError::Io(error)),
    };
    if file_size < journal.received_bytes {
        journal.restart();
        truncate(path, 0).await?;
    } else if file_size > journal.received_bytes {
        // Bytes are made durable before their journal checkpoint. A crash between those
        // operations is recovered by truncating to the last authenticated checkpoint.
        truncate(path, journal.received_bytes).await?;
    }
    Ok(())
}

async fn restart_download(
    path: &Path,
    journal: &mut DownloadJournal,
    identity: &PackRevision,
    journals: &FileDownloadJournalStore,
) -> Result<(), DownloadError> {
    truncate(path, 0).await?;
    journal.restart();
    journals.save(identity, journal).await?;
    Ok(())
}

async fn truncate(path: &Path, length: u64) -> Result<(), DownloadError> {
    validate_download_destination(path)?;
    let protected_root = path
        .parent()
        .ok_or_else(|| DownloadError::UnsafeDestination(path.to_owned()))?;
    let mut file = secure_open_rw_file(protected_root, path)
        .map_err(DownloadError::Io)?
        .into_tokio();
    file.file_mut()
        .set_len(length)
        .await
        .map_err(DownloadError::Io)?;
    file.file_mut().sync_data().await.map_err(DownloadError::Io)
}

fn validate_download_destination(path: &Path) -> Result<(), DownloadError> {
    for ancestor in path.ancestors() {
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(DownloadError::Io(error)),
        };
        if metadata.file_type().is_symlink() || metadata_is_reparse(&metadata) {
            return Err(DownloadError::UnsafeDestination(path.to_owned()));
        }
    }
    Ok(())
}

fn metadata_is_reparse(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

async fn verify_completed_file(
    identity: &PackRevision,
    artifact: &ArtifactV1,
    path: &Path,
    journals: &FileDownloadJournalStore,
    journal: &mut DownloadJournal,
) -> Result<ArtifactEvidence, DownloadError> {
    let verification_path = path.to_owned();
    let id = artifact.id.clone();
    let size = artifact.size_bytes;
    let digest = artifact.sha256.clone();
    let result = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(verification_path).map_err(VerificationError::Io)?;
        verify_artifact(id, size, &digest, file)
    })
    .await
    .map_err(|error| DownloadError::Join(error.to_string()))?;
    let evidence = match result {
        Ok(evidence) => evidence,
        Err(error) => {
            // A full-length but untrusted body must not poison retries or alternate mirrors.
            restart_download(path, journal, identity, journals).await?;
            return Err(DownloadError::Verification(error));
        }
    };
    journal.mark_verified(&evidence)?;
    journals.save(identity, journal).await?;
    Ok(evidence)
}

fn response_extent(
    response: &reqwest::Response,
    expected_total: u64,
) -> Result<(Option<u64>, u64), DownloadError> {
    if response.status() == StatusCode::PARTIAL_CONTENT {
        let raw = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or(DownloadError::MissingContentRange)?;
        let (range, total) = raw
            .strip_prefix("bytes ")
            .and_then(|value| value.split_once('/'))
            .ok_or(DownloadError::InvalidContentRange)?;
        let (start, _end) = range
            .split_once('-')
            .ok_or(DownloadError::InvalidContentRange)?;
        let start = start
            .parse()
            .map_err(|_| DownloadError::InvalidContentRange)?;
        let total = total
            .parse()
            .map_err(|_| DownloadError::InvalidContentRange)?;
        if total != expected_total {
            return Err(DownloadError::RemoteSizeChanged {
                expected: expected_total,
                actual: total,
            });
        }
        Ok((Some(start), total))
    } else {
        if let Some(length) = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
        {
            let length: u64 = length
                .parse()
                .map_err(|_| DownloadError::InvalidContentLength)?;
            if length != expected_total {
                return Err(DownloadError::RemoteSizeChanged {
                    expected: expected_total,
                    actual: length,
                });
            }
        }
        Ok((None, expected_total))
    }
}

fn strong_etag(value: Option<&reqwest::header::HeaderValue>) -> Option<String> {
    let value = value?.to_str().ok()?;
    (!value.starts_with("W/") && !value.is_empty()).then(|| value.to_owned())
}

fn allowed_url(url: &Url, allow_http_loopback: bool) -> bool {
    if url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
    {
        return true;
    }
    allow_http_loopback
        && url.scheme() == "http"
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("download policy is invalid")]
    InvalidPolicy,
    #[error("artifact declared size {0} violates download policy")]
    DeclaredSizeOutsidePolicy(u64),
    #[error("artifact has no source URLs")]
    NoSources,
    #[error("invalid source URL: {0}")]
    InvalidUrl(String),
    #[error("source or redirect violates HTTPS policy: {0}")]
    InsecureUrl(String),
    #[error("HTTP client failed: {0}")]
    Client(reqwest::Error),
    #[error("HTTP server returned status {0}")]
    HttpStatus(u16),
    #[error("download was cancelled")]
    Cancelled,
    #[error("download stream exceeded its idle timeout")]
    IdleTimeout,
    #[error("download I/O failed: {0}")]
    Io(io::Error),
    #[error("download destination contains a symlink or reparse ancestor: {0}")]
    UnsafeDestination(PathBuf),
    #[error("download destination directory must be prepared by trusted storage: {0}")]
    UnpreparedDestination(PathBuf),
    #[error("download journal failed: {0}")]
    Journal(#[from] JournalError),
    #[error("download journal does not match the signed manifest")]
    JournalManifestMismatch,
    #[error("resume validation failed: {0}")]
    Resume(#[from] ResumeError),
    #[error("server did not honor a safe resume request")]
    ResumeNotHonored,
    #[error("partial response lacks Content-Range")]
    MissingContentRange,
    #[error("invalid Content-Range header")]
    InvalidContentRange,
    #[error("invalid Content-Length header")]
    InvalidContentLength,
    #[error("remote artifact size changed from {expected} to {actual}")]
    RemoteSizeChanged { expected: u64, actual: u64 },
    #[error("download body exceeds declared size")]
    BodyTooLarge,
    #[error("download size overflow")]
    SizeOverflow,
    #[error("artifact verification failed: {0}")]
    Verification(#[from] VerificationError),
    #[error("download verification task failed: {0}")]
    Join(String),
    #[error("model pack download selection rejected: {0}")]
    Selection(#[from] PackSelectionError),
}
