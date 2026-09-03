#!/usr/bin/env python3
"""Bind immutable OpenSeeFace measurements to a metadata-corrected manifest.

This tool never imports ONNX Runtime, OpenCV, or the model weights. It fails
closed unless the immutable report, prior candidate, final manifest, artifact
inventory, runtime contract, and old catalog binding match the reviewed hashes.
The output signature is deliberately ephemeral and non-authoritative; release
trust remains the resource governor's separate responsibility.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


OLD_RAW = "aaa5a2feff73829b0c2bc31f34ca64c684b933f79145364741ed6e1d628405a5"
OLD_CANONICAL = "d953de63fc421b1fae5b4408850c6d8025d0e2464ef7073ccaf0ce8e2a3b0770"
OLD_CORE = "3f442ae21a4456cba1d83680b2926d89345b9485c5908bc928201dd535dc9c56"
FINAL_RAW = "2d10e01c1b1ab5177d594375f94dbc62ccd926b4fa82f75b35cda33c27e38dae"
FINAL_CANONICAL = "f430962ca08a6f93684b8bad82a6ae3ed966815e7c672dd1560095a7964a65bf"
PRIOR_FINAL_CORE = "faafcf7295c9002ca9637f1b91a652b36bcaf36b12b97ab17a33a84e4cbfcc00"
FINAL_CORE = "0127b61113b1f838bf70bdc4127bd7d5f7f9a75fde95429c89c5e726751f0552"
PRIOR_DERIVATION_SHA = "34a2c799110642d4493bc64e5efa355051caeb10866ff76f5dfd3c729fe431ae"
REPORT_SHA = "e8ee03e11f8ca55c5e588e393c4668c188c6896722ac4e7bfb382dd9aa2435c8"
CANDIDATE_SHA = "b74876101b2ffddcedc201a24f5b62f6f0bfd62798aa138db0a64a42714f4b9c"
DEVICE_FINGERPRINT = "480a0ebfe9b7f9ce9edc0b5e98d97491c7c3a2c8e5d3e52bcb4d13f9e3a230ce"
PACK_ID = "openseeface-mnv3-lm1-mouth-signal"
REVISION = "85aa70fc67582d046e771ea73625182a0d8f7475"
EXPECTED_ARTIFACTS = {
    "mnv3-detection-opt": (568302, "0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809"),
    "lm-model1-opt": (4842329, "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f"),
    "openseeface-license": (1364, "28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146"),
    "onnxruntime-1.22.1-cpu-windows-x64": (
        73731806,
        "855276cd4be3cda14fe636c69eb038d75bf5bcd552bda1193a5d79c51f436dfe",
    ),
}


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise ValueError(detail)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--prior-derivation", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--telemetry-captured-unix-millis", type=int, required=True)
    args = parser.parse_args()

    require(sha256(args.manifest) == FINAL_RAW, "final manifest raw digest changed")
    require(sha256(args.report) == REPORT_SHA, "immutable qualification report changed")
    require(sha256(args.candidate) == CANDIDATE_SHA, "immutable candidate envelope changed")
    manifest = read_json(args.manifest)
    report = read_json(args.report)
    candidate = read_json(args.candidate)
    require(sha256(args.prior_derivation) == PRIOR_DERIVATION_SHA,
            "prior immutable derivation changed")
    prior = read_json(args.prior_derivation)["derivation"]
    require(prior["old_manifest"] == {
        "raw_sha256": OLD_RAW,
        "canonical_sha256": OLD_CANONICAL,
        "normalized_core_sha256": OLD_CORE,
    }, "prior derivation old binding changed")
    require(prior["final_manifest"] == {
        "raw_sha256": FINAL_RAW,
        "canonical_sha256": FINAL_CANONICAL,
        "normalized_core_sha256": PRIOR_FINAL_CORE,
    }, "prior derivation final binding changed")
    require(manifest["pack_id"] == PACK_ID and manifest["revision"] == REVISION, "identity changed")
    require(report["pack"]["id"] == PACK_ID and report["pack"]["revision"] == REVISION, "report identity changed")
    require(candidate["signed"]["identity"] == {"pack_id": PACK_ID, "revision": REVISION}, "candidate identity changed")
    artifacts = {item["id"]: (item["size_bytes"], item["sha256"]) for item in manifest["artifacts"]}
    require(artifacts == EXPECTED_ARTIFACTS, "model/license/runtime artifact inventory changed")
    require(manifest["runtime"]["runtime"] == "onnxruntime", "runtime family changed")
    require(manifest["runtime"]["immutable_revision"] == "1.22.1", "runtime revision changed")
    require(manifest["runtime"]["abi"] == "onnxruntime-cpu-v1", "runtime ABI changed")
    require(manifest["runtime"]["backends"] == ["onnxruntime-1.22.1-cpu"], "runtime backend changed")
    require(manifest["self_test"]["kind"] == "openseeface-mouth-signal-v1", "self-test changed")
    require(manifest["self_test"]["suite_revision"] == "2026-08-30", "self-test suite changed")
    require(manifest["extensions"]["vision"]["maximum_signal_rate_hz"] == 15, "signal rate changed")
    require(manifest["admission"]["allowed_residencies"] == ["cpu_resident"], "residency changed")
    require(report["runtime"]["onnxruntime"] == "1.22.1", "measured runtime changed")
    require(report["runtime"]["providers"] == ["CPUExecutionProvider"], "measured backend changed")
    require(report["runtime"]["inference_threads"] == 1, "measured threading changed")

    derivation = {
        "schema": "npc.openseeface-manifest-measurement-derivation/v1",
        "status": "candidate_non_authoritative_requires_resource_governor_review",
        "identity": {"pack_id": PACK_ID, "revision": REVISION},
        "inference_rerun": False,
        "measurement_recomputed": False,
        "prior_derivation_sha256": PRIOR_DERIVATION_SHA,
        "old_manifest": {
            "raw_sha256": OLD_RAW,
            "canonical_sha256": OLD_CANONICAL,
            "normalized_core_sha256": OLD_CORE,
        },
        "final_manifest": {
            "raw_sha256": FINAL_RAW,
            "canonical_sha256": FINAL_CANONICAL,
            "normalized_core_sha256": FINAL_CORE,
        },
        "immutable_inputs": {
            "qualification_report_sha256": REPORT_SHA,
            "candidate_envelope_sha256": CANDIDATE_SHA,
            "artifact_inventory": [
                {"id": key, "size_bytes": value[0], "sha256": value[1]}
                for key, value in sorted(EXPECTED_ARTIFACTS.items())
            ],
            "fixture_sha256": report["fixture"]["sha256"],
            "runtime": report["runtime"],
            "self_test_kind": manifest["self_test"]["kind"],
            "self_test_suite_revision": manifest["self_test"]["suite_revision"],
            "allowed_residencies": manifest["admission"]["allowed_residencies"],
        },
        "metadata_only_change_proof": {
            "artifact_bytes_changed": False,
            "runtime_executable_or_revision_changed": False,
            "abi_backend_or_threading_changed": False,
            "self_test_or_capability_role_changed": False,
            "measurement_or_rendered_evidence_changed": False,
            "changed_scope": [
                "schema normalizer now preserves the already-declared archive strip_prefix",
                "schema normalizer now preserves the already-declared closed-world required_paths",
            ],
            "transferability_reason": "The raw and canonical manifest, measured model files, exact ORT 1.22.1 CPU execution contract, fixture, benchmark suite, one-thread placement, residency, and evidence are byte-for-byte unchanged. Only the schema normalizer's faithful preservation of existing extraction metadata changed the normalized core digest.",
        },
        "authoritative_device_binding": {
            "collector": "npc-system-telemetry",
            "schema": "npc.system-telemetry/resource-snapshot-v1",
            "device_fingerprint_sha256": DEVICE_FINGERPRINT,
            "fingerprint_provenance_source": "dxgi_adapter_description",
            "captured_unix_millis": args.telemetry_captured_unix_millis,
            "selected_game_pid": None,
            "raw_serial_or_pii_recorded": False,
            "replaces_non_authoritative_candidate_fingerprint": candidate["signed"]["device_fingerprint_sha256"],
        },
        "copied_without_recomputation": {
            "sample_count": candidate["signed"]["sample_count"],
            "placements": candidate["signed"]["placements"],
            "p99": report["p99"],
            "resources": report["resources"],
            "quality": report["quality"],
            "rendered_proof": report["rendered_proof"],
            "capture_environment": report["capture_environment"],
        },
        "admission_truth": {
            "release_trusted": False,
            "activation_authority": False,
            "activate_now": False,
            "live_game_certified": False,
            "resource_governor_must_verify_and_threshold_sign_new_envelope": True,
        },
    }
    private_key = Ed25519PrivateKey.generate()
    signature = private_key.sign(canonical(derivation))
    public_key = private_key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    document = {
        "derivation": derivation,
        "signatures": [{
            "key_id": "ephemeral-manifest-rebind-not-release-trusted",
            "algorithm": "ed25519",
            "signature": base64.b64encode(signature).decode("ascii"),
        }],
        "verification": {
            "canonicalization": "UTF-8 RFC8259 JSON; recursively sorted keys; compact separators",
            "public_key_base64": base64.b64encode(public_key).decode("ascii"),
            "release_trusted": False,
            "activation_authority": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "sha256": sha256(args.output)}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
