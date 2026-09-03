"""HTTPS-only, redirect-constrained, exact-length artifact retrieval."""

from __future__ import annotations

import hashlib
import os
import ssl
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Protocol

from .constants import ALLOWED_DOWNLOAD_HOSTS
from .errors import LocalLlmError
from .manifest import Artifact, validate_pinned_https_url

DOWNLOAD_CHUNK_BYTES = 4 * 1_048_576


class ArtifactFetcher(Protocol):
    def fetch(
        self,
        artifact: Artifact,
        destination: Path,
        *,
        cancelled: Callable[[], bool] | None = None,
    ) -> "DownloadEvidence": ...


@dataclass(frozen=True, slots=True)
class DownloadEvidence:
    url: str
    size_bytes: int
    sha256: str


class _PinnedRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, new_url):  # type: ignore[no-untyped-def]
        validate_pinned_or_cdn_url(new_url)
        return super().redirect_request(request, fp, code, message, headers, new_url)


def validate_pinned_or_cdn_url(url: str) -> None:
    from urllib.parse import urlsplit

    parsed = urlsplit(url)
    if parsed.scheme != "https" or parsed.username or parsed.password or parsed.fragment:
        raise LocalLlmError("unsafe_redirect", "artifact redirect is not credential-free HTTPS")
    if (parsed.hostname or "").lower() not in ALLOWED_DOWNLOAD_HOSTS:
        raise LocalLlmError("unsafe_redirect", "artifact redirect left the approved host set")


class HttpsArtifactFetcher:
    """Production network adapter; model-manager still owns catalog authorization."""

    def __init__(self, *, timeout_seconds: float = 60.0) -> None:
        if timeout_seconds <= 0 or timeout_seconds > 600:
            raise ValueError("timeout_seconds is outside the supported range")
        self.timeout_seconds = timeout_seconds
        context = ssl.create_default_context()
        self._opener = urllib.request.build_opener(
            _PinnedRedirectHandler(),
            urllib.request.HTTPSHandler(context=context),
        )

    def fetch(
        self,
        artifact: Artifact,
        destination: Path,
        *,
        cancelled: Callable[[], bool] | None = None,
    ) -> DownloadEvidence:
        if destination.exists():
            raise LocalLlmError("unsafe_staging", "download destination already exists")
        last_error: Exception | None = None
        for source_url in artifact.source_urls:
            validate_pinned_https_url(source_url)
            try:
                return self._fetch_one(artifact, source_url, destination, cancelled=cancelled)
            except LocalLlmError as error:
                last_error = error
                if error.code in {"cancelled", "digest_mismatch", "size_mismatch", "unsafe_redirect"}:
                    raise
            except (OSError, urllib.error.URLError) as error:
                last_error = error
            destination.unlink(missing_ok=True)
        raise LocalLlmError("download_failed", "all approved artifact sources failed", retryable=True) from last_error

    def _fetch_one(
        self,
        artifact: Artifact,
        source_url: str,
        destination: Path,
        *,
        cancelled: Callable[[], bool] | None,
    ) -> DownloadEvidence:
        request = urllib.request.Request(
            source_url,
            headers={
                "Accept": "application/octet-stream",
                "Accept-Encoding": "identity",
                "User-Agent": "InteractiveNPCs-ModelManager/2.0",
            },
            method="GET",
        )
        descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        digest = hashlib.sha256()
        total = 0
        try:
            with self._opener.open(request, timeout=self.timeout_seconds) as response, os.fdopen(
                descriptor, "wb", buffering=0
            ) as output:
                descriptor = -1
                validate_pinned_or_cdn_url(response.geturl())
                encoding = response.headers.get("Content-Encoding", "identity").strip().lower()
                if encoding not in {"", "identity"}:
                    raise LocalLlmError("encoded_artifact", "artifact response used an unsupported content encoding")
                declared = response.headers.get("Content-Length")
                if declared is not None:
                    try:
                        declared_length = int(declared)
                    except ValueError as error:
                        raise LocalLlmError("invalid_content_length", "artifact response length is invalid") from error
                    if declared_length != artifact.size_bytes:
                        raise LocalLlmError("size_mismatch", "artifact response length does not match the manifest")
                while True:
                    if cancelled is not None and cancelled():
                        raise LocalLlmError("cancelled", "artifact download was cancelled")
                    block = response.read(DOWNLOAD_CHUNK_BYTES)
                    if not block:
                        break
                    total += len(block)
                    if total > artifact.size_bytes:
                        raise LocalLlmError("size_mismatch", "artifact exceeded its declared byte length")
                    digest.update(block)
                    output.write(block)
                output.flush()
                os.fsync(output.fileno())
        except Exception:
            if descriptor >= 0:
                os.close(descriptor)
            destination.unlink(missing_ok=True)
            raise
        actual = digest.hexdigest()
        if total != artifact.size_bytes:
            destination.unlink(missing_ok=True)
            raise LocalLlmError("size_mismatch", "artifact byte length does not match the manifest")
        if actual != artifact.sha256:
            destination.unlink(missing_ok=True)
            raise LocalLlmError("digest_mismatch", "artifact digest does not match the manifest")
        return DownloadEvidence(source_url, total, actual)


class MemoryArtifactFetcher:
    """Test adapter that never opens a network connection."""

    def __init__(self, payloads: dict[str, bytes]) -> None:
        self.payloads = dict(payloads)
        self.calls: list[str] = []

    def fetch(
        self,
        artifact: Artifact,
        destination: Path,
        *,
        cancelled: Callable[[], bool] | None = None,
    ) -> DownloadEvidence:
        self.calls.append(artifact.artifact_id)
        if cancelled is not None and cancelled():
            raise LocalLlmError("cancelled", "artifact download was cancelled")
        payload = self.payloads.get(artifact.artifact_id)
        if payload is None:
            raise LocalLlmError("download_failed", "fixture artifact is unavailable")
        actual = hashlib.sha256(payload).hexdigest()
        if len(payload) != artifact.size_bytes:
            raise LocalLlmError("size_mismatch", "fixture artifact length does not match the manifest")
        if actual != artifact.sha256:
            raise LocalLlmError("digest_mismatch", "fixture artifact digest does not match the manifest")
        descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as output:
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
        return DownloadEvidence(artifact.source_urls[0], len(payload), actual)
