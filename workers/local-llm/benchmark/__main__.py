"""CLI for non-model CUDA preparation and explicitly gated model benchmarks."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from npc_local_llm.digest import sha256_file
from npc_local_llm.download import HttpsArtifactFetcher
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.lifecycle import ModelPackLifecycle, RuntimeBundleLifecycle
from npc_local_llm.manifest import ModelPack, RuntimeBundle

from .bundle import CudaBenchmarkBundle, install_bundle, verify_bundle
from .config import BenchmarkContract
from .harness import run_benchmark, run_game_matrix
from .lmstudio import run_observational_control
from .schedule import abba_schedule, warmup_schedule


def _shared(options: argparse.Namespace) -> tuple[Path, BenchmarkContract, CudaBenchmarkBundle]:
    repo_root = options.repo_root.resolve(strict=True)
    contract = BenchmarkContract.load((repo_root / options.contract).resolve(strict=True), repo_root)
    cuda_bundle = CudaBenchmarkBundle.load((repo_root / options.cuda_bundle).resolve(strict=True))
    return repo_root, contract, cuda_bundle


def command_plan(options: argparse.Namespace) -> dict[str, Any]:
    _repo, contract, bundle = _shared(options)
    archives = sum(item.size_bytes for item in bundle.assets)
    return {
        "schema": "npc.local-llm.backend-ab-plan/v1",
        "network_access": False,
        "downloads_started": False,
        "model_process_started": False,
        "gpu_lock_read_or_changed": False,
        "cuda_archive_download_bytes": archives,
        "cuda_expanded_payload_bytes": bundle.expected_expanded_bytes,
        "cuda_extraction_peak_bytes": archives + bundle.expected_expanded_bytes,
        "cuda_installed_additional_allocated_bytes": bundle.expected_expanded_bytes,
        "runtime": f"llama.cpp {bundle.release_tag}@{bundle.source_commit}",
        "cuda_runtime": bundle.cuda_runtime,
        "warmup_schedule": list(warmup_schedule(contract.warmups_per_backend)),
        "measured_schedule": list(abba_schedule(contract.measured_samples_per_backend)),
        "measured_samples_per_backend": contract.measured_samples_per_backend,
        "direct_p99_sample_gate": 20,
        "observational_control_samples": 10,
        "observational_control_p99_allowed": False,
    }


def command_install_cuda(options: argparse.Namespace) -> dict[str, Any]:
    _repo, _contract, bundle = _shared(options)
    if not options.i_understand_local_extraction:
        raise LocalLlmError("confirmation_required", "CUDA runtime extraction requires the explicit confirmation flag")
    return install_bundle(bundle, options.archive_root.resolve(strict=True), options.cuda_install_root.resolve(strict=False))


def command_verify_cuda(options: argparse.Namespace) -> dict[str, Any]:
    _repo, _contract, bundle = _shared(options)
    return verify_bundle(
        bundle,
        options.cuda_install_root.resolve(strict=True),
        options.archive_root.resolve(strict=True) if options.archive_root is not None else None,
    )


def _direct_paths(
    options: argparse.Namespace, bundle: CudaBenchmarkBundle, repo_root: Path
) -> tuple[Path, Path, Path, dict[str, Any]]:
    model_pack = ModelPack.load((repo_root / "packaging/model-packs/qwen3-4b-instruct-2507-q4-k-m.json").resolve(strict=True))
    vulkan_bundle = RuntimeBundle.load((repo_root / "workers/local-llm/runtime-bundle.b10689.json").resolve(strict=True))
    local_review_root = options.local_review_install_root.resolve(strict=True)
    fetcher = HttpsArtifactFetcher()
    model_receipt = ModelPackLifecycle(local_review_root, fetcher).verify(model_pack)
    vulkan_receipt = RuntimeBundleLifecycle(local_review_root, fetcher).verify(vulkan_bundle, "windows-x64-vulkan")
    model_path = (
        local_review_root
        / "packs"
        / model_pack.pack_id
        / model_pack.revision
        / Path(*model_pack.model_artifact.destination.parts)
    ).resolve(strict=True)
    vulkan_root = local_review_root / "runtimes" / vulkan_bundle.abi / "windows-x64-vulkan" / "payload"
    vulkan_candidates = list(vulkan_root.rglob(vulkan_bundle.entrypoint))
    if len(vulkan_candidates) != 1:
        raise LocalLlmError("vulkan_entrypoint_mismatch", "installed Vulkan runtime entrypoint is not unique")
    vulkan_executable = vulkan_candidates[0].resolve(strict=True)
    cuda_install = options.cuda_install_root.resolve(strict=True)
    cuda_receipt = verify_bundle(bundle, cuda_install, options.archive_root.resolve(strict=True))["receipt"]
    candidates = list((cuda_install / "payload").rglob(bundle.entrypoint))
    if len(candidates) != 1:
        raise LocalLlmError("cuda_entrypoint_mismatch", "installed CUDA runtime entrypoint is not unique")
    runtime_identity = {
        "release_tag": bundle.release_tag,
        "source_commit": bundle.source_commit,
        "cuda_runtime": bundle.cuda_runtime,
        "cuda_bundle_sha256": sha256_file(bundle.path),
        "cuda_receipt": cuda_receipt,
        "vulkan": {
            "bundle_sha256": sha256_file(vulkan_bundle.path),
            "archive_sha256": vulkan_bundle.variant("windows-x64-vulkan").artifact.sha256,
            "archive_size_bytes": vulkan_bundle.variant("windows-x64-vulkan").artifact.size_bytes,
            "install_receipt": vulkan_receipt.to_json(),
            "entrypoint_size_bytes": vulkan_executable.stat().st_size,
            "entrypoint_sha256": sha256_file(vulkan_executable),
        },
        "model_manifest_sha256": sha256_file(model_pack.path),
        "model_install_receipt": model_receipt.to_json(),
    }
    return model_path, vulkan_executable, candidates[0], runtime_identity


def command_run_direct(options: argparse.Namespace) -> dict[str, Any]:
    repo, contract, bundle = _shared(options)
    model, vulkan, cuda, identity = _direct_paths(options, bundle, repo)
    report = run_benchmark(
        contract=contract,
        model_path=model,
        vulkan_executable=vulkan,
        cuda_executable=cuda,
        gpu_lock_file=options.gpu_lock_file,
        execute_model_benchmark=options.execute_model_benchmark,
        runtime_identity=identity,
    )
    _write_report(options.report, report)
    return report


def command_game_matrix(options: argparse.Namespace) -> dict[str, Any]:
    repo, contract, bundle = _shared(options)
    model, vulkan, cuda, _identity = _direct_paths(options, bundle, repo)
    report = run_game_matrix(
        contract=contract,
        repo_root=repo,
        model_path=model,
        vulkan_executable=vulkan,
        cuda_executable=cuda,
        gpu_lock_file=options.gpu_lock_file,
        execute_model_benchmark=options.execute_model_benchmark,
        allow_synthetic_game_gui=options.allow_synthetic_game_gui,
    )
    _write_report(options.report, report)
    return report


def command_lmstudio_control(options: argparse.Namespace) -> dict[str, Any]:
    repo, contract, _bundle = _shared(options)
    model_pack = ModelPack.load((repo / "packaging/model-packs/qwen3-4b-instruct-2507-q4-k-m.json").resolve(strict=True))
    local_review_root = options.local_review_install_root.resolve(strict=True)
    ModelPackLifecycle(local_review_root, HttpsArtifactFetcher()).verify(model_pack)
    model_path = (
        local_review_root
        / "packs"
        / model_pack.pack_id
        / model_pack.revision
        / Path(*model_pack.model_artifact.destination.parts)
    ).resolve(strict=True)
    report = run_observational_control(
        control_path=(repo / "workers/local-llm/benchmark/lmstudio-control.2.31.2.json").resolve(strict=True),
        fixture_path=contract.fixture_path,
        model_path=model_path,
        gpu_lock_file=options.gpu_lock_file,
        execute_model_benchmark=options.execute_model_benchmark,
        vram_return_tolerance_mib=contract.vram_return_tolerance_mib,
        vram_return_timeout_seconds=contract.vram_return_timeout_seconds,
    )
    _write_report(options.report, report)
    return report


def _write_report(path: Path, report: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(json.dumps(report, indent=2, sort_keys=True).encode("utf-8") + b"\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Pinned Qwen3 direct-backend benchmark lane")
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--contract", type=Path, default=Path("workers/local-llm/benchmark/benchmark-config.json"))
    parser.add_argument(
        "--cuda-bundle", type=Path, default=Path("workers/local-llm/benchmark/runtime-bundle.cuda12.4.b10689.json")
    )
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("plan")
    install = subcommands.add_parser("install-cuda")
    install.add_argument("--archive-root", type=Path, required=True)
    install.add_argument("--cuda-install-root", type=Path, required=True)
    install.add_argument("--i-understand-local-extraction", action="store_true")
    verify = subcommands.add_parser("verify-cuda")
    verify.add_argument("--cuda-install-root", type=Path, required=True)
    verify.add_argument("--archive-root", type=Path)
    for name in ("run-direct", "game-matrix"):
        command = subcommands.add_parser(name)
        command.add_argument("--local-review-install-root", type=Path, required=True)
        command.add_argument("--cuda-install-root", type=Path, required=True)
        command.add_argument("--archive-root", type=Path, required=True)
        command.add_argument("--gpu-lock-file", type=Path, required=True)
        command.add_argument("--report", type=Path, required=True)
        command.add_argument("--execute-model-benchmark", action="store_true")
        if name == "game-matrix":
            command.add_argument("--allow-synthetic-game-gui", action="store_true")
    lmstudio = subcommands.add_parser("lmstudio-control")
    lmstudio.add_argument("--local-review-install-root", type=Path, required=True)
    lmstudio.add_argument("--gpu-lock-file", type=Path, required=True)
    lmstudio.add_argument("--report", type=Path, required=True)
    lmstudio.add_argument("--execute-model-benchmark", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    options = build_parser().parse_args(argv)
    try:
        if options.command == "plan":
            result = command_plan(options)
        elif options.command == "install-cuda":
            result = command_install_cuda(options)
        elif options.command == "verify-cuda":
            result = command_verify_cuda(options)
        elif options.command == "run-direct":
            result = command_run_direct(options)
        elif options.command == "game-matrix":
            result = command_game_matrix(options)
        else:
            result = command_lmstudio_control(options)
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except LocalLlmError as error:
        print(json.dumps({"ok": False, "error": error.event_error()}, sort_keys=True))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
