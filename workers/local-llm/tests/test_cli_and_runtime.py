from __future__ import annotations

import hashlib
import io
import tempfile
import types
import unittest
import zipfile
from pathlib import Path, PurePosixPath

from npc_local_llm.cli import _gpu_lock_required, command_plan
from npc_local_llm.download import MemoryArtifactFetcher
from npc_local_llm.errors import LocalLlmError
from npc_local_llm.lifecycle import RuntimeBundleLifecycle
from npc_local_llm.manifest import Artifact, RuntimeBundle, RuntimeVariant
from npc_local_llm.measurements import distribution_millis

from support import MANIFEST, RUNTIME_BUNDLE


class CliTests(unittest.TestCase):
    def test_plan_is_exact_and_declares_no_network(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            options = types.SimpleNamespace(
                manifest=MANIFEST,
                runtime_bundle=RUNTIME_BUNDLE,
                install_root=Path(temporary),
                runtime_variant="windows-x64-vulkan",
            )
            plan = command_plan(options)
        self.assertFalse(plan["network_access"])
        self.assertFalse(plan["downloads_started"])
        self.assertEqual(plan["total_download_bytes"], 2_532_209_573)
        self.assertIsNone(plan["runtime_expanded_bytes"])

    def test_vulkan_qualification_requires_yes_without_mutating_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            lock = Path(temporary) / "gpu use.txt"
            lock.write_text("no\n", encoding="utf-8")
            options = types.SimpleNamespace(runtime_variant="windows-x64-vulkan", gpu_lock_file=lock)
            with self.assertRaises(LocalLlmError):
                _gpu_lock_required(options)
            self.assertEqual(lock.read_text(encoding="utf-8"), "no\n")
            lock.write_text("yes\n", encoding="utf-8")
            _gpu_lock_required(options)
            self.assertEqual(lock.read_text(encoding="utf-8"), "yes\n")

    def test_percentiles_are_measured_not_invented(self) -> None:
        result = distribution_millis([1.0, 2.0, 3.0, 4.0, 5.0])
        self.assertEqual(result["sample_count"], 5)
        self.assertEqual(result["p50"], 3.0)
        self.assertAlmostEqual(result["p95"], 4.8)


class RuntimeLifecycleTests(unittest.TestCase):
    def test_runtime_archive_install_verify_remove(self) -> None:
        archive_bytes = io.BytesIO()
        with zipfile.ZipFile(archive_bytes, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("runtime/llama-server.exe", b"fixture-server")
            archive.writestr("LICENSE", b"MIT")
        payload = archive_bytes.getvalue()
        artifact = Artifact(
            artifact_id="windows-x64-cpu",
            kind="archive",
            source_urls=(f"https://github.com/test/runtime/releases/download/v1/runtime.zip",),
            size_bytes=len(payload),
            sha256=hashlib.sha256(payload).hexdigest(),
            destination=PurePosixPath("runtime/windows-x64-cpu.zip"),
        )
        bundle = RuntimeBundle(
            path=RUNTIME_BUNDLE,
            abi="fixture-runtime-abi",
            release_tag="v1",
            source_commit="a" * 40,
            entrypoint="llama-server.exe",
            variants=(RuntimeVariant("windows-x64-cpu", "cpu", artifact),),
        )
        with tempfile.TemporaryDirectory() as temporary:
            lifecycle = RuntimeBundleLifecycle(
                Path(temporary), MemoryArtifactFetcher({"windows-x64-cpu": payload})
            )
            with self.assertRaises(LocalLlmError):
                lifecycle.install(bundle, "windows-x64-cpu", explicit_user_confirmation=False)
            receipt = lifecycle.install(bundle, "windows-x64-cpu", explicit_user_confirmation=True)
            self.assertEqual(receipt.extracted_members, 2)
            lifecycle.verify(bundle, "windows-x64-cpu")
            self.assertTrue(
                lifecycle.remove(
                    bundle,
                    "windows-x64-cpu",
                    explicit_user_confirmation=True,
                    require_unreferenced=True,
                    is_referenced=lambda _abi, _variant: False,
                )
            )


if __name__ == "__main__":
    unittest.main()
