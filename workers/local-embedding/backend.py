"""ONNX Runtime BGE backend and a deterministic test double.

The production class imports third-party packages only after a verified model
lease has been loaded.  Merely starting the worker cannot import or initialize
an accelerator runtime.
"""

from __future__ import annotations

import hashlib
import json
import math
import threading
import time
from pathlib import Path
from typing import Protocol, Sequence

from model_spec import (
    DIMENSIONS,
    MAX_SEQUENCE_TOKENS,
    MODEL_ID,
    SOURCE_REVISION,
    ArtifactSpec,
    PackSpec,
    SpecError,
    normalize_vector,
    sha256_file,
)

PINNED_ONNXRUNTIME_VERSION = "1.29.0"
PINNED_NUMPY_VERSION = "2.5.2"
PINNED_TOKENIZERS_VERSION = "0.23.1"
SELF_TEST_FIXTURE_SHA256 = "3abdba8b0018a4f553a96cbd55662e1d803cc38bb5e24040337309e695e45f03"


class BackendError(RuntimeError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class Cancelled(RuntimeError):
    pass


class Backend(Protocol):
    backend_id: str

    def load(self, install_dir: Path, pack: PackSpec, *, cpu_threads: int) -> None: ...
    def unload(self) -> None: ...
    def infer(self, texts: Sequence[str], cancelled: threading.Event) -> list[tuple[float, ...]]: ...
    def cancel_active(self) -> None: ...
    def self_test(self) -> dict[str, object]: ...


class OnnxBgeBackend:
    backend_id = "cpu"

    def __init__(self) -> None:
        self._session = None
        self._tokenizer = None
        self._np = None
        self._ort = None
        self._input_names: frozenset[str] = frozenset()
        self._output_name: str | None = None
        self._active_run_options = None
        self._lock = threading.RLock()

    @staticmethod
    def _artifact(pack: PackSpec, artifact_id: str) -> ArtifactSpec:
        artifact = next((item for item in pack.artifacts if item.artifact_id == artifact_id), None)
        if artifact is None:
            raise BackendError("artifact_missing", f"required artifact {artifact_id} is absent")
        return artifact

    def load(self, install_dir: Path, pack: PackSpec, *, cpu_threads: int) -> None:
        if not 1 <= cpu_threads <= 8:
            raise BackendError("invalid_threads", "CPU inference threads must be in 1..=8")
        try:
            import numpy as np
            import onnxruntime as ort
            import tokenizers
        except ImportError as exc:
            raise BackendError("runtime_missing", "the pinned local embedding runtime is not installed") from exc
        versions = {
            "onnxruntime": getattr(ort, "__version__", ""),
            "numpy": getattr(np, "__version__", ""),
            "tokenizers": getattr(tokenizers, "__version__", ""),
        }
        expected = {
            "onnxruntime": PINNED_ONNXRUNTIME_VERSION,
            "numpy": PINNED_NUMPY_VERSION,
            "tokenizers": PINNED_TOKENIZERS_VERSION,
        }
        if versions != expected:
            raise BackendError("runtime_revision_mismatch", "local embedding runtime revisions do not match the reviewed ABI")

        model_artifact = self._artifact(pack, "bge-small-en-v1.5-onnx-fp32")
        tokenizer_artifact = self._artifact(pack, "bge-small-en-v1.5-tokenizer-json")
        model_path = install_dir.joinpath(*model_artifact.destination.parts)
        tokenizer_path = install_dir.joinpath(*tokenizer_artifact.destination.parts)
        for path, artifact in ((model_path, model_artifact), (tokenizer_path, tokenizer_artifact)):
            if path.is_symlink() or not path.is_file():
                raise BackendError("artifact_missing", "a verified model artifact is unavailable")
            digest, size = sha256_file(path, expected_size=artifact.size_bytes)
            if digest != artifact.sha256 or size != artifact.size_bytes:
                raise BackendError("artifact_corrupt", "a model artifact no longer matches its immutable digest")

        options = ort.SessionOptions()
        options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        options.intra_op_num_threads = cpu_threads
        options.inter_op_num_threads = 1
        options.add_session_config_entry("session.intra_op.allow_spinning", "0")
        options.add_session_config_entry("session.inter_op.allow_spinning", "0")
        options.log_severity_level = 3
        if hasattr(ort, "disable_telemetry_events"):
            ort.disable_telemetry_events()
        try:
            tokenizer = tokenizers.Tokenizer.from_file(str(tokenizer_path))
            tokenizer.enable_truncation(max_length=MAX_SEQUENCE_TOKENS, strategy="longest_first")
            pad_id = tokenizer.token_to_id("[PAD]")
            if pad_id is None:
                raise BackendError("tokenizer_invalid", "tokenizer has no [PAD] token")
            tokenizer.enable_padding(direction="right", pad_id=pad_id, pad_token="[PAD]")
            session = ort.InferenceSession(str(model_path), sess_options=options, providers=["CPUExecutionProvider"])
        except BackendError:
            raise
        except Exception as exc:
            raise BackendError("load_failed", "ONNX Runtime could not load the verified embedding pack") from exc

        inputs = frozenset(value.name for value in session.get_inputs())
        required = {"input_ids", "attention_mask"}
        if not required.issubset(inputs) or not inputs.issubset(required | {"token_type_ids"}):
            raise BackendError("model_abi_mismatch", "ONNX model inputs do not match the reviewed BERT embedding ABI")
        outputs = session.get_outputs()
        if not outputs:
            raise BackendError("model_abi_mismatch", "ONNX model exposes no hidden-state output")
        shape = outputs[0].shape
        if not shape or shape[-1] not in (DIMENSIONS, "hidden_size", None):
            raise BackendError("model_abi_mismatch", "ONNX hidden-state dimensions do not match BGE Small")
        with self._lock:
            self._session = session
            self._tokenizer = tokenizer
            self._np = np
            self._ort = ort
            self._input_names = inputs
            self._output_name = outputs[0].name

    def unload(self) -> None:
        self.cancel_active()
        with self._lock:
            self._active_run_options = None
            self._session = None
            self._tokenizer = None
            self._np = None
            self._ort = None
            self._input_names = frozenset()
            self._output_name = None

    def infer(self, texts: Sequence[str], cancelled: threading.Event) -> list[tuple[float, ...]]:
        with self._lock:
            session = self._session
            tokenizer = self._tokenizer
            np = self._np
            ort = self._ort
            inputs = self._input_names
            output_name = self._output_name
        if session is None or tokenizer is None or np is None or ort is None or output_name is None:
            raise BackendError("model_not_loaded", "embedding model is not loaded")
        if cancelled.is_set():
            raise Cancelled()
        try:
            encodings = tokenizer.encode_batch(list(texts), add_special_tokens=True)
            feeds = {
                "input_ids": np.asarray([encoding.ids for encoding in encodings], dtype=np.int64),
                "attention_mask": np.asarray([encoding.attention_mask for encoding in encodings], dtype=np.int64),
            }
            if "token_type_ids" in inputs:
                feeds["token_type_ids"] = np.asarray([encoding.type_ids for encoding in encodings], dtype=np.int64)
            run_options = ort.RunOptions()
            with self._lock:
                self._active_run_options = run_options
            hidden = session.run([output_name], feeds, run_options=run_options)[0]
            if cancelled.is_set():
                raise Cancelled()
            if len(hidden.shape) != 3 or hidden.shape[0] != len(texts) or hidden.shape[2] != DIMENSIONS:
                raise BackendError("model_abi_mismatch", "ONNX hidden-state output shape changed")
            cls = hidden[:, 0, :]
            return [normalize_vector(row.tolist()) for row in cls]
        except Cancelled:
            raise
        except BackendError:
            raise
        except Exception as exc:
            if cancelled.is_set():
                raise Cancelled() from exc
            raise BackendError("inference_failed", "ONNX embedding inference failed") from exc
        finally:
            with self._lock:
                self._active_run_options = None

    def cancel_active(self) -> None:
        with self._lock:
            options = self._active_run_options
            if options is not None:
                options.terminate = True

    def wait_until_active(self, timeout_seconds: float) -> bool:
        """Qualification-only observation hook; no model data is exposed."""

        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            with self._lock:
                if self._active_run_options is not None:
                    return True
            time.sleep(0.0005)
        return False

    def self_test(self) -> dict[str, object]:
        fixture_path = Path(__file__).resolve().parent / "fixtures" / "self-test.request.json"
        fixture_bytes = fixture_path.read_bytes()
        if hashlib.sha256(fixture_bytes).hexdigest() != SELF_TEST_FIXTURE_SHA256:
            raise BackendError("self_test_fixture_tampered", "embedding self-test fixture digest changed")
        try:
            fixture = json.loads(fixture_bytes)
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise BackendError("self_test_fixture_invalid", "embedding self-test fixture is invalid") from exc
        if (
            not isinstance(fixture, dict)
            or fixture.get("schema") != "npc.embedding-self-test-fixture/v1"
            or fixture.get("visibility") != "supervisor_only"
            or fixture.get("mode") != "passage"
            or not isinstance(fixture.get("texts"), list)
            or len(fixture["texts"]) != 3
            or not all(isinstance(text, str) and text for text in fixture["texts"])
            or fixture.get("assertions")
            != {
                "dimensions": DIMENSIONS,
                "finite_f32": True,
                "l2_normalized_tolerance": 0.0001,
                "near_pair": [0, 1],
                "unrelated_pair": [0, 2],
                "minimum_near_cosine": 0.5,
                "near_must_exceed_unrelated": True,
            }
        ):
            raise BackendError("self_test_fixture_invalid", "embedding self-test fixture contract changed")
        texts = fixture["texts"]
        vectors = self.infer(texts, threading.Event())
        near = math.fsum(a * b for a, b in zip(vectors[0], vectors[1], strict=True))
        far = math.fsum(a * b for a, b in zip(vectors[0], vectors[2], strict=True))
        if not (near > far and near > 0.5 and all(abs(math.sqrt(math.fsum(v * v for v in vector)) - 1.0) < 1e-4 for vector in vectors)):
            raise BackendError("self_test_failed", "embedding semantic and normalization invariants failed")
        digest = hashlib.sha256(b"".join(float(value).hex().encode("ascii") + b"\0" for vector in vectors for value in vector)).hexdigest()
        return {
            "contract_version": "npc.embedding-self-test/v1",
            "model_id": MODEL_ID,
            "model_revision": SOURCE_REVISION,
            "backend": self.backend_id,
            "dimensions": DIMENSIONS,
            "normalization": "l2",
            "semantic_ordering_passed": True,
            "input_fixture_sha256": SELF_TEST_FIXTURE_SHA256,
            "output_digest_sha256": digest,
        }


class DeterministicFixtureBackend:
    """Test-only backend. Production worker construction never selects it."""

    backend_id = "fixture"

    def __init__(self, *, delay_seconds: float = 0.0) -> None:
        self.loaded = False
        self.delay_seconds = delay_seconds

    def load(self, install_dir: Path, pack: PackSpec, *, cpu_threads: int) -> None:
        self.loaded = True

    def unload(self) -> None:
        self.loaded = False

    def infer(self, texts: Sequence[str], cancelled: threading.Event) -> list[tuple[float, ...]]:
        import time

        deadline = time.monotonic() + self.delay_seconds
        while time.monotonic() < deadline:
            if cancelled.wait(0.001):
                raise Cancelled()
        vectors = []
        for text in texts:
            seed = hashlib.sha256(text.encode("utf-8")).digest()
            raw = [((seed[index % len(seed)] / 255.0) * 2.0 - 1.0) for index in range(DIMENSIONS)]
            vectors.append(normalize_vector(raw))
        return vectors

    def cancel_active(self) -> None:
        return None

    def self_test(self) -> dict[str, object]:
        return {
            "contract_version": "npc.embedding-self-test/v1",
            "model_id": MODEL_ID,
            "model_revision": SOURCE_REVISION,
            "backend": self.backend_id,
            "dimensions": DIMENSIONS,
            "normalization": "l2",
            "semantic_ordering_passed": False,
            "fixture_only": True,
        }
