from __future__ import annotations

import hashlib
import io
import tempfile
import types
import unittest
import zipfile
from collections import Counter
from pathlib import Path, PurePosixPath

from benchmark.__main__ import command_plan
from benchmark.bundle import BundleAsset, CudaBenchmarkBundle, install_bundle, verify_bundle
from benchmark.config import BenchmarkContract
from benchmark.harness import comparable_argv, direct_backend_configs, require_explicit_gpu_grant, run_game_matrix
from benchmark.lmstudio import build_load_command, load_control
from benchmark.schedule import abba_schedule, measured_distribution, warmup_schedule
from npc_local_llm.errors import LocalLlmError

from support import ROOT


CONTRACT = ROOT / "workers/local-llm/benchmark/benchmark-config.json"
CUDA_BUNDLE = ROOT / "workers/local-llm/benchmark/runtime-bundle.cuda12.4.b10689.json"
LM_CONTROL = ROOT / "workers/local-llm/benchmark/lmstudio-control.2.31.2.json"


class BenchmarkContractTests(unittest.TestCase):
    def test_frozen_contract_and_balanced_twenty_sample_schedule(self) -> None:
        contract = BenchmarkContract.load(CONTRACT, ROOT)
        schedule = abba_schedule(contract.measured_samples_per_backend)
        self.assertEqual(len(schedule), 40)
        self.assertEqual(Counter(schedule), Counter({"vulkan": 20, "cuda": 20}))
        self.assertEqual(schedule[:8], ("vulkan", "cuda", "cuda", "vulkan", "cuda", "vulkan", "vulkan", "cuda"))
        self.assertEqual(warmup_schedule(contract.warmups_per_backend), ("vulkan", "cuda", "cuda", "vulkan"))

    def test_p99_refuses_fewer_than_twenty_observations(self) -> None:
        with self.assertRaises(LocalLlmError):
            measured_distribution(range(19))
        result = measured_distribution(range(20))
        self.assertEqual(result["sample_count"], 20)
        self.assertIsNotNone(result["p99"])

    def test_direct_backend_argv_is_identical_beyond_runtime_path(self) -> None:
        contract = BenchmarkContract.load(CONTRACT, ROOT)
        configs = direct_backend_configs(
            contract,
            Path(r"C:\models\model.gguf"),
            Path(r"C:\vulkan\llama-server.exe"),
            Path(r"C:\cuda\llama-server.exe"),
        )
        commands = comparable_argv(configs)
        self.assertEqual(commands["vulkan"], commands["cuda"])
        command = commands["cuda"]
        for flag, value in (
            ("--ctx-size", "8192"),
            ("--batch-size", "2048"),
            ("--ubatch-size", "512"),
            ("--cache-type-k", "f16"),
            ("--cache-type-v", "f16"),
            ("--flash-attn", "auto"),
            ("--gpu-layers", "99"),
            ("--threads", "8"),
            ("--threads-batch", "8"),
        ):
            self.assertEqual(command[command.index(flag) + 1], value)

    def test_execution_gate_never_mutates_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            lock = Path(temporary) / "gpu use.txt"
            lock.write_text("no\n", encoding="utf-8")
            with self.assertRaises(LocalLlmError):
                require_explicit_gpu_grant(lock, True)
            self.assertEqual(lock.read_text(encoding="utf-8"), "no\n")
            lock.write_text("yes\n", encoding="utf-8")
            with self.assertRaises(LocalLlmError):
                require_explicit_gpu_grant(lock, False)
            self.assertEqual(lock.read_text(encoding="utf-8"), "yes\n")
            require_explicit_gpu_grant(lock, True)
            self.assertEqual(lock.read_text(encoding="utf-8"), "yes\n")

    def test_plan_declares_no_lock_or_model_activity(self) -> None:
        options = types.SimpleNamespace(
            repo_root=ROOT,
            contract=Path("workers/local-llm/benchmark/benchmark-config.json"),
            cuda_bundle=Path("workers/local-llm/benchmark/runtime-bundle.cuda12.4.b10689.json"),
        )
        plan = command_plan(options)
        self.assertFalse(plan["network_access"])
        self.assertFalse(plan["model_process_started"])
        self.assertFalse(plan["gpu_lock_read_or_changed"])
        self.assertEqual(plan["cuda_archive_download_bytes"], 641_985_859)
        self.assertEqual(plan["cuda_expanded_payload_bytes"], 1_158_588_189)

    def test_game_matrix_has_a_separate_visible_gui_gate(self) -> None:
        contract = BenchmarkContract.load(CONTRACT, ROOT)
        with tempfile.TemporaryDirectory() as temporary:
            lock = Path(temporary) / "gpu use.txt"
            lock.write_text("yes\n", encoding="utf-8")
            with self.assertRaisesRegex(LocalLlmError, "separate coordination"):
                run_game_matrix(
                    contract=contract,
                    repo_root=ROOT,
                    model_path=Path(r"C:\missing\model.gguf"),
                    vulkan_executable=Path(r"C:\missing\vulkan\llama-server.exe"),
                    cuda_executable=Path(r"C:\missing\cuda\llama-server.exe"),
                    gpu_lock_file=lock,
                    execute_model_benchmark=True,
                    allow_synthetic_game_gui=False,
                )
            self.assertEqual(lock.read_text(encoding="utf-8"), "yes\n")

    def test_lmstudio_control_is_exact_and_cannot_claim_p99(self) -> None:
        value = load_control(LM_CONTROL)
        self.assertEqual(value["schema"], "npc.local-llm.observational-control/v1")
        self.assertEqual(value["engine"]["engine_version"], "2.31.2")
        self.assertEqual(value["engine"]["llama_cpp_release"], "b10662")
        self.assertEqual(value["engine"]["cuda_label"], "12.8")
        self.assertTrue(value["model_access"]["dry_run_verified"])
        self.assertFalse(value["model_access"]["copy_model"])
        self.assertEqual(value["limits"]["samples"], 10)
        self.assertFalse(value["limits"]["p99_or_admission_claims_allowed"])
        for section in (value["engine"]["files"], value["vendor_runtime"]["files"]):
            for item in section:
                self.assertRegex(item["sha256"], r"^[0-9a-f]{64}$")
                self.assertGreater(item["size_bytes"], 0)
        command = build_load_command(Path(r"C:\lmstudio\lms.exe"), "interactive-npcs/qwen3", "npc-control")
        self.assertEqual(command[1:3], ["load", "interactive-npcs/qwen3"])
        self.assertIn("max", command)
        self.assertIn("8192", command)
        self.assertIn("npc-control", command)


class BenchmarkBundleTests(unittest.TestCase):
    @staticmethod
    def _zip(files: dict[str, bytes]) -> bytes:
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, payload in files.items():
                archive.writestr(name, payload)
        return stream.getvalue()

    def test_cuda_bundle_manifest_is_fully_pinned(self) -> None:
        bundle = CudaBenchmarkBundle.load(CUDA_BUNDLE)
        self.assertEqual(bundle.release_tag, "b10689")
        self.assertEqual(bundle.source_commit, "57291f2644af8c9df0dd8d44395881c5bdcf0ecd")
        self.assertEqual(bundle.cuda_runtime, "12.4")
        self.assertEqual(sum(item.size_bytes for item in bundle.assets), 641_985_859)
        self.assertEqual(bundle.expected_expanded_bytes, 1_158_588_189)

    def test_archive_anchored_install_and_verify(self) -> None:
        first_files = {"llama-server.exe": b"entry", "ggml-cuda.dll": b"cuda"}
        second_files = {"cudart64_12.dll": b"runtime"}
        payloads = (self._zip(first_files), self._zip(second_files))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "bundle.json"
            manifest.write_text("{}", encoding="utf-8")
            assets = []
            for index, payload in enumerate(payloads):
                filename = f"asset-{index}.zip"
                (root / filename).write_bytes(payload)
                files = first_files if index == 0 else second_files
                assets.append(
                    BundleAsset(
                        asset_id=f"asset-{index}",
                        filename=filename,
                        size_bytes=len(payload),
                        sha256=hashlib.sha256(payload).hexdigest(),
                        expected_member_count=len(files),
                        expected_expanded_bytes=sum(len(item) for item in files.values()),
                        extraction_ceiling_bytes=1_048_576,
                        required_files=tuple(PurePosixPath(item) for item in files),
                    )
                )
            bundle = CudaBenchmarkBundle(
                path=manifest,
                release_tag="b1",
                source_commit="a" * 40,
                cuda_runtime="12.4",
                entrypoint="llama-server.exe",
                assets=tuple(assets),
                expected_member_count=3,
                expected_expanded_bytes=sum(len(item) for files in (first_files, second_files) for item in files.values()),
            )
            installed = root / "installed"
            install_bundle(bundle, root, installed)
            verify_bundle(bundle, installed, root)
            (installed / "payload" / "ggml-cuda.dll").write_bytes(b"evil")
            with self.assertRaises(LocalLlmError):
                verify_bundle(bundle, installed, root)


if __name__ == "__main__":
    unittest.main()
