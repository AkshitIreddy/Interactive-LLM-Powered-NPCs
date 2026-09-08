#!/usr/bin/env python3
"""Run the production Rust hosted-TTS adapters with redacted credentials."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import subprocess
from pathlib import Path

from provider_timing_alternatives import DEFAULT_OUTPUT, atomic_json, resolve_credentials


PROVIDERS = ("cartesia", "inworld", "deepgram")
FIXTURE_TEXT = "The harbor lantern is ready for tonight's test."


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--target-dir", type=Path, default=Path(r"E:\temp\InteractiveNPCs\cargo-target"))
    parser.add_argument("--report-file", type=Path)
    args = parser.parse_args()

    credentials, sources = resolve_credentials(args.credentials)
    missing = [provider for provider in PROVIDERS if provider not in credentials]
    if missing:
        parser.error(f"missing provider credential labels: {missing}")

    root = Path(__file__).resolve().parents[2]
    manifest = Path(__file__).resolve().parent / "rust-adapter-qualification" / "Cargo.toml"
    environment = os.environ.copy()
    environment.update(
        {
            "CARTESIA_API_KEY": credentials["cartesia"],
            "INWORLD_API_KEY": credentials["inworld"],
            "DEEPGRAM_API_KEY": credentials["deepgram"],
            "CARGO_TARGET_DIR": str(args.target_dir),
        }
    )
    process = subprocess.run(
        ["cargo", "run", "--quiet", "--manifest-path", str(manifest)],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=240,
        check=False,
    )
    combined = process.stdout + process.stderr
    leaked = any(value and value in combined for value in credentials.values())
    if leaked:
        print(json.dumps({"status": "credential-output-blocked", "exitCode": process.returncode}))
        return 1
    if process.returncode != 0:
        safe_error = process.stderr.replace(FIXTURE_TEXT, "[FIXTURE REDACTED]")[-4_000:]
        print(json.dumps({"status": "adapter-run-failed", "exitCode": process.returncode, "diagnostic": safe_error}, indent=2))
        return 1

    lines = [line for line in process.stdout.splitlines() if line.strip()]
    if not lines:
        print(json.dumps({"status": "adapter-report-missing"}))
        return 1
    try:
        report = json.loads(lines[-1])
    except json.JSONDecodeError:
        print(json.dumps({"status": "adapter-report-invalid"}))
        return 1
    if not isinstance(report, dict):
        print(json.dumps({"status": "adapter-report-invalid"}))
        return 1
    serialized = json.dumps(report, sort_keys=True)
    if FIXTURE_TEXT in serialized or any(value and value in serialized for value in credentials.values()):
        print(json.dumps({"status": "sensitive-report-output-blocked"}))
        return 1

    checked_at = utc_now()
    report.update(
        {
            "checkedAtUtc": checked_at,
            "credentialSources": {provider: sources[provider] for provider in PROVIDERS},
            "credentialAbsenceVerification": {
                "processOutputMatches": 0,
                "reportMatches": 0,
                "passed": True,
            },
        }
    )
    output = args.report_file or (
        args.output_dir / f"rust-production-adapter-qualification-{checked_at.replace(':', '').replace('-', '')}.json"
    )
    if output.exists():
        parser.error(f"report file already exists: {output}")
    report["reportFile"] = str(output)
    atomic_json(output, report)
    summary = {
        "status": "passed",
        "report": str(output),
        "cartesia": {
            "firstPcmMs": report["cartesia"]["first"]["requestToFirstPcmMs"],
            "reusedPcmMs": report["cartesia"]["secondSameIdentity"]["requestToFirstPcmMs"],
            "postCancelPcmMs": report["cartesia"]["afterCancelIsolationSameIdentity"]["requestToFirstPcmMs"],
            "finalConnectionStats": report["cartesia"]["finalStats"],
        },
        "inworld": {
            "firstPcmMs": report["inworld"]["session"]["requestToFirstPcmMs"],
            "providerVisemeItems": report["inworld"]["session"]["providerVisemeItems"],
        },
        "deepgram": {"firstPcmMs": report["deepgram"]["session"]["requestToFirstPcmMs"]},
        "credentialMatches": 0,
    }
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
