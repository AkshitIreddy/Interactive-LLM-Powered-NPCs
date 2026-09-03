"""Explicit local-review lifecycle and one-run qualification CLI."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import threading
import time
from pathlib import Path

from .constants import MODEL_ID
from .digest import canonical_json_bytes, sha256_bytes
from .download import HttpsArtifactFetcher
from .errors import LocalLlmError
from .lifecycle import ModelPackLifecycle, RuntimeBundleLifecycle
from .manifest import ModelPack, RuntimeBundle
from .measurements import local_review_report, nvidia_smi_snapshot, windows_process_memory
from .protocol import parse_chat_request
from .server import LlamaServerSupervisor, ServerConfig


def _paths(options) -> tuple[ModelPack, RuntimeBundle]:  # type: ignore[no-untyped-def]
    return ModelPack.load(options.manifest.resolve(strict=True)), RuntimeBundle.load(options.runtime_bundle.resolve(strict=True))


def _gpu_lock_required(options) -> None:  # type: ignore[no-untyped-def]
    if options.runtime_variant != "windows-x64-vulkan":
        return
    lock = options.gpu_lock_file
    if lock is None:
        raise LocalLlmError("gpu_lock_required", "Vulkan qualification requires an explicit GPU lock file")
    try:
        state = lock.read_text(encoding="utf-8").strip().lower()
    except OSError as error:
        raise LocalLlmError("gpu_lock_required", "GPU lock state could not be read") from error
    if state != "yes":
        raise LocalLlmError("gpu_lock_required", "GPU lock is not authorized")


def command_plan(options) -> dict[str, object]:  # type: ignore[no-untyped-def]
    pack, bundle = _paths(options)
    variant = bundle.variant(options.runtime_variant)
    total = sum(artifact.size_bytes for artifact in pack.artifacts) + variant.artifact.size_bytes
    return {
        "network_access": False,
        "downloads_started": False,
        "model_pack_download_bytes": sum(artifact.size_bytes for artifact in pack.artifacts),
        "runtime_download_bytes": variant.artifact.size_bytes,
        "total_download_bytes": total,
        "model_installed_payload_bytes": sum(artifact.size_bytes for artifact in pack.artifacts),
        "model_atomic_staging_peak_bytes": 4_994_607_340,
        "runtime_expanded_bytes": None,
        "runtime_expanded_bytes_reason": "unknown until the pinned archive is explicitly downloaded and safely inspected",
        "manifest": str(pack.path),
        "runtime_bundle": str(bundle.path),
        "runtime_variant": variant.variant_id,
    }


def command_install(options) -> dict[str, object]:  # type: ignore[no-untyped-def]
    pack, bundle = _paths(options)
    fetcher = HttpsArtifactFetcher(timeout_seconds=120)
    model_lifecycle = ModelPackLifecycle(options.install_root, fetcher)
    runtime_lifecycle = RuntimeBundleLifecycle(options.install_root, fetcher)
    model_receipt = model_lifecycle.install(
        pack, explicit_user_confirmation=options.i_understand_downloads
    )
    runtime_receipt = runtime_lifecycle.install(
        bundle,
        options.runtime_variant,
        explicit_user_confirmation=options.i_understand_downloads,
    )
    return {"model": model_receipt.to_json(), "runtime": runtime_receipt.to_json()}


def command_verify(options) -> dict[str, object]:  # type: ignore[no-untyped-def]
    pack, bundle = _paths(options)
    fetcher = HttpsArtifactFetcher()
    model = ModelPackLifecycle(options.install_root, fetcher).verify(pack)
    runtime = RuntimeBundleLifecycle(options.install_root, fetcher).verify(bundle, options.runtime_variant)
    return {"verified": True, "model": model.to_json(), "runtime": runtime.to_json()}


def command_qualify(options) -> dict[str, object]:  # type: ignore[no-untyped-def]
    _gpu_lock_required(options)
    pack, bundle = _paths(options)
    variant = bundle.variant(options.runtime_variant)
    fetcher = HttpsArtifactFetcher()
    ModelPackLifecycle(options.install_root, fetcher).verify(pack)
    RuntimeBundleLifecycle(options.install_root, fetcher).verify(bundle, options.runtime_variant)
    model_path = options.install_root / "packs" / pack.pack_id / pack.revision / Path(*pack.model_artifact.destination.parts)
    runtime_root = options.install_root / "runtimes" / bundle.abi / variant.variant_id / "payload"
    executables = list(runtime_root.rglob(bundle.entrypoint))
    if len(executables) != 1:
        raise LocalLlmError("runtime_entrypoint_mismatch", "runtime entrypoint is not uniquely installed")
    fixture = json.loads(options.self_test.resolve(strict=True).read_bytes())
    chat = parse_chat_request(
        {
            "messages": fixture["messages"],
            "max_tokens": fixture["sampling"]["max_tokens"],
            "temperature": fixture["sampling"]["temperature"],
            "top_p": fixture["sampling"]["top_p"],
            "seed": fixture["sampling"]["seed"],
            "response_json_schema": fixture["response_json_schema"],
        }
    )
    before = nvidia_smi_snapshot("before") if variant.backend == "vulkan" else {"label": "before", "devices": []}
    config = ServerConfig(
        executable=executables[0],
        model_path=model_path,
        runtime_abi=bundle.abi,
        backend=variant.backend,
        context_tokens=options.context_tokens,
        cpu_threads=options.cpu_threads,
        gpu_layers=options.gpu_layers if variant.backend == "vulkan" else 0,
        startup_timeout_seconds=options.startup_timeout_seconds,
        request_timeout_seconds=options.request_timeout_seconds,
    )
    server = LlamaServerSupervisor(config)
    load_started = time.perf_counter()
    server.start()
    load_millis = (time.perf_counter() - load_started) * 1000
    assert server.process is not None
    process_memory = windows_process_memory(server.process.pid)
    during = nvidia_smi_snapshot("loaded") if variant.backend == "vulkan" else {"label": "loaded", "devices": []}
    started = time.perf_counter()
    first_token = None
    token_times: list[float] = []
    text_parts: list[str] = []
    usage: dict[str, int] = {}
    for delta in server.stream_chat(chat, lambda: False):
        now = time.perf_counter()
        if delta.text:
            if first_token is None:
                first_token = now
            token_times.append(now)
            text_parts.append(delta.text)
        if delta.usage:
            usage.update(delta.usage)
    finished = time.perf_counter()
    if first_token is None:
        server.stop()
        raise LocalLlmError("self_test_failed", "local LLM self-test emitted no output")
    try:
        actual = json.loads("".join(text_parts))
    except json.JSONDecodeError as error:
        server.stop()
        raise LocalLlmError("self_test_failed", "local LLM self-test output was not JSON") from error
    expected = fixture["expected_canonical_output"]
    if actual != expected:
        server.stop()
        raise LocalLlmError("self_test_failed", "local LLM self-test output did not match the constrained fixture")
    expected_digest = pack.self_test.get("expected_output_sha256")
    if sha256_bytes(canonical_json_bytes(actual)) != expected_digest:
        server.stop()
        raise LocalLlmError("self_test_failed", "local LLM self-test digest did not match the manifest")
    cancel_event = threading.Event()
    cancellation_result: dict[str, object] = {"started": False, "cancelled": False, "runtime_preserved": False}

    def cancellation_probe() -> None:
        request = parse_chat_request(
            {
                "prompt": "Write a long numbered list of short neutral words.",
                "max_tokens": 512,
                "temperature": 0.0,
            }
        )
        cancellation_result["started"] = True
        try:
            for delta in server.stream_chat(request, cancel_event.is_set):
                if delta.text:
                    cancel_event.set()
        except LocalLlmError as error:
            cancellation_result["cancelled"] = error.code == "cancelled"

    cancel_thread = threading.Thread(target=cancellation_probe, daemon=True)
    cancel_thread.start()
    cancel_thread.join(timeout=10)
    if cancel_thread.is_alive():
        cancel_event.set()
    cancellation_result["runtime_preserved"] = server.cancel_active(grace_seconds=2.0)
    cancel_thread.join(timeout=5)
    server.stop()
    reload_started = time.perf_counter()
    reloaded = LlamaServerSupervisor(config)
    reloaded.start()
    reload_millis = (time.perf_counter() - reload_started) * 1000
    reloaded.stop()
    after = nvidia_smi_snapshot("after_unload") if variant.backend == "vulkan" else {"label": "after_unload", "devices": []}
    token_gaps = [(right - left) * 1000 for left, right in zip(token_times, token_times[1:])]
    report = local_review_report(
        backend=variant.backend,
        model_manifest_sha256=hashlib.sha256(pack.path.read_bytes()).hexdigest(),
        runtime_archive_sha256=variant.artifact.sha256,
        load_millis=load_millis,
        reload_millis=reload_millis,
        ttft_millis=(first_token - started) * 1000,
        total_millis=(finished - started) * 1000,
        output_tokens=usage.get("completion_tokens"),
        inter_token_millis=token_gaps,
        process_memory=process_memory,
        gpu_snapshots=[before, during, after],
        cancellation=cancellation_result,
        checksum_and_license_verified=True,
    )
    if options.report is not None:
        options.report.parent.mkdir(parents=True, exist_ok=True)
        options.report.write_bytes(json.dumps(report, indent=2, sort_keys=True).encode("utf-8") + b"\n")
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Qwen3 local-review pack lifecycle")
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("packaging/model-packs/qwen3-4b-instruct-2507-q4-k-m.json"),
    )
    parser.add_argument(
        "--runtime-bundle",
        type=Path,
        default=Path("workers/local-llm/runtime-bundle.b10689.json"),
    )
    parser.add_argument("--install-root", type=Path, required=True)
    parser.add_argument(
        "--runtime-variant",
        choices=("windows-x64-vulkan", "windows-x64-cpu"),
        default="windows-x64-vulkan",
    )
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("plan")
    install = subcommands.add_parser("install")
    install.add_argument("--i-understand-downloads", action="store_true")
    subcommands.add_parser("verify")
    qualify = subcommands.add_parser("qualify")
    qualify.add_argument("--gpu-lock-file", type=Path)
    qualify.add_argument("--self-test", type=Path, default=Path("workers/local-llm/fixtures/self-test.request.json"))
    qualify.add_argument("--report", type=Path)
    qualify.add_argument("--context-tokens", type=int, default=8_192)
    qualify.add_argument("--cpu-threads", type=int, default=8)
    qualify.add_argument("--gpu-layers", type=int, default=99)
    qualify.add_argument("--startup-timeout-seconds", type=float, default=180.0)
    qualify.add_argument("--request-timeout-seconds", type=float, default=120.0)
    return parser


def main(argv: list[str] | None = None) -> int:
    options = build_parser().parse_args(argv)
    try:
        if options.command == "plan":
            result = command_plan(options)
        elif options.command == "install":
            result = command_install(options)
        elif options.command == "verify":
            result = command_verify(options)
        else:
            result = command_qualify(options)
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except LocalLlmError as error:
        print(json.dumps({"ok": False, "error": error.event_error()}, sort_keys=True))
        return 2
