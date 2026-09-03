from __future__ import annotations

import dataclasses
import hashlib
import io
import tarfile
from pathlib import Path


def tar_bz2(prefix: str, files: dict[str, bytes]) -> bytes:
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:bz2") as archive:
        root = tarfile.TarInfo(prefix)
        root.type = tarfile.DIRTYPE
        archive.addfile(root)
        for relative, payload in files.items():
            item = tarfile.TarInfo(f"{prefix}/{relative}")
            item.size = len(payload)
            item.mode = 0o644
            archive.addfile(item, io.BytesIO(payload))
    return output.getvalue()


def fixture_manifest(source_manifest: Path):
    from manifest import CriticalFile, load_manifest

    manifest = load_manifest(source_manifest)
    runtime_files = {
        "lib/sherpa-onnx-c-api.dll": b"fixture-c-api",
        "lib/onnxruntime.dll": b"fixture-onnxruntime",
    }
    model_files = {
        "model.int8.onnx": b"fixture-onnx-model",
        "voices.bin": b"fixture-voices",
        "tokens.txt": b"fixture-tokens",
        "lexicon-us-en.txt": b"fixture-lexicon",
        "espeak-ng-data/phondata": b"fixture-phondata",
        "LICENSE": b"fixture-license",
    }
    archives = {
        "sherpa-runtime": tar_bz2(
            manifest.artifacts[0].strip_prefix,
            runtime_files,
        ),
        "kokoro-model": tar_bz2(
            manifest.artifacts[1].strip_prefix,
            model_files,
        ),
    }
    artifacts = tuple(
        dataclasses.replace(
            artifact,
            size_bytes=len(archives[artifact.artifact_id]),
            sha256=hashlib.sha256(
                archives[artifact.artifact_id]
            ).hexdigest(),
        )
        for artifact in manifest.artifacts
    )
    critical_files = tuple(
        CriticalFile(
            path=f"model/{path}",
            size_bytes=len(payload),
            sha256=hashlib.sha256(payload).hexdigest(),
        )
        for path, payload in model_files.items()
    )
    return archives, dataclasses.replace(
        manifest,
        canonical_sha256="f" * 64,
        artifacts=artifacts,
        critical_files=critical_files,
    )


class FakeDownloader:
    def __init__(self, archives: dict[str, bytes]) -> None:
        self.archives = archives
        self.calls: list[str] = []

    def fetch(self, artifact, destination, allowed_hosts) -> None:
        assert "github.com" in allowed_hosts
        payload = self.archives[artifact.artifact_id]
        assert len(payload) == artifact.size_bytes
        assert hashlib.sha256(payload).hexdigest() == artifact.sha256
        destination.write_bytes(payload)
        self.calls.append(artifact.artifact_id)
