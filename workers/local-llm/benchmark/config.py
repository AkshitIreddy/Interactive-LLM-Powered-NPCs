"""Strict parser for the frozen direct-backend benchmark contract."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from npc_local_llm.constants import MODEL_ARTIFACT_SHA256, MODEL_ARTIFACT_SIZE, MODEL_ID, RUNTIME_COMMIT, RUNTIME_RELEASE
from npc_local_llm.digest import sha256_file
from npc_local_llm.errors import LocalLlmError


@dataclass(frozen=True, slots=True)
class BenchmarkControls:
    context_tokens: int
    batch_tokens: int
    ubatch_tokens: int
    cache_type_k: str
    cache_type_v: str
    flash_attention: str
    gpu_layers: int
    cpu_threads: int
    cpu_threads_batch: int
    cache_prompt: bool


@dataclass(frozen=True, slots=True)
class BenchmarkContract:
    path: Path
    raw: dict[str, Any]
    fixture_path: Path
    controls: BenchmarkControls
    warmups_per_backend: int
    measured_samples_per_backend: int
    vram_return_tolerance_mib: int
    vram_return_timeout_seconds: int

    @classmethod
    def load(cls, path: Path, repo_root: Path) -> "BenchmarkContract":
        try:
            raw = json.loads(path.read_bytes())
            if raw["schema"] != "npc.local-llm.backend-ab-benchmark/v1":
                raise ValueError("schema")
            if raw["admission_evidence"] is not False:
                raise ValueError("admission classification")
            model = raw["model"]
            if model != {
                "pack_id": MODEL_ID,
                "revision": "Qwen3-4B-Instruct-2507",
                "size_bytes": MODEL_ARTIFACT_SIZE,
                "sha256": MODEL_ARTIFACT_SHA256,
            }:
                raise ValueError("model identity")
            runtime = raw["runtime"]
            if runtime["name"] != "llama.cpp" or runtime["release_tag"] != RUNTIME_RELEASE:
                raise ValueError("runtime release")
            if runtime["source_commit"] != RUNTIME_COMMIT or runtime["backends"] != ["vulkan", "cuda"]:
                raise ValueError("runtime identity")
            fixture = raw["fixture"]
            fixture_path = (repo_root / fixture["path"]).resolve(strict=True)
            if sha256_file(fixture_path) != fixture["sha256"]:
                raise ValueError("fixture identity")
            if fixture["chat_template"] != "gguf_embedded_jinja" or fixture["seed"] != 424242:
                raise ValueError("prompt controls")
            if fixture["temperature"] != 0.0 or fixture["top_p"] != 1.0 or fixture["greedy"] is not True:
                raise ValueError("sampling controls")
            controls = raw["server_controls"]
            expected_extras = {
                "parallel_slots": 1,
                "reasoning_format": "none",
                "offline": True,
            }
            for key, value in expected_extras.items():
                if controls[key] != value:
                    raise ValueError(f"server control {key}")
            parsed_controls = BenchmarkControls(
                context_tokens=_integer(controls["context_tokens"], 512, 32768),
                batch_tokens=_integer(controls["batch_tokens"], 1, 8192),
                ubatch_tokens=_integer(controls["ubatch_tokens"], 1, 8192),
                cache_type_k=_choice(controls["cache_type_k"], {"f16", "q8_0"}),
                cache_type_v=_choice(controls["cache_type_v"], {"f16", "q8_0"}),
                flash_attention=_choice(controls["flash_attention"], {"on", "off", "auto"}),
                gpu_layers=_integer(controls["gpu_layers"], 1, 256),
                cpu_threads=_integer(controls["cpu_threads"], 1, 128),
                cpu_threads_batch=_integer(controls["cpu_threads_batch"], 1, 128),
                cache_prompt=_boolean(controls["cache_prompt"]),
            )
            if parsed_controls.ubatch_tokens > parsed_controls.batch_tokens:
                raise ValueError("ubatch exceeds batch")
            sampling = raw["sampling"]
            if sampling["schedule"] != "alternating_abba_baab":
                raise ValueError("schedule")
            if sampling["minimum_signed_samples_for_each_p99"] != 20:
                raise ValueError("p99 threshold")
            for field in (
                "structured_output_every_sample",
                "cancellation_recovery_every_sample",
                "unload_reload_every_sample",
                "vram_return_every_sample",
            ):
                if sampling[field] is not True:
                    raise ValueError(field)
            if sampling["warmups_per_backend"] != 2 or sampling["measured_samples_per_backend"] != 20:
                raise ValueError("sample counts")
            game = raw["game_matrix"]
            if game["classification"] != "bounded_synthetic_host_load_not_real_game_frame_impact":
                raise ValueError("game classification")
            if game["repeats_per_backend_per_state"] != 5 or game["report_percentiles"] is not False:
                raise ValueError("game sample policy")
            control = raw["observational_control"]
            if control["samples"] != 10 or control["admission_evidence"] is not False:
                raise ValueError("observational control")
            if control["p99_reporting_allowed"] is not False:
                raise ValueError("observational p99 policy")
            return cls(
                path=path.resolve(strict=True),
                raw=raw,
                fixture_path=fixture_path,
                controls=parsed_controls,
                warmups_per_backend=2,
                measured_samples_per_backend=20,
                vram_return_tolerance_mib=_integer(sampling["vram_return_tolerance_mib"], 0, 4096),
                vram_return_timeout_seconds=_integer(sampling["vram_return_timeout_seconds"], 1, 120),
            )
        except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError, LocalLlmError) as error:
            raise LocalLlmError("invalid_benchmark_contract", "local LLM benchmark contract is invalid") from error


def _integer(value: object, minimum: int, maximum: int) -> int:
    if type(value) is not int or not minimum <= value <= maximum:
        raise ValueError("bounded integer required")
    return value


def _choice(value: object, choices: set[str]) -> str:
    if not isinstance(value, str) or value not in choices:
        raise ValueError("unexpected choice")
    return value


def _boolean(value: object) -> bool:
    if type(value) is not bool:
        raise ValueError("boolean required")
    return value

